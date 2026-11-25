//! egui manual source assignment, powered ONLY by the existing Rust
//! apply_manual_classification reducer. No second classification algorithm.
//!
//! This first stage provides source-block ownership, row/item structure,
//! quantity validation and applying an unconfirmed preview. The chooser never
//! edits a confirmed order or directly writes the Tauri business database.

use std::{sync::{Arc,mpsc::{self,Receiver}},thread,time::{SystemTime,UNIX_EPOCH},sync::atomic::{AtomicU64,Ordering}};
use eframe::egui::{self,Context};
use serde_json::{json,Value};
use crate::{
    cell_navigation::CellNavigationKey,
    gateway::{BandoGateway,ReadOperation,
        rules::{scope_manual_model_ids,scope_recognition_rows_for_order,scope_reception_edit_ids}},
    preview_store::WorkingReception,
    theme::{self,ButtonRole},
    typography,components,source_highlight,
    item_editor::{self,ItemEditor,ProductClickIntent},
};

fn text<'a>(value:&'a Value,key:&str)->&'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}
fn list<'a>(value:&'a Value,key:&str)->&'a [Value] {
    value.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}
fn unique_id()->String {
    static NEXT:AtomicU64=AtomicU64::new(0);
    let nanos=SystemTime::now().duration_since(UNIX_EPOCH)
        .map(|d|d.as_nanos()).unwrap_or(0);
    format!("egui-manual-{}-{nanos}-{}",std::process::id(),
        NEXT.fetch_add(1,Ordering::Relaxed))
}
fn rule(gateway:&dyn BandoGateway,payload:Value)->Result<Value,String> {
    gateway.execute(ReadOperation::ManualClassification,payload)
}

/// A UI-only reconstruction of the native order shape for Rust manual rules.
/// Recipients and evidence are the ACTUAL Rust analysis, not local parsing.
pub fn native_order(
    gateway:&dyn BandoGateway, row:&WorkingReception, recognition:&Value
)->Result<Value,String> {
    if row.analysis_raw.as_deref()!=Some(row.raw.as_str()) ||
       !recognition.get("recipients").is_some_and(Value::is_array) {
        return Err("발주서 분석이 끝난 뒤 수동 분류를 진행하세요.".into());
    }
    let now=chrono::Utc::now().to_rfc3339();
    let initialized=gateway.execute(ReadOperation::NewReception,json!({
        "action":"new_reception","id":row.id,"clientId":row.client_id,
        "orderType":row.order_type,"now":now
    }))?;
    let mut order=initialized.get("order").filter(|v|v.is_object())
        .ok_or("발주서를 준비하지 못했습니다.")?.clone();
    order["raw"]=json!(row.raw);
    order["analysisRaw"]=json!(row.raw);
    let mut candidates=recognition.get("candidates")
        .cloned().unwrap_or_else(||json!([]));
    let mut recipients=recognition.get("recipients")
        .cloned().unwrap_or_else(||json!([]));
    if let Some(saved)=row.manual_model.as_ref() {
        let mut model=saved.get("model").filter(|v|v.is_object())
            .cloned().ok_or("저장된 수동 분류 작업본이 올바르지 않습니다.")?;
        scope_manual_model_ids(&row.id,&mut model)?;
        recipients=model.get("rows").cloned().unwrap_or_else(||json!([]));
        order["manualClassification"]=json!({
            "blocks":model.get("blocks"),"assignments":model.get("assignments")
        });
    }
    let recipient_rows=recipients.as_array_mut()
        .ok_or("분석한 수취인 정보를 확인할 수 없습니다.")?;
    scope_recognition_rows_for_order(&row.id,recipient_rows,&mut candidates)?;
    order["analysisEvidence"]=candidates;
    order["recipients"]=recipients;
    Ok(order)
}

/// Build the same native order shell for a manually entered draft. No source
/// text, recognition result or evidence blocks are fabricated. The exact
/// same manual reducers and worksheet operate on the empty source model.
pub fn native_direct_order(
    gateway:&dyn BandoGateway,row:&WorkingReception
)->Result<Value,String>{
    if !row.raw.trim().is_empty(){
        return Err("원문이 있는 발주서는 직접 입력 모드로 열 수 없습니다.".into());
    }
    let now=chrono::Utc::now().to_rfc3339();
    let initialized=gateway.execute(ReadOperation::NewReception,json!({
        "action":"new_reception","id":row.id,"clientId":row.client_id,
        "orderType":row.order_type,"now":now
    }))?;
    let mut order=initialized.get("order").filter(|v|v.is_object())
        .ok_or("직접 입력을 시작하지 못했습니다.")?.clone();
    order["id"]=json!(row.id);
    order["raw"]=json!("");
    order["orderType"]=json!(row.order_type);
    order["clientId"]=json!(row.client_id);
    order["recipients"]=json!([]);
    if let Some(saved)=row.manual_model.as_ref().filter(|saved|
        text(saved,"mode")=="DIRECT"
    ){
        if saved.get("sourceRaw").and_then(Value::as_str)!=Some("") ||
            saved.get("orderType").and_then(Value::as_str)!=Some(row.order_type.as_str()) ||
            saved.get("clientId").and_then(Value::as_str)!=row.client_id.as_deref(){
            return Err("직접 입력 작업본과 접수 정보가 일치하지 않습니다.".into());
        }
        let mut model=saved.get("model").filter(|v|v.is_object()).cloned()
            .ok_or("저장된 직접 입력 작업본이 올바르지 않습니다.")?;
        scope_manual_model_ids(&row.id,&mut model)?;
        if !list(&model,"blocks").is_empty() ||
            model.get("assignments").and_then(Value::as_object)
                .is_none_or(|assignments|!assignments.is_empty()) ||
            saved.get("assignmentOrder").and_then(Value::as_array)
                .is_none_or(|keys|!keys.is_empty()){
            return Err("직접 입력 작업본에는 발주원문 블록이나 원문 배정이 존재할 수 없습니다.".into());
        }
        order["recipients"]=model.get("rows").cloned().unwrap_or_else(||json!([]));
        order["manualClassification"]=json!({
            "blocks":model.get("blocks").cloned().unwrap_or_else(||json!([])),
            "assignments":model.get("assignments").cloned().unwrap_or_else(||json!({}))
        });
    }
    if let Some(fields)=order.as_object_mut(){
        fields.remove("analysisRaw");
        fields.remove("analysisEvidence");
    }
    Ok(order)
}

/// Each decision is computed by native Rust off the egui frame and is
/// paired with both recipient ID and exact address to reject stale matches.
type RemoteRow=(String,String,Result<Value,String>);

#[derive(Clone)]
struct ManualHistoryFrame{
    model:Value,
    assignment_order:Vec<String>,
    remote_rows:Vec<RemoteRow>,
    view_quote:Option<Value>,
    selected_recipient:usize,
    selected_item:Option<String>,
    selected_field:String,
}

#[derive(Clone,Debug,PartialEq,Eq)]
struct ManualCellTarget{
    row:usize,
    item:Option<String>,
    field:String,
}

#[derive(Default)]
struct ManualAssignmentGesture{
    source:Option<String>,
    cell:Option<ManualCellTarget>,
    dragging:Option<String>,
    /// egui confirms a drag only after its native movement threshold.
    /// Until then the pointer press is a pending drag preview, not a drop.
    drag_moved:bool,
    drag_anchor_offset:Option<egui::Vec2>,
    drag_layout_width:f32,
}

impl ManualAssignmentGesture{
    fn clear(&mut self){*self=Self::default();}
    fn source_click(&mut self,id:String)->Option<(String,ManualCellTarget)>{
        if let Some(cell)=self.cell.take(){
            self.source=None;
            Some((id,cell))
        }else{
            if self.source.as_deref()==Some(id.as_str()){self.source=None;}
            else{self.source=Some(id);}
            None
        }
    }
    fn cell_click(&mut self,cell:ManualCellTarget)->Option<(String,ManualCellTarget)>{
        if let Some(source)=self.source.take(){
            self.cell=None;
            Some((source,cell))
        }else{
            self.cell=Some(cell);
            None
        }
    }
    fn start_drag(
        &mut self,id:String,anchor_offset:Option<egui::Vec2>,layout_width:f32
    ){
        self.clear();
        self.dragging=Some(id);
        self.drag_anchor_offset=anchor_offset;
        self.drag_layout_width=layout_width;
    }
    fn mark_drag_moved(&mut self){
        if self.dragging.is_some(){self.drag_moved=true;}
    }
    fn cancel_pending_drag(&mut self){
        self.dragging=None;
        self.drag_moved=false;
        self.drag_anchor_offset=None;
        self.drag_layout_width=0.0;
    }
    fn finish_drag(&mut self,target:Option<ManualCellTarget>)->Option<(String,ManualCellTarget)>{
        let id=self.dragging.take()?;
        self.drag_moved=false;
        self.drag_anchor_offset=None;
        self.drag_layout_width=0.0;
        target.map(|cell|(id,cell))
    }
}

fn manual_assignment_payload(
    model:&Value,id:&str,target:&ManualCellTarget,pending:Option<&ManualCellTarget>
)->Result<Value,&'static str>{
    if model["assignments"].get(id).is_some(){
        return Err("이미 배정된 원문입니다. 먼저 연결을 해제하세요.");
    }
    let block=list(model,"blocks").iter().find(|b|text(b,"id")==id)
        .ok_or("원문 블록을 확인하세요.")?;
    let field=text(block,"field");
    if !manual_cell_allowed(pending,target) ||
        !manual_source_assignment_allowed(false,None,pending.and_then(|c|c.item.as_deref()),field){
        return Err("선택한 상품의 수량을 먼저 확정하세요.");
    }
    if !manual_cell_matches_source(target.item.is_some(),&target.field,field){
        return Err("원문 항목과 같은 종류의 입력 셀을 선택하세요.");
    }
    let row=list(model,"rows").get(target.row).ok_or("배송 건을 확인하세요.")?;
    let mut cell=json!({"recipientId":row.get("id"),"field":target.field});
    if let Some(item)=target.item.as_ref(){
        if !list(row,"items").iter().any(|v|text(v,"id")==item){
            return Err("상품 셀을 확인하세요.");
        }
        cell["itemId"]=json!(item);
    }
    Ok(json!({"id":id,"cell":cell}))
}

/// Hit test uses the real shared cell Rect and viewport clip. The pointer may
/// be captured by the source during a drag, so Response::hovered is not used.
fn manual_assignment_drop_hit(ui:&egui::Ui,rect:egui::Rect,pointer:Option<egui::Pos2>)->bool{
    ui.is_enabled() && pointer.is_some_and(|pos|rect.intersect(ui.clip_rect()).contains(pos))
}

/// Return every visible worksheet target that accepts this exact source block.
/// The same payload validator is used again at drop time, so recommendations
/// cannot drift from type, assignment, or pending-quantity business rules.
fn manual_assignment_recommendations(
    model:&Value,id:&str,pending:Option<&ManualCellTarget>
)->Vec<ManualCellTarget>{
    manual_cell_targets(model).into_iter().filter(|target|
        manual_assignment_payload(model,id,target,pending).is_ok()
    ).collect()
}

/// A translucent fill marks every valid destination; the stronger outline
/// marks only the cell currently under the pointer.
fn paint_manual_assignment_recommendation(
    ui:&egui::Ui,rect:egui::Rect,hovered:bool
){
    let clip=rect.intersect(ui.clip_rect());
    if clip.width()<=0.0 || clip.height()<=0.0{return;}
    let color=theme::SELECTION;
    let fill=egui::Color32::from_rgba_unmultiplied(
        color.r(),color.g(),color.b(),if hovered{42}else{20}
    );
    let painter=ui.painter().with_clip_rect(clip);
    painter.rect_filled(rect,theme::CONTROL_RADIUS,fill);
    painter.rect_stroke(
        rect,theme::CONTROL_RADIUS,
        egui::Stroke::new(if hovered{2.0}else{1.25},color),
        egui::StrokeKind::Inside
    );
}

/// Paint only the selected source evidence while preserving the original
/// document's glyph positions, wrapping, rounded block geometry and colors.
/// The pointer remains attached to the exact point grabbed by the user.
fn paint_manual_source_drag_preview(
    ctx:&Context,
    raw:&str,
    spans:&[source_highlight::LinkedSpan],
    source_id:&str,
    pointer:egui::Pos2,
    anchor_offset:egui::Vec2,
    layout_width:f32,
    selected_recipient:Option<&str>,
    active:Option<&source_highlight::LinkKey>,
){
    let Some(span)=spans.iter().find(|span|span.source_id.as_deref()==Some(source_id))
        else{return};
    let Some(value)=raw.get(span.start..span.end)
        else{return};
    if value.is_empty(){return;}

    // The source is unassigned when a drag can begin. Keep all surrounding
    // glyphs transparent but retain their advances so wrapping and the grabbed
    // block's position match the original source document exactly.
    let assigned_ids=std::collections::HashSet::new();
    let style=source_highlight::manual_source_block_style(span,active,&assigned_ids);
    let font=egui::FontId::proportional(typography::FONT_RECEPTION_SOURCE_TEXT);
    let mut job=egui::text::LayoutJob::default();
    job.wrap.max_width=layout_width.max(1.0);
    if span.start>0 {
        job.append(&raw[..span.start],0.0,egui::TextFormat{
            font_id:font.clone(),color:egui::Color32::TRANSPARENT,
            ..Default::default()
        });
    }
    job.append(value,0.0,egui::TextFormat{
        font_id:font.clone(),color:style.text,..Default::default()
    });
    if span.end<raw.len() {
        job.append(&raw[span.end..],0.0,egui::TextFormat{
            font_id:font,color:egui::Color32::TRANSPARENT,
            ..Default::default()
        });
    }
    let galley=ctx.fonts_mut(|fonts|fonts.layout_job(job));
    let Some(original_geometry)=source_highlight::manual_source_block_visual(
        raw,&galley,egui::Pos2::ZERO,spans,source_id,
        selected_recipient,active,&assigned_ids
    ) else{return};
    let Some(original_bounds)=original_geometry.rects.iter().copied()
        .reduce(|left,right|left.union(right)) else{return};

    let desired_block_min=pointer-anchor_offset;
    let translation=desired_block_min-original_bounds.min;
    let galley_pos=egui::Pos2::ZERO+translation;
    let painter=ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,egui::Id::new("manual-source-drag-preview")
    ));
    for rect in &original_geometry.rects {
        painter.rect_filled(
            rect.translate(translation),original_geometry.style.radius,
            original_geometry.style.fill
        );
    }
    painter.galley(galley_pos,galley,theme::TEXT);
    // Match the source document's paint order: glyphs first, then the outline.
    for rect in &original_geometry.rects {
        painter.rect_stroke(
            rect.translate(translation),original_geometry.style.radius,
            egui::Stroke::new(
                original_geometry.style.border_width,original_geometry.style.border
            ),
            egui::StrokeKind::Inside
        );
    }
}

fn item_quantity_pending(item:&Value)->bool{
    let has_product=item.get("productId").is_some_and(|value|!value.is_null()) ||
        !text(item,"name").trim().is_empty();
    let confirmed=item.get("quantityConfirmed").and_then(Value::as_bool)==Some(true);
    has_product && !confirmed
}

fn model_pending_quantity_count(model:&Value)->usize{
    list(model,"rows").iter().map(|row|
        list(row,"items").iter().filter(|item|item_quantity_pending(item)).count()
    ).sum()
}

fn model_has_pending_quantity(model:&Value)->bool{
    model_pending_quantity_count(model)>0
}

/// While a product has just been selected, React locks every other cell
/// until its quantity is confirmed. Keep that lock tied to the exact cell.
fn manual_cell_allowed(
    pending:Option<&ManualCellTarget>,target:&ManualCellTarget
)->bool{
    pending.is_none_or(|cell|cell==target)
}

/// React's useBlock permits releasing an assignment on the same pending item,
/// or assigning that item's quantity block. It rejects all other source moves.
fn manual_source_assignment_allowed(
    assigned:bool,
    assigned_item:Option<&str>,
    pending_item:Option<&str>,
    source_field:&str,
)->bool{
    let Some(pending_item)=pending_item else{return true};
    if assigned {
        assigned_item==Some(pending_item)
    }else{
        source_field=="quantity"
    }
}

fn push_manual_history(
    history:&mut Vec<ManualHistoryFrame>,frame:ManualHistoryFrame
){
    history.push(frame);
    if history.len()>100 {history.remove(0);}
}

fn manual_cell_key(cell:&ManualCellTarget)->String{
    format!("{}:{}:{}",cell.row,cell.item.as_deref().unwrap_or(""),cell.field)
}
fn manual_cell_targets(model:&Value)->Vec<ManualCellTarget>{
    let mut cells=Vec::new();
    for (row_index,row) in list(model,"rows").iter().enumerate(){
        for field in ["name","address","mobile","phone","note"]{
            cells.push(ManualCellTarget{
                row:row_index,item:None,field:field.into()
            });
        }
        for item in list(row,"items"){
            let id=text(item,"id").to_owned();
            for field in ["product","quantity","unitPrice"]{
                cells.push(ManualCellTarget{
                    row:row_index,item:Some(id.clone()),field:field.into()
                });
            }
        }
    }
    cells
}

/// Resolve movement by *visible worksheet geometry*. Recipient fields are
/// the same four rows as Reception; Up/Down follow those rows,
/// Left/Right switch mobile/phone, and Tab still follows all five fields.
/// Crossing shipment boundaries and the product worksheet remain intact.
/// remain a three-column worksheet with vertical item-row navigation.
fn manual_navigation_target(
    model:&Value,current:&ManualCellTarget,key:CellNavigationKey,
    pending:Option<&ManualCellTarget>
)->Option<ManualCellTarget>{
    let rows=list(model,"rows");
    let row=rows.get(current.row)?;
    let next=match key{
        CellNavigationKey::Left|CellNavigationKey::Right=>{
            // The approved Reception contact row is two adjacent cells.
            // Other recipient fields span one entire row.
            if current.item.is_none(){
                let next=match (current.field.as_str(),key){
                    ("mobile",CellNavigationKey::Right)=>"phone",
                    ("phone",CellNavigationKey::Left)=>"mobile",
                    _=>return None,
                };
                return manual_cell_allowed(pending,&ManualCellTarget{
                    row:current.row,item:None,field:next.into()
                }).then(||ManualCellTarget{
                    row:current.row,item:None,field:next.into()
                });
            }
            let fields=&["product","quantity","unitPrice"];
            let index=fields.iter().position(|field|*field==current.field)?;
            let adjacent=if key==CellNavigationKey::Left{
                index.checked_sub(1)?
            }else{index+1};
            ManualCellTarget{
                row:current.row,item:current.item.clone(),
                field:fields.get(adjacent)?.to_string()
            }
        }
        CellNavigationKey::Up|CellNavigationKey::Down=>{
            if let Some(item_id)=current.item.as_deref(){
                let items=list(row,"items");
                let index=items.iter().position(|item|text(item,"id")==item_id)?;
                let adjacent=if key==CellNavigationKey::Up{
                    index.checked_sub(1)?
                }else{index+1};
                let item=items.get(adjacent)?;
                ManualCellTarget{
                    row:current.row,item:Some(text(item,"id").to_owned()),
                    field:current.field.clone()
                }
            }else{
                // Match the actual four Reception rows:
                // name / address / [mobile | phone] / note.
                let up=key==CellNavigationKey::Up;
                let (target_row,target_field)=match (current.field.as_str(),up){
                    ("name",true)=>(current.row.checked_sub(1)?,"note"),
                    ("name",false)=>(current.row,"address"),
                    ("address",true)=>(current.row,"name"),
                    ("address",false)=>(current.row,"mobile"),
                    ("mobile"|"phone",true)=>(current.row,"address"),
                    ("mobile"|"phone",false)=>(current.row,"note"),
                    ("note",true)=>(current.row,"mobile"),
                    ("note",false)=>(current.row+1,"name"),
                    _=>return None,
                };
                rows.get(target_row)?;
                ManualCellTarget{
                    row:target_row,item:None,field:target_field.into()
                }
            }
        }
        CellNavigationKey::Next|CellNavigationKey::Previous=>{
            let cells=manual_cell_targets(model);
            let index=cells.iter().position(|cell|cell==current)?;
            let adjacent=if key==CellNavigationKey::Previous{
                index.checked_sub(1)?
            }else{index+1};
            cells.get(adjacent)?.clone()
        }
    };
    manual_cell_allowed(pending,&next).then_some(next)
}

/// Examine raw key events BEFORE TextEdit consumes arrow keys. Keep IME
/// preedit state across frames, not merely the frame with the first event.
fn manual_navigation_key_before_editor(
    events:&[egui::Event],ime_active:&mut bool
)->Option<(egui::Key,egui::Modifiers,CellNavigationKey)>{
    let mut ime_event=false;
    for event in events{
        match event{
            egui::Event::Ime(egui::ImeEvent::Preedit(value))=>{
                *ime_active=!value.is_empty();
                ime_event=true;
            }
            egui::Event::Ime(egui::ImeEvent::Commit(_))
            |egui::Event::Ime(egui::ImeEvent::Disabled)=>{
                *ime_active=false;
                ime_event=true;
            }
            egui::Event::Ime(_)=>{ime_event=true;}
            _=>{}
        }
    }
    if *ime_active || ime_event {return None;}
    events.iter().find_map(|event|{
        let egui::Event::Key{key,pressed:true,modifiers,..}=event else{
            return None;
        };
        // Modified arrow shortcuts (e.g. Ctrl+Left) are not worksheet
        // movement; IME and application shortcuts retain ownership.
        if modifiers.command || modifiers.ctrl || modifiers.alt{
            return None;
        }
        let navigation=match key{
            egui::Key::ArrowUp=>CellNavigationKey::Up,
            egui::Key::ArrowDown=>CellNavigationKey::Down,
            egui::Key::ArrowLeft=>CellNavigationKey::Left,
            egui::Key::ArrowRight=>CellNavigationKey::Right,
            egui::Key::Enter=>CellNavigationKey::Next,
            egui::Key::Tab=>{
                if modifiers.shift{
                    CellNavigationKey::Previous
                }else{CellNavigationKey::Next}
            }
            _=>return None,
        };
        Some((*key,*modifiers,navigation))
    })
}

fn manual_row_has_data(row:&Value,assignments:&Value)->bool{
    ["name","address","mobile","phone","note"].iter()
        .any(|field|!text(row,field).trim().is_empty()) ||
    list(row,"items").iter().any(|item|{
        ["name","raw"].iter().any(|field|!text(item,field).trim().is_empty()) ||
        ["productId","variantId","quantity","unitPrice","manualUnitPrice"]
            .iter().any(|field|item.get(*field).is_some_and(|value|!value.is_null()))
    }) ||
    text(row,"dispatchOrigin")=="STORE" ||
    text(row,"freightCode")=="020" ||
    row.get("shippingCharge").and_then(Value::as_i64).is_some_and(|amount|amount>0) ||
    row.get("remoteDelivery").is_some_and(|decision|
        decision.get("manual").and_then(Value::as_bool)==Some(true)
    ) ||
    assignments.as_object().is_some_and(|map|map.values().any(|cell|
        text(cell,"recipientId")==text(row,"id")
    ))
}

/// Keep the worksheet focused on one delivery at a time while retaining
/// every shipment in the Rust manual model. The selected index is clamped
/// defensively if an asynchronous resize completed in the previous frame.
fn visible_manual_shipment(selected:usize,total:usize)->Option<usize>{
    (total>0).then(||selected.min(total.saturating_sub(1)))
}

fn parse_manual_row_count(raw:&str)->Result<usize,String>{
    if raw.is_empty() || !raw.bytes().all(|byte|byte.is_ascii_digit()){
        return Err("배송 건수는 1~100 사이의 정수로 입력하세요.".into());
    }
    let count=raw.parse::<usize>()
        .map_err(|_|"배송 건수를 확인하세요.".to_owned())?;
    if !(1..=100).contains(&count){
        return Err("배송 건수는 1~100건입니다.".into());
    }
    Ok(count)
}

fn next_missing_recipient_cell(
    model:&Value,current_row:usize,current_field:&str
)->Option<ManualCellTarget>{
    let fields=["name","address","mobile"];
    let current_field_index=fields.iter().position(|field|*field==current_field)
        .unwrap_or(0);
    for (row_index,row) in list(model,"rows").iter().enumerate().skip(current_row){
        for (field_index,field) in fields.iter().enumerate(){
            if row_index==current_row && field_index<=current_field_index {continue;}
            if text(row,field).trim().is_empty(){
                return Some(ManualCellTarget{
                    row:row_index,item:None,field:(*field).to_owned()
                });
            }
        }
    }
    None
}

fn resolve_remote_rows(gateway:&dyn BandoGateway,model:&Value)->Vec<RemoteRow>{
    refresh_remote_rows(gateway,model,&Value::Null,&[])
}

fn reusable_remote_decision<'a>(previous_model:&Value,previous_rows:&'a [RemoteRow],
    row:&Value)->Option<&'a Value>{
    let id=text(row,"id");
    let previous=list(previous_model,"rows").iter()
        .find(|old|!id.is_empty() && text(old,"id")==id)?;
    // The native resolver depends on address and explicit remote override.
    // Never reuse an error, a different recipient, or a changed override.
    if previous["remoteDelivery"]!=row["remoteDelivery"]{return None;}
    remote_for(previous_rows,row)?.as_ref().ok()
}

fn refresh_remote_rows(gateway:&dyn BandoGateway,model:&Value,
    previous_model:&Value,previous_rows:&[RemoteRow])->Vec<RemoteRow>{
    list(model,"rows").iter().map(|row|{
        let id=text(row,"id").to_owned();
        let address=text(row,"address").to_owned();
        let decision=if address.trim().is_empty(){Ok(Value::Null)}
        else if let Some(cached)=reusable_remote_decision(previous_model,previous_rows,row){
            Ok(cached.clone())
        }else{gateway.execute(ReadOperation::ResolveRemoteDeliveryArea,json!({
            "recipient":row
        }))};
        (id,address,decision)
    }).collect()
}
fn remote_for<'a>(rows:&'a [RemoteRow],recipient:&Value)
    ->Option<&'a Result<Value,String>>
{
    let id=text(recipient,"id");
    let address=text(recipient,"address");
    rows.iter().find(|(key,source,_)|key==id && source==address)
        .map(|(_,_,decision)|decision)
}

// Read-only preview from the SAME native quote as the intake pane.
// No arithmetic or default tariff is calculated in the worksheet renderer.
fn manual_view_quote(
    gateway:&dyn BandoGateway,order:&Value,model:&Value,keys:&[String],
    client_id:Option<&str>,previous:Option<&Value>
)->Option<Value>{
    let id=client_id?;
    let preview=rule(gateway,json!({
        "action":"preview_manual","order":order,"model":model,"assignmentOrder":keys
    })).ok()?;
    let updated=preview.get("order")?;
    let state=gateway.load_business_state().ok()?;
    let client=list(&state,"clients").iter().find(|client|text(client,"id")==id)?;
    let inputs=price_input_snapshot(updated);
    let basis=crate::pricing_basis::snapshot(&state,client,updated);
    if let Some(previous)=previous.filter(|quote|
        quote.get("manualViewInput")==Some(&inputs) &&
        quote.get("manualViewBasis")==Some(&basis)
    ){
        return Some(previous.clone());
    }
    let mut quote=gateway.calculate_reception_edit_quote(
        json!({"order":updated,"state":&state,"client":client})).ok()?;
    quote["orderId"]=order["id"].clone();
    quote["manualViewInput"]=inputs;
    quote["manualViewBasis"]=basis;
    Some(quote)
}

enum Outcome {
    Initial { order:Value, model:Value, assignment_order:Vec<String>,
        remote_rows:Vec<RemoteRow>,view_quote:Option<Value> },
    Updated { model:Value, assignment_order:Vec<String>,
        remote_rows:Vec<RemoteRow>,view_quote:Option<Value> },
    Applied { order:Value, model:Value, assignment_order:Vec<String>,
        quote:Option<Value>,quote_error:Option<String> },
}
pub enum Event {
    Closed,
    Applied { order_id:String,raw:String,order_type:String,client_id:Option<String>,
        direct_input:bool,model:Value,assignment_order:Vec<String>,order:Value,
        quote:Option<Value>,quote_error:Option<String> },
}

/// A direct in-place edit from the reception pane. The visible cell owns
/// focus and typing; the Rust business reducer remains the only mutation
/// authority. No modal or UI-side price/quantity calculation is used.
#[derive(Clone,Debug)]
pub struct InlineRequest {
    pub recipient_id:String,
    pub item_id:Option<String>,
    pub field:String,
    pub value:Value,
}
/// Native pricing depends on these ordered line/recipient values and
/// shipment/VAT options, NOT on recipient name, phone, note or OCR source
/// assignment metadata. This is a comparison of Rust business inputs only;
/// no monetary arithmetic is performed in the UI.
fn price_input_snapshot(order:&Value)->Value{
    let root=|name:&str|order.get(name).cloned().unwrap_or(Value::Null);
    let recipients=list(order,"recipients").iter().map(|recipient|{
        let get=|name:&str|recipient.get(name).cloned().unwrap_or(Value::Null);
        let items=list(recipient,"items").iter().map(|item|{
            let item_get=|name:&str|item.get(name).cloned().unwrap_or(Value::Null);
            json!({
                "id":item_get("id"),"productId":item_get("productId"),
                "variantId":item_get("variantId"),"quantity":item_get("quantity"),
                "unitPrice":item_get("unitPrice"),
                "manualUnitPrice":item_get("manualUnitPrice"),
                "origin":item_get("origin"),"freightCode":item_get("freightCode"),
                "shippingGroupId":item_get("shippingGroupId")
            })
        }).collect::<Vec<_>>();
        json!({
            "id":get("id"),"address":get("address"),
            "dispatchOrigin":get("dispatchOrigin"),
            "freightCode":get("freightCode"),
            "shippingCharges":get("shippingCharges"),
            "shippingCharge":get("shippingCharge"),
            "remoteDelivery":get("remoteDelivery"),
            "vatOverride":get("vatOverride"),
            "vatRateBps":get("vatRateBps"),
            "items":items
        })
    }).collect::<Vec<_>>();
    json!({
        "applyVat":root("applyVat"),
        "vatRateBps":root("vatRateBps"),
        "freightCode":root("freightCode"),
        "recipients":recipients
    })
}

/// Convert a previously calculated, complete native quote back to the
/// worker response shape. No rules are duplicated and no amounts changed.
/// Explicit edits that affect any pricing input always call native Rust.
fn reuse_native_quote(saved:Option<&Value>,updated:&Value)->Option<Value>{
    let saved=saved?;
    if !saved.get("authorityRevision").is_some_and(Value::is_u64) ||
       !saved.get("pendingRecipients").is_some_and(Value::is_array) ||
       !saved.get("freightLines").is_some_and(Value::is_array) ||
       !saved.get("linePrices").and_then(Value::as_array)
           .is_some_and(|lines|lines.iter().all(|row|row.get("amount").is_some())) {
        return None;
    }
    let mut order=updated.clone();
    order["shippingFee"]=saved.get("shippingFee").cloned().unwrap_or(Value::Null);
    order["shippingVersion"]=saved.get("shippingVersion").cloned().unwrap_or(Value::Null);
    Some(json!({
        "order":order,"authorityRevision":saved["authorityRevision"],
        "pricingBasis":saved.get("pricingBasis"),
        "totals":{
            "subtotal":saved.get("subtotal"),
            "vat":saved.get("vatAmount"),"total":saved.get("total"),
            "pendingRecipients":saved.get("pendingRecipients")
        },
        "linePrices":saved.get("linePrices"),
        "freightLines":saved.get("freightLines")
    }))
}

fn add_selected_manual_product(
    gateway:&dyn BandoGateway,order:&Value,payload:&Value,selection:&Value,
    recipient_id:&str,keys:&Value
)->Result<Value,String>{
        // Build the row only in the temporary native model. A failed catalog
        // lookup or confirmation never returns an empty row to the order.
        let pid=selection["productId"].as_str()
            .filter(|id|!id.is_empty()).ok_or("추가할 상품을 선택하세요.")?;
        let vid=selection["variantId"].as_str()
            .filter(|id|!id.is_empty()).ok_or("추가할 상품 옵션을 선택하세요.")?;
        let state=if std::env::var_os("BANDO_EGUI_BUSINESS_DB")
            .is_some_and(|value|!value.is_empty()){
            gateway.load_business_state()?
        }else{gateway.load_catalog()?};
        let product=state["products"].as_array().into_iter().flatten()
            .find(|product|text(product,"id")==pid)
            .ok_or("선택한 상품이 최신 상품 목록에 없습니다.")?;
        let variants=product["variants"].as_array()
            .ok_or("등록된 판매 구성이 없습니다.")?;
        if product["requiresOption"]==true || product["active"]==false
            || variants.len()!=1 || text(&variants[0],"id")!=vid{
            return Err("등록된 독립 판매 상품을 다시 선택하세요.".into());
        }
        let iid=payload["itemId"].clone();
        let added=rule(gateway,payload.clone())?;
        let cell=json!({"recipientId":recipient_id,"itemId":iid,"field":"product"});
        let selected=rule(gateway,json!({
            "action":"set_manual_product","order":order,"model":added["model"],
            "assignmentOrder":keys,"cell":cell,"value":text(product,"name"),
            "product":product,"state":state,"preserveExistingQuantity":true
        }))?;
        let item=selected["model"]["rows"].as_array().into_iter().flatten()
            .find(|row|text(row,"id")==recipient_id)
            .and_then(|row|row["items"].as_array()).into_iter().flatten()
            .find(|item|item["id"]==iid)
            .ok_or("추가된 상품 행을 확인할 수 없습니다.")?;
        if text(item,"productId")!=pid || text(item,"variantId")!=vid{
            return Err("선택한 상품을 주문 품목으로 확정하지 못했습니다.".into());
        }
        let quantity_cell=json!({"recipientId":recipient_id,
            "itemId":iid,"field":"quantity"});
        let draft=rule(gateway,json!({
            "action":"set_manual_quantity_draft","model":selected["model"],
            "cell":quantity_cell,"value":"1"
        }))?;
        rule(gateway,json!({
            "action":"confirm_manual_quantity","model":draft["model"],
            "cell":quantity_cell,"assignmentOrder":keys
        }))
}

pub fn apply_inline(
    gateway:&dyn BandoGateway,source:&WorkingReception,change:&InlineRequest
)->Result<Event,String>{
    // A visible legacy/initial recognition cell may still carry its
    // document-local ID. Normalize the request with the SAME native rule
    // as the worksheet, including top-level add/remove and approval targets.
    // Already scoped IDs and direct/manual UUIDs remain unchanged.
    let (recipient_id,item_id)=scope_reception_edit_ids(
        &source.id,&change.recipient_id,change.item_id.as_deref()
    );
    let change=&InlineRequest{recipient_id,item_id,
        field:change.field.clone(),value:change.value.clone()};
    let direct_input=source.raw.trim().is_empty() &&
        source.manual_model.as_ref().is_some_and(|saved|text(saved,"mode")=="DIRECT");
    let order=if direct_input{
        native_direct_order(gateway,source)?
    }else{
        let evidence=source.analysis_evidence.as_ref()
            .ok_or("최초 발주서 분석 결과가 없습니다.")?;
        native_order(gateway,source,evidence)?
    };
    let previous=source.manual_model.as_ref();
    // A complete persisted worksheet is already the native edit model.
    // Initialization is needed only for new drafts or incomplete legacy
    // snapshots. The existing ID scoping and reducer validation still run.
    let initialized=if previous.is_some_and(|saved|
        saved.get("model").is_some_and(Value::is_object) &&
        saved.get("assignmentOrder").is_some_and(Value::is_array)){
        Value::Null
    }else{
        rule(gateway,json!({
            "action":"initial_manual","order":&order,
            "assignmentOrder":previous.and_then(|v|v.get("assignmentOrder"))
                .cloned().unwrap_or_else(||json!([]))
        }))?
    };
    let mut model=previous.and_then(|v|v.get("model")).filter(|v|v.is_object())
        .cloned().or_else(||initialized.get("model").cloned())
        .ok_or("수취인 편집 작업본이 없습니다.")?;
    // Old saved drafts may still contain provisional IDs. Normalize the rows
    // and their assignment targets before the reducer can write them back.
    scope_manual_model_ids(&source.id,&mut model)?;
    let keys=previous.and_then(|v|v.get("assignmentOrder"))
        .or_else(||initialized.get("assignmentOrder"))
        .and_then(Value::as_array).cloned().unwrap_or_default();
    let mut cell=json!({"recipientId":change.recipient_id,"field":change.field});
    if let Some(item)=change.item_id.as_deref(){cell["itemId"]=json!(item);}
    let action=if change.item_id.is_some(){
        match change.field.as_str(){
            "quantity"=>"native_confirm_quantity",
            "approveOne"=>"native_approve_one",
            "cancelApproval"=>"native_cancel_approval",
            "unitPrice"=>"set_manual_unit_price",
            "product"=>"set_manual_product",
            "removeItem"=>"remove_manual_item",
            _=>return Err("지원하지 않는 품목 입력 항목입니다.".into())
        }
    }else{
        match change.field.as_str(){
            "name"|"address"|"mobile"|"phone"|"note"=>"set_manual_recipient_field",
            "shippingCharge"|"dispatchOrigin"|"freightCode"|
            "remoteDelivery"|"vatOverride"=>"set_manual_delivery_option",
            "vatSettings"=>"set_manual_vat_settings",
            "addItem"=>"add_manual_item",
            _=>return Err("지원하지 않는 수취인 입력 항목입니다.".into())
        }
    };
    let mut payload=json!({
        "action":action,"order":&order,"model":&model,
        "assignmentOrder":&keys,"cell":cell,"value":change.value
    });
    if action=="add_manual_item"{
        payload["recipientId"]=json!(change.recipient_id);
        payload["itemId"]=json!(unique_id());
    }
    if action=="remove_manual_item"{
        payload["recipientId"]=json!(change.recipient_id);
        payload["itemId"]=json!(change.item_id.as_deref()
            .ok_or("삭제할 품목을 확인하세요.")?);
    }
    if action=="set_manual_product"{
        // React's product dialog sends a verified catalog row rather than
        // free text. Rust owns the independent variant, pricing and model.
        // An existing row retains its confirmed quantity and original source.
        if let Some(product)=change.value.get("product").filter(|v|v.is_object()){
            let title=product.get("name").and_then(Value::as_str)
                .or_else(||product.get("code").and_then(Value::as_str))
                .filter(|name|!name.trim().is_empty())
                .ok_or("선택한 상품에 이름이 없습니다.")?.to_owned();
            payload["value"]=json!(title);
            payload["product"]=product.clone();
            payload["preserveExistingQuantity"]=json!(true);
        }
        // Follow the original native editor's catalog/recognition path.
        let state=if std::env::var_os("BANDO_EGUI_BUSINESS_DB")
            .is_some_and(|value|!value.is_empty()){
            gateway.load_business_state()?
        }else{gateway.load_catalog()?};
        payload["state"]=state;
        payload["candidateIds"]=json!((0..20).map(|_|unique_id()).collect::<Vec<_>>());
    }
    let response=if action=="add_manual_item"{
        add_selected_manual_product(gateway,&order,&payload,&change.value,
            &change.recipient_id,&json!(keys))?
    }else if action=="native_approve_one" || action=="native_cancel_approval"{
        let rid=change.recipient_id.as_str();
        let iid=change.item_id.as_deref()
            .ok_or("수량 승인 대상 품목이 없습니다.")?;
        let chosen=model["rows"].as_array_mut()
            .and_then(|rows|rows.iter_mut().find(|row|
                text(row,"id")==rid))
            .and_then(|row|row["items"].as_array_mut())
            .and_then(|items|items.iter_mut().find(|item|
                text(item,"id")==iid))
            .ok_or("수량 승인 대상이 최신 주문에 없습니다.")?;
        if action=="native_approve_one"{
            // The authoritative Rust issue predicate decides eligibility.
            // Copy its exact approved item (including quantityApproval)
            // into the native manual model; never emulate approval in UI.
            let actor=gateway.load_settings().ok()
                .and_then(|settings|settings.pointer("/settings/operatorName")
                    .and_then(Value::as_str).map(str::trim)
                    .filter(|name|!name.is_empty()).map(str::to_owned))
                .unwrap_or_else(||"사용자".to_owned());
            let approved=gateway.execute(ReadOperation::ApplyIssueAction,json!({
                "action":"approve_missing_quantity","order":&order,
                "recipientId":rid,"itemId":iid,
                "actor":actor,"now":chrono::Utc::now().to_rfc3339()
            }))?;
            let approved_item=approved["order"]["recipients"].as_array()
                .into_iter().flatten().find(|row|text(row,"id")==rid)
                .and_then(|row|row["items"].as_array())
                .into_iter().flatten().find(|item|text(item,"id")==iid)
                .ok_or("확정한 수량이 현재 품목과 일치하지 않습니다.")?;
            *chosen=approved_item.clone();
        }else{
            // React cancels the provisional decision by deleting only its
            // approval record; the existing quantity stays unconfirmed.
            if chosen.get("quantityApproval").is_none_or(Value::is_null){
                return Err("취소할 잠정 승인 기록이 없습니다.".into());
            }
            chosen.as_object_mut().ok_or("품목 형식이 올바르지 않습니다.")?
                .remove("quantityApproval");
        }
        json!({"model":model})
    }else if action=="native_confirm_quantity"{
        let first=rule(gateway,json!({
            "action":"set_manual_quantity_draft","model":model,
            "cell":payload["cell"],"value":payload["value"]
        }))?;
        rule(gateway,json!({
            "action":"confirm_manual_quantity","model":first["model"],
            "cell":payload["cell"],"assignmentOrder":&keys
        }))?
    }else{
        rule(gateway,payload)?
    };
    model=response.get("model").filter(|v|v.is_object())
        .ok_or("수정한 내용을 확인할 수 없습니다.")?.clone();
    let status=rule(gateway,json!({
        "action":"manual_quantity_status","model":&model
    }))?;
    if status["pending"]==true{
        // Editing the next cell must not silently commit a partly confirmed
        // quantity. The draft still lives only in the native edit projection.
        // Other existing quantity issues can be resolved one field at a time.
    }
    let applied=rule(gateway,json!({
        "action":"preview_manual","order":&order,"model":&model,
        "assignmentOrder":&keys
    }))?;
    let updated=applied.get("order").filter(|v|v.is_object())
        .ok_or("수정한 주문 정보를 확인할 수 없습니다.")?.clone();
    let (quote,quote_error)=if let Some(id)=source.client_id.as_deref(){
        let result=(||->Result<Value,String>{
            let state=gateway.load_business_state()?;
            let client=state["clients"].as_array().and_then(|clients|
                clients.iter().find(|client|client["id"].as_str()==Some(id))
            ).ok_or("거래처의 가격 기준이 없습니다.")?;
            let basis=crate::pricing_basis::snapshot(&state,client,&updated);
            // The native PRICE MASTER must also be unchanged. An unrelated
            // database revision does not alter this order-scoped dependency.
            if price_input_snapshot(&order)==price_input_snapshot(&updated) &&
               source.pricing_preview.as_ref().is_some_and(|saved|
                   saved.get("pricingBasis")==Some(&basis)) {
                if let Some(reused)=reuse_native_quote(
                    source.pricing_preview.as_ref(),&updated
                ){return Ok(reused);}
            }
            let mut projected=gateway.calculate_reception_edit_quote(
                json!({"order":&updated,"state":&state,"client":client})
            )?;
            projected["authorityRevision"]=state.get("revision").cloned()
                .unwrap_or(Value::Null);
            let quoted_basis=crate::pricing_basis::snapshot(
                &state,client,projected.get("order").unwrap_or(&updated)
            );
            projected["pricingBasis"]=quoted_basis;
            Ok(projected)
        })();
        match result{
            Ok(quote)=>(Some(quote),None),
            Err(error)=>(None,Some(format!("금액 재계산에 실패했습니다: {error}")))
        }
    }else{(None,None)};
    Ok(Event::Applied{
        order_id:source.id.clone(),raw:source.raw.clone(),
        order_type:source.order_type.clone(),client_id:source.client_id.clone(),
        direct_input,
        model,assignment_order:keys.iter().filter_map(Value::as_str)
            .map(str::to_owned).collect(),
        order:updated,quote,quote_error
    })
}

// Immutable display snapshot: one model copy per native edit/undo, not
// per render. The actual reducer and save payload continue to own self.model.
struct ManualRenderData{
    model:Value,
    spans:Vec<source_highlight::LinkedSpan>,
    assigned_ids:std::collections::HashSet<String>,
    pending_count:usize,
    recommendations:std::cell::RefCell<Option<(String,Option<ManualCellTarget>,std::rc::Rc<Vec<ManualCellTarget>>)>>,
}
impl ManualRenderData{
    fn new(raw:&str,model:&Value)->Self{
        let assignments=model.get("assignments").unwrap_or(&Value::Null);
        let spans=source_highlight::collect_manual(raw,list(model,"rows"),list(model,"blocks"),assignments);
        let assigned_ids=assignments.as_object().map(|map|map.iter()
            .filter(|(_,cell)|cell.is_object()).map(|(id,_)|id.to_owned()).collect()).unwrap_or_default();
        Self{model:model.clone(),spans,assigned_ids,pending_count:model_pending_quantity_count(model),
            recommendations:std::cell::RefCell::new(None)}
    }
    fn recommendations(&self,source:&str,pending:Option<&ManualCellTarget>)
        ->std::rc::Rc<Vec<ManualCellTarget>>{
        let mut cached=self.recommendations.borrow_mut();
        if let Some((id,target,rows))=cached.as_ref(){
            if id==source && target.as_ref()==pending{return rows.clone();}
        }
        let rows=std::rc::Rc::new(manual_assignment_recommendations(&self.model,source,pending));
        *cached=Some((source.to_owned(),pending.cloned(),rows.clone()));
        rows
    }
}
pub struct ManualWorkspace {
    pub order_id:String,
    direct_input:bool,
    raw:String,
    order_type:String,
    client_id:Option<String>,
    gateway:Arc<dyn BandoGateway>,
    base_order:Option<Value>,
    cached_quote:Option<Value>,
    model:Option<Value>,
    render_data:Option<std::rc::Rc<ManualRenderData>>,
    assignment_order:Vec<String>,
    remote_rows:Vec<RemoteRow>,
    view_quote:Option<Value>,
    selected_recipient:usize,
    selected_item:Option<String>,
    selected_field:String,
    assignment_gesture:ManualAssignmentGesture,
    edit_text:String,
    focus_editor:bool,
    active_editor_id:Option<egui::Id>,
    ime_preedit_active:bool,
    row_count_text:String,
    history:Vec<ManualHistoryFrame>,
    pending_select_after_apply:Option<ManualCellTarget>,
    advance_after_apply:Option<CellNavigationKey>,
    last_action:Option<String>,
    action_pending:bool,
    confirm_discard:bool,
    confirm_apply:bool,
    item_detail:Option<ItemEditor>,
    pending_item_detail:Option<(String,String)>,
    detail_write_pending:bool,
    add_product_picker:Option<(String,crate::product_manager::ProductManager)>,
    item_remove_confirmation:Option<(String,String)>,
    remove_recipient_pending:Option<String>,
    remove_recipient_target:Option<usize>,
    reset_pending:bool,
    receiver:Option<Receiver<Result<Outcome,String>>>,
    error:Option<String>,
    dirty:bool,
    applied:Option<(Value,Option<Value>,Option<String>)>,
}


/// Exact content-first geometry for the manual worksheet. The recipient card
/// uses four 36pt rows; the freight card keeps its established 340pt minimum.
/// Keeping this arithmetic beside the renderer prevents the modal from
/// reserving a full-screen body after the visible cards have ended.
fn manual_recipient_card_height()->f32{
    components::docked_section_chrome_height()+theme::RECEPTION_RECIPIENT_BODY_HEIGHT
}

fn manual_workspace_header_height()->f32{
    components::modal_master_header_with_toolbar_height()
}

fn manual_workspace_columns_height()->f32{
    manual_recipient_card_height()
        +components::RECEPTION_STACKED_CARD_GAP
        +theme::MANUAL_ITEMS_CARD_MIN_HEIGHT
}

fn manual_workspace_dialog_size(work_area:egui::Rect)->egui::Vec2{
    let width=(work_area.width()-theme::MANUAL_DIALOG_SIDE_GAP)
        .min(theme::MANUAL_DIALOG_MAX_WIDTH).max(0.0);
    // Status and shipment count sit below the shared title divider.
    // The card body begins one 16pt master gap below the toolbar.
    let body=manual_workspace_columns_height();
    let frame_chrome=2.0*(theme::MODAL_PANEL_PADDING_Y+theme::FRAME_STROKE_WIDTH);
    let fixed_chrome=frame_chrome+manual_workspace_header_height()
        +2.0*theme::MODAL_MASTER_WORKSPACE_GAP+theme::MODAL_FOOTER_HEIGHT;
    let height=(body+fixed_chrome).min(work_area.height()).max(1.0);
    egui::vec2(width,height)
}

fn manual_workspace_content_width(available_width:f32,reception_source_width:f32)->f32{
    available_width.max(
        reception_source_width.max(theme::MANUAL_SOURCE_MIN_WIDTH)
            +theme::MANUAL_SHEET_MIN_WIDTH+theme::MANUAL_COLUMN_GAP
    )
}

fn manual_workspace_content_width_for_mode(
    available_width:f32,reception_source_width:f32,direct_input:bool
)->f32{
    if direct_input{
        available_width.max(theme::MANUAL_SHEET_MIN_WIDTH)
    }else{
        manual_workspace_content_width(available_width,reception_source_width)
    }
}

/// Match Reception: an ancestor horizontal ScrollArea exists only when the
/// minimum workspace columns cannot fit. A full-width ScrollArea can leave a
/// long hover-only scrollbar even when the panels already fit the viewport.
fn manual_workspace_needs_horizontal_scroll(viewport_width:f32,content_width:f32)->bool{
    content_width>viewport_width+0.5
}

fn manual_source_panel_width(reception_source_width:f32,content_width:f32)->f32{
    let maximum=(content_width-theme::MANUAL_SHEET_MIN_WIDTH
        -theme::MANUAL_COLUMN_GAP).max(theme::MANUAL_SOURCE_MIN_WIDTH);
    reception_source_width.max(theme::MANUAL_SOURCE_MIN_WIDTH).min(maximum)
}

impl ManualWorkspace {
    pub fn new(source:WorkingReception,gateway:Arc<dyn BandoGateway>,ctx:Context)->Self {
        Self::build(source,gateway,ctx,false)
    }
    pub fn new_direct(source:WorkingReception,gateway:Arc<dyn BandoGateway>,ctx:Context)->Self {
        Self::build(source,gateway,ctx,true)
    }
    fn build(source:WorkingReception,gateway:Arc<dyn BandoGateway>,ctx:Context,direct_input:bool)->Self{
        let (tx,rx)=mpsc::channel();
        let row=source.clone();
        let service=Arc::clone(&gateway);
        thread::spawn(move||{
            let result=(||->Result<Outcome,String>{
                let (order,model,keys)=if direct_input{
                    let order=native_direct_order(service.as_ref(),&row)?;
                    let initial=rule(service.as_ref(),json!({
                        "action":"initial_manual","order":order,
                        "assignmentOrder":row.manual_model.as_ref()
                            .and_then(|saved|saved.get("assignmentOrder"))
                            .cloned().unwrap_or_else(||json!([]))
                    }))?;
                    let mut model=initial.get("model").filter(|v|v.is_object())
                        .ok_or("직접 입력을 준비하지 못했습니다.")?.clone();
                    let mut keys=initial.get("assignmentOrder").and_then(Value::as_array)
                        .ok_or("발주원문 분류 순서를 확인할 수 없습니다.")?.iter()
                        .map(|v|v.as_str().map(str::to_owned).ok_or("발주원문 분류 정보를 확인할 수 없습니다.".to_owned()))
                        .collect::<Result<Vec<_>,_>>()?;
                    if list(&model,"rows").is_empty(){
                        let resized=rule(service.as_ref(),json!({
                            "action":"resize_manual_rows","model":model,"count":1,
                            "newRecipientIds":[unique_id()],"assignmentOrder":keys
                        }))?;
                        model=resized.get("model").filter(|v|v.is_object())
                            .ok_or("새 배송 건을 준비하지 못했습니다.")?.clone();
                        keys=resized.get("assignmentOrder").and_then(Value::as_array)
                            .ok_or("직접 입력한 배송 정보의 순서를 확인할 수 없습니다.")?.iter()
                            .map(|v|v.as_str().map(str::to_owned).ok_or("발주원문 분류 정보를 확인할 수 없습니다.".to_owned()))
                            .collect::<Result<Vec<_>,_>>()?;
                    }
                    (order,model,keys)
                }else{
                    let recognition=row.analysis_evidence.as_ref()
                        .ok_or("현재 발주서의 분석 결과가 없습니다.")?;
                    let order=native_order(service.as_ref(),&row,recognition)?;
                    let initial=rule(service.as_ref(),json!({
                        "action":"initial_manual","order":order,
                        "assignmentOrder":row.manual_model.as_ref()
                            .and_then(|saved|saved.get("assignmentOrder"))
                            .and_then(Value::as_array).cloned().unwrap_or_default()
                    }))?;
                    let model=initial.get("model").filter(|v|v.is_object())
                        .ok_or("수동 분류를 준비하지 못했습니다.")?.clone();
                    let keys=initial.get("assignmentOrder").and_then(Value::as_array)
                        .ok_or("발주원문 분류 순서를 확인할 수 없습니다.")?.iter()
                        .map(|v|v.as_str().map(str::to_owned).ok_or("발주원문 분류 정보를 확인할 수 없습니다.".to_owned()))
                        .collect::<Result<Vec<_>,_>>()?;
                    (order,model,keys)
                };
                let remote_rows=resolve_remote_rows(service.as_ref(),&model);
                let view_quote=manual_view_quote(
                    service.as_ref(),&order,&model,&keys,row.client_id.as_deref(),None
                );
                Ok(Outcome::Initial{order,model,assignment_order:keys,remote_rows,view_quote})
            })();
            let _=tx.send(result);
            ctx.request_repaint();
        });
        Self {
            order_id:source.id,direct_input,raw:source.raw,order_type:source.order_type,
            client_id:source.client_id,gateway,base_order:None,
            cached_quote:source.pricing_preview,model:None,render_data:None,
            assignment_order:Vec::new(),remote_rows:Vec::new(),view_quote:None,
            selected_recipient:0,selected_item:None,selected_field:"name".into(),
            assignment_gesture:ManualAssignmentGesture::default(),
            edit_text:String::new(),focus_editor:false,active_editor_id:None,
            ime_preedit_active:false,row_count_text:"1".into(),
            history:Vec::new(),pending_select_after_apply:None,
            advance_after_apply:None,last_action:None,action_pending:false,
            confirm_discard:false,confirm_apply:false,
            item_detail:None,pending_item_detail:None,detail_write_pending:false,
            add_product_picker:None,item_remove_confirmation:None,
            remove_recipient_pending:None,remove_recipient_target:None,reset_pending:false,
            receiver:Some(rx),error:None,dirty:false,applied:None,
        }
    }
    /// Open the existing native editor on the recipient currently selected
    /// in the main intake pane, not silently on the first shipment.
    pub fn focus_recipient(&mut self,index:usize){
        self.focus_cell(index,None,"name");
    }
    pub fn focus_cell(&mut self,index:usize,item:Option<String>,field:&str){
        // Called before the asynchronous native model is initialized. The
        // Initial outcome then loads this exact cell's current value.
        let same_cell=self.selected_recipient==index &&
            self.selected_item.as_ref()==item.as_ref() &&
            self.selected_field==field;
        if !same_cell {self.last_action=None;}
        self.selected_recipient=index;
        self.selected_item=item;
        self.selected_field=field.to_owned();
        self.focus_editor=true;
    }
    fn busy(&self)->bool {self.receiver.is_some()}

    fn open_item_detail(&mut self,rid:&str,iid:&str,ctx:&Context){
        if self.busy() || self.nested_modal_window_id().is_some() ||
            self.pending_quantity_cell().is_some() || self.edit_text!=self.current_value(){return;}
        let Some(item)=self.model.as_ref().and_then(|model|
            list(model,"rows").iter().find(|row|text(row,"id")==rid)
        ).and_then(|row|list(row,"items").iter().find(|item|text(item,"id")==iid)).cloned()
        else{return;};
        let price=item.get("manualUnitPrice").and_then(Value::as_i64)
            .or_else(||self.view_quote.as_ref().and_then(|quote|crate::quoted_item_line(quote,iid))
                .and_then(|line|line["unitPrice"].as_i64()))
            .or_else(||item.get("unitPrice").and_then(Value::as_i64));
        self.assignment_gesture.clear();
        self.focus_editor=false;
        if let Some(id)=self.active_editor_id.take(){ctx.memory_mut(|m|m.surrender_focus(id));}
        self.item_detail=Some(ItemEditor::new(
            self.order_id.clone(),self.raw.clone(),rid.to_owned(),item,
            self.client_id.clone().unwrap_or_default(),price
        ).for_manual_draft());
        ctx.request_repaint();
    }

    fn product_dialog_open(&self)->bool{
        self.item_detail.is_some() || self.add_product_picker.is_some()
            || self.item_remove_confirmation.is_some()
    }

    fn nested_modal_window_id(&self)->Option<&'static str>{
        if let Some(detail)=self.item_detail.as_ref(){return Some(detail.top_window_id());}
        if self.add_product_picker.is_some(){return Some("manual-add-product-picker");}
        if self.item_remove_confirmation.is_some(){return Some("reception-item-delete-confirm");}
        nested_modal_id(
            self.product_dialog_open(),
            self.remove_recipient_pending.is_some(),
            self.reset_pending,
            self.confirm_apply,
            self.confirm_discard,
        )
    }

    fn poll(&mut self) {
        let received=self.receiver.as_ref().map(|r|r.try_recv());
        match received {
            Some(Ok(Ok(Outcome::Initial{
                order,model,assignment_order,remote_rows,view_quote
            })))=>{
                self.view_quote=view_quote;
                self.base_order=Some(order);
                self.remote_rows=remote_rows;
                let count=list(&model,"rows").len();
                self.row_count_text=count.to_string();
                self.selected_recipient=self.selected_recipient.min(count.saturating_sub(1));
                self.render_data=None;
                self.model=Some(model);
                self.assignment_order=assignment_order;
                self.receiver=None;
                self.edit_text=self.current_value();
                self.focus_editor=true;
            }
            Some(Ok(Ok(Outcome::Updated{
                model,assignment_order,remote_rows,view_quote
            })))=>{
                let advance_to_missing=self.last_action.as_deref()==Some("assign_manual_block")
                    && self.selected_item.is_none()
                    && ["name","address","mobile"].contains(&self.selected_field.as_str());
                if let Some(previous)=self.model.as_ref(){
                    push_manual_history(&mut self.history,ManualHistoryFrame{
                        model:previous.clone(),
                        assignment_order:self.assignment_order.clone(),
                        remote_rows:self.remote_rows.clone(),
                        view_quote:self.view_quote.clone(),
                        selected_recipient:self.selected_recipient,
                        selected_item:self.selected_item.clone(),
                        selected_field:self.selected_field.clone(),
                    });
                }
                self.remote_rows=remote_rows;
                self.view_quote=view_quote;
                self.render_data=None;
                self.model=Some(model);
                self.assignment_order=assignment_order;
                let count=self.model.as_ref().map(|m|list(m,"rows").len()).unwrap_or(0);
                self.row_count_text=count.to_string();
                self.selected_recipient=self.selected_recipient.min(count.saturating_sub(1));
                if self.selected_item.as_ref().is_some_and(|id|
                    !self.model.as_ref().is_some_and(|m|
                        list(m,"rows").get(self.selected_recipient).is_some_and(|r|
                            list(r,"items").iter().any(|item|text(item,"id")==id)
                        )
                    )
                ){
                    self.selected_item=None;
                    self.selected_field="name".into();
                }
                self.dirty=true;
                self.error=None;
                self.receiver=None;
                self.action_pending=false;
                self.edit_text=self.current_value();
                if self.detail_write_pending{
                    self.detail_write_pending=false;
                    self.item_detail=None;
                }
                if let Some(target)=self.pending_select_after_apply.take(){
                    let valid=self.model.as_ref().is_some_and(|model|
                        manual_cell_targets(model).contains(&target)
                    );
                    if valid {
                        self.select_cell(target.row,target.item,&target.field);
                    }
                }else if let Some(navigation)=self.advance_after_apply.take(){
                    self.move_selection(navigation);
                }else if advance_to_missing {
                    if let Some(target)=self.model.as_ref().and_then(|model|
                        next_missing_recipient_cell(
                            model,self.selected_recipient,&self.selected_field
                        )
                    ){
                        self.select_cell(target.row,target.item,&target.field);
                    }
                }
            }
            Some(Ok(Ok(Outcome::Applied{
                order,model,assignment_order,quote,quote_error
            })))=>{
                self.render_data=None;
                self.model=Some(model);
                self.assignment_order=assignment_order;
                self.applied=Some((order,quote,quote_error));
                self.receiver=None;
            }
            Some(Ok(Err(error)))=>{
                if self.detail_write_pending{
                    if let Some(detail)=self.item_detail.as_mut(){detail.report_error(error.clone());}
                    self.detail_write_pending=false;
                }
                self.pending_item_detail=None;
                self.error=Some(error);
                if self.edit_text!=self.current_value(){self.focus_editor=true;}
                self.advance_after_apply=None;
                self.pending_select_after_apply=None;
                if self.action_pending {self.last_action=None;}
                self.action_pending=false;
                self.receiver=None;
            }
            Some(Err(mpsc::TryRecvError::Disconnected))=>{
                self.error=Some("수동 분류 작업이 중단되었습니다.".into());
                if self.detail_write_pending{
                    if let Some(detail)=self.item_detail.as_mut(){
                        detail.report_error("수동 분류 작업이 중단되었습니다.".into());
                    }
                    self.detail_write_pending=false;
                }
                self.pending_item_detail=None;
                self.advance_after_apply=None;
                self.pending_select_after_apply=None;
                if self.action_pending {self.last_action=None;}
                self.action_pending=false;
                self.receiver=None;
            }
            _=>{}
        }
    }
    fn select_cell(&mut self,row:usize,item:Option<String>,field:&str) {
        if self.edit_text!=self.current_value() {
            self.error=Some("입력 중인 내용을 먼저 반영하세요.".into());
            return;
        }
        let target=ManualCellTarget{
            row,item:item.clone(),field:field.to_owned()
        };
        let pending=self.pending_quantity_cell();
        if !manual_cell_allowed(pending.as_ref(),&target) {
            self.error=Some("선택한 상품의 수량을 먼저 확정하세요.".into());
            return;
        }
        self.selected_recipient=row;
        self.selected_item=item;
        self.selected_field=field.to_owned();
        self.edit_text=self.current_value();
        self.active_editor_id=None;
        self.focus_editor=true;
    }

    fn undo(&mut self){
        if self.busy() || self.product_dialog_open() ||
            self.remove_recipient_pending.is_some() || self.reset_pending ||
            self.confirm_apply || self.confirm_discard {return;}
        if self.edit_text!=self.current_value(){
            self.edit_text=self.current_value();
            self.error=None;
            return;
        }
        let cancel_product=matches!(
            self.last_action.as_deref(),
            Some("set_manual_product"|"assign_manual_block")
        );
        let Some(frame)=self.history.pop() else {
            self.error=Some("되돌릴 수 있는 변경사항이 없습니다.".into());
            return;
        };
        self.render_data=None;
        self.model=Some(frame.model);
        self.assignment_order=frame.assignment_order;
        self.remote_rows=frame.remote_rows;
        self.view_quote=frame.view_quote;
        self.selected_recipient=frame.selected_recipient;
        self.selected_item=frame.selected_item;
        self.selected_field=if cancel_product{
            "product".into()
        }else{frame.selected_field};
        let count=self.model.as_ref().map(|model|list(model,"rows").len()).unwrap_or(0);
        self.selected_recipient=self.selected_recipient.min(count.saturating_sub(1));
        self.row_count_text=count.to_string();
        if self.selected_item.as_ref().is_some_and(|id|
            !self.model.as_ref().is_some_and(|model|
                list(model,"rows").get(self.selected_recipient).is_some_and(|row|
                    list(row,"items").iter().any(|item|text(item,"id")==id)
                )
            )
        ){
            self.selected_item=None;
            self.selected_field="name".into();
        }
        self.edit_text=self.current_value();
        self.dirty=true;
        self.error=None;
        self.focus_editor=true;
        self.last_action=None;
        self.action_pending=false;
    }

    fn move_selection(&mut self,navigation:CellNavigationKey){
        let Some(model)=self.model.as_ref() else{return};
        let current=ManualCellTarget{
            row:self.selected_recipient,item:self.selected_item.clone(),
            field:self.selected_field.clone()
        };
        let pending=self.pending_quantity_cell();
        let Some(target)=manual_navigation_target(
            model,&current,navigation,pending.as_ref()
        ) else{return};
        self.select_cell(target.row,target.item,&target.field);
    }

    fn selected_quantity_pending(&self)->bool{
        let Some(model)=self.model.as_ref() else{return false};
        let Some(item_id)=self.selected_item.as_deref() else{return false};
        list(model,"rows").get(self.selected_recipient)
            .and_then(|row|list(row,"items").iter().find(|item|
                text(item,"id")==item_id
            ))
            .is_some_and(item_quantity_pending)
    }

    fn pending_quantity_cell(&self)->Option<ManualCellTarget>{
        if !self.selected_quantity_pending() ||
            !matches!(self.last_action.as_deref(),
                Some("set_manual_product"|"assign_manual_block")) {
            return None;
        }
        Some(ManualCellTarget{
            row:self.selected_recipient,
            item:self.selected_item.clone(),
            field:"quantity".into(),
        })
    }

    fn current_value(&self)->String {
        let Some(model)=self.model.as_ref() else{return String::new()};
        let Some(row)=list(model,"rows").get(self.selected_recipient) else {
            return String::new();
        };
        if let Some(id)=&self.selected_item {
            let Some(item)=list(row,"items").iter().find(|v|text(v,"id")==id)
                else{return String::new()};
            return match self.selected_field.as_str(){
                "quantity"=>item.get("quantity").and_then(Value::as_i64)
                    .map(|n|n.to_string()).unwrap_or_default(),
                "unitPrice"=>item.get("manualUnitPrice")
                    .or_else(||item.get("unitPrice"))
                    .and_then(Value::as_i64)
                    .or_else(||self.view_quote.as_ref()
                        .and_then(|quote|crate::quoted_item_line(quote,id))
                        .and_then(|line|line["unitPrice"].as_i64()))
                    .map(components::money_edit_text).unwrap_or_default(),
                _=>text(item,"name").to_owned()
            };
        }
        if self.selected_field=="shippingCharge"{
            return crate::quoted_billable_freight(self.view_quote.as_ref(),row)
                .map(components::money_edit_text).unwrap_or_default();
        }
        text(row,&self.selected_field).to_owned()
    }

    // A selected assignment target is not necessarily an active editor.
    fn cell_editor_open(&self)->bool{
        self.focus_editor || self.active_editor_id.is_some()
    }

    /// Overlay the approved Reception TextEdit on the already allocated
    /// recipient/item/freight cell. Editing never changes row geometry.
    fn selected_cell_editor_on_rect(
        &mut self,ui:&mut egui::Ui,rect:egui::Rect,linked:bool,busy:bool
    )->(egui::Response,Option<CellNavigationKey>,bool){
        let had_focus=!self.focus_editor && self.active_editor_id.is_some_and(
            |id|ui.ctx().memory(|memory|memory.has_focus(id))
        );
        let key_event=ui.input(|input|
            manual_navigation_key_before_editor(
                &input.events,&mut self.ime_preedit_active
            )
        );
        let navigation=if had_focus && !busy &&
            !self.confirm_apply && !self.confirm_discard &&
            !self.product_dialog_open() && self.remove_recipient_pending.is_none()
            && !self.reset_pending {
            key_event.and_then(|(key,modifiers,navigation)|{
                let consumed=ui.input_mut(|input|{
                    let mut any=false;
                    while input.consume_key(modifiers,key){any=true;}
                    any
                });
                consumed.then_some(navigation)
            })
        }else{None};
        let focus=self.focus_editor && !busy &&
            !self.confirm_apply && !self.confirm_discard &&
            !self.product_dialog_open() &&
            self.remove_recipient_pending.is_none() && !self.reset_pending;
        let mut step_interaction=false;
        let response=if self.selected_item.is_none() {
            if self.selected_field=="shippingCharge" {
                components::reception_freight_inline_cell_editor(
                    ui,rect,&mut self.edit_text,focus,None
                )
            }else{
                components::reception_recipient_inline_cell_editor(
                    ui,rect,&mut self.edit_text,focus,None,
                    matches!(self.selected_field.as_str(),"mobile"|"phone"),
                    linked
                )
            }
        }else if matches!(self.selected_field.as_str(),"quantity"|"unitPrice"){
            let (input,step)=components::reception_item_numeric_editor(
                ui,rect,&mut self.edit_text,focus,None,
                if self.selected_field=="quantity"{components::ReceptionNumericCell::Quantity}
                else{components::ReceptionNumericCell::Money}
            );
            step_interaction=step;
            input
        }else{
            components::reception_inline_cell_editor(
                ui,rect,&mut self.edit_text,focus,None,false
            )
        };
        self.active_editor_id=Some(response.id);
        if focus {
            ui.scroll_to_rect(rect,Some(egui::Align::Center));
            self.focus_editor=false;
        }
        // Preserve the assignment target while dismissing its editing
        // overlay. Blank-space clicks must end editing even if egui kept focus.
        let numeric=matches!(self.selected_field.as_str(),"quantity"|"unitPrice"|"shippingCharge");
        let outside=ui.input(|input|input.pointer.any_pressed() &&
            input.pointer.interact_pos().is_some_and(|pos|!rect.contains(pos)));
        let finished=numeric && !busy && navigation.is_none() && !step_interaction &&
            (components::reception_numeric_edit_finished(&response,step_interaction)
                || outside || (!focus && !response.has_focus()));
        if finished{
            ui.ctx().memory_mut(|memory|memory.surrender_focus(response.id));
            self.active_editor_id=None;
            self.focus_editor=false;
        }
        (response,navigation,finished)
    }

    fn queue_cell_navigation(
        &mut self,navigation:CellNavigationKey,
        submit_direct:&mut bool,navigate_now:&mut Option<CellNavigationKey>
    ){
        let needs_commit=self.edit_text!=self.current_value() ||
            self.selected_field=="quantity";
        if needs_commit{
            *submit_direct=true;
            self.pending_select_after_apply=None;
            self.advance_after_apply=Some(navigation);
        }else{
            *navigate_now=Some(navigation);
        }
    }

    fn queue_cell_selection(
        &mut self,target:ManualCellTarget,submit_direct:&mut bool
    ){
        if self.edit_text!=self.current_value(){
            self.advance_after_apply=None;
            self.pending_select_after_apply=Some(target);
            *submit_direct=true;
        }else{
            self.select_cell(target.row,target.item,&target.field);
        }
    }

    fn request_resize(&mut self,target:usize,ctx:&Context){
        let current=self.model.as_ref()
            .map(|model|list(model,"rows").len()).unwrap_or(0);
        if current==0{return;}
        if !(1..=100).contains(&target){
            self.row_count_text=current.to_string();
            self.error=Some("배송 건수는 1~100건입니다.".into());
            return;
        }
        if self.busy(){return;}
        if self.pending_quantity_cell().is_some(){
            self.row_count_text=current.to_string();
            self.error=Some("수량을 확정하거나 상품 선택을 취소하세요.".into());
            return;
        }
        if self.edit_text!=self.current_value(){
            self.row_count_text=current.to_string();
            self.error=Some("현재 셀의 입력을 먼저 확정하세요.".into());
            return;
        }
        if target==current{
            self.row_count_text=current.to_string();
            return;
        }
        if target>current{
            let new_recipient_ids=(0..target-current).map(|_|unique_id())
                .collect::<Vec<_>>();
            self.action("resize_manual_rows",json!({
                "count":target,"newRecipientIds":new_recipient_ids
            }),ctx);
            return;
        }
        let (last_id,populated)={
            let model=self.model.as_ref().expect("manual model exists");
            let rows=list(model,"rows");
            let assignments=model.get("assignments").unwrap_or(&Value::Null);
            (
                rows.last().map(|row|text(row,"id").to_owned()),
                rows[target..].iter().any(|row|manual_row_has_data(row,assignments))
            )
        };
        self.row_count_text=current.to_string();
        if populated{
            self.remove_recipient_pending=last_id;
            self.remove_recipient_target=Some(target);
        }else{
            self.action("resize_manual_rows",json!({
                "count":target,"newRecipientIds":[]
            }),ctx);
        }
    }

    fn action(&mut self,action:&str,extra:Value,ctx:&Context) {
        if self.busy(){return;}
        self.assignment_gesture.clear();
        let Some(model)=self.model.clone() else{return};
        let order=self.base_order.clone().unwrap_or(Value::Null);
        let keys=self.assignment_order.clone();
        let service=Arc::clone(&self.gateway);
        let action=action.to_owned();
        let selected_client=self.client_id.clone();
        let previous_view_quote=self.view_quote.clone();
        let previous_remote_rows=self.remote_rows.clone();
        let (tx,rx)=mpsc::channel();
        self.receiver=Some(rx);
        self.error=None;
        self.last_action=Some(action.clone());
        self.action_pending=true;
        let ctx=ctx.clone();
        thread::spawn(move||{
            let result=(||->Result<Outcome,String>{
                let mut payload=json!({
                    "action":action.clone(),"model":model.clone(),
                    "assignmentOrder":keys.clone(),"order":order.clone()
                });
                if let Some(fields)=extra.as_object() {
                    for (key,value) in fields {
                        payload[key]=value.clone();
                    }
                }
                // Product recognition relies on the same native catalog and
                // customer pricing context as the main intake, not UI rules.
                if ["assign_manual_block","release_manual_block","set_manual_product"]
                    .contains(&action.as_str()) {
                    // A real dealer-specific manual product decision must
                    // see the complete operational price tier and overrides.
                    // Never price from the read-only name search projection.
                    let state=if std::env::var_os("BANDO_EGUI_BUSINESS_DB")
                        .is_some_and(|value|!value.is_empty()) {
                        service.load_business_state()?
                    }else{
                        service.load_catalog()?
                    };
                    payload["state"]=state;
                    payload["candidateIds"]=json!((0..20).map(|_|unique_id()).collect::<Vec<_>>());
                }
                let result=if action=="add_selected_manual_product"{
                    payload["action"]=json!("add_manual_item");
                    let recipient_id=text(&extra,"recipientId");
                    add_selected_manual_product(service.as_ref(),&order,&payload,
                        &extra["selection"],recipient_id,&json!(keys))?
                }else if action=="native_confirm_quantity" {
                    let first=rule(service.as_ref(),json!({
                        "action":"set_manual_quantity_draft",
                        "model":&model,"cell":extra.get("cell"),
                        "value":extra.get("value")
                    }))?;
                    rule(service.as_ref(),json!({
                        "action":"confirm_manual_quantity",
                        "model":first.get("model"),"cell":extra.get("cell"),
                        "assignmentOrder":keys.clone()
                    }))?
                }else{
                    rule(service.as_ref(),payload)?
                };
                let next=result.get("model").filter(|v|v.is_object())
                    .ok_or("수동 분류 변경 내용을 확인할 수 없습니다.")?.clone();
                let assignment_order=result.get("assignmentOrder")
                    .and_then(Value::as_array).map(|v|v.iter().filter_map(|s|s.as_str()
                        .map(str::to_owned)).collect()).unwrap_or_else(||keys.clone());
                let remote_rows=refresh_remote_rows(service.as_ref(),&next,&model,
                    &previous_remote_rows);
                let view_quote=manual_view_quote(service.as_ref(),&order,&next,
                    &assignment_order,selected_client.as_deref(),previous_view_quote.as_ref());
                Ok(Outcome::Updated{
                    model:next,assignment_order,remote_rows,view_quote
                })
            })();
            let _=tx.send(result);
            ctx.request_repaint();
        });
    }
    fn apply(&mut self,ctx:&Context) {
        if self.busy(){return;}
        let (Some(model),Some(order))=(self.model.clone(),self.base_order.clone())
            else{return};
        let cached_quote=self.cached_quote.clone();
        let keys=self.assignment_order.clone();
        let service=Arc::clone(&self.gateway);
        let selected_client=self.client_id.clone();
        let (tx,rx)=mpsc::channel();
        self.receiver=Some(rx);
        self.error=None;
        let ctx=ctx.clone();
        thread::spawn(move||{
            let result=(||->Result<Outcome,String>{
                let quantity=rule(service.as_ref(),json!({
                    "action":"manual_quantity_status","model":model.clone()
                }))?;
                if quantity.get("pending").and_then(Value::as_bool)==Some(true) {
                    return Err("선택한 상품의 수량을 먼저 확정하세요.".into());
                }
                let applied=rule(service.as_ref(),json!({
                    "action":"apply_manual","order":order.clone(),
                    "model":model.clone(),"assignmentOrder":keys.clone()
                }))?;
                let updated=applied.get("order").filter(|v|v.is_object())
                    .ok_or("수동 분류 적용 결과를 확인할 수 없습니다.")?.clone();
                let price_inputs_unchanged=
                    price_input_snapshot(&order)==price_input_snapshot(&updated);
                let order=updated;
                let (quote,quote_error)=if let Some(id)=selected_client.as_deref(){
                    let result=(||->Result<Value,String>{
                        let state=service.load_business_state()?;
                        let client=state["clients"].as_array()
                            .and_then(|clients|clients.iter().find(|client|
                                client["id"].as_str()==Some(id)))
                            .ok_or("선택한 거래처의 단가 기준을 찾지 못했습니다.")?;
                        let basis=crate::pricing_basis::snapshot(&state,client,&order);
                        if price_inputs_unchanged &&
                           cached_quote.as_ref().is_some_and(|saved|
                               saved.get("pricingBasis")==Some(&basis)) {
                            if let Some(reused)=reuse_native_quote(
                                cached_quote.as_ref(),&order
                            ){return Ok(reused);}
                        }
                        let revision=state["revision"].as_u64()
                            .ok_or("최신 업무 정보를 확인할 수 없습니다.")?;
                        let mut quote=service.execute(
                            ReadOperation::CalculateReceptionQuote,
                            json!({"order":&order,"state":&state,"client":client})
                        )?;
                        quote["authorityRevision"]=json!(revision);
                        let quoted_basis=crate::pricing_basis::snapshot(
                            &state,client,quote.get("order").unwrap_or(&order)
                        );
                        quote["pricingBasis"]=quoted_basis;
                        Ok(quote)
                    })();
                    match result{
                        Ok(quote)=>(Some(quote),None),
                        Err(error)=>(None,Some(format!(
                            "금액 재계산에 실패했습니다: {error}"
                        )))
                    }
                }else{(None,None)};
                Ok(Outcome::Applied{
                    order,model,assignment_order:keys,quote,quote_error
                })
            })();
            let _=tx.send(result);
            ctx.request_repaint();
        });
    }
    pub fn show(
        &mut self,
        ctx:&Context,
        work_area:egui::Rect,
        reception_source_width:f32,
        link_hover:&mut source_highlight::HoverState,
        source_hover:&mut Option<source_highlight::LinkKey>,
    )->Option<Event> {
        self.poll();
        if !self.busy(){
            if let Some((rid,iid))=self.pending_item_detail.take(){
                self.open_item_detail(&rid,&iid,ctx);
            }
        }
        if self.busy() || self.nested_modal_window_id().is_some(){self.assignment_gesture.clear();}
        if ctx.input(|input|input.modifiers.command && input.key_pressed(egui::Key::Z)){
            self.assignment_gesture.clear();
            self.undo();
        }
        if let Some((order,quote,quote_error))=self.applied.take() {
            if order.get("id").and_then(Value::as_str)!=Some(self.order_id.as_str()) ||
               order.get("raw").and_then(Value::as_str)!=Some(self.raw.as_str()) {
                self.error=Some("적용 결과의 발주서 ID나 원문이 일치하지 않습니다.".into());
            }else if let Some(model)=self.model.take() {
                return Some(Event::Applied {
                    order_id:self.order_id.clone(),raw:self.raw.clone(),
                    order_type:self.order_type.clone(),client_id:self.client_id.clone(),
                    direct_input:self.direct_input,model,
                    assignment_order:self.assignment_order.clone(),order,
                    quote,quote_error
                });
            }
        }
        let mut open=true;
        if ctx.input(|input|input.key_pressed(egui::Key::Escape)) && self.item_detail.is_some(){
            let busy=self.busy();
            if matches!(self.item_detail.as_mut().and_then(|detail|detail.escape(busy)),Some(item_editor::Event::Closed)){
                self.item_detail=None;
            }
        }else if ctx.input(|input|input.key_pressed(egui::Key::Escape)) &&
            (self.assignment_gesture.source.is_some() || self.assignment_gesture.dragging.is_some()){
            self.assignment_gesture.clear();
        }else if ctx.input(|input|input.key_pressed(egui::Key::Escape)) {
            match escape_action(
                self.busy(),
                self.product_dialog_open(),
                self.remove_recipient_pending.is_some(),
                self.reset_pending,
                self.confirm_apply,
                self.confirm_discard,
                self.edit_text!=self.current_value(),
            ){
                EscapeAction::CloseProductSearch=>{
                    self.item_detail=None;
                    self.add_product_picker=None;
                    self.item_remove_confirmation=None;
                },
                EscapeAction::CancelShipmentRemoval=>{
                    self.remove_recipient_pending=None;
                    self.remove_recipient_target=None;
                    let count=self.model.as_ref().map(|model|
                        list(model,"rows").len()).unwrap_or(0);
                    self.row_count_text=count.to_string();
                }
                EscapeAction::CancelReset=>self.reset_pending=false,
                EscapeAction::CancelApply=>{
                    self.confirm_apply=false;
                    self.focus_editor=true;
                },
                EscapeAction::CancelDiscard=>self.confirm_discard=false,
                EscapeAction::RevertCell=>{
                    self.edit_text=self.current_value();
                    self.error=None;
                }
                EscapeAction::RequestClose=>open=false,
                EscapeAction::Ignore=>{},
            }
        }
        if self.render_data.is_none(){
            self.render_data=self.model.as_ref().map(|model|
                std::rc::Rc::new(ManualRenderData::new(&self.raw,model)));
        }
        let render_data=self.render_data.clone();
        let spans=render_data.as_ref().map(|data|data.spans.as_slice()).unwrap_or(&[]);
        let selected_recipient_id=self.model.as_ref().and_then(|model|
            list(model,"rows").get(self.selected_recipient)
                .map(|row|text(row,"id").to_owned())
        );
        let active_link=link_hover.active_for(&self.order_id).cloned();
        let scroll_link=link_hover.scroll_to.clone();
        let mut hovered_source:Option<source_highlight::LinkKey>=None;
        let mut hovered_detail:Option<source_highlight::LinkKey>=None;
        let mut source_did_scroll=false;
        let mut navigate_now:Option<CellNavigationKey>=None;
        let mut chosen:Option<(String,Value)>=None;
        let mut source_clicked=None;
        let mut clicked_target:Option<ManualCellTarget>=None;
        let mut drop_target:Option<ManualCellTarget>=None;
        let mut drop_allowed=false;
        let drag_pointer=ctx.input(|i|i.pointer.interact_pos());
        let drag_released=ctx.input(|i|i.pointer.button_released(egui::PointerButton::Primary));
        let mut apply=false;
        let mut discard_confirmed=false;
        let mut submit_direct=false;
        let mut blur_commit=false;
        let mut lookup:Option<(String,String)>=None;
        let mut add_product:Option<String>=None;
        let mut remove_item:Option<(String,String)>=None;
        let workspace_title=if self.direct_input{"직접 입력"}else{"수동 분류"};
        components::modal_window(workspace_title,"bando-native-manual-classification",work_area)
            .fixed_size(manual_workspace_dialog_size(work_area))
            .show(ctx,|ui|{
                crate::floating_notice::section(ctx,"reception.manual",ui.available_rect_before_wrap());

                components::modal_flow_workspace(ui,|workspace|{
                    // The status and shipment controls belong to the shared
                    // modal header, not to a separate row above the card body.
                    let header_busy=self.busy();
                    let header_has_model=self.model.is_some();
                    let (header_row_count,header_unassigned)=
                        self.model.as_ref().map(|model|{
                            let rows=list(model,"rows");
                            let assignments=model.get("assignments").unwrap_or(&Value::Null);
                            let unassigned=list(model,"blocks").iter()
                                .filter(|block|assignments.get(text(block,"id")).is_none())
                                .count();
                            (rows.len(),unassigned)
                        }).unwrap_or((0,0));
                    let header_pending_quantity_active=
                        self.pending_quantity_cell().is_some();
                    let mut count_resize_requested=false;
                    let (close_clicked,())=workspace.header_with_toolbar(
                        workspace_title,"닫기",
                        !header_busy&&!self.product_dialog_open()
                            &&self.remove_recipient_pending.is_none()
                            &&!self.reset_pending
                            &&!self.confirm_apply
                            &&!self.confirm_discard,
                        |status|{
                            status.set_min_height(theme::MODAL_MASTER_WORKSPACE_TOOLBAR_HEIGHT);
                            status.spacing_mut().item_spacing.x=theme::SPACE_8;
                            if self.direct_input{
                                status.label(typography::caption("직접 입력 · 발주 원문 없음"))
                            }else{
                                status.label(typography::caption(
                                    &format!("미배정 원문: {header_unassigned}개")
                                ))
                            };
                            status.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),|right|{
                                    components::form_field_label(right,"건");
                                    let current_count=header_row_count;
                                    let can_resize=header_has_model && !header_busy
                                        && !header_pending_quantity_active
                                        && self.edit_text==self.current_value();
                                    let count_control_id=
                                        right.make_persistent_id("manual-shipment-count");
                                    let count_control=components::date_navigation_count_control(
                                        right,count_control_id,&mut self.row_count_text,
                                        current_count,can_resize,1,100
                                    );
                                    let minus_clicked=count_control.decrement_clicked;
                                    let plus_clicked=count_control.increment_clicked;
                                    let count_input=count_control.input;
                                    let filtered=self.row_count_text.chars()
                                        .filter(|ch|ch.is_ascii_digit()).take(3)
                                        .collect::<String>();
                                    if filtered!=self.row_count_text{
                                        self.row_count_text=filtered;
                                    }
                                    let count_commit=can_resize && ((
                                        count_input.lost_focus() &&
                                        self.row_count_text!=current_count.to_string()
                                    ) || (count_input.has_focus() && right.input(|input|
                                        input.key_pressed(egui::Key::Enter)
                                    )));
                                    if minus_clicked || plus_clicked || count_commit{
                                        let typed=self.row_count_text!=current_count.to_string();
                                        let requested=if typed{
                                            parse_manual_row_count(&self.row_count_text)
                                                .and_then(|entered|{
                                                    let adjusted=if minus_clicked{
                                                        entered.saturating_sub(1)
                                                    }else if plus_clicked{
                                                        entered.saturating_add(1)
                                                    }else{entered};
                                                    if (1..=100).contains(&adjusted){
                                                        Ok(adjusted)
                                                    }else{
                                                        Err("배송 건수는 1~100건입니다.".into())
                                                    }
                                                })
                                        }else if minus_clicked{
                                            Ok(current_count.saturating_sub(1))
                                        }else if plus_clicked{
                                            Ok(current_count.saturating_add(1))
                                        }else{
                                            parse_manual_row_count(&self.row_count_text)
                                        };
                                        match requested{
                                            Ok(count)=>self.request_resize(count,ctx),
                                            Err(error)=>{
                                                self.row_count_text=current_count.to_string();
                                                self.error=Some(error);
                                            }
                                        }
                                        count_resize_requested=true;
                                    }
                                    components::form_field_label(right,"배송 건수");
                                }
                            );
                        }
                    );
                    if close_clicked{open=false;}
                    // Prompts and asynchronous native mutations own input while
                    // open. The background worksheet remains visible but inert.
                    if self.busy() || self.product_dialog_open() ||
                        self.remove_recipient_pending.is_some() ||
                        self.reset_pending || self.confirm_apply || self.confirm_discard {
                        workspace.disable();
                    }
                    let Some((busy,pending_quantity_active,has_pending_quantity))=workspace.body_region(|ui|{
                        let Some(data)=render_data.as_ref() else {return None};
                        let model=&data.model;
                        let quote_index=self.view_quote.as_ref().map(crate::indexed_quote);
                        let rows=list(model,"rows");
                        let visible_shipment=visible_manual_shipment(self.selected_recipient,rows.len());
                        let assignments=model.get("assignments").unwrap_or(&Value::Null);
                        let assigned_source_ids=&data.assigned_ids;
                        let busy=self.busy();
                        let pending_quantity_cell=self.pending_quantity_cell();
                        let mut drag_recommendations=self.assignment_gesture.dragging.as_deref()
                            .map(|source|data.recommendations(source,pending_quantity_cell.as_ref()))
                            .unwrap_or_default();
                        let pending_quantity_active=pending_quantity_cell.is_some();
                        let pending_quantity_count=data.pending_count;
                        let has_pending_quantity=pending_quantity_count>0;
                        // The status/count toolbar is part of the shared modal
                        // header. The body begins directly at the card workspace.
                        ui.spacing_mut().item_spacing.y=0.0;
                        // The source column matches Reception's live pane width.
                        // The right worksheet takes the remaining width; on narrow windows keep
                        // both minimum widths and scroll the workspace horizontally
                        // instead of letting one panel clip or move the footer.
                        let viewport_width=ui.available_width();
                        let content_width=manual_workspace_content_width_for_mode(
                            viewport_width,reception_source_width,self.direct_input
                        );
                        // Keep the worksheet at its natural one-recipient / one-item
                        // height. More items remain reachable through the existing
                        // nested vertical scroll area; the footer stays in the modal.
                        let footer_clearance=components::modal_workspace_footer_reserve();
                        let available_content_height=(ui.available_height().min(work_area.height())
                            -footer_clearance).max(1.0);
                        let content_height=available_content_height
                            .min(manual_workspace_columns_height()).max(1.0);
                        let mut draw_columns=|outer:&mut egui::Ui|{
                            outer.set_min_width(content_width);
                            outer.horizontal(|ui|{
                            ui.spacing_mut().item_spacing.x=if self.direct_input{
                                0.0
                            }else{theme::MANUAL_COLUMN_GAP};
                            let width=content_width;
                            let height=content_height;
                            if !self.direct_input{
                                let source_width=manual_source_panel_width(
                                    reception_source_width,width
                                );
                                ui.allocate_ui_with_layout(
                                egui::vec2(source_width,height),
                                egui::Layout::top_down(egui::Align::Min),
                                |source_ui|{
                                    components::reception_section_with_header(
                                        source_ui,"발주 원문",
                                        height,|_|{},|body|{
                                            // Use the same outer Reception card, title row,
                                            // divider, and 16pt inset on all four sides
                                            // as the intake source pane.
                                            body.spacing_mut().item_spacing.y=0.0;
                                            theme::source_document_frame().show(body,|document|{
                                                components::scroll_area(components::ScrollAxes::Vertical)
                                                    .id_salt("manual-source-blocks")
                                                    .max_height((
                                                        components::docked_section_body_height(height)
                                                        -theme::SPACE_8
                                                        -2.0*theme::FRAME_STROKE_WIDTH
                                                    ).max(0.0))
                                                    .auto_shrink([false,false])
                                                    .show(document,|ui|{
                                                        let interaction=crate::manual_source_view::source_document(
                                                            ui,&self.raw,&spans,&assigned_source_ids,
                                                            selected_recipient_id.as_deref(),active_link.as_ref(),
                                                            scroll_link.as_ref(),!busy,
                                                            self.assignment_gesture.dragging.as_deref().or(self.assignment_gesture.source.as_deref())
                                                        );
                                                        hovered_source=interaction.hovered;
                                                        source_did_scroll=interaction.did_scroll;
                                                        if let Some(id)=interaction.drag_started_block {
                                                            if assignments.get(&id).is_none(){
                                                                self.assignment_gesture.start_drag(
                                                                    id.clone(),interaction.drag_anchor_offset,
                                                                    interaction.drag_layout_width
                                                                );
                                                                // Recommend valid cells on the initial press frame,
                                                                // even while the pointer remains completely still.
                                                                drag_recommendations=data.recommendations(
                                                                    &id,pending_quantity_cell.as_ref()
                                                                );
                                                            }
                                                        }
                                                        if interaction.drag_moved{
                                                            self.assignment_gesture.mark_drag_moved();
                                                        }
                                                        source_clicked=interaction.clicked_block;
                                                    });
                                            });
                                        },
                                    );
                                },
                            );
                            }

                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(),height),
                                egui::Layout::top_down(egui::Align::Min),
                                |sheet_ui|{
                                    // Share Reception's actual card renderer, field
                                    // geometry and product/freight controls. Do not
                                    // imitate their layout with a manual Grid.
                                    let panel_width=sheet_ui.available_width().max(
                                        theme::MANUAL_SHEET_MIN_WIDTH
                                    );
                                    sheet_ui.vertical(|panel|{
                                        panel.set_min_width(panel_width);
                                        panel.set_min_height(height.max(0.0));
                                        panel.spacing_mut().item_spacing.y=theme::SPACE_8;
                                        if count_resize_requested {panel.disable();}
                                        let can_switch=!busy && !pending_quantity_active
                                            && !count_resize_requested;
                                        let can_mutate=can_switch &&
                                            self.edit_text==self.current_value();
                                        // The count control now lives in the fixed
                                        // toolbar; only the recipient/item cards scroll.
                                        let body_height=panel.available_height().max(1.0);
                                        components::scroll_area(components::ScrollAxes::Vertical)
                                            .id_salt("manual-reception-style-cards")
                                            .scroll_bar_visibility(
                                                egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded
                                            )
                                            .auto_shrink([false,false])
                                            .max_height(body_height)
                                            .show(panel,|sheet|{
                                                // The shared stack gap is allocated explicitly below;
                                                // do not add egui's implicit item spacing too.
                                                sheet.spacing_mut().item_spacing.y=0.0;
                                                if let Some(index)=visible_shipment {
                                                    let row=&rows[index];
                                                    let rid=text(row,"id");
                                                    let mut next_recipient=None;
                                                    components::reception_section_with_header(
                                                        sheet,"수취인별 주문 정보",0.0,
                                                        |header|{
                                                            let name=text(row,"name");
                                                            // Keep name and 1/N visible even
                                                            // while navigation is locked; only
                                                            // the arrow segments become disabled.
                                                            next_recipient=
                                                                components::reception_recipient_navigator(
                                                                    header,name,index,rows.len(),can_switch
                                                                );
                                                        },|fields|{
                                                            // The five master inputs are
                                                            // one shared renderer. Only
                                                            // the selected field gets an
                                                            // overlaid native editor.
                                                            let mut name=text(row,"name").to_owned();
                                                            let mut address=text(row,"address").to_owned();
                                                            let mut mobile=text(row,"mobile").to_owned();
                                                            let mut phone=text(row,"phone").to_owned();
                                                            let mut note=text(row,"note").to_owned();
                                                            let recipient_keys=[
                                                                "name","address","mobile","phone","note"
                                                            ];
                                                            let active_fields=recipient_keys.map(|field|
                                                                source_highlight::is_active(
                                                                    active_link.as_ref(),
                                                                    &source_highlight::key(rid,rid,field)
                                                                )
                                                            );
                                                            let responses=components::reception_recipient_fields(
                                                                fields,&mut name,&mut address,
                                                                &mut mobile,&mut phone,&mut note,
                                                                active_fields
                                                            );
                                                            for (field,response) in recipient_keys
                                                                .into_iter().zip(responses.into_iter()){
                                                                let target=ManualCellTarget{row:index,item:None,field:field.to_owned()};
                                                                let recommended=drag_recommendations.contains(&target);
                                                                let hovered=recommended &&
                                                                    manual_assignment_drop_hit(fields,response.rect,drag_pointer);
                                                                if hovered{
                                                                    drop_allowed=true;
                                                                    drop_target=Some(target.clone());
                                                                }
                                                                let key=source_highlight::key(rid,rid,field);
                                                                let selected=index==self.selected_recipient &&
                                                                    self.selected_item.is_none() &&
                                                                    self.selected_field==field;
                                                                if selected {
                                                                    let (editing,navigation,_finished)=
                                                                        self.selected_cell_editor_on_rect(
                                                                            fields,response.rect,
                                                                            source_highlight::is_active(
                                                                                active_link.as_ref(),&key
                                                                            ),busy
                                                                        );
                                                                    if let Some(navigation)=navigation{
                                                                        self.queue_cell_navigation(
                                                                            navigation,&mut submit_direct,
                                                                            &mut navigate_now
                                                                        );
                                                                    }
                                                                    if editing.clicked(){clicked_target=Some(target.clone());}
                                                                    if editing.hovered(){hovered_detail=Some(key);}
                                                                }else{
                                                                    if response.hovered(){
                                                                        hovered_detail=Some(key.clone());
                                                                    }
                                                                    if response.clicked(){
                                                                        clicked_target=Some(target.clone());
                                                                        self.queue_cell_selection(target,&mut submit_direct);
                                                                    }
                                                                }
                                                                if recommended{
                                                                    paint_manual_assignment_recommendation(fields,response.rect,hovered);
                                                                }
                                                            }
                                                        }
                                                    );
                                                    if let Some(next)=next_recipient {
                                                        self.assignment_gesture.cell=None;
                                                        self.queue_cell_selection(
                                                            ManualCellTarget{
                                                                row:next,item:None,field:"name".into()
                                                            },&mut submit_direct
                                                        );
                                                    }
                                                    sheet.add_space(components::RECEPTION_STACKED_CARD_GAP);
                                                    let mut header_add_item=false;
                                                    let mut header_rollback=false;
                                                    let rollback_pending=self.selected_quantity_pending()
                                                        && matches!(self.last_action.as_deref(),
                                                            Some("set_manual_product"|"assign_manual_block"));
                                                    let rollback_enabled=!busy && self.edit_text==self.current_value();
                                                    components::reception_section_with_header(
                                                        sheet,"품목 및 운임",theme::MANUAL_ITEMS_CARD_MIN_HEIGHT,
                                                        |header|{
                                                            if rollback_pending{
                                                                header_rollback=components::reception_item_header_action(
                                                                    header,true,rollback_enabled
                                                                ).clicked();
                                                            }else{
                                                                header_add_item=
                                                                    components::reception_item_add_action_editable(
                                                                        header,can_mutate
                                                                    );
                                                            }
                                                        },|items_ui|{
                                                            components::reception_worksheet_items_body(items_ui);
                                                            components::reception_editable_items_header(items_ui);
                                                            for item in list(row,"items"){
                                                                let item_id=text(item,"id");
                                                                let quantity=item.get("quantity")
                                                                    .and_then(Value::as_i64)
                                                                    .map(|n|format!("{n}개"))
                                                                    .unwrap_or_else(||"—".to_owned());
                                                                // Exactly the same native line amount as reception.
                                                                let price=quote_index.as_deref()
                                                                    .and_then(|quote|quote.item(item_id))
                                                                    .and_then(|line|line["amount"].as_i64())
                                                                    .map(components::display_won)
                                                                    .unwrap_or_else(||"—".into());
                                                                let product_key=source_highlight::key(
                                                                    rid,item_id,"product"
                                                                );
                                                                let quantity_key=source_highlight::key(
                                                                    rid,item_id,"quantity"
                                                                );
                                                                let price_key=source_highlight::key(
                                                                    rid,item_id,"unitPrice"
                                                                );
                                                                let keys=[
                                                                    product_key,quantity_key,price_key
                                                                ];
                                                                let active_fields=keys.clone().map(|key|
                                                                    source_highlight::is_active(
                                                                        active_link.as_ref(),&key
                                                                    )
                                                                );
                                                                let cells=components::reception_item_row_with_remove(
                                                                    items_ui,crate::reception_service::item_title_ref(item),
                                                                    &quantity,&price,active_fields,can_switch
                                                                );
                                                                if cells.remove_clicked && can_mutate{
                                                                    remove_item=Some((rid.to_owned(),item_id.to_owned()));
                                                                }
                                                                for (field,response,key) in [
                                                                    ("product",cells.product,&keys[0]),
                                                                    ("quantity",cells.quantity,&keys[1]),
                                                                    ("unitPrice",cells.amount,&keys[2])
                                                                ]{
                                                                    let target=ManualCellTarget{row:index,item:Some(item_id.to_owned()),field:field.to_owned()};
                                                                    let recommended=drag_recommendations.contains(&target);
                                                                    let hovered=recommended &&
                                                                        manual_assignment_drop_hit(items_ui,response.rect,drag_pointer);
                                                                    if hovered{
                                                                        drop_allowed=true;
                                                                        drop_target=Some(target.clone());
                                                                    }
                                                                    let selected=index==self.selected_recipient &&
                                                                        self.selected_item.as_deref()==Some(item_id) &&
                                                                        self.selected_field==field;
                                                                    let cell_rect=response.rect;
                                                                    let interaction=if selected && field!="product" && self.cell_editor_open(){
                                                                        let (editing,navigation,finished)=
                                                                            self.selected_cell_editor_on_rect(
                                                                                items_ui,response.rect,
                                                                                source_highlight::is_active(
                                                                                    active_link.as_ref(),key
                                                                                ),busy
                                                                            );
                                                                        if let Some(navigation)=navigation{
                                                                            self.queue_cell_navigation(
                                                                                navigation,&mut submit_direct,
                                                                                &mut navigate_now
                                                                            );
                                                                        }
                                                                        if finished && self.edit_text!=self.current_value(){
                                                                            submit_direct=true;
                                                                            blur_commit=true;
                                                                        }
                                                                        if editing.hovered(){
                                                                            hovered_detail=Some(key.clone());
                                                                        }
                                                                        editing
                                                                    }else{
                                                                        if response.hovered(){
                                                                            hovered_detail=Some(key.clone());
                                                                        }
                                                                        if field=="product"{
                                                                            let assignment_requested=self.assignment_gesture.source.is_some()
                                                                                || items_ui.input(|input|input.modifiers.shift);
                                                                            match item_editor::product_click_intent(response.clicked(),can_switch,assignment_requested){
                                                                                ProductClickIntent::OpenDetail=>{
                                                                                    lookup=Some((rid.to_owned(),item_id.to_owned()));
                                                                                }
                                                                                ProductClickIntent::SelectAssignmentTarget=>{
                                                                                    self.queue_cell_selection(target.clone(),&mut submit_direct);
                                                                                    clicked_target=Some(target.clone());
                                                                                }
                                                                                ProductClickIntent::None=>{}
                                                                            }
                                                                        }else if response.clicked(){
                                                                            self.queue_cell_selection(target.clone(),&mut submit_direct);
                                                                        }
                                                                        response
                                                                    };
                                                                    if field!="product" && interaction.clicked(){clicked_target=Some(target.clone());}
                                                                    // Keep the shared three-column table. Product replacement
                                                                    // and removal target the actual clicked native item, not
                                                                    // a separate selection-dependent toolbar.
                                                                    let actions_enabled=can_mutate
                                                                        && self.edit_text==self.current_value()
                                                                        && !submit_direct;
                                                                    if actions_enabled {
                                                                        interaction.context_menu(|menu|{
                                                                            if theme::action(
                                                                                menu,"상품 상세 수정",ButtonRole::Standard,true
                                                                            ).clicked(){
                                                                                lookup=Some((rid.to_owned(),item_id.to_owned()));
                                                                                menu.close();
                                                                            }
                                                                            if field=="product" && theme::action(
                                                                                menu,"원문 배정 대상 선택",ButtonRole::Standard,true
                                                                            ).clicked(){
                                                                                self.queue_cell_selection(target.clone(),&mut submit_direct);
                                                                                clicked_target=Some(target.clone());
                                                                                menu.close();
                                                                            }
                                                                            if theme::action(
                                                                                menu,"상품 제거",ButtonRole::Danger,true
                                                                            ).clicked(){
                                                                                remove_item=Some((rid.to_owned(),item_id.to_owned()));
                                                                                menu.close();
                                                                            }
                                                                        });
                                                                    }
                                                                    if recommended{
                                                                        paint_manual_assignment_recommendation(
                                                                            items_ui,cell_rect,hovered
                                                                        );
                                                                    }
                                                                }
                                                            }
                                                            // Pending assignment rollback lives in the fixed header action.
                                                            components::reception_items_to_freight_gap(items_ui);
                                                            let code=if text(row,"freightCode")=="020"{
                                                                "020"
                                                            }else{"010"};
                                                            let remote_result=remote_for(&self.remote_rows,row)
                                                                .cloned();
                                                            let remote_value=remote_result.as_ref()
                                                                .and_then(|result|result.as_ref().ok())
                                                                .filter(|value|value.is_object());
                                                            let remote=remote_value
                                                                .and_then(|value|value["checked"].as_bool())
                                                                .unwrap_or(false);
                                                            let remote_ready=remote_value.is_some();
                                                            let freight=components::reception_freight_row_editable(
                                                                items_ui,"운임",
                                                                crate::indexed_billable_freight(quote_index.as_deref(),row),
                                                                Some(code=="020"),
                                                                remote_ready.then_some(remote),
                                                                true,can_mutate
                                                            );
                                                            if freight.amount_clicked {
                                                                self.queue_cell_selection(
                                                                    ManualCellTarget{
                                                                        row:index,item:None,
                                                                        field:"shippingCharge".into()
                                                                    },&mut submit_direct
                                                                );
                                                            }
                                                            if self.selected_item.is_none() &&
                                                                self.selected_field=="shippingCharge" && code!="020" &&
                                                                self.cell_editor_open() {
                                                                if let Some(rect)=freight.amount_rect {
                                                                    let (_,navigation,finished)=
                                                                        self.selected_cell_editor_on_rect(
                                                                            items_ui,rect,false,busy
                                                                        );
                                                                    if finished && self.edit_text!=self.current_value(){
                                                                        submit_direct=true;
                                                                        blur_commit=true;
                                                                    }
                                                                    if let Some(navigation)=navigation{
                                                                        self.queue_cell_navigation(
                                                                            navigation,&mut submit_direct,
                                                                            &mut navigate_now
                                                                        );
                                                                    }
                                                                }
                                                            }
                                                            if freight.collect_clicked {
                                                                chosen=Some(("set_manual_delivery_option".into(),
                                                                    json!({"cell":{
                                                                        "recipientId":rid,"field":"freightCode"
                                                                    },"value":if code=="020"{"010"}else{"020"}})
                                                                ));
                                                            }
                                                            if freight.remote_clicked {
                                                                chosen=Some(("set_manual_delivery_option".into(),
                                                                    json!({"cell":{
                                                                        "recipientId":rid,"field":"remoteDelivery"
                                                                    },"value":!remote})
                                                                ));
                                                            }
                                                            items_ui.add_space(theme::SPACE_8);
                                                            let origin=if text(row,"dispatchOrigin")=="STORE"{
                                                                "STORE"
                                                            }else{"FACTORY"};
                                                            if let Some(next_origin)=
                                                                components::reception_item_origin_action_editable(
                                                                    items_ui,Some(origin),can_mutate
                                                                ){
                                                                chosen=Some(("set_manual_delivery_option".into(),
                                                                    json!({"cell":{
                                                                        "recipientId":rid,"field":"dispatchOrigin"
                                                                    },"value":next_origin})
                                                                ));
                                                            }
                                                        }
                                                    );
                                                    if header_rollback{
                                                        self.undo();
                                                    }else if header_add_item {
                                                        add_product=Some(rid.to_owned());
                                                    }
                                                }
                                            });
                                    });
                                },
                            );
                                });
                        };
                        if manual_workspace_needs_horizontal_scroll(viewport_width,content_width){
                            components::scroll_area(components::ScrollAxes::Horizontal)
                                .drag_to_scroll(false)
                                .id_salt("manual-columns-horizontal-scroll")
                                .max_height(content_height)
                                .auto_shrink([false,false])
                                .show(ui,&mut draw_columns);
                        }else{
                            // No ancestor ScrollArea at full width: preserve the
                            // fixed modal body height without creating a hover-only
                            // horizontal scrollbar over already-fitted columns.
                            ui.allocate_ui_with_layout(
                                egui::vec2(viewport_width,content_height),
                                egui::Layout::top_down(egui::Align::Min),
                                |outer|draw_columns(outer)
                            );
                        }
                        Some((busy,pending_quantity_active,has_pending_quantity))
                    }) else{return;};
                    let input_settled=self.edit_text==self.current_value();
                    workspace.footer(|left|{
                            // Reset is a destructive worksheet operation with its
                            // existing confirmation, not a toolbar filter.
                            if theme::action(left,"초기화",ButtonRole::Danger,
                                !busy && !pending_quantity_active && input_settled
                            ).clicked(){
                                if input_settled {
                                    self.reset_pending=true;
                                }
                            }
                        },
                        |right|{
                            let apply_button=theme::action(right,"초안에 적용",
                                ButtonRole::Primary,!busy && input_settled &&
                                    !has_pending_quantity);
                            let apply_clicked=apply_button.clicked();
                            if apply_clicked{apply=true;}
                        }
                    );
                });
            });
        if let Some(navigation)=navigate_now{
            self.assignment_gesture.clear();
            self.move_selection(navigation);
        }
        // Resolve all gestures after both panes have been drawn. Never bypass
        // the native assignment action or overwrite an uncommitted editor.
        let gesture_allowed=!self.busy() && self.nested_modal_window_id().is_none()
            && !submit_direct && self.edit_text==self.current_value()
            && chosen.is_none() && lookup.is_none() && add_product.is_none() && remove_item.is_none();
        if gesture_allowed{
            let mut assignment=None;
            // A press without crossing egui's drag threshold is a click.
            // Clear its temporary drag preview first so source-click selection
            // keeps the existing two-click assignment behavior.
            if drag_released && self.assignment_gesture.dragging.is_some()
                && !self.assignment_gesture.drag_moved{
                self.assignment_gesture.cancel_pending_drag();
            }
            if drag_released && self.assignment_gesture.dragging.is_some(){
                assignment=self.assignment_gesture.finish_drag(drop_target);
            }else if let Some(id)=source_clicked{
                let assigned=self.model.as_ref().and_then(|m|m["assignments"].get(&id));
                if let Some(cell)=assigned{
                    let pending=self.pending_quantity_cell();
                    if manual_source_assignment_allowed(true,cell.get("itemId").and_then(Value::as_str),
                        pending.as_ref().and_then(|c|c.item.as_deref()),text(cell,"field")){
                        chosen=Some(("release_manual_block".into(),json!({"id":id,"clear":true})));
                    }else{self.error=Some("선택한 상품의 수량을 먼저 확정하세요.".into());}
                    self.assignment_gesture.clear();
                }else{
                    assignment=self.assignment_gesture.source_click(id);
                }
            }else if let Some(target)=clicked_target{
                assignment=self.assignment_gesture.cell_click(target);
            }
            if let Some((id,target))=assignment{
                let pending=self.pending_quantity_cell();
                let payload=self.model.as_ref().ok_or("수동 분류 모델이 없습니다.")
                    .and_then(|m|manual_assignment_payload(m,&id,&target,pending.as_ref()));
                match payload{
                    Ok(payload)=>{
                        self.select_cell(target.row,target.item,&target.field);
                        chosen=Some(("assign_manual_block".into(),payload));
                    }
                    Err(error)=>{
                        self.error=Some(error.into());
                        self.assignment_gesture.source=Some(id);
                    }
                }
            }
        }else if drag_released{
            self.assignment_gesture.clear();
        }
        if self.assignment_gesture.dragging.is_some(){
            if !ctx.input(|input|input.pointer.primary_down()){
                self.assignment_gesture.clear();
            }else{
                ctx.set_cursor_icon(if self.assignment_gesture.drag_moved && !drop_allowed{
                    egui::CursorIcon::NotAllowed
                }else{egui::CursorIcon::Grabbing});
                if let (Some(source_id),Some(pointer),Some(anchor_offset))=(
                    self.assignment_gesture.dragging.as_deref(),drag_pointer,
                    self.assignment_gesture.drag_anchor_offset
                ){
                    paint_manual_source_drag_preview(
                        ctx,&self.raw,&spans,source_id,pointer,anchor_offset,
                        self.assignment_gesture.drag_layout_width,
                        selected_recipient_id.as_deref(),active_link.as_ref()
                    );
                }
            }
        }
        if let Some(rid)=add_product{
            if !self.busy() && !self.product_dialog_open() && self.edit_text==self.current_value(){
                self.add_product_picker=Some((rid,crate::product_manager::ProductManager::new_selection(
                    Arc::clone(&self.gateway),ctx.clone(),self.client_id.clone().unwrap_or_default()
                )));
            }
        }
        if let Some(target)=remove_item{
            if !self.busy() && !self.product_dialog_open() && self.edit_text==self.current_value(){
                self.item_remove_confirmation=Some(target);
            }
        }
        if let Some((rid,manager))=self.add_product_picker.as_mut(){
            let (cancelled,product)=manager.show_selection_modal(ctx,work_area,"manual-add-product-picker");
            let rid=rid.clone();
            if cancelled{self.add_product_picker=None;}
            else if let Some(product)=product{
                self.add_product_picker=None;
                chosen=Some(("add_selected_manual_product".into(),json!({
                    "recipientId":rid,"itemId":unique_id(),"selection":product
                })));
            }
        }
        if let Some((rid,iid))=self.item_remove_confirmation.clone(){
            let item=self.model.as_ref().and_then(|model|list(model,"rows").iter()
                .find(|row|text(row,"id")==rid))
                .and_then(|row|list(row,"items").iter().find(|item|text(item,"id")==iid));
            if let Some(item)=item{
                let (cancel,remove)=components::reception_item_delete_confirmation(
                    ctx,work_area,crate::reception_service::item_title_ref(item),None,self.busy()
                );
                if cancel || remove{self.item_remove_confirmation=None;}
                if remove && !cancel{
                    chosen=Some(("remove_manual_item".into(),json!({"recipientId":rid,"itemId":iid})));
                }
            }else{self.item_remove_confirmation=None;}
        }
        // Ordinary product clicks use the shared ItemEditor. Dirty numeric
        // input is committed first; only a successful native result opens it.
        if let Some((rid,iid))=lookup{
            if !self.busy() && self.nested_modal_window_id().is_none(){
                if self.edit_text!=self.current_value(){
                    self.pending_item_detail=Some((rid,iid));
                    submit_direct=true;
                }else{
                    self.open_item_detail(&rid,&iid,ctx);
                }
            }
        }
        let detail_busy=self.busy();
        let detail_event=self.item_detail.as_mut().and_then(|detail|
            detail.show(ctx,Arc::clone(&self.gateway),work_area,detail_busy)
        );
        match detail_event{
            Some(item_editor::Event::Closed)=>self.item_detail=None,
            Some(event) if !detail_busy=>{
                let target=self.item_detail.as_ref().and_then(|detail|
                    self.model.as_ref().and_then(|model|
                        list(model,"rows").iter().enumerate().find_map(|(index,row)|
                            if text(row,"id")==detail.recipient_id{
                                list(row,"items").iter().find(|item|text(item,"id")==detail.item_id)
                                    .map(|item|(index,detail.recipient_id.clone(),detail.item_id.clone(),item.clone()))
                            }else{None}
                        )
                    )
                );
                if let Some((index,rid,iid,item))=target{
                    match event{
                        item_editor::Event::Apply(product)=>{
                            let preserve=item.get("quantityConfirmed").and_then(Value::as_bool)==Some(true)
                                && item.get("quantity").and_then(Value::as_i64).is_some_and(|q|q>0);
                            let title=text(&product,"name").to_owned();
                            self.select_cell(index,Some(iid.clone()),"quantity");
                            self.detail_write_pending=true;
                            self.action("set_manual_product",json!({
                                "cell":{"recipientId":rid,"itemId":iid,"field":"product"},
                                "value":title,"product":product,"preserveExistingQuantity":preserve
                            }),ctx);
                        }
                        item_editor::Event::Remove=>{
                            self.detail_write_pending=true;
                            self.action("remove_manual_item",json!({"recipientId":rid,"itemId":iid}),ctx);
                        }
                        item_editor::Event::Closed=>{}
                    }
                }else{self.item_detail=None;}
            }
            _=>{}
        }

        if submit_direct {
            self.focus_editor=!blur_commit;
            let rid=self.model.as_ref().and_then(|m|list(m,"rows")
                .get(self.selected_recipient)).and_then(|r|r.get("id"))
                .and_then(Value::as_str).map(str::to_owned);
            if let Some(id)=rid {
                let mut cell=json!({"recipientId":id,"field":self.selected_field});
                if let Some(item)=self.selected_item.as_ref() {
                    cell["itemId"]=json!(item);
                }
                let mut value=json!(self.edit_text.clone());
                let action=if self.selected_field=="quantity" {
                    "native_confirm_quantity"
                }else if self.selected_field=="product"{
                    "set_manual_product"
                }else if self.selected_field=="unitPrice"{
                    let raw=self.edit_text.trim().replace(',',"");
                    if raw.is_empty(){
                        value=Value::Null;
                    }else if let Ok(price)=raw.parse::<i64>(){
                        value=json!(price);
                    }else{
                        self.error=Some("단가는 0 이상의 정수로 입력하세요.".into());
                        return None;
                    }
                    "set_manual_unit_price"
                }else if self.selected_field=="shippingCharge"{
                    let collect=self.model.as_ref().and_then(|model|
                        list(model,"rows").get(self.selected_recipient)
                    ).is_some_and(|row|text(row,"freightCode")=="020");
                    if collect {
                        self.error=Some("착불인 주문은 운임을 수정할 수 없습니다.".into());
                        return None;
                    }
                    // The UI only parses an entered integer. The existing
                    // Rust rule is the sole authority for 2x normalization,
                    // COD and allowed base freight.
                    let raw=self.edit_text.trim().replace(',',"");
                    if raw.is_empty(){
                        value=Value::Null;
                    }else if let Ok(n)=raw.parse::<i64>(){
                        value=json!(n);
                    }else{
                        self.error=Some("운임은 0 이상의 정수로 입력하세요.".into());
                        return None;
                    }
                    "set_manual_delivery_option"
                }else{
                    "set_manual_recipient_field"
                };
                chosen=Some((action.into(),json!({"cell":cell,"value":value})));
            }
        }
        if let Some((action,payload))=chosen {
            if self.edit_text!=self.current_value() && !submit_direct {
                self.error=Some("입력 중인 내용을 먼저 반영하세요.".into());
            }else{
                if action=="assign_manual_block" &&
                    self.selected_item.is_some() && self.selected_field=="product" {
                    self.select_cell(
                        self.selected_recipient,self.selected_item.clone(),"quantity"
                    );
                }
                if action=="set_manual_product" &&
                    self.selected_item.is_some() && self.selected_field=="product" {
                    self.select_cell(
                        self.selected_recipient,self.selected_item.clone(),"quantity"
                    );
                }
                self.action(&action,payload,ctx);
            }
        }
        // React already confirms truncation only when the final shipment
        // holds actual values or source assignments. No popup for empty rows.
        if let Some(target)=self.remove_recipient_pending.clone(){
            let mut confirm=false;
            let mut cancel=false;
            let current_count=self.model.as_ref().map(|model|
                list(model,"rows").len()).unwrap_or(0);
            let target_count=self.remove_recipient_target
                .unwrap_or_else(||current_count.saturating_sub(1));
            let removing_count=current_count.saturating_sub(target_count);
            let removal_message=if self.direct_input{
                format!("마지막 {removing_count}개 배송 건의 입력값을 제거할까요?")
            }else{
                format!("마지막 {removing_count}개 배송 건의 입력값과 원문 배정을 제거할까요?")
            };
            components::show_decision_window(ctx,"배송 건수 줄이기","manual-shipment-removal",work_area,|ui|{
                    components::decision_workspace(ui,|decision|{
                        cancel=decision.header("배송 건수 줄이기","취소",true
                        );
                        decision.body(|ui|{
                            ui.label(&removal_message);
                        });
                        decision.actions(|actions|{
                            if theme::action(actions,"확인",ButtonRole::Danger,true).clicked(){
                                confirm=true;
                            }
                        });
                    });
                });
            if cancel {
                self.remove_recipient_pending=None;
                self.remove_recipient_target=None;
                let count=self.model.as_ref().map(|model|
                    list(model,"rows").len()).unwrap_or(0);
                self.row_count_text=count.to_string();
            }
            if confirm {
                let count=self.model.as_ref().map(|model|
                    list(model,"rows").len()).unwrap_or(0);
                let target_count=self.remove_recipient_target
                    .unwrap_or_else(||count.saturating_sub(1));
                let unchanged=self.model.as_ref().and_then(|model|
                    list(model,"rows").last()
                ).is_some_and(|row|text(row,"id")==target);
                self.remove_recipient_pending=None;
                self.remove_recipient_target=None;
                if unchanged && target_count>=1 && target_count<count &&
                   !self.busy() && self.edit_text==self.current_value() {
                    self.action("resize_manual_rows",json!({
                        "count":target_count,"newRecipientIds":[]
                    }),ctx);
                }else{
                    self.error=Some("삭제 대상이 변경됐습니다. 현재 배송 목록을 확인하세요.".into());
                    self.row_count_text=count.to_string();
                }
            }
        }
        if self.reset_pending {
            let mut confirm=false;
            let mut cancel=false;
            let reset_title=if self.direct_input{"직접 입력 초기화"}else{"전체 초기화"};
            let reset_message=if self.direct_input{
                "입력한 배송 정보와 품목을 초기화할까요?"
            }else{
                "구성표와 배정을 초기화할까요? 최초 원문과 마스터는 보존됩니다."
            };
            components::show_decision_window(ctx,reset_title,"manual-reset-confirm",work_area,|ui|{
                    components::decision_workspace(ui,|decision|{
                        cancel=decision.header(reset_title,"취소",true
                        );
                        decision.body(|ui|{
                            ui.label(reset_message);
                        });
                        confirm=decision.actions(|actions|{
                                theme::action(
                                    actions,"확인",ButtonRole::Primary,true
                                ).clicked()
                            }
                        ).inner;
                    });
                });
            if cancel{self.reset_pending=false;}
            if confirm {
                self.reset_pending=false;
                self.action("reset_manual_rows",json!({
                    "recipientId":unique_id()
                }),ctx);
            }
        }
        if !open {
            // Dirty typed text is just as important as a completed Rust edit.
            // A parent close cannot destroy an active modal/async operation.
            if self.busy() || self.product_dialog_open() ||
               self.remove_recipient_pending.is_some() || self.reset_pending ||
               self.confirm_apply {
                return None;
            }
            if self.dirty || self.edit_text!=self.current_value() {
                self.confirm_discard=true;
            } else {
                return Some(Event::Closed);
            }
        }
        if self.confirm_discard {
            let mut keep_editing=false;
            let discard_title=if self.direct_input{"직접 입력 나가기"}else{"수동 분류 나가기"};
            components::show_decision_window(ctx,discard_title,"manual-discard-prompt",work_area,|ui|{
                    components::decision_workspace(ui,|decision|{
                        keep_editing=decision.header(discard_title,"계속 편집",true
                        );
                        decision.body(|ui|{
                            ui.label("초안에 반영하지 않은 변경사항이 있습니다. 변경사항을 버리고 닫을까요?");
                        });
                        decision.actions(|actions|{
                            if theme::action(
                                actions,"변경사항 버리고 닫기",ButtonRole::Danger,true
                            ).clicked(){
                                discard_confirmed=true;
                            }
                        });
                    });
                });
            if discard_confirmed {return Some(Event::Closed);}
            if keep_editing {self.confirm_discard=false;}
        }
        if apply {
            if self.edit_text!=self.current_value() {
                self.error=Some("아직 반영되지 않은 입력 내용이 있습니다.".into());
            }else{
                self.confirm_apply=true;
            }
        }
        if self.confirm_apply {
            let mut cancel=false;
            let mut proceed=false;
            let apply_title=if self.direct_input{"직접 입력 완료"}else{"수동 분류 완료"};
            let apply_message=if self.direct_input{
                "직접 입력한 배송 정보와 품목을 발주서에 반영할까요?"
            }else{
                "수동 분류 결과를 발주서에 반영할까요?"
            };
            components::show_decision_window(ctx,apply_title,"manual-completion-prompt",work_area,|ui|{
                components::decision_workspace(ui,|decision|{
                    cancel=decision.header(apply_title,"취소",true
                    );
                    decision.body(|ui|{
                        ui.label(apply_message);
                    });
                    decision.actions(|right|{
                        if theme::action(
                            right,"적용",ButtonRole::Primary,true
                        ).clicked(){proceed=true;}
                    });
                });
            });
            if cancel {
                self.confirm_apply=false;
                self.focus_editor=true;
            }
            if proceed {
                self.confirm_apply=false;
                self.apply(ctx);
            }
        }
        // Raise ONLY the actual nested dialog after it exists. The parent's
        // disabled state plus the main Middle backdrop already protect
        // background controls; avoid a pointer-blocking Foreground overlay.
        if let Some(window_id)=self.nested_modal_window_id(){
            components::modal_foreground_for_window(ctx,window_id);
        }
        let hovered=manual_hover_target(
            hovered_detail,hovered_source.clone(),&spans
        );
        link_hover.update_for(&self.order_id,hovered,ctx);
        *source_hover=hovered_source.filter(|key|
            spans.iter().any(|span|span.key==*key)
        );
        if source_did_scroll {link_hover.scroll_to=None;}
        // Keep only actual failures in the existing common notice channel.
        // Routine edit/quantity/remote states remain in their own controls.
        let error=self.error.as_deref().map(|error|
            crate::user_error::display("reception.manual",error).into_owned());
        let remote_error=self.model.as_ref().and_then(|model|{
            let rows=list(model,"rows");
            visible_manual_shipment(self.selected_recipient,rows.len())
                .and_then(|index|rows.get(index))
                .and_then(|row|remote_for(&self.remote_rows,row))
                .and_then(|result|result.as_ref().err())
                .map(|error|format!("도서/산간 판정 실패 · {}",
                    crate::user_error::display("reception.remote",error)))
        });
        let quote_error=(self.model.is_some() && self.client_id.is_some()
            && self.view_quote.is_none() && !self.busy())
            .then_some("상품 금액과 운임을 확인할 수 없습니다.");
        let notice=[error.as_deref(),remote_error.as_deref(),quote_error]
            .into_iter().flatten().collect::<Vec<_>>().join("\n");
        crate::floating_notice::sync(ctx,"manual_workspace_error","reception.manual",
            crate::floating_notice::Tone::Error,(!notice.is_empty()).then_some(notice.as_str()));
        None
    }
}

/// Both the source document and destination cells resolve to the same
/// order-scoped LinkKey. Keep the shared 50/150ms HoverState as the single
/// owner of bidirectional highlight and source auto-scroll behavior.
fn manual_hover_target(
    hovered_detail:Option<source_highlight::LinkKey>,
    hovered_source:Option<source_highlight::LinkKey>,
    spans:&[source_highlight::LinkedSpan],
)->Option<source_highlight::LinkKey>{
    hovered_detail.or(hovered_source)
        .filter(|key|spans.iter().any(|span|span.key==*key))
}

#[cfg(test)]
mod manual_assignment_gesture_tests{
    use super::*;
    fn target()->ManualCellTarget{ManualCellTarget{row:0,item:None,field:"name".into()}}
    #[test]
    fn both_click_orders_and_drag_resolve_same_native_target(){
        let mut gesture=ManualAssignmentGesture::default();
        assert!(gesture.source_click("block".into()).is_none());
        assert_eq!(gesture.cell_click(target()),Some(("block".into(),target())));
        assert!(gesture.cell_click(target()).is_none());
        assert_eq!(gesture.source_click("block".into()),Some(("block".into(),target())));
        gesture.start_drag("block".into(),Some(egui::vec2(13.0,7.0)),320.0);
        assert_eq!(gesture.drag_anchor_offset,Some(egui::vec2(13.0,7.0)));
        assert_eq!(gesture.drag_layout_width,320.0);
        assert!(!gesture.drag_moved);
        gesture.mark_drag_moved();
        assert!(gesture.drag_moved);
        assert_eq!(gesture.finish_drag(Some(target())),Some(("block".into(),target())));
        gesture.start_drag("block".into(),None,0.0);
        assert!(!gesture.drag_moved);
        gesture.cancel_pending_drag();
        assert!(gesture.dragging.is_none());
        assert!(!gesture.drag_moved);
        assert!(gesture.drag_anchor_offset.is_none());
        assert_eq!(gesture.drag_layout_width,0.0);
    }
    #[test]
    fn holding_a_source_shows_a_pending_drag_but_release_without_movement_remains_a_click(){
        let mut gesture=ManualAssignmentGesture::default();
        gesture.start_drag("block".into(),None,0.0);
        assert_eq!(gesture.dragging.as_deref(),Some("block"));
        assert!(!gesture.drag_moved,
            "holding still must keep the gesture pending rather than dropping");
        gesture.cancel_pending_drag();
        assert!(gesture.source_click("block".into()).is_none());
        assert_eq!(gesture.cell_click(target()),Some(("block".into(),target())));
    }

    #[test]
    fn all_gestures_share_type_and_pending_quantity_guards(){
        let model=json!({"rows":[{"id":"r","items":[{"id":"i"}]}],
            "blocks":[{"id":"block","field":"name"}],"assignments":{}});
        assert_eq!(manual_assignment_payload(&model,"block",&target(),None).unwrap(),
            json!({"id":"block","cell":{"recipientId":"r","field":"name"}}));
        let wrong=ManualCellTarget{row:0,item:None,field:"address".into()};
        assert!(manual_assignment_payload(&model,"block",&wrong,None).is_err());
        let pending=ManualCellTarget{row:0,item:Some("i".into()),field:"quantity".into()};
        assert!(manual_assignment_payload(&model,"block",&target(),Some(&pending)).is_err());
        let mut assigned=model.clone();
        assigned["assignments"]["block"]=json!({"recipientId":"r","field":"name"});
        assert!(manual_assignment_payload(&assigned,"block",&target(),None).is_err());
    }

    #[test]
    fn drag_recommends_every_compatible_cell_and_only_those_cells(){
        let model=json!({
            "rows":[
                {"id":"r1","items":[{"id":"i1"}]},
                {"id":"r2","items":[{"id":"i2"}]}
            ],
            "blocks":[
                {"id":"name","field":"name"},
                {"id":"product","field":"product"},
                {"id":"quantity","field":"quantity"}
            ],
            "assignments":{}
        });
        let names=manual_assignment_recommendations(&model,"name",None);
        assert_eq!(names.len(),2);
        assert!(names.iter().all(|cell|cell.field=="name" && cell.item.is_none()));
        assert!(names.contains(&ManualCellTarget{row:0,item:None,field:"name".into()}));
        assert!(names.contains(&ManualCellTarget{row:1,item:None,field:"name".into()}));

        let products=manual_assignment_recommendations(&model,"product",None);
        assert_eq!(products.len(),2);
        assert!(products.iter().all(|cell|cell.field=="product" && cell.item.is_some()));
        let quantities=manual_assignment_recommendations(&model,"quantity",None);
        assert_eq!(quantities.len(),2);
        assert!(quantities.iter().all(|cell|cell.field=="quantity" && cell.item.is_some()));

        let wrong=ManualCellTarget{row:0,item:None,field:"address".into()};
        assert!(!names.contains(&wrong));
        let pending=ManualCellTarget{row:0,item:Some("i1".into()),field:"quantity".into()};
        assert_eq!(manual_assignment_recommendations(&model,"quantity",Some(&pending)),
            vec![pending.clone()]);
        assert!(manual_assignment_recommendations(&model,"product",Some(&pending)).is_empty());

        let mut assigned=model.clone();
        assigned["assignments"]["name"]=json!({"recipientId":"r1","field":"name"});
        assert!(manual_assignment_recommendations(&assigned,"name",None).is_empty());
    }
}

fn manual_cell_matches_source(
    item_selected:bool,selected_field:&str,source_field:&str
)->bool{
    if item_selected {
        ["product","quantity"].contains(&source_field) &&
            selected_field==source_field
    }else{
        ["name","address","mobile","phone","note"].contains(&source_field) &&
            selected_field==source_field
    }
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum EscapeAction {
    Ignore,
    CloseProductSearch,
    CancelShipmentRemoval,
    CancelReset,
    CancelApply,
    CancelDiscard,
    RevertCell,
    RequestClose,
}

fn escape_action(
    busy:bool,
    product_search:bool,
    remove_recipient:bool,
    reset:bool,
    apply_prompt:bool,
    discard:bool,
    cell_dirty:bool,
)->EscapeAction{
    if busy {EscapeAction::Ignore}
    else if product_search {EscapeAction::CloseProductSearch}
    else if remove_recipient {EscapeAction::CancelShipmentRemoval}
    else if reset {EscapeAction::CancelReset}
    else if apply_prompt {EscapeAction::CancelApply}
    else if discard {EscapeAction::CancelDiscard}
    else if cell_dirty {EscapeAction::RevertCell}
    else {EscapeAction::RequestClose}
}

fn nested_modal_id(
    product_search:bool,
    remove_recipient:bool,
    reset:bool,
    apply_prompt:bool,
    discard:bool,
)->Option<&'static str>{
    if product_search {Some("native-manual-product-search")}
    else if remove_recipient {Some("manual-shipment-removal")}
    else if reset {Some("manual-reset-confirm")}
    else if apply_prompt {Some("manual-completion-prompt")}
    else if discard {Some("manual-discard-prompt")}
    else {None}
}

#[cfg(test)]
mod manual_workspace_modal_routing_tests {
    use super::*;

    #[test]
    fn product_selection_requires_a_confirmed_quantity_before_it_is_complete(){
        assert!(item_quantity_pending(&json!({
            "productId":"p1","quantity":null,"quantityConfirmed":false
        })));
        assert!(item_quantity_pending(&json!({
            "name":"BD 709","quantity":0,"quantityConfirmed":false
        })));
        assert!(!item_quantity_pending(&json!({
            "quantity":2,"quantityConfirmed":true
        })));
        assert!(!item_quantity_pending(&json!({
            "productId":"p1","quantity":2,"quantityConfirmed":true
        })));
    }

    #[test]
    fn keyboard_cell_order_matches_the_manual_worksheet(){
        let model=json!({"rows":[
            {"id":"r1","items":[{"id":"i1"},{"id":"i2"}]},
            {"id":"r2","items":[{"id":"i3"}]}
        ]});
        let cells=manual_cell_targets(&model);
        let keys=cells.iter().map(|cell|(
            cell.row,cell.item.as_deref(),cell.field.as_str()
        )).collect::<Vec<_>>();
        assert_eq!(keys[0],(0,None,"name"));
        assert_eq!(keys[4],(0,None,"note"));
        assert_eq!(keys[5],(0,Some("i1"),"product"));
        assert_eq!(keys[6],(0,Some("i1"),"quantity"));
        assert_eq!(keys[7],(0,Some("i1"),"unitPrice"));
        assert_eq!(keys[10],(0,Some("i2"),"unitPrice"));
        assert_eq!(keys[11],(1,None,"name"));
    }

    #[test]
    fn arrows_move_to_visible_adjacent_cells_not_text_caret_positions(){
        let model=json!({"rows":[
            {"id":"r1","items":[{"id":"i1"},{"id":"i2"},{"id":"i3"}]},
            {"id":"r2","items":[{"id":"j1"}]}
        ]});
        let recipient=ManualCellTarget{
            row:0,item:None,field:"address".into()
        };
        let product=ManualCellTarget{
            row:0,item:Some("i2".into()),field:"quantity".into()
        };
        let move_to=|from:&ManualCellTarget,key|
            manual_navigation_target(&model,from,key,None);
        assert_eq!(move_to(&recipient,CellNavigationKey::Down),
            Some(ManualCellTarget{field:"mobile".into(),..recipient.clone()}));
        assert_eq!(move_to(&recipient,CellNavigationKey::Up),
            Some(ManualCellTarget{field:"name".into(),..recipient.clone()}));
        let last_recipient_field=ManualCellTarget{
            row:0,item:None,field:"note".into()
        };
        assert_eq!(move_to(&last_recipient_field,CellNavigationKey::Down),
            Some(ManualCellTarget{row:1,field:"name".into(),
                ..last_recipient_field.clone()}));
        assert_eq!(move_to(&product,CellNavigationKey::Up),
            Some(ManualCellTarget{item:Some("i1".into()),..product.clone()}));
        assert_eq!(move_to(&product,CellNavigationKey::Down),
            Some(ManualCellTarget{item:Some("i3".into()),..product.clone()}));
        assert_eq!(move_to(&product,CellNavigationKey::Left),
            Some(ManualCellTarget{field:"product".into(),..product.clone()}));
        assert_eq!(move_to(&product,CellNavigationKey::Right),
            Some(ManualCellTarget{field:"unitPrice".into(),..product.clone()}));
        assert_eq!(move_to(&recipient,CellNavigationKey::Left),None);
        let mobile=ManualCellTarget{
            row:0,item:None,field:"mobile".into()
        };
        let phone=ManualCellTarget{
            row:0,item:None,field:"phone".into()
        };
        assert_eq!(move_to(&mobile,CellNavigationKey::Right),Some(phone.clone()));
        assert_eq!(move_to(&phone,CellNavigationKey::Left),Some(mobile.clone()));
        assert_eq!(move_to(&phone,CellNavigationKey::Up),
            Some(ManualCellTarget{field:"address".into(),..phone.clone()}));
        assert_eq!(move_to(&mobile,CellNavigationKey::Down),
            Some(ManualCellTarget{field:"note".into(),..mobile.clone()}));
        assert_eq!(move_to(&last_recipient_field,CellNavigationKey::Up),
            Some(mobile));

        assert_eq!(move_to(&product,CellNavigationKey::Next),
            Some(ManualCellTarget{field:"unitPrice".into(),..product.clone()}));
        let first=ManualCellTarget{
            row:0,item:Some("i1".into()),field:"quantity".into()
        };
        assert_eq!(move_to(&first,CellNavigationKey::Up),None);
        let pending=Some(&product);
        assert_eq!(manual_navigation_target(
            &model,&product,CellNavigationKey::Down,pending
        ),None,"unconfirmed quantity must still block leaving its cell");
    }

    #[test]
    fn arrow_navigation_is_captured_before_text_edit_and_ime_is_respected(){
        let key_event=|key|egui::Event::Key{
            key,physical_key:None,pressed:true,repeat:false,
            modifiers:egui::Modifiers::NONE
        };
        let mut ime_active=false;
        let event=key_event(egui::Key::ArrowDown);
        assert_eq!(
            manual_navigation_key_before_editor(&[event.clone()],&mut ime_active)
                .map(|(_,_,navigation)|navigation),
            Some(CellNavigationKey::Down)
        );
        let ctx=egui::Context::default();
        let mut consumed=false;
        ctx.run(egui::RawInput{
            events:vec![event],..Default::default()
        },|ctx|{
            let key=ctx.input(|input|
                manual_navigation_key_before_editor(
                    &input.events,&mut ime_active
                )
            );
            let (key,modifiers,navigation)=key.expect("raw arrow event before editor");
            assert_eq!(navigation,CellNavigationKey::Down);
            consumed=ctx.input_mut(|input|
                input.consume_key(modifiers,key)
            );
            assert!(!ctx.input(|input|input.key_pressed(egui::Key::ArrowDown)),
                "TextEdit must not receive the consumed arrow event");
        });
        assert!(consumed);
        assert_eq!(manual_navigation_key_before_editor(
            &[egui::Event::Ime(egui::ImeEvent::Preedit("한".into()))],
            &mut ime_active
        ),None);
        assert!(ime_active);
        assert_eq!(
            manual_navigation_key_before_editor(
                &[key_event(egui::Key::ArrowUp)],&mut ime_active
            ),None,"IME composition spans multiple frames"
        );
        assert_eq!(manual_navigation_key_before_editor(
            &[egui::Event::Ime(egui::ImeEvent::Commit("한".into()))],
            &mut ime_active
        ),None);
        assert!(!ime_active);
        assert!(manual_navigation_key_before_editor(
            &[key_event(egui::Key::ArrowUp)],&mut ime_active
        ).is_some());
    }

    #[test]
    fn shipment_count_input_accepts_only_whole_numbers_from_one_to_one_hundred(){
        assert_eq!(parse_manual_row_count("1"),Ok(1));
        assert_eq!(parse_manual_row_count("12"),Ok(12));
        assert_eq!(parse_manual_row_count("100"),Ok(100));
        assert!(parse_manual_row_count("").is_err());
        assert!(parse_manual_row_count("0").is_err());
        assert!(parse_manual_row_count("101").is_err());
        assert!(parse_manual_row_count("1.5").is_err());
        assert!(parse_manual_row_count("-2").is_err());
    }

    #[test]
    fn shipment_removal_detects_values_and_source_assignments_in_every_removed_row(){
        let assignments=json!({"block-1":{"recipientId":"r2"}});
        assert!(!manual_row_has_data(
            &json!({"id":"r1","items":[{}]}),&assignments
        ));
        assert!(manual_row_has_data(
            &json!({"id":"r2","items":[{}]}),&assignments
        ));
        assert!(manual_row_has_data(
            &json!({"id":"r3","address":"강원도 홍천군","items":[{}]}),
            &assignments
        ));
        assert!(manual_row_has_data(
            &json!({"id":"r4","dispatchOrigin":"STORE","freightCode":"010"}),
            &assignments
        ));
        assert!(manual_row_has_data(
            &json!({"id":"r5","freightCode":"020"}),
            &assignments
        ));
        assert!(manual_row_has_data(
            &json!({"id":"r6","remoteDelivery":{"manual":true,"checked":false}}),
            &assignments
        ));
    }

    #[test]
    fn source_assignment_moves_to_the_next_missing_required_recipient_field(){
        let model=json!({"rows":[
            {"id":"r1","name":"홍길동","address":"","mobile":"010"},
            {"id":"r2","name":"","address":"","mobile":""}
        ]});
        let next=next_missing_recipient_cell(&model,0,"name").unwrap();
        assert_eq!(next,ManualCellTarget{
            row:0,item:None,field:"address".into()
        });
    }

    #[test]
    fn manual_undo_history_is_bounded_and_keeps_the_latest_frames(){
        let mut history=Vec::new();
        for value in 0..101{
            push_manual_history(&mut history,ManualHistoryFrame{
                model:json!({"value":value}),
                assignment_order:Vec::new(),
                remote_rows:Vec::new(),
                view_quote:None,
                selected_recipient:0,
                selected_item:None,
                selected_field:"name".into(),
            });
        }
        assert_eq!(history.len(),100);
        assert_eq!(history.first().unwrap().model["value"],1);
        assert_eq!(history.last().unwrap().model["value"],100);
        assert_eq!(history.pop().unwrap().model["value"],100);
    }

    #[test]
    fn shipment_count_is_one_date_selector_segmented_control(){
        let manual=include_str!("manual_workspace.rs");
        let components=include_str!("components.rs");
        let count_row=manual.split("let current_count=header_row_count").nth(1).unwrap()
            .split("let count_commit=").next().unwrap();
        assert_eq!(
            count_row.matches("components::date_navigation_count_control(").count(),1
        );
        assert!(!count_row.contains("date_navigation_step_button("));
        assert!(!count_row.contains("TextEdit::singleline("),
            "the count editor must live inside the shared segmented control");
        assert!(count_row.contains("count_control.decrement_clicked"));
        assert!(count_row.contains("count_control.increment_clicked"));
        assert!(count_row.contains("count_control.input"));
        let count_flow=manual.split("let current_count=header_row_count").nth(1).unwrap()
            .split("if count_resize_requested").next().unwrap();
        assert!(count_flow.contains("self.request_resize(count,ctx)"),
            "the native asynchronous resize flow must remain in place");

        let segmented=components.split("fn date_navigation_segmented_surface<R>(").nth(1).unwrap()
            .split("fn date_navigation_button_surface(").next().unwrap();
        assert!(segmented.contains("rect_filled("));
        assert!(segmented.contains("rect_stroke("));
        assert!(segmented.contains("StrokeKind::Inside"));

        let control=components.split("pub fn date_navigation_count_control(").nth(1).unwrap()
            .split("fn date_navigation_button_surface(").next().unwrap();
        for token in [
            "date_navigation_segmented_surface(",
            "date_navigation_segment_button(",
            "management_date_text_field_impl(",
            "FRAME_STROKE_WIDTH",
            "CALENDAR_NAV_SIZE",
            "theme::INPUT_HEIGHT"
        ]{
            assert!(control.contains(token),"missing segmented control detail: {token}");
        }
    }

    #[test]
    fn manual_scroll_surface_owns_quantity_and_freight_controls(){
        let source=include_str!("manual_workspace.rs");
        let layout=source.split("let body_height=panel.available_height()")
            .nth(1).expect("right card scroll height comes from the panel")
            .split("let input_settled=").next().unwrap();
        assert!(layout.contains("manual-reception-style-cards"),
            "all cards must be in the right independent scroll area");
        assert!(layout.contains("ScrollBarVisibility::VisibleWhenNeeded"));
        assert!(layout.contains("reception_freight_row_editable("),
            "freight controls must use the Reception component in the scroll");
        assert!(layout.contains("reception_item_origin_action_editable("),
            "dispatch origin must stay inside the product/freight card");
        assert!(layout.contains("self.selected_field==\"shippingCharge\""),
            "freight direct editing must remain accessible");
        let editor=source.split("fn selected_cell_editor_on_rect(").nth(1)
            .unwrap().split("fn queue_cell_navigation(").next().unwrap();
        assert!(editor.contains("ui.scroll_to_rect(rect,"),
            "focused cells must scroll into view");
    }

    #[test]
    fn manual_cards_reuse_the_real_reception_renderers(){
        // Stop at the first production/test boundary by its module name.
        // Avoid newline-sensitive matching: Windows may check out CRLF.
        let source=include_str!("manual_workspace.rs")
            .split("mod manual_assignment_gesture_tests{")
            .next().expect("production/test source boundary");
        let render=source.split("pub fn show(").nth(1).unwrap()
            .split("fn manual_hover_target(").next().unwrap();
        assert_eq!(render.matches("reception_section_with_header(").count(),3);
        for (ui,title) in [("source_ui","발주 원문"),("sheet","수취인별 주문 정보"),("sheet","품목 및 운임")] {
            let compact=render.split_whitespace().collect::<String>();
            let call=format!("components::reception_section_with_header({ui},\"{}\",",title.replace(' ',""));
            assert!(compact.contains(&call),"missing shared reception section: {title}");
        }
        for common in [
            "reception_recipient_fields(",
            "reception_editable_items_header(",
            "reception_item_row_with_remove(",
            "reception_item_add_action_editable(",
            "reception_freight_row_editable(",
            "reception_item_origin_action_editable(",
            "reception_recipient_navigator("
        ] {
            assert!(render.contains(common),"{common} is not reused");
        }
        // Compare the CURRENT intake master, not merely the presence of a
        // legacy helper that happens to share a lower-level painter.
        let intake=include_str!("main.rs");
        for shared in ["reception_editable_items_header(","reception_item_row_with_remove(",
            "reception_item_numeric_editor(","reception_freight_row_editable(",
            "show_selection_modal("]{
            assert!(intake.contains(shared)&&source.contains(shared),
                "intake/manual editable contract drift: {shared}");
        }
        assert!(!render.contains("reception_items_header_with_amount_label("));
        assert!(!render.contains("reception_item_row_editable("));
        assert!(!render.contains("\"step_manual_quantity\""));
        assert!(render.contains("map(crate::indexed_quote)") &&
            intake.contains("map(indexed_quote)"));
        assert!(render.contains("quote.item(item_id)") &&
            intake.contains("quote.item(item_id)"));
        assert!(render.contains("crate::indexed_billable_freight(") &&
            intake.contains("indexed_billable_freight("));
        assert!(render.contains("field!=\"product\" && self.cell_editor_open()"));
        assert!(render.contains("item_editor::product_click_intent("));
        assert!(intake.contains("item_editor::product_click_intent("));
        assert!(source.contains("ItemEditor::new("));
        assert!(!render.contains("interaction.double_clicked()"));
        assert!(!source.contains("ProductSearch::new("));
        assert!(source.contains("components::reception_numeric_edit_finished("));
        assert!(render.contains("self.focus_editor=!blur_commit"));
        assert!(render.contains("reception_item_delete_confirmation("));
        assert!(source.contains("add_selected_manual_product(service.as_ref()"));
        assert!(source.contains("ReadOperation::CalculateReceptionQuote"));
        assert!(!render.contains("manual_linked_table_cell("));
        assert!(!render.contains("manual_table_heading("));
        assert!(!render.contains("egui::Grid::new((\"manual-products\""));
        assert!(render.contains("selected_cell_editor_on_rect("),
            "direct edits must overlay the shared input cell");
        let navigator=render.split("let mut next_recipient=None;").nth(1).unwrap()
            .split("},|fields|{").next().unwrap();
        assert!(navigator.contains("reception_recipient_navigator("));
        assert!(navigator.contains("rows.len(),can_switch"));
        assert!(!navigator.contains("add_enabled_ui("),
            "only arrow segments may be disabled; name and 1/N stay visible");
    }

    #[test]
    fn manual_editor_keeps_one_delivery_visible_without_dropping_other_rows(){
        assert_eq!(visible_manual_shipment(0,0),None);
        assert_eq!(visible_manual_shipment(0,3),Some(0));
        assert_eq!(visible_manual_shipment(1,3),Some(1));
        assert_eq!(visible_manual_shipment(2,3),Some(2));
        assert_eq!(visible_manual_shipment(9,3),Some(2));
    }

    #[test]
    fn source_drag_feedback_starts_on_press_and_uses_a_cursor_following_preview(){
        let source_view=include_str!("manual_source_view.rs");
        let workspace=include_str!("manual_workspace.rs");
        assert!(source_view.contains("pointer.button_pressed(egui::PointerButton::Primary)"),
            "drag feedback must start on primary press");
        assert!(source_view.contains("response.dragged_by(egui::PointerButton::Primary)"),
            "actual drag classification must retain egui's native threshold");
        assert!(workspace.contains("interaction.drag_moved"));
        assert!(workspace.contains("drag_anchor_offset"));
        assert!(workspace.contains("pointer-anchor_offset"));
        assert!(workspace.contains("paint_manual_source_drag_preview("));
    }

    const PALETTE_RAW:&str="김준희 김준희 서울";

    fn palette_model(assignments:Value)->Value{
        json!({
            "rows":[{"id":"r1","name":"","items":[]},{"id":"r2","name":"","items":[]}],
            "blocks":[
                {"id":"source-a","start":0,"end":3,"text":"김준희","targetId":"r1","field":"name"},
                {"id":"source-b","start":4,"end":7,"text":"김준희","targetId":"r1","field":"name"},
                {"id":"source-c","start":8,"end":10,"text":"서울","targetId":"r1","field":"address"}
            ],
            "assignments":assignments
        })
    }

    fn assert_palette_source(data:&ManualRenderData,id:&str,owner:&str,field:&str,classified:bool){
        let span=data.spans.iter().find(|span|span.source_id.as_deref()==Some(id))
            .expect("exact native source ID must survive projection");
        assert_eq!(span.key,source_highlight::key(owner,owner,field));
        assert_eq!(data.assigned_ids.contains(id),classified);
        let style=source_highlight::manual_source_block_style(span,None,&data.assigned_ids);
        let (fill,ink,border)=if classified{
            (theme::MANUAL_SOURCE_CLASSIFIED_FILL,theme::MANUAL_SOURCE_CLASSIFIED_TEXT,
                theme::MANUAL_SOURCE_CLASSIFIED_BORDER)
        }else{
            (theme::MANUAL_SOURCE_UNASSIGNED_FILL,theme::MANUAL_SOURCE_UNASSIGNED_TEXT,
                theme::MANUAL_SOURCE_UNASSIGNED_BORDER)
        };
        assert_eq!((style.fill,style.text,style.border),(fill,ink,border));
    }

    #[test]
    fn manual_source_palette_reads_exact_native_assignment_ids(){
        let model=palette_model(json!({
            "source-a":{"recipientId":"r2","field":"name"},
            "source-b":null,"source-c":[],
            "not-an-object":"r2","also-not-an-object":true,"number":1
        }));
        let data=ManualRenderData::new(PALETTE_RAW,&model);
        assert_eq!(data.assigned_ids,std::collections::HashSet::from(["source-a".to_owned()]));
        assert_eq!(data.spans.len(),3);
        assert_palette_source(&data,"source-a","r2","name",true);
        assert_palette_source(&data,"source-b","r1","name",false);
        assert_palette_source(&data,"source-c","r1","address",false);

        // Even identical text linked to the same destination must not
        // cause another source block to be marked classified.
        let same_target=ManualRenderData::new(PALETTE_RAW,&palette_model(json!({
            "source-a":{"recipientId":"r1","field":"name"}
        })));
        assert_palette_source(&same_target,"source-a","r1","name",true);
        assert_palette_source(&same_target,"source-b","r1","name",false);
    }

    // No native worker or operational DB: deliver synthetic native outcomes
    // through the real receiver and render the actual workspace headlessly.
    fn palette_test_workspace()->ManualWorkspace{
        ManualWorkspace{
            order_id:"synthetic-palette-order".into(),direct_input:false,
            raw:PALETTE_RAW.into(),order_type:"".into(),
            client_id:None,gateway:Arc::new(crate::gateway::LocalGateway),base_order:None,
            cached_quote:None,model:None,render_data:None,
            assignment_order:Vec::new(),remote_rows:Vec::new(),view_quote:None,
            selected_recipient:0,selected_item:None,selected_field:"name".into(),
            assignment_gesture:ManualAssignmentGesture::default(),
            edit_text:String::new(),focus_editor:false,active_editor_id:None,
            ime_preedit_active:false,row_count_text:"1".into(),
            history:Vec::new(),pending_select_after_apply:None,
            advance_after_apply:None,last_action:None,action_pending:false,
            confirm_discard:false,confirm_apply:false,
            item_detail:None,pending_item_detail:None,detail_write_pending:false,
            add_product_picker:None,item_remove_confirmation:None,
            remove_recipient_pending:None,remove_recipient_target:None,reset_pending:false,
            receiver:None,error:None,dirty:false,applied:None,
        }
    }

    fn deliver_palette_outcome(workspace:&mut ManualWorkspace,outcome:Outcome){
        let (tx,rx)=mpsc::channel();
        assert!(tx.send(Ok(outcome)).is_ok());
        workspace.receiver=Some(rx);
    }

    fn palette_frame(workspace:&mut ManualWorkspace,ctx:&Context)
        ->std::rc::Rc<ManualRenderData>{
        let area=egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1600.0,1000.0));
        let mut hover=source_highlight::HoverState::default();
        let mut source_hover=None;
        let _=ctx.run(egui::RawInput{screen_rect:Some(area),..Default::default()},|ctx|{
            assert!(workspace.show(ctx,area,360.0,&mut hover,&mut source_hover).is_none());
        });
        assert!(!workspace.busy(),"headless frame must not dispatch a business request");
        workspace.render_data.as_ref().expect("workspace must render its native model").clone()
    }

    #[test]
    fn manual_source_palette_cache_reuses_frames_and_refreshes_after_assignment_release_and_undo(){
        let ctx=components::test_context();
        let mut workspace=palette_test_workspace();
        let empty=palette_model(json!({}));
        let assigned=palette_model(json!({"source-a":{"recipientId":"r2","field":"name"}}));
        deliver_palette_outcome(&mut workspace,Outcome::Initial{
            order:json!({}),model:empty.clone(),assignment_order:Vec::new(),
            remote_rows:Vec::new(),view_quote:None
        });
        let initial=palette_frame(&mut workspace,&ctx);
        assert_palette_source(&initial,"source-a","r1","name",false);
        assert!(std::rc::Rc::ptr_eq(&initial,&palette_frame(&mut workspace,&ctx)));

        deliver_palette_outcome(&mut workspace,Outcome::Updated{
            model:assigned,assignment_order:vec!["source-a".into()],
            remote_rows:Vec::new(),view_quote:None
        });
        let after_assign=palette_frame(&mut workspace,&ctx);
        assert!(!std::rc::Rc::ptr_eq(&initial,&after_assign));
        assert_palette_source(&after_assign,"source-a","r2","name",true);
        assert_palette_source(&after_assign,"source-b","r1","name",false);
        assert!(std::rc::Rc::ptr_eq(&after_assign,&palette_frame(&mut workspace,&ctx)));

        deliver_palette_outcome(&mut workspace,Outcome::Updated{
            model:empty,assignment_order:Vec::new(),remote_rows:Vec::new(),view_quote:None
        });
        let after_release=palette_frame(&mut workspace,&ctx);
        assert!(!std::rc::Rc::ptr_eq(&after_assign,&after_release));
        assert_palette_source(&after_release,"source-a","r1","name",false);

        workspace.undo();
        let undo_release=palette_frame(&mut workspace,&ctx);
        assert!(!std::rc::Rc::ptr_eq(&after_release,&undo_release));
        assert_palette_source(&undo_release,"source-a","r2","name",true);
        assert_eq!(workspace.assignment_order,vec!["source-a".to_owned()]);

        workspace.undo();
        let undo_assign=palette_frame(&mut workspace,&ctx);
        assert!(!std::rc::Rc::ptr_eq(&undo_release,&undo_assign));
        assert_palette_source(&undo_assign,"source-a","r1","name",false);
        assert_palette_source(&undo_assign,"source-b","r1","name",false);
        assert!(workspace.assignment_order.is_empty());
        // Held snapshots remain immutable through subsequent model updates.
        assert_palette_source(&after_assign,"source-a","r2","name",true);
    }

    #[test]
    fn manual_source_and_destination_hover_share_the_same_link_identity(){
        let key=source_highlight::key("recipient-1","item-1","product");
        let span=source_highlight::LinkedSpan{
            start:0,end:6,start_char:0,end_char:6,
            key:key.clone(),source_id:Some("0:6".into())
        };
        let spans=vec![span];
        assert_eq!(
            manual_hover_target(Some(key.clone()),None,&spans),
            Some(key.clone()),
            "hovering the destination cell must highlight and scroll to its source"
        );
        assert_eq!(
            manual_hover_target(None,Some(key.clone()),&spans),
            Some(key.clone()),
            "hovering the source block must highlight its destination cell"
        );
        let stale=source_highlight::key("recipient-old","item-1","product");
        assert_eq!(manual_hover_target(None,Some(stale),&spans),None);
    }

    #[test]
    fn source_block_assignment_requires_a_matching_cell_kind(){
        assert!(manual_cell_matches_source(false,"name","name"));
        assert!(manual_cell_matches_source(false,"address","address"));
        assert!(!manual_cell_matches_source(false,"unitPrice","product"));
        assert!(!manual_cell_matches_source(false,"name","quantity"));
        assert!(manual_cell_matches_source(true,"product","product"));
        assert!(manual_cell_matches_source(true,"quantity","quantity"));
        assert!(!manual_cell_matches_source(true,"product","name"));
    }

    #[test]
    fn every_nested_manual_dialog_uses_the_real_foreground_window(){
        assert_eq!(nested_modal_id(true,false,false,false,false),
            Some("native-manual-product-search"));
        assert_eq!(nested_modal_id(false,true,false,false,false),
            Some("manual-shipment-removal"));
        assert_eq!(nested_modal_id(false,false,true,false,false),
            Some("manual-reset-confirm"));
        assert_eq!(nested_modal_id(false,false,false,true,false),
            Some("manual-completion-prompt"));
        assert_eq!(nested_modal_id(false,false,false,false,true),
            Some("manual-discard-prompt"));
        assert_eq!(nested_modal_id(false,false,false,false,false),None);
        let source=include_str!("manual_workspace.rs");
        let render=source.split("pub fn show(").nth(1).unwrap()
            .split("fn manual_hover_target(").next().unwrap();
        assert!(render.contains("modal_foreground_for_window(ctx,window_id)"));
        assert!(!render.contains("modal_backdrop_for_window("));
        assert!(render.contains("if apply_clicked{apply=true;}"));
        assert!(render.contains("right,\"적용\",ButtonRole::Primary,true"));
        assert!(!render.contains("피드백 없이 진행"));
        assert!(!render.contains("AI 자동분류 개선"));
        assert!(render.contains("if proceed"));
        assert!(render.contains("self.confirm_apply=false;"));
        assert!(render.contains("self.apply(ctx);"),
            "the native apply action must be reached from the explicit proceed branch");
        assert!(!render.contains("초안 적용 후 주문 확인 필요"),
            "the generic order-confirmation hint must not occupy a header row");
        assert!(!render.contains("status_label.on_hover_text(&notice)"));
        assert!(render.contains("crate::floating_notice::sync(ctx,\"manual_workspace_error\""),
            "actual failures use the common notice instead of a hidden tooltip");
    }

    #[test]
    fn escape_closes_only_the_topmost_nested_prompt(){
        assert_eq!(escape_action(false,true,false,false,false,false,false),
            EscapeAction::CloseProductSearch);
        assert_eq!(escape_action(false,false,true,false,false,false,false),
            EscapeAction::CancelShipmentRemoval);
        assert_eq!(escape_action(false,false,false,true,false,false,false),
            EscapeAction::CancelReset);
        assert_eq!(escape_action(false,false,false,false,true,false,false),
            EscapeAction::CancelApply);
        assert_eq!(escape_action(false,false,false,false,false,true,false),
            EscapeAction::CancelDiscard);
    }

    #[test]
    fn escape_reverts_uncommitted_cell_before_requesting_workspace_close(){
        assert_eq!(escape_action(false,false,false,false,false,false,true),
            EscapeAction::RevertCell);
        assert_eq!(escape_action(false,false,false,false,false,false,false),
            EscapeAction::RequestClose);
        assert_eq!(escape_action(true,false,false,false,false,false,true),
            EscapeAction::Ignore);
    }

    #[test]
    fn pending_quantity_status_reports_the_exact_number_of_items(){
        let ready=json!({"rows":[{"items":[
            {"productId":"p1","quantity":2,"quantityConfirmed":true}
        ]}]});
        let pending=json!({"rows":[{"items":[
            {"productId":"p1","quantity":null,"quantityConfirmed":false},
            {"name":"BD 709","quantity":0,"quantityConfirmed":false},
            {"quantity":null}
        ]}]});
        assert_eq!(model_pending_quantity_count(&ready),0);
        assert_eq!(model_pending_quantity_count(&pending),2);
    }
}

#[cfg(test)]
mod manual_pending_interaction_tests {
    use super::*;

    #[test]
    fn pending_quantity_locks_other_cells_but_keeps_its_own_cell_editable(){
        let pending=ManualCellTarget{
            row:1,item:Some("item-2".into()),field:"quantity".into()
        };
        let same=ManualCellTarget{
            row:1,item:Some("item-2".into()),field:"quantity".into()
        };
        let other_field=ManualCellTarget{
            row:1,item:Some("item-2".into()),field:"product".into()
        };
        let other_item=ManualCellTarget{
            row:1,item:Some("item-3".into()),field:"quantity".into()
        };
        let other_recipient=ManualCellTarget{
            row:0,item:Some("item-2".into()),field:"quantity".into()
        };
        assert!(manual_cell_allowed(Some(&pending),&same));
        assert!(!manual_cell_allowed(Some(&pending),&other_field));
        assert!(!manual_cell_allowed(Some(&pending),&other_item));
        assert!(!manual_cell_allowed(Some(&pending),&other_recipient));
        assert!(manual_cell_allowed(None,&other_field));
    }

    #[test]
    fn source_assignment_guard_matches_react_use_block_contract(){
        // No active product/quantity hand-off: any valid block can be used.
        assert!(manual_source_assignment_allowed(false,None,None,"product"));
        assert!(manual_source_assignment_allowed(true,Some("item-1"),None,"name"));

        // An already assigned block on the same pending item may be released.
        assert!(manual_source_assignment_allowed(
            true,Some("item-1"),Some("item-1"),"product"
        ));
        // A block owned by another item or recipient must not be released.
        assert!(!manual_source_assignment_allowed(
            true,Some("item-2"),Some("item-1"),"product"
        ));
        assert!(!manual_source_assignment_allowed(
            true,None,Some("item-1"),"name"
        ));
        // Only the matching quantity evidence can complete the pending item.
        assert!(manual_source_assignment_allowed(
            false,None,Some("item-1"),"quantity"
        ));
        assert!(!manual_source_assignment_allowed(
            false,None,Some("item-1"),"product"
        ));
    }

    #[test]
    fn any_unconfirmed_product_quantity_blocks_final_apply(){
        let ready=json!({"rows":[{"items":[
            {"productId":"p1","quantity":2,"quantityConfirmed":true}
        ]}]});
        let pending=json!({"rows":[{"items":[
            {"productId":"p1","quantity":null,"quantityConfirmed":false}
        ]}]});
        let empty=json!({"rows":[{"items":[{"quantity":null}]}]});
        assert!(!model_has_pending_quantity(&ready));
        assert!(model_has_pending_quantity(&pending));
        assert!(!model_has_pending_quantity(&empty));
    }
}

#[cfg(test)]
mod manual_remote_row_tests {
    use super::*;
    #[test]
    fn freight_edits_reuse_remote_decisions_but_address_override_and_error_do_not(){
        let old=json!({"id":"r1","address":"제주특별자치도 제주시 1"});
        let model=json!({"rows":[old.clone()]});
        let cached=json!({"checked":true});
        let rows=vec![("r1".to_owned(),text(&old,"address").to_owned(),Ok(cached.clone()))];
        let mut changed=old.clone();
        changed["shippingCharges"]=json!({"STORE":4000});
        assert_eq!(reusable_remote_decision(&model,&rows,&changed),Some(&cached));
        changed["remoteDelivery"]=json!({"checked":false,"manual":true});
        assert!(reusable_remote_decision(&model,&rows,&changed).is_none());
        changed=old.clone();
        changed["address"]=json!("서울특별시 1");
        assert!(reusable_remote_decision(&model,&rows,&changed).is_none());
        let failed=vec![("r1".to_owned(),text(&old,"address").to_owned(),Err("failed".to_owned()))];
        assert!(reusable_remote_decision(&model,&failed,&old).is_none());
    }
    #[test]
    fn decision_is_bound_to_both_recipient_and_exact_address(){
        let row=json!({"id":"r1","address":"제주특별자치도 제주시 1"});
        let reference=vec![("r1".to_owned(),"제주특별자치도 제주시 1".to_owned(),
            Ok(json!({"status":"auto","checked":true})))];
        let decision=remote_for(&reference,&row)
            .and_then(|result|result.as_ref().ok()).unwrap();
        assert_eq!(decision["checked"],true);
        assert!(remote_for(&reference,&json!({"id":"r1","address":"서울특별시 1"}))
            .is_none());
        assert!(remote_for(&reference,&json!({
            "id":"r2","address":"제주특별자치도 제주시 1"
        })).is_none());
    }
}

#[cfg(test)]
mod quote_reuse_tests {
    use super::*;

    fn sample()->Value {
        json!({
            "id":"draft","applyVat":false,
            "recipients":[{
                "id":"r1","name":"원본 이름","address":"서울특별시 중구",
                "mobile":"010-1234-5678","phone":"","note":"문 앞",
                "freightCode":"010","dispatchOrigin":"FACTORY",
                "shippingCharges":{"FACTORY":3000},
                "remoteDelivery":{"address":"서울특별시 중구",
                    "checked":false,"manual":false},
                "vatOverride":false,"vatRateBps":1000,
                "items":[{
                    "id":"i1","productId":"p1","variantId":"v1",
                    "quantity":2,"unitPrice":6000,"origin":"FACTORY"
                }]
            }]
        })
    }

    #[test]
    fn contact_edits_do_not_change_native_price_inputs(){
        let original=sample();
        for field in ["name","mobile","phone","note"]{
            let mut updated=original.clone();
            updated["recipients"][0][field]=json!("직원 수정");
            assert_eq!(
                price_input_snapshot(&original),price_input_snapshot(&updated),
                "editing {field} must not refresh the invoice"
            );
        }
        let mut source_labels=original.clone();
        source_labels["recipients"][0]["sources"]=json!([{"field":"name"}]);
        assert_eq!(price_input_snapshot(&original),price_input_snapshot(&source_labels));
    }

    #[test]
    fn financial_changes_invalidate_the_prior_quote(){
        let original=sample();
        for (path,value) in [
            ("address",json!("제주특별자치도 제주시")),
            ("dispatchOrigin",json!("STORE")),
            ("freightCode",json!("020")),
            ("shippingCharges",json!({"FACTORY":4500})),
            ("remoteDelivery",json!({"checked":true,"manual":true})),
            ("vatOverride",json!(true)),
            ("vatRateBps",json!(2000)),
        ]{
            let mut updated=original.clone();
            updated["recipients"][0][path]=value;
            assert_ne!(price_input_snapshot(&original),
                price_input_snapshot(&updated),"{path}");
        }
        for (path,value) in [
            ("productId",json!("p2")),("variantId",json!("v2")),
            ("quantity",json!(3)),("unitPrice",json!(7300)),
            ("manualUnitPrice",json!(7200)),("origin",json!("STORE"))
        ]{
            let mut updated=original.clone();
            updated["recipients"][0]["items"][0][path]=value;
            assert_ne!(price_input_snapshot(&original),
                price_input_snapshot(&updated),"{path}");
        }
        let mut vat=original.clone();
        vat["applyVat"]=json!(true);
        assert_ne!(price_input_snapshot(&original),price_input_snapshot(&vat));
    }

    #[test]
    fn existing_complete_quote_is_reused_without_arithmetic(){
        let mut updated=sample();
        updated["recipients"][0]["note"]=json!("관리 메모만 수정됨");
        let saved=json!({
            "authorityRevision":9,"sourceRaw":"원문","orderId":"draft",
            "clientId":"c1","orderType":"AGENCY",
            "shippingFee":3000,"shippingVersion":2,
            "subtotal":12000,"vatAmount":0,"total":15000,
            "pendingRecipients":[],
            "linePrices":[{"itemId":"i1","amount":12000}],
            "freightLines":[{"recipientId":"r1","origin":"FACTORY",
                "appliedCharge":3000,"billableCharge":3000}]
        });
        let restored=reuse_native_quote(Some(&saved),&updated).unwrap();
        assert_eq!(restored["totals"]["total"],15000);
        assert_eq!(restored["totals"]["pendingRecipients"],json!([]));
        assert_eq!(restored["linePrices"],saved["linePrices"]);
        assert_eq!(restored["freightLines"],saved["freightLines"]);
        assert_eq!(restored["order"]["recipients"][0]["note"],
            "관리 메모만 수정됨");
        assert_eq!(restored["order"]["shippingFee"],3000);
    }

    #[test]
    fn legacy_incomplete_quote_requires_native_recalculation(){
        let order=sample();
        let stale=json!({
            "authorityRevision":1,"linePrices":[{"itemId":"i1"}],
            "freightLines":[]
        });
        assert!(reuse_native_quote(Some(&stale),&order).is_none());
        assert!(reuse_native_quote(None,&order).is_none());
    }
}


#[cfg(test)]
mod inline_recognition_identity_tests{
    use super::*;

    #[test]
    fn inline_edit_persists_order_scoped_recipient_and_item_ids(){
        let gateway=crate::gateway::LocalGateway;
        let raw="김가상".to_owned();
        let recognition=json!({
            "recipients":[{"id":"recognition:recipient:0","name":"김가상",
                "address":"서울특별시 강남구 테헤란로 1","phone":"",
                "mobile":"010-1111-2222","note":"","sources":[],
                "items":[{"id":"recognition:item:0:0","raw":"709 1개",
                    "name":"709","quantity":1,"quantityConfirmed":true,
                    "productId":"p709","variantId":"v709","issues":[],
                    "sources":[]}]}],
            "candidates":[{"id":"candidate-1",
                "recipientId":"recognition:recipient:0",
                "itemId":"recognition:item:0:0"}],
            "unassigned":[]
        });
        let source=WorkingReception{
            id:"draft-identity".into(),raw:raw.clone(),order_type:"AGENCY".into(),
            source_input_method:None,last_input_method:None,
            analysis_raw:Some(raw),analysis_evidence:Some(recognition),
            analysis_view:None,client_match:None,chat_handle:None,chat_title:None,
            client_id:None,client_selection:None,pricing_preview:None,
            working_version:None,working_synced:false,manual_model:None,
            edit_history:Vec::new(),
        };
        // Initial recognition and old persisted display snapshots use the
        // provisional ID. Exercise both forms, not only the normalized one.
        for recipient_id in ["recognition:recipient:0","draft-identity:recognition:recipient:0"] {
        let event=apply_inline(&gateway,&source,&InlineRequest{
            recipient_id:recipient_id.into(),
            item_id:None,field:"note".into(),value:json!("직원 메모")
        }).expect("inline cell edit");
        let Event::Applied{order,model,..}=event else{
            panic!("inline edit must return an applied model");
        };
        assert_eq!(order["recipients"][0]["id"],
            "draft-identity:recognition:recipient:0");
        assert_eq!(order["recipients"][0]["items"][0]["id"],
            "draft-identity:recognition:item:0:0");
        assert_eq!(model["rows"][0]["id"],
            "draft-identity:recognition:recipient:0");
        assert_eq!(model["rows"][0]["items"][0]["id"],
            "draft-identity:recognition:item:0:0");
        assert_eq!(order["analysisEvidence"][0]["recipientId"],
            "draft-identity:recognition:recipient:0");
        assert_eq!(order["analysisEvidence"][0]["itemId"],
            "draft-identity:recognition:item:0:0");
        assert_eq!(order["recipients"][0]["note"],"직원 메모");
        let mut legacy=source.clone();
        // Legacy model IDs are normalized at the same boundary.
        let mut saved=model.clone();
        saved["rows"][0]["id"]=json!("recognition:recipient:0");
        saved["rows"][0]["items"][0]["id"]=json!("recognition:item:0:0");
        legacy.manual_model=Some(json!({"model":saved,"assignmentOrder":[]}));
        let edited=apply_inline(&gateway,&legacy,&InlineRequest{
            recipient_id:"recognition:recipient:0".into(),
            item_id:Some("recognition:item:0:0".into()),field:"quantity".into(),value:json!("3")
        }).expect("legacy quantity edit");
        let Event::Applied{order,..}=edited else{panic!("quantity edit");};
        assert_eq!(order["recipients"][0]["items"][0]["quantity"],3);
        let freight=apply_inline(&gateway,&source,&InlineRequest{
            recipient_id:"recognition:recipient:0".into(),
            item_id:None,field:"shippingCharge".into(),value:json!(5000)
        }).expect("provisional freight target");
        let Event::Applied{order,..}=freight else{panic!("freight edit");};
        assert_eq!(order["recipients"][0]["id"],"draft-identity:recognition:recipient:0");
        // Never guess another order's row, or silently choose the first.
        assert!(apply_inline(&gateway,&source,&InlineRequest{
            recipient_id:"another-order:recognition:recipient:0".into(),
            item_id:None,field:"note".into(),value:json!("wrong order")
        }).is_err());
        }
    }
}

#[cfg(test)]
mod direct_input_workspace_tests{
    use super::*;

    #[test]
    fn direct_input_has_one_initial_shipment_and_no_source_evidence(){
        let gateway=crate::gateway::LocalGateway;
        let row=WorkingReception{
            id:"direct-test".into(),raw:String::new(),order_type:"AGENCY".into(),
            source_input_method:None,last_input_method:None,
            analysis_raw:None,analysis_evidence:None,analysis_view:None,
            client_match:None,chat_handle:None,chat_title:None,
            client_id:None,client_selection:None,pricing_preview:None,
            working_version:None,working_synced:false,manual_model:None,
            edit_history:Vec::new(),
        };
        let order=native_direct_order(&gateway,&row).expect("direct order");
        assert_eq!(order["raw"],"");
        assert!(order.get("analysisEvidence").is_none());
        assert!(order.get("analysisRaw").is_none());
        let initial=rule(&gateway,json!({
            "action":"initial_manual","order":order,"assignmentOrder":[]
        })).expect("initial direct model");
        let resized=rule(&gateway,json!({
            "action":"resize_manual_rows","model":initial["model"],
            "count":1,"newRecipientIds":["direct-recipient"],
            "assignmentOrder":initial["assignmentOrder"]
        })).expect("one editable recipient");
        assert_eq!(list(&resized["model"],"rows").len(),1);
        assert_eq!(list(&resized["model"],"blocks").len(),0);
        assert_eq!(text(&resized["model"]["rows"][0],"id"),"direct-recipient");
    }

    #[test]
    fn direct_input_rejects_saved_source_blocks(){
        let gateway=crate::gateway::LocalGateway;
        let model=json!({"rows":[{"id":"r1","name":"x","items":[]}],
            "blocks":[{"id":"b1"}],"assignments":{}});
        let row=WorkingReception{
            id:"direct-invalid".into(),raw:String::new(),order_type:"AGENCY".into(),
            source_input_method:None,last_input_method:None,
            analysis_raw:None,analysis_evidence:None,analysis_view:None,
            client_match:None,chat_handle:None,chat_title:None,
            client_id:None,client_selection:None,pricing_preview:None,
            working_version:None,working_synced:false,
            manual_model:Some(json!({"mode":"DIRECT","sourceRaw":"","orderType":"AGENCY",
                "clientId":null,"model":model,"assignmentOrder":[]})),
            edit_history:Vec::new(),
        };
        assert!(native_direct_order(&gateway,&row).is_err());
    }

    #[test]
    fn direct_input_reopens_the_saved_manual_model_without_recreating_evidence(){
        let gateway=crate::gateway::LocalGateway;
        let model=json!({
            "rows":[{"id":"r1","name":"직접 입력 고객","address":"","phone":"",
                "mobile":"","note":"","items":[]}],
            "blocks":[],"assignments":{}
        });
        let row=WorkingReception{
            id:"direct-resume".into(),raw:String::new(),order_type:"AGENCY".into(),
            source_input_method:None,last_input_method:None,
            analysis_raw:None,analysis_evidence:None,analysis_view:None,
            client_match:None,chat_handle:None,chat_title:None,
            client_id:None,client_selection:None,pricing_preview:None,
            working_version:None,working_synced:false,
            manual_model:Some(json!({"mode":"DIRECT","sourceRaw":"",
                "orderType":"AGENCY","clientId":null,"model":model,
                "assignmentOrder":[]})),
            edit_history:Vec::new(),
        };
        let order=native_direct_order(&gateway,&row).expect("restored direct order");
        assert_eq!(order["recipients"][0]["name"],"직접 입력 고객");
        assert_eq!(order["manualClassification"]["blocks"],json!([]));
        assert!(order.get("analysisEvidence").is_none());
    }

    #[test]
    fn direct_layout_uses_the_same_sheet_without_allocating_a_source_column(){
        assert_eq!(manual_workspace_content_width_for_mode(900.0,360.0,true),900.0);
        assert_eq!(manual_workspace_content_width_for_mode(900.0,360.0,false),
            manual_workspace_content_width(900.0,360.0));
    }
}

#[cfg(test)]
mod manual_workspace_layout_tests{
    use super::*;

    #[test]
    fn popup_height_tracks_the_visible_card_stack_and_fixed_modal_chrome(){
        assert_eq!(theme::RECEPTION_PANE_PADDING_Y,16.0);
        assert_eq!(manual_workspace_header_height(),113.0);
        // Shared section chrome now includes the approved 16pt divider gaps.
        // Keep explicit geometry expectations independent of the budget helper.
        assert_eq!(theme::SECTION_DIVIDER_GAP,16.0);
        assert_eq!(manual_recipient_card_height(),263.0);
        assert_eq!(manual_workspace_columns_height(),627.0);
        assert_eq!(
            components::docked_section_body_height(manual_recipient_card_height()),
            4.0*theme::INPUT_HEIGHT+3.0*theme::RECIPIENT_ROW_GAP
        );
        let work_area=egui::Rect::from_min_size(
            egui::Pos2::ZERO,egui::vec2(1400.0,1000.0)
        );
        let size=manual_workspace_dialog_size(work_area);
        assert_eq!(size.x,1364.0);
        assert_eq!(size.y,864.0);
        let short_area=egui::Rect::from_min_size(
            egui::Pos2::ZERO,egui::vec2(1400.0,700.0)
        );
        assert_eq!(manual_workspace_dialog_size(short_area).y,700.0);
    }

    #[test]
    fn measured_recipient_card_height_matches_manual_workspace_budget(){
        let ctx=components::test_context();
        let mut height=0.0;
        for _ in 0..3 {
            let _=ctx.run(egui::RawInput{
                screen_rect:Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,egui::vec2(1200.0,900.0)
                )),
                ..Default::default()
            },|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    ui.spacing_mut().item_spacing.y=0.0;
                    let measured=ui.scope(|sheet|{
                        sheet.set_width(800.0);
                        sheet.spacing_mut().item_spacing.y=0.0;
                        components::reception_section_with_header(
                            sheet,"수취인별 주문 정보",0.0,
                            |header|{
                                let _=components::reception_recipient_navigator(
                                    header,"테스트",0,1,true
                                );
                            },
                            |body|{
                                let mut name="테스트".to_owned();
                                let mut address="테스트 주소".to_owned();
                                let mut mobile=String::new();
                                let mut phone=String::new();
                                let mut note=String::new();
                                let _=components::reception_recipient_fields(
                                    body,&mut name,&mut address,&mut mobile,
                                    &mut phone,&mut note,[false;5]
                                );
                            }
                        );
                    });
                    height=measured.response.rect.height();
                });
            });
        }
        assert!((height-manual_recipient_card_height()).abs()<=1.0,
            "recipient card measured {height:.2}pt, budget {:.2}pt",
            manual_recipient_card_height());
    }

    #[test]
    fn horizontal_scroll_is_created_only_when_minimum_columns_exceed_the_viewport(){
        let narrow=manual_workspace_content_width(700.0,theme::MANUAL_SOURCE_MIN_WIDTH);
        assert_eq!(narrow,702.0);
        assert!(manual_workspace_needs_horizontal_scroll(700.0,narrow));

        // A half-point rounding tolerance matches the main Reception layout.
        assert!(!manual_workspace_needs_horizontal_scroll(900.0,900.5));
        assert!(manual_workspace_needs_horizontal_scroll(900.0,900.51));

        let fitted_manual=manual_workspace_content_width_for_mode(900.0,360.0,false);
        let fitted_direct=manual_workspace_content_width_for_mode(900.0,360.0,true);
        assert_eq!(fitted_manual,900.0);
        assert_eq!(fitted_direct,900.0);
        assert!(!manual_workspace_needs_horizontal_scroll(900.0,fitted_manual));
        assert!(!manual_workspace_needs_horizontal_scroll(900.0,fitted_direct));
    }

    #[test]
    fn source_panel_uses_reception_width_and_preserves_a_minimum_right_sheet(){
        let reception=crate::layout::LayoutState::default().widths(1600.0)[1];
        assert_eq!(manual_source_panel_width(reception,1200.0),reception);
        let narrow_content=manual_workspace_content_width(700.0,reception);
        assert_eq!(narrow_content,reception+
            theme::MANUAL_SHEET_MIN_WIDTH+theme::MANUAL_COLUMN_GAP);
        assert_eq!(manual_source_panel_width(reception,narrow_content),reception);
        assert_eq!(theme::MANUAL_SOURCE_MIN_WIDTH,crate::layout::MIN_WIDTHS[1]);
    }

    #[test]
    fn popup_columns_and_outer_chrome_have_one_explicit_geometry_owner(){
        let source=include_str!("manual_workspace.rs");
        let production=source.split("mod manual_assignment_gesture_tests{")
            .next().expect("production source boundary");
        let render=production.split("pub fn show(").nth(1).unwrap();
        assert!(render.contains("components::reception_section_with_header("));
        let header_start=render.find("workspace.header_with_toolbar(")
            .expect("status/count controls belong to the shared modal header");
        let toolbar_start=render.find("let mut count_resize_requested=false;")
            .expect("header toolbar owns shipment-count state");
        let body_start=render.find("workspace.body_region(")
            .expect("card workspace follows the shared header");
        let columns_start=render.find("let mut draw_columns=")
            .expect("card columns are drawn inside the body");
        assert!(toolbar_start<header_start && header_start<body_start
            && body_start<columns_start);
        assert!(render[header_start..body_start]
            .contains("components::date_navigation_count_control("));
        assert!(!render[body_start..]
            .contains("components::date_navigation_count_control("));
        assert!(!render[body_start..columns_start]
            .contains("components::section_divider("));
        assert!(render[toolbar_start..body_start]
            .contains("status.set_min_height(theme::MODAL_MASTER_WORKSPACE_TOOLBAR_HEIGHT)"));
        assert!(render.contains("theme::source_document_frame().show(body"));
        assert!(!render.contains("theme::modal_nested_frame().show(source_ui"));
        assert!(render.contains("components::docked_section_body_height(height)"));
        assert!(render.contains("sheet.spacing_mut().item_spacing.y=0.0;"));
        assert!(render.contains("ui.spacing_mut().item_spacing.x=if self.direct_input{"));
        assert!(render.contains("else{theme::MANUAL_COLUMN_GAP}"));
        assert!(render.contains("if !self.direct_input{"));
        assert!(render.contains("manual_workspace_content_width_for_mode("));
        assert!(render.contains("manual_workspace_needs_horizontal_scroll(viewport_width,content_width)"));
        assert!(render.contains(".drag_to_scroll(false)"));
        assert!(render.contains(".show(ui,&mut draw_columns)"));
        assert!(render.contains("|outer|draw_columns(outer)"));
        assert!(render.contains("manual_workspace_columns_height()"));
        assert!(render.contains(".fixed_size(manual_workspace_dialog_size(work_area))"));
        let main=include_str!("main.rs");
        assert!(main.contains("self.layout.widths(content_width)[1]"));
        assert!(main.contains("editor.show("));
        assert!(main.contains("self.layout.widths(content_width)[1]"));
        assert!(main.contains("reception_source_width,hover,source"));
    }
}

#[cfg(test)]
mod manual_render_cache_tests{
    use super::*;
    #[test]
    fn manual_render_recommendations_reuse_only_same_input(){
        let model=json!({"rows":[],"blocks":[],"assignments":{}});
        let first=ManualRenderData::new("",&model);
        let a=first.recommendations("a",None);
        let b=first.recommendations("a",None);
        assert!(std::rc::Rc::ptr_eq(&a,&b));
        let c=first.recommendations("b",None);
        assert!(!std::rc::Rc::ptr_eq(&b,&c));
        let edited=ManualRenderData::new("",&model);
        assert!(!std::rc::Rc::ptr_eq(&c,&edited.recommendations("b",None)));
    }
}
