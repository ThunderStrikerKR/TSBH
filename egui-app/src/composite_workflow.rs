//! Shared workflow card and shipment detail presentation.
use eframe::egui::{self,Color32,Rect,Response,Sense,Ui,Vec2};
use crate::{theme,typography,source_highlight};
use crate::ui_primitives::text_exceeds_rect;
use crate::card::paint_shipment_detail_surface;
use crate::table::{paint_table_galley,shipment_detail_cells,shipment_detail_product_galley,shipment_detail_block_height,paint_shipment_detail_products,workflow_shipment_columns};
#[cfg(test)]
use crate::table::management_inline_list_table_header;

/// Compact workflow money uses the shared semantic font and one-touch
/// geometry. The allocated price slot, rather than its text clip, bounds
/// the highlight so the currency suffix retains the same 4pt clearance.
pub fn workflow_compact_money(
    ui:&Ui,price:Rect,price_ink:Rect,bottom_y:f32,amount:&str,emphasis:bool,state:theme::TextState
)->bool{
    let color=if emphasis{crate::one_touch::INK}else{theme::text_ink(theme::TextRole::Value,state)};
    let galley=ui.painter().layout_no_wrap(
        amount.to_owned(),typography::data_value_font(
            typography::DataValueKind::Money,typography::DataValueContext::CompactCard
        ),color
    );
    let pos=egui::pos2(price_ink.right()-galley.size().x,bottom_y-galley.size().y);
    let bounds=Rect::from_min_size(pos,galley.size());
    let highlight=crate::one_touch::value_rect(bounds,price);
    if emphasis{crate::one_touch::background(ui,highlight);}
    let clip=price_ink.intersect(ui.clip_rect());
    let clipped=text_exceeds_rect(galley.size(),clip);
    ui.painter().with_clip_rect(clip).galley(pos,galley,color);
    if emphasis{crate::one_touch::outline(ui,highlight);}
    clipped
}

/// Noninteractive retail marker occupies the existing unused action slot.
pub fn workflow_idle_caption(ui:&Ui,rect:Rect,caption:&str){
    let galley=ui.painter().layout_no_wrap(caption.to_owned(),
        egui::FontId::proportional(typography::FONT_METADATA),theme::text_ink(theme::TextRole::Label,theme::TextState::Normal));
    let x=rect.right()-galley.size().x;
    let _=paint_table_galley(ui,rect,x,galley,theme::text_ink(theme::TextRole::Label,theme::TextState::Normal));
}

/// Use the existing semantic numeric/body layout for shipment charges.
fn workflow_freight_text_job(text:&str,ink:Color32)->egui::text::LayoutJob{
    let mut parts=Vec::new();
    for (line_index,line) in text.split('\n').enumerate(){
        if line_index>0{parts.push(("\n",None));}
        for (index,part) in line.split(" / ").enumerate(){
            if index>0{parts.push((" / ",None));}
            if let Some(number)=part.strip_suffix("원"){
                parts.push((number,Some(typography::DataValueKind::Money)));
                parts.push(("원",None));
            }else{parts.push((part,None));}
        }
    }
    let mut job=typography::inline_body_values(&parts);
    for section in &mut job.sections{section.format.color=ink;}
    job
}

/// Same shipment detail surface, multiline product renderer and cell
/// coordinates as Order History. Selection only supplies the palette.
pub fn workflow_shipment_row(
    ui:&mut Ui,name:&str,summary:&str,fill:Color32,chosen:bool,previewed:bool,freight:Option<&str>,freight_emphasis:bool
)->egui::Response{
    let outer=ui.available_width();
    let (widths,gap)=workflow_shipment_columns(outer);
    let preview_ink=if chosen{theme::WORKFLOW_SELECTED_ROW_TEXT}
        else if previewed{source_highlight::ACTIVE_TEXT}else{theme::text_ink(theme::TextRole::Value,theme::TextState::Normal)};
    let summary_ink=if chosen{theme::text_ink(theme::TextRole::Value,theme::TextState::Normal)}else{preview_ink};
    let freight_ink=if freight_emphasis{crate::one_touch::INK}else{theme::text_ink(theme::TextRole::Value,theme::TextState::Normal)};
    let name_galley=shipment_detail_product_galley(ui,widths[0],name,preview_ink);
    let product_galley=shipment_detail_product_galley(ui,widths[1],summary,summary_ink);
    let freight_text=freight.unwrap_or("—");
    let mut freight_job=workflow_freight_text_job(&freight_text.replace(" / ","\n"),freight_ink);
    freight_job.wrap.max_width=(widths[2]-2.0*theme::SPACE_8).max(1.0);
    freight_job.wrap.break_anywhere=true;
    let freight_galley=ui.fonts_mut(|fonts|fonts.layout_job(freight_job));
    let height=shipment_detail_block_height(name_galley.size().y.max(product_galley.size().y)
        .max(freight_galley.size().y));
    let (block,response)=ui.allocate_exact_size(Vec2::new(outer,height),Sense::hover());
    let [name_rect,product_rect,freight_rect]=shipment_detail_cells(block,&widths,gap);
    if previewed && !chosen{
        // Keep the existing preview fill; the linked outline replaces the
        // ordinary row border rather than layering over it.
        crate::highlight::paint_parts(ui.painter(),block,crate::highlight::Geometry::Shipment,
            Some(fill),crate::highlight::palette(crate::highlight::Role::LinkedPreview).stroke);
    }else{
        paint_shipment_detail_surface(ui,block,fill,chosen);
    }
    paint_shipment_detail_products(ui,name_rect,name_galley,preview_ink);
    paint_shipment_detail_products(ui,product_rect,product_galley,summary_ink);
    let x=freight_rect.right()-theme::SPACE_8-freight_galley.size().x;
    let bounds=Rect::from_min_size(
        egui::pos2(x,freight_rect.center().y-freight_galley.size().y/2.0),freight_galley.size());
    let highlight=crate::one_touch::value_rect(bounds,freight_rect);
    if freight_emphasis{crate::one_touch::background(ui,highlight);}
    paint_table_galley(ui,freight_rect,x,freight_galley,freight_ink);
    if freight_emphasis{crate::one_touch::outline(ui,highlight);}
    response
}

#[cfg(test)]
mod workflow_freight_typography_tests{
    use super::*;
    #[test]
    fn nested_shipment_header_uses_master_height_and_shared_detail_columns(){
        let ctx=egui::Context::default();
        typography::install(&ctx);
        theme::install(&ctx);
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.set_width(390.0);
                let header=management_inline_list_table_header(ui);
                assert_eq!(header.header_rect.height(),theme::MANAGEMENT_TABLE_HEADER_HEIGHT);
                assert_eq!(header.scrollbar_gutter,0.0);
                assert_eq!(header.header_content_rect,header.header_rect);
                let area=crate::table::master_header_begin(ui,header.header_rect,header.header_content_rect);
                let (widths,gap)=workflow_shipment_columns(header.table_width);
                let headings=shipment_detail_cells(area,&widths,gap);
                let block=Rect::from_min_size(
                    egui::pos2(header.header_rect.left(),header.header_rect.bottom()+8.0),
                    Vec2::new(header.table_width,90.0)
                );
                let body=shipment_detail_cells(block,&widths,gap);
                for (heading,cell) in headings.iter().zip(&body){
                    assert_eq!(heading.left(),cell.left());
                    assert_eq!(heading.right(),cell.right());
                }
                assert!((body[2].right()-(block.right()-theme::SPACE_12)).abs()<0.01);
            });
        });
    }
    #[test]
    fn freight_uses_semantic_money_and_stable_layout_in_both_states(){
        let text="4,000원 / —";
        let regular=workflow_freight_text_job(text,theme::TEXT);
        let emphasized=workflow_freight_text_job(text,crate::one_touch::INK);
        assert_eq!(regular.text,text);
        assert_eq!(regular.sections.len(),emphasized.sections.len());
        let money=typography::data_value_font(typography::DataValueKind::Money,typography::DataValueContext::Form);
        assert_eq!(regular.sections[0].format.font_id,money);
        assert_eq!(regular.sections[0].format.line_height,Some(22.0));
        assert_eq!(regular.sections[1].format.font_id.family,egui::FontFamily::Proportional);
        for (a,b) in regular.sections.iter().zip(&emphasized.sections){
            assert_eq!(a.byte_range,b.byte_range);
            assert_eq!(a.format.font_id,b.format.font_id);
            assert_eq!(a.format.line_height,b.format.line_height);
        }
        let cod=workflow_freight_text_job("착불",theme::TEXT);
        assert_eq!(cod.sections[0].format.font_id.family,egui::FontFamily::Proportional);
    }
}

/// Same state precedence in draft bundles and confirmed orders.
pub fn workflow_shipment_hover(ui:&Ui,response:&Response,chosen:bool,previewed:bool){
    if !chosen && !previewed{
        // The common hover painter already records the action origin.
        crate::button::paint_button_hover(ui,response,crate::highlight::Geometry::Control);
    }else{
        crate::floating_notice::remember_response(ui.ctx(),response);
    }
}
