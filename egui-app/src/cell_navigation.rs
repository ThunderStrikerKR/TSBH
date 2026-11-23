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
