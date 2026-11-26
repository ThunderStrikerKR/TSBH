#!/usr/bin/env node
import { readdirSync, readFileSync } from "node:fs";
import { resolve, relative } from "node:path";
import { fileURLToPath } from "node:url";
import assert from "node:assert/strict";
import { tokenizeRust } from "./check-egui-state-layout.mjs";

// Reviewed compact record/menu/footer dividers; not structural section lines.
// Fingerprints are scoped to file + function + UI argument, not line number.
// A new use needs an explicit review; removing a use does not require approval.
const compact = new Map(Object.entries({
  "main.rs:show_workflow_dialog:list":1,
  "settings_view.rs:show_account_rows:ui":1,
  "settings_view.rs:show_data:panel":1,
  "alias_manager.rs:show_page:list":1,
  "alias_manager.rs:show_dialogs:results":1,
  "brand_manager.rs:show_page:list":1,
  "model_group_manager.rs:show_page:results":1,
  "model_group_manager.rs:show_page:registered":1,
  "model_group_manager.rs:show_page:right":2,
  "automatic_message_manager.rs:show:body":1,
  "order_history_detail.rs:payment_panel:ui":1,
  "freight_manager.rs:show_history:menu":1,
  "excel_manager.rs:show_reply_review:ui":1,
  "workflow_board_ui.rs:group_frame:ui":1,
  "workflow_board_ui.rs:pending_frame:ui":1,
  "manual_product_search.rs:show:ui":1,
  "category_dropdown.rs:menu_body:menu":2
}));
const scrollConstructors = ["vertical","horizontal","both","new","neither"];
const headerScreens = new Set(["product_manager.rs","excel_manager.rs",
  "daily_settlement_manager.rs","order_history_manager.rs","freight_manager.rs"]);
// Table geometry and painting belong to the shared layer.
// Exceptions identify an exact call and occurrence limit, never a whole function.
const tableScreens = new Set([...headerScreens,"client_manager.rs",
  "client_chooser.rs","product_excel_ui.rs"]);
const retiredTable = new Set(["ManagementRowRule","management_row_striped",
  "management_row_impl","management_readonly_table_aligned",
  "paint_management_table_column_rule","paint_column_dividers",
  "manual_table_cell","manual_table_heading","manual_linked_table_editor",
  "management_sticky_table_header_after_inset","management_table_outline_rect","management_master_header_finish"]);
const tablePaintMethods = new Set(["rect_filled","rect_stroke","rect",
  "line_segment","hline","vline"]);
const paintExceptions = [
  // These pre-existing non-table surfaces occur in mixed-purpose files.
  // Keep each exact call and occurrence count; never exempt an entire screen.
  ["main.rs","show_ocr_review",1,
    "rect_filled(image_view_rect,egui::CornerRadius{nw:0,ne:0,sw:inner_radius,se:0},theme::CANVAS)"],
  ["main.rs","show_ocr_review",1,
    "rect_filled(text_view_rect,egui::CornerRadius{nw:0,ne:0,sw:0,se:inner_radius},theme::INPUT)"],
  ["main.rs","show_ocr_review",1,
    "paint_surface_outline(line.painter(),image.rect,0.0,edge)"],
  ["main.rs","source_section",1,
    "rect_filled(body_rect,0.0,theme::CANVAS)"],
  ["model_group_manager.rs","row",1,
    "rect_filled(rect,theme::CONTROL_RADIUS as f32,if self.selected==id{theme::RAISED}else{theme::INPUT})"],
  ["model_group_manager.rs","show_page",1,
    "rect_filled(separator,0.0,theme::BORDER)"],
  // History branch glyphs and date navigation, not table dividers.
  ["order_history_manager.rs","paint_history_branch",1,
    "line_segment([egui::pos2(x,center),egui::pos2(x,row_rect.bottom())],stroke)"],
  ["order_history_manager.rs","paint_history_branch",1,
    "line_segment([egui::pos2(x,row_rect.top()),egui::pos2(x,row_rect.bottom())],stroke)"],
  ["order_history_manager.rs","paint_history_branch",2,
    "line_segment([egui::pos2(x,center),egui::pos2(tip,center)],stroke)"],
  ["order_history_manager.rs","paint_history_branch",1,
    "line_segment([egui::pos2(x,row_rect.top()),egui::pos2(x,center)],stroke)"],
  ["order_history_manager.rs","range_date_picker_rtl",1,
    "line_segment([egui::pos2(x,inner.top()+4.0),egui::pos2(x,inner.bottom()-4.0)],Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER))"],
  // Chooser selection outline.
  ["client_chooser.rs","show",1,
    "rect_stroke(response.rect.shrink(1.0),0.0,egui::Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::SELECTION),egui::StrokeKind::Inside)"],
];
const normalized = source=>tokenizeRust(source).map(t=>t.kind==="literal"?"<literal>":t.v).join(" ");
const paintAllowance = new Map(paintExceptions.map(([file,fn,count,call])=>
  [file+":"+fn+":"+normalized(call),count]));
const masterContract = {
  show:1,fixed_rows:1,fixed_row:1,fixed_row_rect:1,
  management_table_cells:1,header_columns:1,heading:1,sort_clicked:1,
};
const tableContracts = {
  "product_manager.rs:show_page":masterContract,
  "freight_manager.rs:show_groups":masterContract,
  "freight_manager.rs:show_rates":masterContract,
  "freight_manager.rs:freight_history_table":masterContract,
  "excel_manager.rs:table":masterContract,
  "daily_settlement_manager.rs:report_table":masterContract,
  "order_history_manager.rs:show_page":{show:1,variable_rows:1,history_row:1,
    management_table_cells:1,header_columns:1,heading:1,sort_clicked:1},
  "order_history_manager.rs:draw_line":{management_table_cells:1},
  "order_history_manager.rs:history_detail_cells":{shipment_detail_cells:1},
  "order_history_manager.rs:history_line_height":{shipment_detail_product_galley:1,shipment_detail_block_height:1},
  "order_history_manager.rs:paint_detail_products":{shipment_detail_product_galley:1,paint_shipment_detail_products:1},
  // Check the common layer itself: a wrapper must still delegate to the
  // original master, rather than merely retaining the expected name.
  "components.rs:shipment_detail_cells":{management_table_cells:1,shipment_detail_content:1},
  "components.rs:paint_shipment_detail_products":{paint_table_galley:1},
  "components.rs:workflow_shipment_header":{
    management_inline_list_table_header:1,master_header_begin:1,
    shipment_detail_cells:1,paint_table_galley:1,management_table_column_rules:1,
    header_finish:1
  },
  "components.rs:workflow_shipment_row":{
    shipment_detail_cells:1,shipment_detail_product_galley:2,
    shipment_detail_block_height:1,paint_shipment_detail_surface:1,
    paint_shipment_detail_products:2,paint_table_galley:1
  },
  "client_manager.rs:show_page":{show:1,fixed_rows:1,row_scope:1,management_row:1,management_client_header:1},
  "client_chooser.rs:show":{show:1,fixed_rows:1,row_scope:1,management_row:1,management_client_header:1},
  "alias_manager.rs:show_page":{show:1,register_rows:1},
  "brand_manager.rs:show_page":{show:1,register_rows:1},
  "model_group_manager.rs:show_page":{show:1,register_rows:1},
  "product_manager.rs:show_detail":{management_readonly_table:1},
  "product_excel_ui.rs:show_review":{management_readonly_table:1},
};
// Verify the shared renderer itself, not just the calls in screens.
// Removing a required delegation fails even if the public API is unchanged.
const tableOwnerContracts = {
  "table/mod.rs:show":{finish:1},
  "table/mod.rs:inline":{finish_inline:1},
  "table/mod.rs:finish_inline":{finish:1},
  "table/mod.rs:finish":{outline_bounds:1,".rect_stroke":1},
  "table/mod.rs:header_begin":{".rect_filled":1},
  "table/mod.rs:header_finish":{".rect_filled":1},
  "table/mod.rs:master_header_begin":{header_begin:1},
  "table/mod.rs:fixed_row":{row_scope:1,management_table_cells:1,
    paint_management_table_row_background:1,management_table_column_rules:1,
    paint_management_table_row_rule:1},
  "table/mod.rs:history_row":{row_scope:1,management_table_cells:1,
    paint_management_table_row_background:1,management_table_column_rules:1,
    paint_management_table_row_rule:1,paint_shipment_detail_surface:1},
  "table/mod.rs:header_columns":{master_header_begin:1,management_table_column_rules:1,header_finish:1},
  "table/mod.rs:management_table_column_rules":{management_master_column_rules:1},
  "table/mod.rs:paint_management_table_row_background":{management_table_row_fill:1,".rect_filled":1},
  "table/mod.rs:paint_management_table_row_rule":{"theme::paint_soft_rule":1},
  "table/mod.rs:management_master_column_rules":{"theme::paint_soft_rule":1},
  "table/mod.rs:fixed_rows":{viewport_scroll:1,virtual_fixed_rows:1},
  "table/mod.rs:variable_rows":{viewport_scroll:1,management_table_row_body:1},
  "table/mod.rs:viewport_scroll":{management_fixed_table_scroll_area:1},
  "table/mod.rs:register_rows":{management_register_list_scroll_area:1},
};
const tableOnlyCalls = new Set(["show","fixed_rows","fixed_row","fixed_row_rect",
  "variable_rows","history_row","header_columns","heading","sort_clicked","row_scope",
  "register_rows","master_header_begin","header_finish"]);
const tableMarkers = new Set(["management_table_cells","management_table_column_rules",
  "management_row","management_readonly_table","shipment_detail_cells",
  "fixed_rows","variable_rows","fixed_row","history_row","header_columns","register_rows"]);
const componentContractCalls = new Set(Object.entries(tableContracts)
  .filter(([scope])=>scope.startsWith("components.rs:"))
  .flatMap(([,entries])=>Object.keys(entries)));

// Fixed workflow actions must be rendered by components::workflow_fixed_action.
// Existing non-button painting is reviewed by exact function/call/count.
const workflowPaintMethods = new Set(["rect_filled","rect_stroke","rect","line_segment","hline","vline","circle_filled","circle_stroke","galley","text","layout_no_wrap","interact","add_sized","add_enabled"]);
const workflowThemePaintMethods = new Set(["paint_bordered_surface","paint_button_hover","paint_button_hover_corners","paint_surface_outline","paint_soft_rule","action","action_with_min_width","action_full_width","review_action"]);
const workflowPaintAllowance = new Map([
  ["workflow_card:rect_filled ( Rect :: from_min_max ( Pos2 :: new ( r . left ( ) , r . top ( ) + theme :: SOFT_DIVIDER_WIDTH ) , Pos2 :: new ( r . left ( ) + theme :: WORKFLOW_SELECTED_STRIPE , r . bottom ( ) - theme :: SOFT_DIVIDER_WIDTH ) , ) , 0.0 , if preview { source_highlight :: ACTIVE_BORDER } else { theme :: WORKFLOW_SELECTED_BORDER } )",1],
  ["workflow_compact_rows:rect_filled ( divider , 0.0 , theme :: WORKFLOW_CARD_BORDER )",1],
  ["workflow_compact_rows:rect_filled ( lower , 0.0 , theme :: WORKFLOW_CARD_LOWER_BG )",1],
  ["workflow_header:galley ( Pos2 :: new ( text_rect . left ( ) , text_rect . center ( ) . y - galley . size ( ) . y / 2.0 ) , galley , theme :: TEXT )",1],
  ["workflow_header:interact ( arrow . union ( title_rect ) , workflow_widget_id ( <literal> , id , in_preview ) , egui :: Sense :: click ( ) )",1],
  ["workflow_header:paint_bordered_surface ( ui . painter ( ) , block , 5.0 , theme :: WORKFLOW_SIGNAL_BLOCK_BG , theme :: WORKFLOW_SIGNAL_BLOCK_BORDER )",1],
  ["workflow_header:paint_bordered_surface ( ui . painter ( ) , lamp , theme :: WORKFLOW_SIGNAL_RADIUS , fill , border )",1],
  ["workflow_header:paint_button_hover ( ui , & title_response , theme :: CONTROL_RADIUS as f32 )",1],
  ["workflow_action_line:layout_no_wrap ( shipping_text . clone ( ) , egui :: FontId :: proportional ( theme :: WORKFLOW_VAT_FONT_SIZE ) , theme :: SECONDARY )",1],
  ["workflow_action_line:galley ( Pos2 :: new ( shipping . left ( ) , bottom_y - ship_galley . size ( ) . y ) , ship_galley , theme :: SECONDARY )",1],
  ["workflow_action_line:layout_no_wrap ( amount . clone ( ) , typography :: data_value_font ( typography :: DataValueKind :: Money , typography :: DataValueContext :: CompactCard ) , theme :: TEXT )",1],
  ["workflow_action_line:galley ( Pos2 :: new ( price_ink . right ( ) - money_galley . size ( ) . x , bottom_y - money_galley . size ( ) . y ) , money_galley , theme :: TEXT )",1],
  ["workflow_action_line:layout_no_wrap ( visible , egui :: FontId :: proportional ( theme :: WORKFLOW_VAT_FONT_SIZE ) , theme :: WORKFLOW_VAT_TEXT )",1],
  ["workflow_action_line:galley ( Pos2 :: new ( price_ink . right ( ) - vat_galley . size ( ) . x , price . top ( ) + 3.0 ) , vat_galley , theme :: WORKFLOW_VAT_TEXT )",1],
  ["shipment_line:add_sized ( [ name_width , theme :: WORKFLOW_DETAIL_ROW_CONTENT_HEIGHT ] , egui :: Label :: new ( typography :: body ( name ) . color ( preview_ink ) ) . truncate ( ) )",1],
  ["shipment_line:add_sized ( [ product_width , theme :: WORKFLOW_DETAIL_ROW_CONTENT_HEIGHT ] , egui :: Label :: new ( typography :: body ( & summary ) . color ( preview_ink ) ) . truncate ( ) )",1],
  ["shipment_line:rect_filled ( Rect :: from_min_max ( Pos2 :: new ( rect . left ( ) , rect . top ( ) ) , Pos2 :: new ( rect . left ( ) + theme :: WORKFLOW_SELECTED_STRIPE , rect . bottom ( ) ) ) , 0.0 , color )",1],
  ["shipment_line:rect_stroke ( rect . shrink ( 0.5 ) , theme :: CONTROL_RADIUS as f32 , egui :: Stroke :: new ( source_highlight :: ACTIVE_BORDER_WIDTH , source_highlight :: ACTIVE_BORDER ) , egui :: StrokeKind :: Inside , )",1],
  ["group_frame:interact ( label . rect , Id :: new ( ( <literal> , ship . id . as_str ( ) , in_preview ) ) , egui :: Sense :: click ( ) )",1],
  ["group_frame:paint_button_hover ( ui , & response , 0.0 )",1],
  ["pending_frame:interact ( response . rect , Id :: new ( ( <literal> , draft . id . as_str ( ) , 0 usize , in_preview ) ) , egui :: Sense :: click ( ) )",1],
  ["pending_frame:paint_button_hover ( ui , & action , 0.0 )",2],
  ["pending_frame:interact ( response . rect , Id :: new ( ( <literal> , draft . id . as_str ( ) , index , in_preview ) ) , egui :: Sense :: click ( ) )",1],
]);
const workflowContracts = {
  "workflow_auxiliary_at:workflow_fixed_action":1,
  "workflow_primary_at:workflow_fixed_action":1,
  "workflow_header:workflow_auxiliary_at":2,
  "workflow_action_line:workflow_primary_at":1,
  "workflow_action_line:workflow_auxiliary_at":1,
};

const controls = [
  "Button :: new (","Frame :: new (","TextEdit :: singleline (",
  "TextEdit :: multiline (","Checkbox :: new (","ComboBox ::",
  "Window :: new (",". button (",". checkbox (",
  ". text_edit_singleline (",". text_edit_multiline (",
  "ScrollStyle :: solid (","ScrollStyle :: floating ("
];
// Native controls that were previously absent from the shared gate.
// Every production use must go through its owning constructor; the same
// rule also applies inside components.rs rather than trusting the file.
// These primitives have moved out of the legacy compatibility facade.
// Do not grant the new modules a file-wide native-control exception.
const extractedControlOwners = new Map([
  ["Checkbox :: new (",new Set(["checkbox/mod.rs:checkbox"])],
  [". checkbox (",new Set()],
  ["ScrollStyle :: solid (",new Set(["scrollbar/mod.rs:management_scroll_style"])],
  ["ScrollStyle :: floating (",new Set()],
  ["Button :: new (",new Set([
    "button/mod.rs:action_with_palette","button/mod.rs:window_control",
    "button/mod.rs:icon_action_button","button/mod.rs:management_date_segment",
    "button/mod.rs:date_navigation_segment_button","button/mod.rs:date_navigation_button_surface",
    "button/mod.rs:calendar_day","button/mod.rs:dispatch_origin","dropdown/mod.rs:management_dropdown_trigger_inner"
  ])],
  ["Button :: selectable (",new Set(["dropdown/mod.rs:management_dropdown_option_inner"])],
  ["TextEdit :: singleline (",new Set(["input/mod.rs:singleline_editor"])],
  ["TextEdit :: multiline (",new Set(["input/mod.rs:multiline_editor"])],
]);
const ownedControls = new Map([
  ["CollapsingHeader :: new (","components.rs:collapsing_header"],
  ["Slider :: new (","components.rs:ocr_zoom_slider"],
  ["DragValue :: new (",null],["RadioButton :: new (",null],
  ["SelectableLabel :: new (",null],["ProgressBar :: new (",null],
  ["Spinner :: new (",null],["Separator :: new (",null],
  ["ImageButton :: new (",null],[". radio (",null],[". radio_value (",null],
  [". small_button (",null],[". toggle_value (",null],
  [". selectable_label (",null],[". selectable_value (","dropdown/mod.rs:management_dropdown_option_inner"],
  [". spinner (",null],[". show_tooltip_ui (",null],[". show_tooltip_text (",null],
  [". on_hover_ui (",null],[". on_hover_ui_at_pointer (",null],
]);
const paletteConstructors = ["from_rgb","from_rgba_unmultiplied",
  "from_rgba_unmultiplied_const","from_rgba_premultiplied","from_gray",
  "from_rgb_additive","from_rgba_premultiplied_const"];
const retiredControlAliases = {
  "theme": [
    "modal_frame",
    "ModalNestedRole",
    "modal_nested_role_frame",
    "workflow_group_card_frame",
    "workflow_shipment_row_frame",
    "settings_help_surface_frame",
    "category_ancestor_frame",
    "section_frame",
    "management_master_table_frame",
    "freight_summary_frame",
    "review_issue_frame",
    "review_issue_card",
    "preview_surface_frame",
    "source_document_frame",
    "management_scroll_style",
    "management_table_scroll_style",
    "action",
    "action_full_width",
    "action_with_min_width",
    "action_style",
    "review_action",
    "paint_button_hover",
    "paint_button_hover_corners",
    "window_control",
    "WindowControl",
    "field_action_palette"
  ],
  "components": [
    "checkbox",
    "checkbox_list_row",
    "management_table_checkbox",
    "management_table_checkbox_rect",
    "scroll_area",
    "ScrollAxes",
    "management_single_list_scroll_height",
    "management_single_list_scroll_height_geometry",
    "modal_ocr_review_rect",
    "modal_master_content_from_ui",
    "modal_master_overlay_ui",
    "modal_master_ocr_body_is_usable",
    "modal_window",
    "ModalEditorBodyGeometry",
    "ModalEditorGeometry",
    "ModalEditorWorkspace",
    "modal_flow_workspace",
    "modal_flow_workspace_measured",
    "modal_workspace_footer_reserve",
    "modal_editor_workspace",
    "ModalMasterLayout",
    "modal_master_layout",
    "ModalMasterWorkspace",
    "modal_master_pixel_aligned_rect",
    "modal_master_content_rect",
    "modal_master_rule_rect",
    "modal_master_action_area",
    "modal_master_workspace_background",
    "modal_master_workspace_finish",
    "modal_master_header_in_rect",
    "modal_master_header_with_toolbar_height",
    "modal_master_toolbar_rule_rect",
    "modal_master_header_toolbar_rect",
    "modal_master_header_with_toolbar_in_rect",
    "modal_master_footer_in_rect",
    "modal_action_row",
    "modal_editor_body_max_height",
    "modal_body_height_budget",
    "modal_list_max_height",
    "modal_plain_body_region",
    "modal_master_body",
    "modal_section_heading_with_rule",
    "modal_master_body_section",
    "modal_section_measured",
    "modal_master_controlled_spacing",
    "nested_modal_window",
    "DecisionWorkspace",
    "decision_workspace",
    "decision_content",
    "decision_window",
    "show_decision_window",
    "modal_backdrop",
    "modal_window_layer",
    "modal_foreground_for_window",
    "modal_labeled_row",
    "modal_labeled_row_with_width",
    "fixed_scroll_panel",
    "management_pending_primary_card",
    "management_summary_card",
    "management_summary_card_value",
    "measured_management_master_card",
    "measured_management_section_card",
    "align_management_primary_card",
    "management_primary_card_min_inner_height",
    "management_card_min_height_for_insets",
    "management_master_table_card_min_inner_height",
    "fill_management_primary_card",
    "FieldKind",
    "field",
    "compact_text_field",
    "flexible_text_field",
    "multiline_text_field",
    "multiline_text_field_sized",
    "money_field",
    "money_field_with_unit",
    "money_edit_text",
    "parse_money_input",
    "management_date_text_field",
    "management_search_field",
    "reception_source_text_edit",
    "reception_recipient_inline_cell_editor",
    "reception_inline_cell_editor",
    "reception_freight_inline_cell_editor",
    "reception_item_numeric_editor",
    "ReceptionNumericCell",
    "reception_numeric_edit_finished",
    "paint_required_input",
    "ManagementSearchChoice",
    "management_dropdown",
    "management_dropdown_full_label",
    "management_status_dropdown",
    "management_dropdown_compact",
    "management_dropdown_values",
    "management_searchable_dropdown",
    "management_dropdown_option",
    "management_dropdown_option_values",
    "management_status_dropdown_option",
    "management_dropdown_compact_option",
    "management_dropdown_trigger",
    "management_dropdown_trigger_full_label",
    "management_table_container_style",
    "management_table_cells",
    "management_table_row_fill",
    "management_table_row_body",
    "management_table_column_rules",
    "paint_management_table_row_background",
    "paint_management_table_row_rule",
    "management_table_scroll_track_rect",
    "management_readonly_table",
    "icon_action_button",
    "management_date_segment",
    "management_date_segment_corners",
    "ManagementDateSegmentEdge",
    "WorkflowFixedAction",
    "WorkflowAuxiliaryAction",
    "workflow_fixed_action",
    "paint_workflow_auxiliary_content"
  ]
};
const retired = [
  "components :: table_header (","components :: table_row (",
  "components :: record_card (","components :: summary_strip (",
  "modal_master_spaced_divider ("
];

// Exclude test modules/functions, not production code after a test module.
// Inline cfg(test) probes remain visible; they cannot hide subsequent code.
function productionTokens(source){
  const tokens=tokenizeRust(source);
  const v=tokens.map(t=>t.kind==="literal"?"<literal>":t.v);
  const out=[];
  for(let i=0;i<tokens.length;i++){
    if(v.slice(i,i+7).join(" ")==="# [ cfg ( test ) ]" &&
       ["mod","fn"].includes(v[i+7])){
      let k=i+8;
      while(k<v.length && v[k]!=="{" && v[k]!==";")k++;
      if(v[k]==="{"){
        let depth=1;k++;
        while(k<v.length&&depth){if(v[k]==="{")depth++;if(v[k]==="}")depth--;k++;}
        if(depth)throw Error("Unclosed cfg(test) item");
        i=k-1;continue;
      }
    }
    out.push({...tokens[i],v:v[i]});
  }
  return out;
}

export function audit(source,file,checkTableContracts=false){
  const tokens=productionTokens(source);
  const v=tokens.map(t=>t.v);
  const hits=[],used=new Map(),paintUsed=new Map(),sharedCalls=new Map();
  const is=(i,pattern)=>v.slice(i,i+pattern.split(" ").length).join(" ")===pattern;
  const flag=(i,rule)=>hits.push({line:tokens[i].line,rule});
  const workflow=file==="workflow_board_ui.rs";
  const workflowCalls=new Map(),workflowPaintUsed=new Map();
  const base=file==="components.rs"||file==="theme.rs"||file==="table/mod.rs";
  const managed=!base&&(tableScreens.has(file)||v.some((x,i)=>
    (x==="components"||x==="table")&&v[i+1]==="::"&&tableMarkers.has(v[i+2])));
  // Match the complete argument list so nested Rect/Stroke expressions
  // cannot turn a specific exception into a general painter exemption.
  const callEnd=open=>{
    let depth=0;
    for(let j=open;j<v.length;j++){
      if(v[j]==="(")depth++;
      if(v[j]===")"&&--depth===0)return j;
    }
    return v.length-1;
  };
  let fn="";
  for(let i=0;i<v.length;i++){
    if(v[i]==="fn"&&/^[A-Za-z_]\w*$/.test(v[i+1]??""))fn=v[i+1];
    for(const pattern of retired)if(is(i,pattern))flag(i,"retired UI entry: "+pattern);
    if(v[i+1]==="::"&&retiredControlAliases[v[i]]?.includes(v[i+2]))
      flag(i,"retired component alias: "+v[i]+"::"+v[i+2]);
    const overlayNative=["Area :: new (","Window :: new (","Modal :: new (",
      "Popup :: menu (","Popup :: context_menu (","Popup :: from_response (","Popup :: new (",
      ". context_menu (",". layer_painter (",". set_sublayer (",
      ". set_modal_layer (",". move_to_top (",
      ". order ( egui :: Order ::",". order ( Order ::",
      "Popup :: open_id (","Popup :: close_all (","Popup :: close_id ("];
    for(const pattern of overlayNative)
      if(file!=="overlay.rs"&&is(i,pattern))flag(i,"raw overlay outside shared manager: "+pattern);
    if(retiredTable.has(v[i]))flag(i,"retired table entry: "+v[i]);
    if(!base&&is(i,"components ::")&&["management_fixed_table_scroll_area","management_register_list_scroll_area","virtual_fixed_rows","management_master_header_begin"].includes(v[i+2]))
      flag(i,"retired screen table assembly: "+v[i+2]);
    for(const [pattern,owners] of extractedControlOwners)
      if(is(i,pattern)&&!owners.has(file+":"+fn))
        flag(i,"raw control outside owning constructor: "+pattern);
    for(const [pattern,owner] of ownedControls)
      if(is(i,pattern)&&file+":"+fn!==owner)
        flag(i,"raw control outside owning component: "+pattern);
    if(file!=="theme.rs"&&paletteConstructors.some(name=>is(i,"Color32 :: "+name+" (")))
      flag(i,"local color construction; define a semantic theme token");
    if(file!=="typography.rs"){
      if(is(i,"RichText :: new ("))
        flag(i,"raw text styling outside typography");
      // Named semantic font-size tokens are valid for custom painting.
      // Literal font sizes and new monospace faces bypass the font roles.
      if(["new","proportional","monospace"].some(name=>is(i,"FontId :: "+name+" ("))
        && /^\d/.test(v[i+4]??""))
        flag(i,"literal font size outside typography");
      if(is(i,"FontId :: monospace (")&&!/^\d/.test(v[i+4]??""))
        flag(i,"local monospace font; use typography data-value roles");
    }
    if(is(i,". set_fonts (")&&file!=="typography.rs")
      flag(i,"global font installation outside typography");
    if(is(i,". set_style (")&&file!=="typography.rs"&&file!=="theme.rs")
      flag(i,"global style installation outside shared theme");
    if((is(i,"table ::")||(is(i,"components ::")&&!tableOnlyCalls.has(v[i+2])))&&v[i+3]==="("){
      const key=file+":"+fn+":"+v[i+2];
      sharedCalls.set(key,(sharedCalls.get(key)||0)+1);
    }
    if(file==="table/mod.rs"){
      const scope=file+":"+fn;
      const required=tableOwnerContracts[scope]||{};
      for(const name of Object.keys(required)){
        const pattern=name.startsWith(".")?". "+name.slice(1)+" (":
          name.includes("::")?name.replace("::"," :: ")+" (":name+" (";
        // Qualified components call or unqualified local call; never a
        // declaration, unrelated method, or arbitrary namespace.
        const matches=name.startsWith(".")||name.includes("::")?is(i,pattern):
          is(i,pattern)&&(!["fn",".","::"].includes(v[i-1])||
            (v[i-1]==="::"&&v[i-2]==="components"&&name==="paint_shipment_detail_surface"));
        if(matches){
          const key=scope+":"+name;
          if(!(name==="paint_shipment_detail_surface"&&v[i-1]==="::"))
            sharedCalls.set(key,(sharedCalls.get(key)||0)+1);
        }
      }
    }
    // Within the shared module, count direct unqualified calls as well.
    // Exclude declarations, methods and qualified calls (already counted above).
    if(file==="components.rs"&&componentContractCalls.has(v[i])&&v[i+1]==="("
      &&!["fn",".","::"].includes(v[i-1])){
      const key=file+":"+fn+":"+v[i];
      sharedCalls.set(key,(sharedCalls.get(key)||0)+1);
    }
    if(workflow){
      const fixed=is(i,"button :: workflow_fixed_action (");
      const wrapper=["workflow_auxiliary_at","workflow_primary_at"].includes(v[i])
        &&v[i+1]==="("&&v[i-1]!=="fn";
      if(fixed||wrapper){
        const key=fn+":"+(fixed?"workflow_fixed_action":v[i]);
        const count=(workflowCalls.get(key)||0)+1;
        workflowCalls.set(key,count);
        if(count>(workflowContracts[key]||0))
          flag(i,"unexpected workflow action call: "+key);
      }
      const local=v[i]==="."&&workflowPaintMethods.has(v[i+1])&&v[i+2]==="(";
      const themed=(is(i,"theme ::")||is(i,"button ::"))&&workflowThemePaintMethods.has(v[i+2])&&v[i+3]==="(";
      if(local||themed){
        const start=local?i+1:i+2;
        const fingerprint=v.slice(start,callEnd(start+1)+1).join(" ");
        const key=fn+":"+fingerprint;
        const count=(workflowPaintUsed.get(key)||0)+1;
        workflowPaintUsed.set(key,count);
        if(count>(workflowPaintAllowance.get(key)||0))
          flag(i,"raw workflow action/paint: "+v[start]+"; use shared workflow renderer");
      }
      if(is(i,"button :: paint_workflow_auxiliary_content ("))
        flag(i,"workflow auxiliary glyph outside shared renderer");
    }
    if(managed){
      if(is(i,"theme :: TABLE_ROW_ALT_BG"))
        flag(i,"local table stripe color; use shared row fill");
      const localPaint=v[i]==="."&&tablePaintMethods.has(v[i+1])&&v[i+2]==="(";
      const genericPaint=is(i,"theme ::")&&
        ["paint_bordered_surface","paint_surface_outline","paint_soft_rule"].includes(v[i+2])&&v[i+3]==="(";
      if(localPaint||genericPaint){
        const start=localPaint?i+1:i+2;
        const fingerprint=v.slice(start,callEnd(start+1)+1).join(" ");
        const key=file+":"+fn+":"+fingerprint;
        const count=(paintUsed.get(key)||0)+1;paintUsed.set(key,count);
        if(count>(paintAllowance.get(key)||0))
          flag(i,"raw table painter: "+v[start]+"; use shared table rendering");
      }
      if(is(i,"from_fn (")){
        const body=v.slice(i+2,callEnd(i+1)).join(" ");
        if(/Rect :: (?:from_min_size|from_min_max|from_center_size|from_two_pos)/.test(body))
          flag(i,"local table cell geometry; use management_table_cells");
      }
    }
    if(!base){
      for(const pattern of controls)if(is(i,pattern)&&
        !(file==="overlay.rs"&&pattern==="Window :: new (")&&
        !(file==="card/mod.rs"&&fn==="preview_surface_frame"&&pattern==="Frame :: new (")&&
        !extractedControlOwners.has(pattern))
        flag(i,"raw shared control: "+pattern);
      for(const pattern of ["theme :: soft_rule (","theme :: divider (",". separator (",
        ". on_hover_text (",". on_hover_text_at_pointer ("])
        if(is(i,pattern))flag(i,"raw divider/tooltip: "+pattern);
      if(is(i,"components :: soft_divider (")){
        const key=file+":"+fn+":"+v[i+4];
        const count=(used.get(key)||0)+1;used.set(key,count);
        if(count>(compact.get(key)||0))flag(i,"unreviewed compact divider: "+key);
      }
      if(headerScreens.has(file)&&is(i,"theme :: TABLE_HEADER_OUTLINE"))
        flag(i,"local managed header outline");
    }
    if(scrollConstructors.some(name=>is(i,"ScrollArea :: "+name+" (")) &&
       !(file==="scrollbar/mod.rs"&&fn==="scroll_area"))
      flag(i,"raw scroll constructor outside shared scroll_area");
    if(file==="components.rs"){
      if((is(i,". on_hover_text (")||is(i,". on_hover_text_at_pointer ("))&&fn!=="overflow_tooltip")
        flag(i,"tooltip outside shared overflow policy");
    }
  }
  if(workflow&&checkTableContracts){
    for(const [key,required] of Object.entries(workflowContracts)){
      const count=workflowCalls.get(key)||0;
      if(count<required)hits.push({line:1,rule:"missing shared workflow contract: "+
        key+" ("+count+"/"+required+")"});
    }
  }
  if(checkTableContracts){
    for(const [scope,required] of Object.entries({...tableContracts,...tableOwnerContracts})){
      if(!scope.startsWith(file+":"))continue;
      for(const [entry,min] of Object.entries(required)){
        const count=sharedCalls.get(scope+":"+entry)||0;
        if(count<min)hits.push({line:1,rule:"missing shared table contract: "+scope+
          " requires "+entry+" ("+count+"/"+min+")"});
      }
    }
  }
  return hits;
}

function selfTest(){
  const cases=[
    ["workflow_board_ui.rs","fn workflow_auxiliary_at(){crate::button::workflow_fixed_action(ui,id,rect,action,enabled);}",0],
    ["workflow_board_ui.rs","fn workflow_primary_at(){crate::button::workflow_fixed_action(ui,id,rect,action,enabled);}",0],
    ["workflow_board_ui.rs","fn workflow_primary_at(){painter.rect_filled(rect,5.0,fill);}",1],
    ["workflow_board_ui.rs","fn other(){painter.rect_stroke(rect,5.0,stroke,kind);}",1],
    ["workflow_board_ui.rs","fn workflow_auxiliary_at(){crate::button::paint_workflow_auxiliary_content(p,r,a,c);}",1],
    ["workflow_board_ui.rs","fn workflow_header(){theme::paint_bordered_surface(p,r,5.0,c,b);}",1],
    ["workflow_board_ui.rs","fn workflow_action_line(){ui.interact(rect,id,sense);}",1],
    ["workflow_board_ui.rs","fn other(){crate::button::action(ui,label,role,enabled);}",1],
    ["workflow_board_ui.rs","fn other(){crate::button::workflow_fixed_action(ui,id,rect,action,enabled);}",1],
    ["workflow_board_ui.rs","fn workflow_compact_rows(){painter.rect_filled(divider,0.0,theme::WORKFLOW_CARD_BORDER);painter.rect_filled(divider,0.0,theme::WORKFLOW_CARD_BORDER);}",1],
    ["workflow_board_ui.rs","#[cfg(test)] fn t(){painter.rect_filled(rect,5.0,fill);} fn workflow_primary_at(){crate::button::workflow_fixed_action(ui,id,rect,action,enabled);}",0],
    ["workflow_board_ui.rs","fn other(){/* painter.rect_filled(r,5.0,c); */ let note=\"crate::button::action(ui,label,role,enabled)\";}",0],
    ["product_manager.rs","fn render(){egui::ScrollArea :: both ( );}",1],
    ["main.rs","fn render(){egui::ScrollArea::both();}",1],
    ["alias_manager.rs","fn show_page(){egui::ScrollArea::vertical();}",1],
    ["new_screen.rs","fn render(){ScrollArea::horizontal();}",1],
    ["theme.rs","fn wrong(){ScrollArea::new([true,true]);}",1],
    ["new_screen.rs","fn render(){ScrollArea::neither();}",1],
    ["components.rs","fn wrong(){egui::ScrollArea::vertical();}",1],
    ["scrollbar/mod.rs","fn scroll_area(){egui::ScrollArea::vertical();egui::ScrollArea::horizontal();egui::ScrollArea::both();}",0],
    ["components.rs","fn scroll_area(){egui::ScrollArea::vertical();}",1],
    ["scrollbar/mod.rs","fn management_scroll_style(){egui::style::ScrollStyle::solid();}",0],
    ["theme.rs","fn wrong(){egui::style::ScrollStyle::solid();}",1],
    ["checkbox/mod.rs","fn checkbox(){egui::Checkbox::new(value,label);}",0],
    ["components.rs","fn checkbox(){egui::Checkbox::new(value,label);}",1],
    ["popup/mod.rs","fn bad(){egui::Window::new(label);}",2],
    ["card/mod.rs","fn preview_surface_frame(){egui::Frame::new();}",0],
    ["new_screen.rs","fn wrong(){theme::section_frame();}",1],
    ["new_screen.rs","fn render(){crate::scrollbar::scroll_area(crate::scrollbar::ScrollAxes::Vertical);}",0],
    ["new_screen.rs","fn wrong(){components::scroll_area(axes);}",1],
    ["alias_manager.rs","fn render(){crate::table::register_rows(ui,height,width,id,draw);}",0],
    ["main.rs","#[cfg(test)] mod tests {fn t(){egui::ScrollArea::vertical();}} fn render(){egui::ScrollArea::horizontal();}",1],
    ["new_screen.rs","fn render(){egui::Button :: new ( \"x\" );}",1],
    ["new_screen.rs","fn render(){ui.on_hover_text(\"x\");}",1],
    ["new_screen.rs","fn render(){egui::CollapsingHeader::new(title);}",1],
    ["components.rs","fn collapsing_header(){egui::CollapsingHeader::new(title);}",0],
    ["components.rs","fn other(){egui::CollapsingHeader::new(title);}",1],
    ["new_screen.rs","fn render(){egui::Slider::new(value,range);}",1],
    ["components.rs","fn ocr_zoom_slider(){egui::Slider::new(value,range);}",0],
    ["new_screen.rs","fn render(){ui.selectable_value(a,b,c);}",1],
    ["components.rs","fn management_dropdown_option_inner(){ui.selectable_value(a,b,c);}",1],
    ["dropdown/mod.rs","fn management_dropdown_option_inner(){ui.selectable_value(a,b,c);}",0],
    ["dropdown/mod.rs","fn other(){ui.selectable_value(a,b,c);}",1],
    ["dropdown/mod.rs","fn management_dropdown_trigger_inner(){egui::Button::new(label);}",0],
    ["dropdown/mod.rs","fn other(){egui::Button::new(label);}",1],
    ["button/mod.rs","fn action_with_palette(){egui::Button::new(label);}",0],
    ["button/mod.rs","fn window_control(){egui::Button::new(label);}",0],
    ["button/mod.rs","fn icon_action_button(){egui::Button::new(label);}",0],
    ["button/mod.rs","fn other(){egui::Button::new(label);}",1],
    ["button/mod.rs","fn calendar_day(){egui::Button::new(label);}",0],
    ["button/mod.rs","fn dispatch_origin(){egui::Button::new(label);}",0],
    ["new_screen.rs","fn wrong(){components::money_field(ui,v,true);}",1],
    ["new_screen.rs","fn wrong(){theme::action(ui,label,role,true);}",1],
    ["components.rs","fn wrong(){egui::Button::new(label);}",1],
    ["button/mod.rs","fn action_with_palette(){egui::TextEdit::singleline(value);}",1],
    ["input/mod.rs","fn singleline_editor(){egui::TextEdit::singleline(value);}",0],
    ["input/mod.rs","fn multiline_editor(){egui::TextEdit::multiline(value);}",0],
    ["input/mod.rs","fn other(){egui::TextEdit::singleline(value);}",1],
    ["input/mod.rs","fn other(){egui::TextEdit::multiline(value);}",1],
    ["input/mod.rs","fn singleline_editor(){egui::Button::new(label);}",1],
    ["new_screen.rs","fn render(){Color32::from_rgb(1,2,3);}",1],
    ["theme.rs","const INK:Color32=Color32::from_rgb(1,2,3);",0],
    ["new_screen.rs","fn render(){FontId::proportional(16.0);}",1],
    ["new_screen.rs","fn render(){FontId::proportional(typography::FONT_BODY);}",0],
    ["new_screen.rs","fn render(){FontId::monospace(typography::FONT_BODY);}",1],
    ["typography.rs","fn render(){FontId::monospace(16.0);}",0],
    ["new_screen.rs","fn render(){RichText::new(label);}",1],
    ["typography.rs","fn render(){RichText::new(label);}",0],
    ["new_screen.rs","fn render(){ctx.set_fonts(fonts);}",1],
    ["new_screen.rs","fn render(){response.on_hover_ui(|ui|{});}",1],
    ["new_screen.rs","fn render(){egui::Area::new(id);}",1],
    ["overlay.rs","fn area(){egui::Area::new(id);}",0],
    ["new_screen.rs","fn render(){egui::Popup::menu(&response);}",1],
    ["new_screen.rs","fn render(){ctx.set_sublayer(a,b);}",1],
    ["new_screen.rs","fn render(){memory.set_modal_layer(layer);}",1],
    ["overlay.rs","fn activate_modal(){memory.set_modal_layer(layer);}",0],
    ["components.rs","fn nested(){modal_window(ctx,title,id,area).order(egui::Order::Foreground);}",1],
    ["new_screen.rs","fn render(){components::section_divider(ui);}",0],
    ["new_screen.rs","fn render(){components::soft_divider(ui);}",1],
    ["settings_view.rs","fn show_account_rows(){components::soft_divider(ui);}",0],
    ["settings_view.rs","fn show_account_rows(){components::soft_divider(ui);components::soft_divider(ui);}",1],
    ["settings_view.rs","fn other(){components::soft_divider(ui);}",1],
    ["components.rs","fn wrong(){egui::TextEdit::singleline(value);}",1],
    ["components.rs","fn singleline_editor(){egui::TextEdit::singleline(value);}",1],
    ["components.rs","fn multiline_editor(){egui::TextEdit::multiline(value);}",1],
    ["components.rs","fn overflow_tooltip(){response.on_hover_text(full);}",0],
    ["components.rs","fn wrong(){response.on_hover_text(full);}",1],
    ["new_screen.rs","// egui::Button::new(\nfn a(){let x=r#\"ui.on_hover_text(x)\"#;}",0],
    ["new_screen.rs","#[cfg(test)] mod tests {fn a(){egui::Button::new(\"x\");}} fn render(){ui.checkbox(x);}",1],
    ["new_screen.rs","fn a(){#[cfg(test)] record();ui.checkbox(x);}",1],
    ["excel_manager.rs","fn render(){painter.rect_filled(r,0.0,theme::TABLE_HEADER_OUTLINE);}",2],
    ["new_screen.rs","fn render(){components::table_header(ui,x);}",1],
    ["product_manager.rs","fn render(){painter.line_segment([r.left_bottom(),r.right_bottom()],stroke);}",1],
    ["product_manager.rs","fn render(){theme::paint_bordered_surface(p,r,0,c,b);}",1],
    ["product_manager.rs","fn render(){theme::paint_soft_rule(p,r);}",1],
    ["order_history_manager.rs","fn show_page(){theme::paint_bordered_surface(row_ui.painter(),block_rect,theme::ORDER_HISTORY_RECIPIENT_BLOCK_RADIUS,theme::PANEL,theme::DIVIDER);}",1],
    ["order_history_manager.rs","fn show_page(){components::paint_shipment_detail_surface(row_ui,block_rect,theme::PANEL,false);}",0],
    ["new_screen.rs","fn render(){components::shipment_detail_cells(b,w,g);painter.rect_filled(r,0.0,ink);}",1],
    ["excel_manager.rs","fn render(){painter.rect_filled(r,0.0,theme::DIVIDER);}",1],
    ["product_manager.rs","fn render(){let c=theme::TABLE_ROW_ALT_BG;}",1],
    ["product_manager.rs","fn render(){let c=std::array::from_fn(|i|Rect::from_min_size(p,w));}",1],
    ["client_manager.rs","fn render(){let c=std::array::from_fn(|i|i.to_string());}",0],
    ["new_screen.rs","fn render(){crate::table::management_table_cells(p,w,h,g);painter.rect_filled(r,0.0,ink);}",1],
    ["new_screen.rs","fn render(){crate::table::management_table_cells(p,w,h,g);crate::table::management_table_column_rules(ui,r,c,g);}",0],
    ["new_screen.rs","fn render(){components::management_row_striped(ui,id,c,t,i);}",1],
    ["order_history_manager.rs","fn paint_history_branch(){painter.line_segment([egui::pos2(x,center),egui::pos2(tip,center)],stroke);}",0],
    ["order_history_manager.rs","fn paint_history_branch(){painter.line_segment([egui::pos2(x,center),egui::pos2(tip,center)],stroke);painter.line_segment([egui::pos2(x,center),egui::pos2(tip,center)],stroke);painter.line_segment([egui::pos2(x,center),egui::pos2(tip,center)],stroke);}",1],
    ["order_history_manager.rs","fn paint_history_branch(){painter.line_segment([a,b],stroke);}",1],
    ["freight_manager.rs","fn show_groups(){painter.rect_filled(row_rect,0.0,background);}",1],
    ["freight_manager.rs","fn other(){painter.rect_filled(row_rect,0.0,background);}",1],
    ["main.rs","fn a(){table::fixed_row();} fn source_section(){painter.rect_filled(body_rect,0.0,theme::CANVAS);}",0],
    ["main.rs","fn a(){table::fixed_row();} fn source_section(){painter.rect_filled(body_rect,0.0,theme::CANVAS);painter.rect_filled(body_rect,0.0,theme::CANVAS);}",1],
    ["main.rs","fn a(){table::fixed_row();} fn source_section(){painter.rect_filled(body_rect,0.0,theme::INPUT);}",1],
    ["main.rs","fn a(){table::fixed_row();} fn other(){painter.rect_filled(body_rect,0.0,theme::CANVAS);}",1],
    ["model_group_manager.rs","fn show_page(){table::register_rows();painter.rect_filled(separator,0.0,theme::BORDER);}",0],
    ["model_group_manager.rs","fn show_page(){table::register_rows();painter.rect_filled(separator,0.0,theme::BORDER);painter.rect_filled(row_rect,0.0,theme::BORDER);}",1],
    ["product_manager.rs","#[cfg(test)] fn t(){painter.rect_filled(r,0.0,c);} fn render(){crate::table::management_table_cells(p,w,h,g);}",0],
  ];
  for(const [file,source,count] of cases)assert.equal(audit(source,file).length,count,source);
  // This synthetic file omits both the list and the detail renderer.
  const missingProduct=audit("fn show_page(){}","product_manager.rs",true);
  assert.equal(missingProduct.filter(hit=>hit.rule.includes("product_manager.rs:show_page requires ")).length,8);
  assert.equal(missingProduct.filter(hit=>hit.rule.includes("product_manager.rs:show_detail requires ")).length,1);
  assert.equal(missingProduct.length,9);
  assert.equal(audit("fn show(){crate::table::show(ui,h,c);crate::table::fixed_rows(ui,v,id,h,n,c);crate::table::row_scope(ui,id,r,c);components::management_row(ui,id,c,t,i);components::management_client_header(ui,h,c,l,w,s);}","client_chooser.rs",true).length,0);
  assert.equal(audit("fn workflow_primary_at(){}","workflow_board_ui.rs",true).length,5);
  assert.equal(audit("fn workflow_auxiliary_at(){crate::button::workflow_fixed_action(ui,id,r,a,e);} fn workflow_primary_at(){crate::button::workflow_fixed_action(ui,id,r,a,e);} fn workflow_header(){workflow_auxiliary_at();workflow_auxiliary_at();} fn workflow_action_line(){workflow_primary_at();workflow_auxiliary_at();}","workflow_board_ui.rs",true).length,0);
  const missing=(source,file,scope)=>audit(source,file,true)
    .filter(hit=>hit.rule.startsWith("missing shared table contract: "+scope+" requires "));
  const historyScope="order_history_manager.rs:history_detail_cells";
  assert.equal(missing("fn history_detail_cells(){components::shipment_detail_cells(b,w,g);}","order_history_manager.rs",historyScope).length,0);
  assert.equal(missing("fn history_detail_cells(){crate::table::management_table_cells(p,w,h,g);}","order_history_manager.rs",historyScope).length,1);
  const commonScope="components.rs:shipment_detail_cells";
  assert.equal(missing("fn shipment_detail_cells(){let c=shipment_detail_content(b);management_table_cells(p,w,h,g);}","components.rs",commonScope).length,0);
  assert.equal(missing("fn shipment_detail_cells(){shipment_detail_content(b);} fn management_table_cells(){}","components.rs",commonScope).length,1);
  const tableScope="table/mod.rs:fixed_row";
  const fixed="fn fixed_row(){row_scope();management_table_cells();paint_management_table_row_background();management_table_column_rules();paint_management_table_row_rule();}";
  assert.equal(missing(fixed,"table/mod.rs",tableScope).length,0);
  assert.equal(missing(fixed.replace("paint_management_table_row_rule();",""),"table/mod.rs",tableScope).length,1);
  assert.equal(missing(fixed.replace("management_table_cells();","fake::management_table_cells();"),"table/mod.rs",tableScope).length,1);
  assert.equal(missing("fn show(){finish(ui,header,body);}","table/mod.rs","table/mod.rs:show").length,0);
  assert.equal(missing("fn show(){} fn unrelated(){finish(ui,header,body);}","table/mod.rs","table/mod.rs:show").length,1);
  assert.equal(missing("fn finish(){outline_bounds(h,b);painter.rect_stroke(r,x,s,k);}","table/mod.rs","table/mod.rs:finish").length,0);
  assert.equal(missing("fn finish(){outline_bounds(h,b);}","table/mod.rs","table/mod.rs:finish").length,1);
  assert.equal(missing("fn show_page(){components::show();}","product_manager.rs","product_manager.rs:show_page").length,8);
  console.log("EGUI_SHARED_UI_SELFTEST_PASS",cases.length+18);
}

function main(){
  selfTest();
  if(process.argv.includes("--self-test"))return;
  const root=resolve(fileURLToPath(new URL("..",import.meta.url)));
  const walk=p=>readdirSync(p,{withFileTypes:true}).flatMap(e=>
    e.isDirectory()?walk(resolve(p,e.name)):e.name.endsWith(".rs")?[resolve(p,e.name)]:[]);
  const files=walk(resolve(root,"egui-app/src"));
  let failures=0;
  for(const path of files){
    const file=relative(resolve(root,"egui-app/src"),path).replaceAll("\\","/");
    for(const hit of audit(readFileSync(path,"utf8"),file,true)){
      failures++;console.error(file+":"+hit.line+" "+hit.rule);
    }
  }
  console.log("EGUI_SHARED_UI_AUDIT",JSON.stringify({files:files.length,failures}));
  if(failures)process.exitCode=1;
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url))main();
