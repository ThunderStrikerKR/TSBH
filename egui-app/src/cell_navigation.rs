//! Shared spreadsheet-style cell navigation contract for native egui grids.
//! Each screen supplies explicit row/column/order metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellNavigationKey { Left, Right, Up, Down, Next, Previous, EnterDown, EnterUp }
impl CellNavigationKey {
    pub fn direction(self)->Self{
        match self{Self::EnterDown=>Self::Down,Self::EnterUp=>Self::Up,other=>other}
    }
    /// Enter and Tab explicitly confirm even at the edge of a worksheet.
    /// An arrow with no destination never commits or abandons the editor.
    pub fn commits_at_boundary(self)->bool{
        matches!(self,Self::EnterDown|Self::EnterUp|Self::Next|Self::Previous)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellNavigationCell {
    pub id: String,
    pub row: usize,
    pub column: usize,
    pub order: usize,
    pub enabled: bool,
}
pub fn resolve_cell_navigation_target(
    cells: &[CellNavigationCell], current_id: &str, key: CellNavigationKey,
) -> Option<String> {
    let current = cells.iter().find(|cell| cell.id == current_id && cell.enabled)?;
    nearest_cell(current,cells.iter(),key).map(|cell|cell.id.clone())
}
fn nearest_cell<'a>(
    current:&CellNavigationCell,cells:impl Iterator<Item=&'a CellNavigationCell>,
    key:CellNavigationKey
)->Option<&'a CellNavigationCell>{
    let key=key.direction();
    // Stable input-order tie breaking; no cloned candidate grid.
    cells.filter(|cell|cell.enabled && cell.id!=current.id)
        .filter_map(|cell|{
            let rank=match key{
                CellNavigationKey::Left if cell.row==current.row && cell.column<current.column=>
                    (usize::MAX-cell.column,usize::MAX-cell.order),
                CellNavigationKey::Right if cell.row==current.row && cell.column>current.column=>
                    (cell.column,cell.order),
                CellNavigationKey::Up if cell.column==current.column && cell.row<current.row=>
                    (usize::MAX-cell.row,cell.order),
                CellNavigationKey::Down if cell.column==current.column && cell.row>current.row=>
                    (cell.row,cell.order),
                CellNavigationKey::Next if cell.order>current.order=>(cell.order,0),
                CellNavigationKey::Previous if cell.order<current.order=>(usize::MAX-cell.order,0),
                _=>return None,
            };
            Some((rank,cell))
        })
        .min_by_key(|(rank,_)|*rank).map(|(_,cell)|cell)
}

/// Shared geometry for Reception and the manual delivery worksheet.
pub const RECIPIENT_FIELDS:[&str;5]=["name","address","mobile","phone","note"];
pub fn recipient_visual_position(field:&str)->(usize,usize){
    match field{
        "name"=>(0,0),"address"=>(1,0),"mobile"=>(2,0),"phone"=>(2,1),
        "note"=>(3,0),_=>(usize::MAX,usize::MAX)
    }
}
pub struct ReceptionNavigationCell<'a>{
    pub cell:&'a CellNavigationCell,
    pub shipment:usize,
    pub field:&'a str,
    pub has_item:bool,
}
impl ReceptionNavigationCell<'_>{
    fn recipient_section(&self)->bool{
        !self.has_item && self.field!="shippingCharge"
    }
}
/// Resolve recipient/item/freight card boundaries in one place. The manual
/// workspace permits adjacent-delivery navigation; Reception supplies only
/// the selected delivery. Field IDs and write/validation rules stay local.
pub fn resolve_reception_navigation(
    entries:&[ReceptionNavigationCell<'_>],current_id:&str,key:CellNavigationKey,
    cross_shipments:bool
)->Option<String>{
    let key=key.direction();
    let active=entries.iter().find(|entry|entry.cell.enabled && entry.cell.id==current_id)?;
    let recipient=active.recipient_section();
    let local=||entries.iter().filter(|entry|entry.cell.enabled && entry.shipment==active.shipment);
    // Phone shares the mobile row, but its vertical neighbors span the card.
    if recipient && active.field=="phone"{
        let field=match key{
            CellNavigationKey::Up=>Some("address"),CellNavigationKey::Down=>Some("note"),_=>None
        };
        if let Some(next)=field.and_then(|field|local().find(|entry|
            entry.recipient_section() && entry.field==field)){
            return Some(next.cell.id.clone());
        }
    }
    let sequential=matches!(key,CellNavigationKey::Next|CellNavigationKey::Previous);
    let candidates=entries.iter().filter(|entry|
        (sequential && (cross_shipments || entry.shipment==active.shipment))
        || (entry.shipment==active.shipment && entry.recipient_section()==recipient)
    ).map(|entry|entry.cell);
    if let Some(next)=nearest_cell(active.cell,candidates,key){
        return Some(next.id.clone());
    }
    let boundary=match key{
        CellNavigationKey::Down if recipient=>
            local().filter(|entry|!entry.recipient_section()).min_by_key(|entry|entry.cell.order),
        CellNavigationKey::Up if !recipient=>
            local().filter(|entry|entry.recipient_section()).max_by_key(|entry|entry.cell.order),
        CellNavigationKey::Down if !recipient && active.field!="shippingCharge"=>
            local().find(|entry|entry.field=="shippingCharge"),
        _=>None
    };
    if let Some(next)=boundary{return Some(next.cell.id.clone());}
    if !cross_shipments{return None;}
    let next=match key{
        CellNavigationKey::Down if !recipient || active.field=="note"=>
            entries.iter().find(|entry|entry.cell.enabled && active.shipment.checked_add(1)==Some(entry.shipment)),
        CellNavigationKey::Up if recipient && active.field=="name"=>
            entries.iter().rev().find(|entry|entry.cell.enabled && active.shipment.checked_sub(1)==Some(entry.shipment)),
        _=>None
    };
    next.map(|entry|entry.cell.id.clone())
}

/// One pointer transition policy for staged and native-backed worksheets.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum CellSwitch { KeepCurrent, OpenNow, CommitThenOpen }
pub fn cell_switch(current_is_target:bool,dirty:bool)->CellSwitch{
    if current_is_target{CellSwitch::KeepCurrent}
    else if dirty{CellSwitch::CommitThenOpen}
    else{CellSwitch::OpenNow}
}

#[cfg(test)]
mod pointer_cell_switch_tests{
    use super::*;
    #[test]
    fn one_click_keeps_target_or_opens_after_exactly_one_commit(){
        assert_eq!(cell_switch(true,false),CellSwitch::KeepCurrent);
        assert_eq!(cell_switch(true,true),CellSwitch::KeepCurrent);
        assert_eq!(cell_switch(false,false),CellSwitch::OpenNow);
        assert_eq!(cell_switch(false,true),CellSwitch::CommitThenOpen);
    }

    #[test]
    fn pointer_press_does_not_commit_before_destination_release(){
        use eframe::egui::{self,Event,Modifiers,PointerButton,RawInput,Sense,Vec2};
        let ctx=egui::Context::default();
        let mut result=Vec::new();
        for pressed in [true,false]{
            let _=ctx.run(RawInput{
                events:vec![Event::PointerButton{pos:egui::pos2(50.0,50.0),
                    button:PointerButton::Primary,pressed,modifiers:Modifiers::NONE}],
                ..Default::default()
            },|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    let (_,response)=ui.allocate_exact_size(Vec2::new(100.0,30.0),Sense::hover());
                    assert!(!response.has_focus());
                    result.push(worksheet_blur_finished(&response,false));
                    assert!(!worksheet_blur_finished(&response,true));
                });
            });
        }
        assert_eq!(result,vec![false,true]);
    }
}

/// A press may drop TextEdit focus before the destination emits clicked().
/// Defer closing/saving the old overlay until release, when that click can
/// win the transition. A retained overlay also observes focus loss on later frames.
pub fn worksheet_blur_finished(response:&eframe::egui::Response,step_interaction:bool)->bool{
    !step_interaction && !response.ctx.input(|input|input.pointer.any_down())
        && (response.lost_focus() || !response.has_focus())
}

/// Shared, focus-scoped spreadsheet input contract for egui TextEdit.
/// Call BEFORE the TextEdit is rendered, then register the Response.id once
/// rendered. Other fields (search, dates, memos, forms) are never intercepted.
pub fn remember_worksheet_editor(
    ctx:&eframe::egui::Context,slot:eframe::egui::Id,editor_id:eframe::egui::Id
){
    ctx.data_mut(|data|data.insert_temp(slot,editor_id));
}

/// Extract only worksheet keys using one Enter/Tab mapping. IME
/// preedit ownership persists across frames, unlike a one-frame Ime check.
pub(crate) fn worksheet_key_from_events(
    events:&[eframe::egui::Event],ime_active:&mut bool
)->Option<(eframe::egui::Key,eframe::egui::Modifiers,CellNavigationKey)>{
    use eframe::egui::{Event,ImeEvent,Key};
    let mut ime_event=false;
    for event in events{
        match event{
            Event::Ime(ImeEvent::Preedit(text))=>{
                *ime_active=!text.is_empty();
                ime_event=true;
            },
            Event::Ime(ImeEvent::Commit(_))
            |Event::Ime(ImeEvent::Disabled)=>{
                *ime_active=false;
                ime_event=true;
            },
            Event::Ime(_)=>{ime_event=true;},
            _=>{}
        }
    }
    if *ime_active || ime_event{return None;}
    events.iter().find_map(|event|{
        let Event::Key{key,pressed:true,modifiers,..}=event else{return None};
        // Keep standard shortcuts out of the spreadsheet navigation path.
        if modifiers.command||modifiers.ctrl||modifiers.alt{return None;}
        let direction=match key{
            Key::ArrowLeft=>CellNavigationKey::Left,
            Key::ArrowRight=>CellNavigationKey::Right,
            Key::ArrowUp=>CellNavigationKey::Up,
            Key::ArrowDown=>CellNavigationKey::Down,
            Key::Tab=>if modifiers.shift{
                CellNavigationKey::Previous
            }else{CellNavigationKey::Next},
            Key::Enter=>if modifiers.shift{CellNavigationKey::EnterUp}else{CellNavigationKey::EnterDown},
            _=>return None
        };
        Some((*key,*modifiers,direction))
    })
}

/// Return a worksheet navigation command only when the CURRENT editor
/// owns egui keyboard focus. Consume it before TextEdit changes the caret.
/// Do not call for normal search, document, date or standalone form fields.
pub fn consume_worksheet_navigation_before_editor(
    ctx:&eframe::egui::Context,slot:eframe::egui::Id
)->Option<CellNavigationKey>{
    if crate::overlay::popup_blocks_shortcuts(ctx){return None;}
    let focused=ctx.data(|data|data.get_temp::<eframe::egui::Id>(slot))
        .is_some_and(|id|ctx.memory(|memory|memory.has_focus(id)));
    if !focused{return None;}
    let mut ime_active=ctx.data(|data|data.get_temp::<bool>(slot.with("ime")))
        .unwrap_or(false);
    let events=ctx.input(|input|input.events.clone());
    let navigation=worksheet_key_from_events(
        &events,&mut ime_active
    );
    ctx.data_mut(|data|data.insert_temp(slot.with("ime"),ime_active));
    navigation.and_then(|(key,modifiers,direction)|{
        let consumed=ctx.input_mut(|input|{
            let mut any=false;
            while input.consume_key(modifiers,key){any=true;}
            any
        });
        consumed.then_some(direction)
    })
}

/// Navigation for explicitly opted-in, locally staged form cells. Render
/// all controls first, then request the destination focus by its real ID.
/// This does not save data or alter a form's existing save/cancel contract.
#[derive(Clone)]
struct StagedCellState {
    original:String,
    focused:bool,
}
pub struct WorksheetFocus {
    scope:eframe::egui::Id,
    cells:Vec<CellNavigationCell>,
    responses:Vec<(String,eframe::egui::Response)>,
    request:Option<(String,CellNavigationKey)>,
}
impl WorksheetFocus {
    pub fn new(scope:eframe::egui::Id,cells:Vec<CellNavigationCell>)->Self{
        Self{scope,cells,responses:Vec::new(),request:None}
    }
    pub fn before(&mut self,ctx:&eframe::egui::Context,id:&str,value:&str){
        if !self.cells.iter().any(|cell|cell.id==id && cell.enabled){return;}
        let slot=self.scope.with(id);
        let state_key=slot.with("staged-original");
        ctx.data_mut(|data|{
            let state=data.get_temp::<StagedCellState>(state_key);
            if !state.is_some_and(|state|state.focused){
                data.insert_temp(state_key,StagedCellState{original:value.to_owned(),focused:false});
            }
        });
        if self.request.is_some(){return;}
        if let Some(key)=consume_worksheet_navigation_before_editor(ctx,slot){
            self.request=Some((id.to_owned(),key));
        }
    }
    pub fn record(&mut self,id:&str,response:eframe::egui::Response,value:&mut String){
        let slot=self.scope.with(id);
        remember_worksheet_editor(&response.ctx,slot,response.id);
        let state_key=slot.with("staged-original");
        let state=response.ctx.data(|data|data.get_temp::<StagedCellState>(state_key));
        if let Some(mut state)=state{
            if crate::overlay::take_cell_escape(&response,state.focused){
                *value=state.original.clone();
                response.surrender_focus();
                self.request=None;
            }
            state.focused=response.has_focus();
            response.ctx.data_mut(|data|data.insert_temp(state_key,state));
        }
        self.responses.push((id.to_owned(),response));
    }
    pub fn finish(self,ctx:&eframe::egui::Context){
        let Some((current,key))=self.request else{return;};
        if let Some(next)=resolve_cell_navigation_target(&self.cells,&current,key){
            if let Some((_,response))=self.responses.iter().find(|(id,_)|id==&next){
                response.request_focus();
                response.scroll_to_me(Some(eframe::egui::Align::Center));
                ctx.request_repaint();
            }
        }else if key.commits_at_boundary(){
            if let Some((_,response))=self.responses.iter().find(|(id,_)|id==&current){
                response.surrender_focus();
            }
        }
    }
}
/// Metadata for a vertical run of editable cells in a staged form.
pub fn vertical_worksheet_cells<'a>(
    ids:impl IntoIterator<Item=&'a str>,enabled:bool
)->Vec<CellNavigationCell>{
    ids.into_iter().enumerate().map(|(row,id)|CellNavigationCell{
        id:id.to_owned(),row,column:0,order:row,enabled
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn grid() -> Vec<CellNavigationCell> {
        vec![
            CellNavigationCell { id: "a".into(), row: 0, column: 0, order: 0, enabled: true },
            CellNavigationCell { id: "b".into(), row: 0, column: 1, order: 1, enabled: true },
            CellNavigationCell { id: "c".into(), row: 1, column: 0, order: 2, enabled: true },
            CellNavigationCell { id: "d".into(), row: 1, column: 1, order: 3, enabled: false },
            CellNavigationCell { id: "e".into(), row: 2, column: 1, order: 4, enabled: true },
        ]
    }
    #[test]
    fn keyboard_extractor_honors_ime_across_frames_and_preserves_shortcuts(){
        use eframe::egui::{Event,ImeEvent,Key,Modifiers};
        let arrow=|key,modifiers|Event::Key{
            key,physical_key:None,pressed:true,repeat:false,modifiers
        };
        let mut ime=false;
        let read=|events:&[Event],ime:&mut bool|
            worksheet_key_from_events(
                events,ime
            ).map(|(_,_,key)|key);
        assert_eq!(read(&[arrow(Key::ArrowUp,Modifiers::NONE)],&mut ime),
            Some(CellNavigationKey::Up));
        assert_eq!(read(&[arrow(Key::ArrowDown,Modifiers::NONE)],&mut ime),
            Some(CellNavigationKey::Down));
        assert_eq!(read(&[arrow(Key::ArrowLeft,Modifiers::NONE)],&mut ime),
            Some(CellNavigationKey::Left));
        assert_eq!(read(&[arrow(Key::ArrowRight,Modifiers::NONE)],&mut ime),
            Some(CellNavigationKey::Right));
        assert_eq!(read(&[arrow(Key::Enter,Modifiers::NONE)],&mut ime),
            Some(CellNavigationKey::EnterDown));
        assert_eq!(read(&[arrow(Key::Enter,Modifiers::SHIFT)],&mut ime),
            Some(CellNavigationKey::EnterUp));
        assert_eq!(read(&[arrow(Key::Tab,Modifiers::SHIFT)],&mut ime),
            Some(CellNavigationKey::Previous));
        assert_eq!(read(&[arrow(Key::ArrowUp,Modifiers::CTRL)],&mut ime),None);
        assert_eq!(read(&[Event::Ime(ImeEvent::Preedit("한".into()))],&mut ime),None);
        assert!(ime);
        assert_eq!(read(&[arrow(Key::ArrowDown,Modifiers::NONE)],&mut ime),
            None,"preedit state persists even when this frame has no Ime event");
        assert_eq!(read(&[Event::Ime(ImeEvent::Commit("한".into()))],&mut ime),None);
        assert!(!ime);
        assert_eq!(read(&[arrow(Key::ArrowDown,Modifiers::NONE)],&mut ime),
            Some(CellNavigationKey::Down));
    }

    #[test]
    fn focused_worksheet_consumes_arrow_before_editor_other_fields_do_not(){
        use eframe::egui::{Context,Event,Key,Modifiers,RawInput};
        let ctx=Context::default();
        let slot=eframe::egui::Id::new("selected-sheet-cell");
        let editor_id=eframe::egui::Id::new("selected-sheet-editor");
        let arrow=Event::Key{
            key:Key::ArrowUp,physical_key:None,pressed:true,
            repeat:false,modifiers:Modifiers::NONE
        };
        ctx.run(RawInput{events:vec![arrow.clone()],..Default::default()},|ctx|{
            assert_eq!(consume_worksheet_navigation_before_editor(
                ctx,slot
            ),None,"unfocused inputs are not intercepted");
            assert!(ctx.input(|input|input.key_pressed(Key::ArrowUp)));
        });
        remember_worksheet_editor(&ctx,slot,editor_id);
        ctx.memory_mut(|memory|memory.request_focus(editor_id));
        ctx.run(RawInput{events:vec![arrow],..Default::default()},|ctx|{
            assert_eq!(consume_worksheet_navigation_before_editor(
                ctx,slot
            ),Some(CellNavigationKey::Up));
            assert!(!ctx.input(|input|input.key_pressed(Key::ArrowUp)),
                "TextEdit must not be able to consume a worksheet arrow");
        });
    }

    #[test]
    fn horizontal_navigation_stays_on_the_same_row() {
        let cells = grid();
        assert_eq!(resolve_cell_navigation_target(&cells, "a", CellNavigationKey::Right).as_deref(), Some("b"));
        assert_eq!(resolve_cell_navigation_target(&cells, "b", CellNavigationKey::Left).as_deref(), Some("a"));
        assert_eq!(resolve_cell_navigation_target(&cells, "a", CellNavigationKey::Left), None);
    }
    #[test]
    fn vertical_navigation_stays_in_the_same_column_and_skips_disabled_cells() {
        let cells = grid();
        assert_eq!(resolve_cell_navigation_target(&cells, "a", CellNavigationKey::Down).as_deref(), Some("c"));
        assert_eq!(resolve_cell_navigation_target(&cells, "b", CellNavigationKey::Down).as_deref(), Some("e"));
        assert_eq!(resolve_cell_navigation_target(&cells, "e", CellNavigationKey::Up).as_deref(), Some("b"));
    }
    #[test]
    fn enter_moves_vertically_and_confirms_at_edges_without_changing_arrow_rules(){
        let cells=grid();
        assert_eq!(resolve_cell_navigation_target(&cells,"a",CellNavigationKey::EnterDown).as_deref(),Some("c"));
        assert_eq!(resolve_cell_navigation_target(&cells,"c",CellNavigationKey::EnterUp).as_deref(),Some("a"));
        assert!(CellNavigationKey::EnterDown.commits_at_boundary());
        assert!(CellNavigationKey::EnterUp.commits_at_boundary());
        assert!(CellNavigationKey::Next.commits_at_boundary());
        assert!(!CellNavigationKey::Down.commits_at_boundary());
        assert!(!CellNavigationKey::Right.commits_at_boundary());
        assert_eq!(resolve_cell_navigation_target(&cells,"c",CellNavigationKey::EnterDown),None);
    }
    #[test]
    fn tab_order_is_explicit_and_skips_disabled_cells() {
        let cells = grid();
        assert_eq!(resolve_cell_navigation_target(&cells, "b", CellNavigationKey::Next).as_deref(), Some("c"));
        assert_eq!(resolve_cell_navigation_target(&cells, "c", CellNavigationKey::Previous).as_deref(), Some("b"));
    }
    #[test]
    fn disabled_current_cell_and_grid_edges_have_no_target() {
        let cells = grid();
        assert_eq!(resolve_cell_navigation_target(&cells, "d", CellNavigationKey::Next), None);
        assert_eq!(resolve_cell_navigation_target(&cells, "e", CellNavigationKey::Next), None);
    }
}

#[cfg(test)]
mod navigation_single_pass_tests{
    use super::*;
    #[test]
    fn nearest_enabled_cell_keeps_original_tie_order(){
        let make=|id:&str,row,column,order,enabled|CellNavigationCell{id:id.into(),row,column,order,enabled};
        let cells=[make("current",1,1,1,true),make("disabled",1,2,2,false),
            make("first",1,3,3,true),make("second",1,3,3,true),make("far",1,4,4,true)];
        assert_eq!(resolve_cell_navigation_target(&cells,"current",CellNavigationKey::Right).as_deref(),Some("first"));
        assert_eq!(resolve_cell_navigation_target(&cells,"current",CellNavigationKey::Next).as_deref(),Some("first"));
    }
}
