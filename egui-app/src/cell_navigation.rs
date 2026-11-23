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
    let mut candidates: Vec<&CellNavigationCell> = cells.iter()
        .filter(|cell| cell.enabled && cell.id != current_id).collect();
    match key {
        CellNavigationKey::Left => {
            candidates.retain(|cell| cell.row == current.row && cell.column < current.column);
            candidates.sort_by_key(|cell| (usize::MAX - cell.column, usize::MAX - cell.order));
        }
        CellNavigationKey::Right => {
            candidates.retain(|cell| cell.row == current.row && cell.column > current.column);
            candidates.sort_by_key(|cell| (cell.column, cell.order));
        }
        CellNavigationKey::Up => {
            candidates.retain(|cell| cell.column == current.column && cell.row < current.row);
            candidates.sort_by_key(|cell| (usize::MAX - cell.row, cell.order));
        }
        CellNavigationKey::Down => {
            candidates.retain(|cell| cell.column == current.column && cell.row > current.row);
            candidates.sort_by_key(|cell| (cell.row, cell.order));
        }
        CellNavigationKey::Next => {
            candidates.retain(|cell| cell.order > current.order);
            candidates.sort_by_key(|cell| cell.order);
        }
        CellNavigationKey::Previous => {
            candidates.retain(|cell| cell.order < current.order);
            candidates.sort_by_key(|cell| usize::MAX - cell.order);
        }
    }
    candidates.first().map(|cell| cell.id.clone())
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
fn worksheet_key_from_events(
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
