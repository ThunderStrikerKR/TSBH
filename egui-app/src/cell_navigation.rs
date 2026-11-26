//! Shared spreadsheet-style cell navigation contract for native egui grids.
//! Each screen supplies explicit row/column/order metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellNavigationKey { Left, Right, Up, Down, Next, Previous }
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
    // Preserve stable input-order tie breaking while selecting the nearest
    // cell in a single pass. No temporary candidate list or sort.
    cells.iter().filter(|cell|cell.enabled && cell.id!=current_id)
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
        .min_by_key(|(rank,_)|*rank).map(|(_,cell)|cell.id.clone())
}

/// Shared, focus-scoped spreadsheet input contract for egui TextEdit.
/// Call BEFORE the TextEdit is rendered, then register the Response.id once
/// rendered. Other fields (search, dates, memos, forms) are never intercepted.
pub fn remember_worksheet_editor(
    ctx:&eframe::egui::Context,slot:eframe::egui::Id,editor_id:eframe::egui::Id
){
    ctx.data_mut(|data|data.insert_temp(slot,editor_id));
}

/// Extract only worksheet keys; callers choose the Enter mapping. IME
/// preedit ownership persists across frames, unlike a one-frame Ime check.
pub(crate) fn worksheet_key_from_events(
    events:&[eframe::egui::Event],ime_active:&mut bool,
    enter:CellNavigationKey,shift_enter:CellNavigationKey
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
            Key::Enter=>if modifiers.shift{shift_enter}else{enter},
            _=>return None
        };
        Some((*key,*modifiers,direction))
    })
}

/// Return a worksheet navigation command only when the CURRENT editor
/// owns egui keyboard focus. Consume it before TextEdit changes the caret.
/// Do not call for normal search, document, date or standalone form fields.
pub fn consume_worksheet_navigation_before_editor(
    ctx:&eframe::egui::Context,slot:eframe::egui::Id,
    enter:CellNavigationKey,shift_enter:CellNavigationKey
)->Option<CellNavigationKey>{
    let focused=ctx.data(|data|data.get_temp::<eframe::egui::Id>(slot))
        .is_some_and(|id|ctx.memory(|memory|memory.has_focus(id)));
    if !focused{return None;}
    let mut ime_active=ctx.data(|data|data.get_temp::<bool>(slot.with("ime")))
        .unwrap_or(false);
    let events=ctx.input(|input|input.events.clone());
    let navigation=worksheet_key_from_events(
        &events,&mut ime_active,enter,shift_enter
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
    pub fn before(&mut self,ctx:&eframe::egui::Context,id:&str){
        if self.request.is_some() || crate::overlay::popup_blocks_shortcuts(ctx)
            || !self.cells.iter().any(|cell|cell.id==id && cell.enabled){return;}
        if let Some(key)=consume_worksheet_navigation_before_editor(
            ctx,self.scope.with(id),CellNavigationKey::Next,CellNavigationKey::Previous
        ){
            self.request=Some((id.to_owned(),key));
        }
    }
    pub fn record(&mut self,id:&str,response:eframe::egui::Response){
        remember_worksheet_editor(&response.ctx,self.scope.with(id),response.id);
        self.responses.push((id.to_owned(),response));
    }
    pub fn finish(self,ctx:&eframe::egui::Context){
        let Some((current,key))=self.request else{return;};
        let Some(next)=resolve_cell_navigation_target(&self.cells,&current,key)else{return;};
        if let Some((_,response))=self.responses.iter().find(|(id,_)|id==&next){
            response.request_focus();
            response.scroll_to_me(Some(eframe::egui::Align::Center));
            ctx.request_repaint();
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
                events,ime,CellNavigationKey::Next,CellNavigationKey::Previous
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
            Some(CellNavigationKey::Next));
        assert_eq!(read(&[arrow(Key::Enter,Modifiers::SHIFT)],&mut ime),
            Some(CellNavigationKey::Previous));
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
                ctx,slot,CellNavigationKey::Down,CellNavigationKey::Up
            ),None,"unfocused inputs are not intercepted");
            assert!(ctx.input(|input|input.key_pressed(Key::ArrowUp)));
        });
        remember_worksheet_editor(&ctx,slot,editor_id);
        ctx.memory_mut(|memory|memory.request_focus(editor_id));
        ctx.run(RawInput{events:vec![arrow],..Default::default()},|ctx|{
            assert_eq!(consume_worksheet_navigation_before_editor(
                ctx,slot,CellNavigationKey::Down,CellNavigationKey::Up
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
