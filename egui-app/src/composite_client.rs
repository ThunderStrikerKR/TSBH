//! Shared client summaries, tier badges and client rows.
use eframe::egui::{self,Align,Color32,Layout,Rect,Response,Sense,Stroke,Ui,Vec2};
use crate::{theme,typography};
use crate::ui_primitives::{text_exceeds_rect,overflow_region_tooltip};
use crate::table::{paint_table_galley,management_client_column_widths,management_client_column_edges,management_table_row_fill,paint_management_table_row_rule,management_master_column_rules};
use crate::card::paint_reception_client_summary_surface;
use crate::text_fit::fitted_client_name;

/// Select a row in an editor's left-hand list. This is NOT a table
/// record: it has a 14pt item title and a separate 12pt state caption,
/// following the original 48pt message-template chooser hierarchy.
/// Painting is confined to the reserved fixed-height rectangle, so hover
/// and selected styles never change the following row positions.
pub fn management_chooser_row(
    ui:&mut Ui,title:&str,status:&str,selected:bool
)->Response{
    let width=ui.available_width().max(1.0);
    let (rect,response)=ui.allocate_exact_size(
        Vec2::new(width,theme::MANAGEMENT_CHOOSER_ROW_HEIGHT),Sense::click()
    );
    let background=if selected {
        theme::SELECTION.linear_multiply(0.22)
    }else if response.hovered(){theme::INPUT}else{theme::PANEL};
    let painter=ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect,theme::CONTROL_RADIUS as f32,background);
    if selected{
        painter.rect_stroke(
            rect.shrink(0.5),theme::CONTROL_RADIUS as f32,
            Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::SELECTION),
            egui::StrokeKind::Inside
        );
    }
    let primary=typography::management_table_cell_galley(
        ui,title,typography::ManagementTableCellRole::ClientName
    );
    let secondary=typography::client_table_metadata_galley(
        ui,status,theme::MUTED
    );
    let lines=primary.size().y+secondary.size().y+theme::SPACE_4;
    let x=rect.left()+theme::SPACE_12;
    let y=rect.center().y-lines/2.0;
    let content_rect=Rect::from_min_max(
        egui::pos2(x,rect.top()),egui::pos2(rect.right()-theme::SPACE_12,rect.bottom())
    );
    let ink=painter.with_clip_rect(content_rect);
    ink.galley(egui::pos2(x,y),primary,theme::TEXT);
    ink.galley(
        egui::pos2(x,y+lines-secondary.size().y),
        secondary,theme::MUTED
    );
    if !selected {
        crate::button::paint_button_hover(
            ui,&response,theme::CONTROL_RADIUS as f32
        );
    }
    response
}

/// Read-only presentation of a canonical client. Used by both management
/// rows and the compact Reception card; never modifies billing or matching.
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct ClientDisplaySummary{
    pub name:String,pub tier:String,pub address:String,
    pub phone:String,pub mobile:String,pub conditions:String,
}

pub fn client_tier_label(tier:&str)->&'static str{
    match tier{
        "wholesale"=>"도매","semi"=>"반도매",
        "retail"=>"소매","special"=>"특수",_=>"미지정"
    }
}

pub fn client_exception_terms(client:&serde_json::Value)->String{
    let mut flags=Vec::new();
    if client["payment"].as_str()==Some("POSTPAID"){flags.push("후불");}
    if client["defaultApplyVat"].as_bool().unwrap_or(false){
        flags.push("부가세 포함");
    }
    if client["shippingFeeExcluded"].as_bool().unwrap_or(false){
        flags.push("운임 청구 제외");
    }
    if client["active"]==serde_json::Value::Bool(false){
        flags.push("거래 중지");
    }
    flags.join(" / ")
}

impl ClientDisplaySummary{
    pub fn from_client(client:&serde_json::Value)->Self{
        let field=|key:&str|client.get(key)
            .and_then(serde_json::Value::as_str).unwrap_or("").to_owned();
        let name=client.get("name").and_then(serde_json::Value::as_str)
            .filter(|s|!s.trim().is_empty())
            .or_else(||client.get("displayName").and_then(serde_json::Value::as_str))
            .unwrap_or("").to_owned();
        Self{name,tier:field("tier"),address:field("address"),
            phone:field("phone"),mobile:field("mobile"),
            conditions:client_exception_terms(client)}
    }
    pub fn primary_phone(&self)->&str{
        if self.phone.trim().is_empty(){&self.mobile}else{&self.phone}
    }
    pub fn secondary_mobile(&self)->&str{
        if self.phone.trim().is_empty() || self.mobile.trim().is_empty()
            || self.phone==self.mobile{""}else{&self.mobile}
    }
}

/// The tier palette belongs to the shared design system. Badge geometry is
/// painted inside the client column and cannot expand the row's layout.
fn client_tier_colors(tier:&str)->(Color32,Color32) {
    match tier {
        "wholesale"=>(theme::CLIENT_TIER_WHOLESALE_BG,theme::CLIENT_TIER_WHOLESALE_TEXT),
        "semi"=>(theme::CLIENT_TIER_SEMI_BG,theme::CLIENT_TIER_SEMI_TEXT),
        "retail"=>(theme::CLIENT_TIER_RETAIL_BG,theme::CLIENT_TIER_RETAIL_TEXT),
        "special"=>(theme::CLIENT_TIER_SPECIAL_BG,theme::CLIENT_TIER_SPECIAL_TEXT),
        _=>(theme::RAISED,theme::SECONDARY),
    }
}

/// Shared measured badge: the longest label (반도매) controls width.
/// Both the management table and Reception render through this painter.
fn paint_client_tier_badge(ui:&Ui,cell:Rect,label:&str,tier:&str)->Rect{
    let (fill,ink)=client_tier_colors(tier);
    let galley=typography::client_table_metadata_galley(ui,label,ink);
    let widest=typography::client_table_metadata_galley(ui,"반도매",ink);
    let width=(widest.size().x.max(galley.size().x)+theme::SPACE_8)
        .min(cell.width().max(0.0));
    let height=widest.size().y.max(galley.size().y)+theme::SPACE_4;
    let badge=Rect::from_min_size(
        egui::pos2(cell.left(),cell.center().y-height/2.0),
        Vec2::new(width,height)
    );
    ui.painter().with_clip_rect(cell).rect_filled(
        badge,theme::CONTROL_RADIUS as f32,fill
    );
    paint_table_galley(
        ui,badge,badge.left()+(badge.width()-galley.size().x)/2.0,galley,ink
    );
    badge
}

/// Keep names and the first exceptional condition adjacent on a compact
/// card. The management table's address column may expand; the intake card's
/// name must never use that empty width as a spacer.
fn client_card_name_limit(
    available:f32,name_width:f32,terms_width:f32,has_terms:bool
)->f32{
    let available=available.max(0.0);
    if !has_terms || available<=160.0{return name_width.min(available);}
    let reserved=terms_width.min(available*0.45)+theme::SPACE_8;
    name_width.min((available-reserved).max(0.0))
}

/// Keep the exact client surface when workflow permissions change.
pub fn reception_client_summary_card_editable(
    ui:&mut Ui,rect:Rect,linked:bool,
    summary:Option<&ClientDisplaySummary>,fallback:&str,editable:bool,
)->bool{
    paint_reception_client_summary_surface(ui,rect);
    if rect.width()<40.0||rect.height()<18.0{return false;}
    let inset=theme::SPACE_12.min(rect.width()/6.0);
    let button_label=if linked{"변경"}else{"거래처 찾기"};
    let button_width=if linked{76.0}else{112.0};
    let has_button=rect.width()>button_width+2.0*inset+48.0
        && rect.height()>=theme::ACTION_STANDARD_HEIGHT;
    let button_x=rect.right()-inset-button_width;
    let text_right=if has_button{
        button_x-theme::SPACE_8
    }else{rect.right()-inset};
    let left=rect.left()+inset;
    // Only one visible line: badge, client name, exceptional terms.
    // Center it vertically alongside the standard change/search action.
    // Clip inside the existing card so short panes cannot overlap the source.
    let row=Rect::from_min_max(
        egui::pos2(left,rect.top()),
        egui::pos2(text_right,rect.bottom())
    ).intersect(rect);
    if row.width()>0.0&&row.height()>0.0{
        let mut x=row.left();
        if let Some(data)=summary.filter(|c|!c.tier.is_empty()){
            let badge=paint_client_tier_badge(
                ui,row,client_tier_label(&data.tier),&data.tier
            );
            x=badge.right()+theme::SPACE_8;
        }
        if x<row.right(){
            let name=summary.map(|data|data.name.as_str())
                .filter(|value|!value.trim().is_empty()).unwrap_or(fallback);
            let terms=summary.map(|data|data.conditions.as_str()).unwrap_or("");
            let available=row.right()-x;
            let terms_galley=typography::client_table_metadata_galley(
                ui,terms,theme::MUTED
            );
            let full=typography::management_table_cell_galley(
                ui,name,typography::ManagementTableCellRole::ClientName
            );
            let name_limit=client_card_name_limit(
                available,full.size().x,terms_galley.size().x,
                !terms.is_empty()
            );
            // Truncate the name only when genuinely necessary. Short names
            // occupy their measured width, not the whole flexible subcell:
            // the next condition starts directly after the visible name.
            let name_overflow=full.size().x>name_limit;
            let shown_galley=if name_overflow{
                let shown=fitted_client_name(ui,name,name_limit);
                typography::management_table_cell_galley(
                    ui,&shown,typography::ManagementTableCellRole::ClientName
                )
            }else{full};
            let name_width=shown_galley.size().x.min(name_limit);
            if name_width>0.0{
                let name_cell=Rect::from_min_max(
                    egui::pos2(x,row.top()),
                    egui::pos2(x+name_width,row.bottom())
                );
                paint_table_galley(ui,name_cell,x,shown_galley,theme::TEXT);
                overflow_region_tooltip(ui,name_cell,
                    ui.make_persistent_id("source-client-name-overflow"),name_overflow,name);
            }
            if !terms.is_empty(){
                let terms_x=x+name_width+theme::SPACE_8;
                if terms_x<row.right(){
                    let terms_cell=Rect::from_min_max(
                        egui::pos2(terms_x,row.top()),row.right_bottom()
                    );
                    let overflow=paint_table_galley(
                        ui,terms_cell,terms_x,terms_galley,theme::MUTED
                    );
                    overflow_region_tooltip(ui,terms_cell,
                        ui.make_persistent_id("source-client-terms-overflow"),overflow,terms);
                }
            }
        }
    }
    if !has_button{return false;}
    let button_rect=Rect::from_min_size(
        egui::pos2(button_x,
            rect.center().y-theme::ACTION_STANDARD_HEIGHT/2.0),
        Vec2::new(button_width,theme::ACTION_STANDARD_HEIGHT)
    );
    let clip=button_rect.intersect(ui.clip_rect());
    ui.scope_builder(
        egui::UiBuilder::new()
            .id_salt("source-client-card-action")
            .max_rect(button_rect)
            .layout(Layout::left_to_right(Align::Center)),
        |buttons|{
            buttons.set_clip_rect(clip);
            crate::button::action_with_min_width(
                buttons,button_label,theme::ButtonRole::Standard,
                editable,button_width
            ).clicked()
        }
    ).inner
}

/// Four client data columns. The visible index determines the alternating
/// background; the dedicated management_client_header owns header rendering.
pub fn management_row(
    ui:&mut Ui,row_id:&str,columns:[(&str,&str);4],tier:&str,visible_index:usize
) -> Response {
    let width=ui.available_width().max(theme::MANAGEMENT_CLIENT_TABLE_MIN_WIDTH);
    let (rect,response)=ui.allocate_exact_size(
        Vec2::new(width,theme::MANAGEMENT_CLIENT_ROW_HEIGHT),Sense::click()
    );
    let bg=if response.hovered() {theme::INPUT}
        else {management_table_row_fill(visible_index)};
    ui.painter().rect_filled(rect,0.0,bg);
    // Paint the 1pt row rule inside its allocated rect. A centered
    // line_segment on bottom() was half-clipped and looked inconsistent
    // against the master's crisp 2pt header band at fractional DPI.
    paint_management_table_row_rule(ui.painter(),rect);

    let mut x=rect.left();
    let column_widths=management_client_column_widths(rect.width());
    for (i,(primary,secondary)) in columns.iter().enumerate() {
        let col_width=column_widths[i];
        let cell=Rect::from_min_max(
            egui::pos2(x+theme::SPACE_12,rect.top()),
            egui::pos2(x+col_width-theme::SPACE_12,rect.bottom())
        );
        // Every header and value shares the column's leading inset.
        // Only the exact field that exceeds its painted rectangle gets a tooltip.
        if i==3 {
            let badge=paint_client_tier_badge(
                ui,cell,primary,tier
            );
            let (_,ink)=client_tier_colors(tier);
            let badge_size=typography::client_table_metadata_galley(ui,primary,ink).size();
            overflow_region_tooltip(ui,badge,
                ui.make_persistent_id(("management-row-hover",row_id,i,0)),
                text_exceeds_rect(badge_size,badge.intersect(ui.clip_rect())),primary);
            if !secondary.is_empty(){
                let second=Rect::from_min_max(
                    egui::pos2((badge.right()+theme::SPACE_8).min(cell.right()),cell.top()),
                    cell.max
                );
                let galley=typography::client_table_metadata_galley(ui,secondary,theme::MUTED);
                let overflow=paint_table_galley(ui,second,second.left(),galley,theme::MUTED);
                overflow_region_tooltip(ui,second,
                    ui.make_persistent_id(("management-row-hover",row_id,i,1)),overflow,secondary);
            }
        }else{
            // A secondary contact is a separate field, not clipped primary text.
            let visible=if primary.is_empty(){secondary}else{primary};
            if !visible.is_empty(){
                let metadata=primary.is_empty() && i==0;
                let galley=if metadata{
                    typography::client_table_metadata_galley(ui,visible,theme::MUTED)
                }else{
                    typography::management_table_cell_galley(
                        ui,visible,if i==0{
                            typography::ManagementTableCellRole::ClientName
                        }else{
                            typography::ManagementTableCellRole::Text
                        }
                    )
                };
                let overflow=paint_table_galley(
                    ui,cell,cell.left(),galley,
                    if metadata{theme::MUTED}else{theme::TEXT}
                );
                overflow_region_tooltip(ui,cell,
                    ui.make_persistent_id(("management-row-hover",row_id,i,0)),overflow,visible);
            }
        }
        x+=col_width;
    }
    management_master_column_rules(
        ui,rect,
        management_client_column_edges(rect.left(),rect.width())
    );
    crate::button::paint_button_hover(ui,&response,0.0);
    response
}

#[cfg(test)]
mod client_display_summary_tests{
    use super::*;
    use serde_json::json;
    #[test]
    fn short_client_names_do_not_create_extra_space_before_conditions(){
        assert_eq!(client_card_name_limit(400.0,62.0,94.0,true),62.0);
        assert_eq!(client_card_name_limit(400.0,340.0,94.0,true),298.0);
        assert_eq!(client_card_name_limit(400.0,62.0,94.0,false),62.0);
        assert_eq!(client_card_name_limit(130.0,200.0,94.0,true),130.0);
    }
    #[test]
    fn shared_tier_and_default_conditions(){
        let data=json!({"name":"테스트 상사","tier":"semi",
            "payment":"PREPAID","defaultApplyVat":false,
            "shippingFeeExcluded":false,"active":true,
            "phone":"02-1111-2222","mobile":"010-1111-2222"});
        let summary=ClientDisplaySummary::from_client(&data);
        assert_eq!(client_tier_label(&summary.tier),"반도매");
        assert_eq!(summary.conditions,"");
        assert_eq!(summary.primary_phone(),"02-1111-2222");
        assert_eq!(summary.secondary_mobile(),"010-1111-2222");
    }
    #[test]
    fn exceptional_conditions_are_shared(){
        let data=json!({"payment":"POSTPAID","defaultApplyVat":true,
            "shippingFeeExcluded":true,"active":false});
        assert_eq!(client_exception_terms(&data),
            "후불 / 부가세 포함 / 운임 청구 제외 / 거래 중지");
    }
}
