#![cfg(test)]
#![allow(unused_imports)]
//! Regression tests owned by the checkbox component.

use eframe::egui::{self,Frame};


use crate::ui_primitives::*;








use crate::checkbox::*;
use crate::scrollbar::*;
use crate::popup::*;
use crate::card::*;
// Composite widgets import primitive renderers from their owning modules.











// Shared table primitives used by reception and other composite widgets.

#[cfg(test)]
mod shared_checkbox_contract_tests {
    #[test]
    fn vat_dialog_uses_shared_checkbox_and_nested_frame_components(){
        let source=include_str!("../main.rs");
        let dialog=source.split("fn show_vat_settings(").nth(1).unwrap()
            .split("fn show_edit_history(").next().unwrap();
        assert!(dialog.contains("crate::checkbox::checkbox("));
        assert!(dialog.contains("crate::checkbox::checkbox_list_row("));
        assert!(dialog.contains("crate::popup::modal_nested_role_frame(crate::popup::ModalNestedRole::RecipientChecklist)"));
        assert!(!dialog.contains(".inner_margin("));
        assert!(!dialog.contains("egui::Checkbox::new("));
        assert!(!dialog.contains("egui::Frame::new()"));

        let client=include_str!("../client_manager.rs");
        let boolean=client.split("fn bool_edit(").nth(1).unwrap()
            .split("fn new_id(").next().unwrap();
        assert!(boolean.contains("crate::checkbox::checkbox("));
        assert!(!boolean.contains("ui.checkbox("));

        let main=include_str!("../main.rs");
        let message=main.split("fn message_section(").nth(1).unwrap()
            .split("fn ").next().unwrap();
        assert!(message.contains("crate::checkbox::checkbox("));
        assert!(!message.contains("Checkbox::new("));
        let workflow=main.split("fn show_workflow_dialog(").nth(1).unwrap()
            .split("\n    fn ").next().unwrap();
        assert!(workflow.contains("crate::checkbox::checkbox("));
        assert!(!workflow.contains("ui.checkbox("));

        // OCR learning, category transfer consent and product filtering are
        // standard checkboxes too; none may construct a local egui checkbox.
        assert!(main.contains("changed=crate::checkbox::checkbox("));
        assert!(main.contains("&mut review.learned_analysis_enabled"));
        assert!(!main.contains("option.checkbox("));

        let category=include_str!("../category_manager.rs");
        assert!(category.contains("crate::checkbox::checkbox("));
        assert!(!category.contains("notice.checkbox("));

        let products=include_str!("../product_manager.rs");
        assert!(products.contains("crate::checkbox::checkbox("));
        assert!(products.contains("&mut self.missing"));
        assert!(!products.contains("filter.checkbox("));
    }
}
