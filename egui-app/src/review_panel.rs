//! Staff review: the same Rust issue projection consumed by React StaffChecks.
//! No local validation or approval decisions. Each input has a source-bound
//! background request; older results cannot replace a changed order or edit.
use std::{sync::{mpsc::{self,Receiver},Arc},thread};
use eframe::egui::{self,Ui};
use serde_json::{json,Value};
use crate::{
    gateway::{BandoGateway,ReadOperation},
    manual_workspace,
    preview_store::WorkingReception,
    reception_service::ReceptionAnalysis,
    theme::{self,ButtonRole,ReviewTone},typography,
};

fn text<'a>(row:&'a Value,key:&str)->&'a str {
    row.get(key).and_then(Value::as_str).unwrap_or("")
}
/// Keep the normal empty caption centered in the entire content viewport.
/// If a splitter makes the panel too short, reserve the count's footprint
/// and center only within a usable remainder rather than overlap the count.
fn review_empty_overlay_rect(body:egui::Rect,count_bottom:f32)->Option<egui::Rect>{
    let minimum=typography::METADATA_LINE_HEIGHT;
    if !body.is_positive(){return None;}
    let top=(count_bottom+theme::SPACE_4).max(body.top());
    if body.center().y-minimum/2.0>=top && body.height()>=minimum{
        return Some(body);
    }
    let remaining=egui::Rect::from_min_max(
        egui::pos2(body.left(),top),body.right_bottom()
    );
    (remaining.width()>0.0 && remaining.height()>=minimum).then_some(remaining)
}
#[derive(Clone,Debug,PartialEq)]
struct ReviewKey {
    id:String,raw:String,order_type:String,client_id:Option<String>,
    recipients:Value,client_match:Value,price:Option<Value>,
}
impl ReviewKey {
    // Compare the actual borrowed inputs before allocating any owned key.
    // This also catches edits in place and restored snapshots with reused IDs.
    fn matches(&self,source:&WorkingReception,analysis:&ReceptionAnalysis)->bool{
        self.id==source.id && self.raw==source.raw &&
        self.order_type==source.order_type && self.client_id==source.client_id &&
        &self.recipients==analysis.recognition.get("recipients").unwrap_or(&Value::Null) &&
        &self.client_match==source.client_match.as_ref().unwrap_or(&analysis.client_match) &&
        self.price.as_ref()==source.pricing_preview.as_ref()
    }
}
#[derive(Default)]
pub struct ReviewPanel {
    key:Option<ReviewKey>,
    pending:Option<Receiver<Result<Vec<Value>,String>>>,
    issues:Vec<Value>,
    action_indices:Vec<usize>,
    fact_indices:Vec<usize>,
    inherited_indices:Vec<usize>,
    ready:bool,
    error:Option<String>,
}
#[derive(Debug)]
pub enum Action {
    Navigate{kind:String,recipient_id:String,item_id:Option<String>,
        link_key:Option<String>},
    ChooseClient(String),
    Candidate{recipient_id:String,field:String,value:String},
    ApproveOne{recipient_id:String,item_id:String},
    CancelApproval{recipient_id:String,item_id:String},
}
impl ReviewPanel {
    pub fn clear(&mut self){*self=Self::default();}
    fn replace_issues(&mut self,issues:Vec<Value>){
        self.action_indices.clear();
        self.fact_indices.clear();
        self.inherited_indices.clear();
        for (index,issue) in issues.iter().enumerate(){
            if text(issue,"severity")!="INFO"{self.action_indices.push(index);}
            else if text(issue,"kind")=="APPLIED" &&
                text(issue,"resolutionSource")=="코어" &&
                text(issue,"title")=="공통 상품 자동 적용"{
                self.inherited_indices.push(index);
            }else{self.fact_indices.push(index);}
        }
        self.issues=issues;
    }
    pub fn refresh(
        &mut self,ctx:&egui::Context,gateway:Arc<dyn BandoGateway>,
        source:Option<&WorkingReception>,analysis:Option<&ReceptionAnalysis>,
    ){
        let (Some(source),Some(analysis))=(source,analysis) else {
            self.clear();return;
        };
        if source.analysis_raw.as_deref()!=Some(source.raw.as_str())
            ||source.analysis_evidence.is_none(){
            self.clear();return;
        }
        if !self.key.as_ref().is_some_and(|key|key.matches(source,analysis)){
        let key=ReviewKey{
            id:source.id.clone(),raw:source.raw.clone(),
            order_type:source.order_type.clone(),client_id:source.client_id.clone(),
            recipients:analysis.recognition.get("recipients").cloned().unwrap_or(Value::Null),
            client_match:source.client_match.clone().unwrap_or_else(||analysis.client_match.clone()),
            price:source.pricing_preview.clone(),
        };
            let same_source=self.key.as_ref().is_some_and(|last|
                last.id==key.id && last.raw==key.raw);
            if !same_source{self.replace_issues(Vec::new());}
            self.error=None;
            self.ready=false;
            self.key=Some(key.clone());
            let saved=source.clone();
            let (sender,receiver)=mpsc::channel();
            self.pending=Some(receiver);
            let context=ctx.clone();
            thread::spawn(move||{
                let result=(||->Result<Vec<Value>,String>{
                    let evidence=saved.analysis_evidence.as_ref()
                        .ok_or("발주서 분석 결과를 확인할 수 없습니다.")?;
                    let mut order=manual_workspace::native_order(
                        gateway.as_ref(),&saved,evidence
                    )?;
                    order["recipients"]=key.recipients;
                    order["clientMatch"]=key.client_match;
                    // Prices were already calculated by Rust for this exact
                    // draft/source/client. Reuse only exact item IDs and never
                    // overwrite an employee-entered manual unit price.
                    if let Some(quote)=key.price.as_ref().filter(|quote|
                        text(quote,"sourceRaw")==key.raw
                            &&text(quote,"orderId")==key.id
                            &&quote.get("clientId").and_then(Value::as_str)
                                ==key.client_id.as_deref()
                    ){
                        let lines=quote.get("linePrices").and_then(Value::as_array);
                        if let (Some(lines),Some(recipients))=(
                            lines,order.get_mut("recipients").and_then(Value::as_array_mut)
                        ){
                            for recipient in recipients {
                                if let Some(items)=recipient.get_mut("items")
                                    .and_then(Value::as_array_mut){
                                    for item in items {
                                        if item.get("manualUnitPrice").is_some_and(|v|!v.is_null()){
                                            continue;
                                        }
                                        if let Some(line)=lines.iter().find(|line|
                                            line.get("itemId")==item.get("id")){
                                            if let Some(price)=line.get("unitPrice").filter(|v|v.is_i64()){
                                                item["unitPrice"]=price.clone();
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    let response=gateway.execute(
                        ReadOperation::OrderIssuesWithActions,json!({"order":order})
                    )?;
                    response.get("issues").and_then(Value::as_array)
                        .cloned().ok_or_else(||
                            "확인할 내용을 불러오지 못했습니다.".into())
                })();
                let _=sender.send(result);
                context.request_repaint();
            });
        }
        let done=self.pending.as_ref().map(|rx|rx.try_recv());
        match done{
            Some(Ok(Ok(items)))=>{
                self.pending=None;self.replace_issues(items);self.ready=true;self.error=None;
            }
            Some(Ok(Err(error)))=>{
                self.pending=None;self.replace_issues(Vec::new());self.ready=false;self.error=Some(error);
            }
            Some(Err(mpsc::TryRecvError::Disconnected))=>{
                self.pending=None;self.replace_issues(Vec::new());self.ready=false;
                self.error=Some("확인할 내용 조회가 중단되었습니다.".into());
            }
            _=>{}
        }
    }
    pub fn ready(&self)->bool{self.ready && self.error.is_none()}
    /// A client action may be queued while an unrelated delivery edit commits.
    /// Other issue actions still require the ordinary edit admission policy.
    pub fn show_with_client_actions(&self,ui:&mut Ui,analysis:&ReceptionAnalysis,
                selected_recipient:usize,enabled:bool,client_enabled:bool)->Option<Action>{
        let mut selected=None;
        let interactive=self.ready() && enabled;
        let client_interactive=self.ready() && client_enabled;
        if let Some(error)=self.error.as_deref(){
            crate::components::body_error_overlay(ui,ui.available_rect_before_wrap(),
                "reception.review",error);
            return None;
        }
        if self.pending.is_some() && self.issues.is_empty(){
            return None;
        }
        let actions=&self.action_indices;
        let facts=&self.fact_indices;
        let inherited=&self.inherited_indices;
        let remote=analysis.reference.get(selected_recipient)
            .and_then(|row|row.remote.as_ref()).filter(|decision|
                ["auto","review"].contains(&text(decision,"status")));
        let info_count=facts.len()+inherited.len()+usize::from(remote.is_some());
        // Capture the visible body BEFORE the count advances the cursor.
        // The fixed count is an upper-left annotation, not a second table
        // header: using the remaining rect shifts the empty state downward.
        let body=ui.available_rect_before_wrap().intersect(ui.clip_rect());
        let count=ui.label(typography::caption(&format!(
            "조치 {}건 / 안내 {}건",actions.len(),info_count
        )));
        if actions.is_empty() && facts.is_empty() && inherited.is_empty() && remote.is_none(){
            if let Some(viewport)=review_empty_overlay_rect(body,count.rect.bottom()){
                crate::components::management_body_state_overlay(
                    ui,viewport,Some("필수 확인 사항 없음")
                );
            }
        }
        for &index in actions {
            let issue=&self.issues[index];
            let kind=text(issue,"kind");
            let is_client=kind=="CLIENT" || kind.starts_with("CLIENT_MATCH_");
            issue_card(ui,issue,analysis,if is_client{client_interactive}else{interactive},&mut selected);
        }
        if let Some(remote)=remote{
            let tone=ReviewTone::Info;
            theme::review_issue_card(ui,tone,|content|{
                    content.add(egui::Label::new(typography::review_title("도서·산간 지역 확인",tone)).wrap());
                    content.add(egui::Label::new(typography::review_detail(
                        if text(remote,"status")=="auto" {
                            "도서·산간 지역으로 판단했습니다. 확인 후 체크 여부를 결정하세요."
                        }else{
                            "도서·산간 지역일 수 있습니다. 주소를 확인 후 체크 여부를 결정하세요."
                        },tone
                )).wrap());
                });
        }
        if !inherited.is_empty(){
            ui.collapsing(
                format!("공통 상품 자동 적용 / {}개 항목",inherited.len()),
                |details|{
                    for &index in inherited {
                        issue_card(details,&self.issues[index],analysis,interactive,&mut selected);
                    }
                }
            );
        }
        for &index in facts {
            issue_card(ui,&self.issues[index],analysis,interactive,&mut selected);
        }
        selected
    }
}
/// Visual mapping only. The native severity and action routing remain authoritative.
fn issue_tone(issue:&Value)->ReviewTone {
    match (text(issue,"severity"),text(issue,"kind"),text(issue,"resolution")) {
        ("ERROR",_,_) | (_,"CLIENT_MATCH_ERROR",_) => ReviewTone::Error,
        ("INFO",_,"RESOLVED") => ReviewTone::Complete,
        ("INFO",_,_) => ReviewTone::Info,
        _ => ReviewTone::Action,
    }
}

fn issue_card(
    ui:&mut Ui,issue:&Value,analysis:&ReceptionAnalysis,
    enabled:bool,selected:&mut Option<Action>,
){
    let kind=text(issue,"kind");
    let rid=text(issue,"recipientId");
    let iid=issue.get("itemId").and_then(Value::as_str);
    let title=text(issue,"title");
    let detail=text(issue,"detail");
    let severity=text(issue,"severity");
    let link_key=issue.get("linkKey").and_then(Value::as_str);
    let tone=issue_tone(issue);
    theme::review_issue_card(ui,tone,|cell|{
            cell.add(egui::Label::new(typography::review_title(
                if kind=="QUANTITY_MISSING"
                    &&text(issue,"resolution")=="PROVISIONALLY_APPROVED"{
                    "수량 1개 잠정 처리됨"
                }else{title},tone
            )).wrap());
            if !detail.is_empty() && detail!=title {
                cell.add(egui::Label::new(typography::review_detail(detail,tone)).wrap());
            }
            if kind=="UNASSIGNED_EVIDENCE" {
                cell.add(egui::Label::new(typography::review_detail(
                    "상품 또는 수량으로 인식된 문구입니다. 수동 분류에서 실제 주문 내용과 연결할지 확인하세요.",tone
                )).wrap());
            }
            if text(issue,"resolution")=="PENDING_CUSTOMER" {
                cell.add(egui::Label::new(typography::review_detail(
                    "거래처 확인 대기 / 문의 복사만으로 해결되지 않습니다.",tone
                )).wrap());
            }
            if kind=="QUANTITY_MISSING"
                &&text(issue,"resolution")=="PROVISIONALLY_APPROVED"{
                cell.add(egui::Label::new(typography::review_detail(
                    "1개 발송 예정 안내가 복사용 메시지에 포함됩니다. 실제 수량이 확인되면 수량을 수정하세요.",tone
                )).wrap());
            }
            let is_client=kind=="CLIENT" || kind.starts_with("CLIENT_MATCH_");
            let is_candidate=kind.starts_with("CANDIDATE_");
            let recipient=analysis.recognition.get("recipients")
                .and_then(Value::as_array)
                .and_then(|rows|rows.iter().find(|row|text(row,"id")==rid));
            cell.horizontal_wrapped(|buttons|{
                if is_client && severity!="INFO"{
                    // React uses the current native match's candidate list,
                    // falling back to directory candidates for a missing match.
                    let native=analysis.client_match.get("candidates")
                        .and_then(Value::as_array).filter(|rows|!rows.is_empty());
                    let candidates=native.unwrap_or(&analysis.client_candidates);
                    for candidate in candidates {
                        let id=text(candidate,"clientId");
                        let name=text(candidate,"name");
                        if !id.is_empty() && !name.is_empty()
                            &&theme::review_action(buttons,name,ButtonRole::Quiet,enabled).clicked(){
                            *selected=Some(Action::ChooseClient(id.to_owned()));
                        }
                    }
                }
                if is_candidate {
                    let field=kind.trim_start_matches("CANDIDATE_");
                    if ["name","address","phone","mobile"].contains(&field){
                        for candidate in recipient.into_iter()
                            .flat_map(|row|row.get("candidates").and_then(Value::as_array))
                            .flatten().filter(|candidate|text(candidate,"field")==field)
                        {
                            let label=text(candidate,"label");
                            let value=text(candidate,"value");
                            if !value.is_empty() &&theme::review_action(
                                buttons,&format!("{} → 적용",
                                    if label.is_empty(){value}else{label}),
                                ButtonRole::Quiet,enabled
                            ).clicked(){
                                *selected=Some(Action::Candidate{
                                    recipient_id:rid.to_owned(),
                                    field:field.to_owned(),value:value.to_owned(),
                                });
                            }
                        }
                    }
                }
                if kind=="QUANTITY_MISSING"{
                    if let Some(item_id)=iid {
                        if theme::review_action(
                            buttons,"1개로 잠정 처리",ButtonRole::Standard,
                            enabled && issue["canApproveOne"]==true
                        ).clicked(){
                            *selected=Some(Action::ApproveOne{
                                recipient_id:rid.to_owned(),item_id:item_id.to_owned()
                            });
                        }
                    }
                }
                if kind=="QUANTITY_MISSING"
                    &&text(issue,"resolution")=="PROVISIONALLY_APPROVED" {
                    if let Some(item_id)=iid {
                        if theme::review_action(
                            buttons,"잠정 승인 취소",ButtonRole::Standard,enabled
                        ).clicked(){
                            *selected=Some(Action::CancelApproval{
                                recipient_id:rid.to_owned(),item_id:item_id.to_owned()
                            });
                        }
                    }
                }
                // React's informational client-match evidence has no action.
                if kind=="CLIENT_MATCH_EVIDENCE" && severity=="INFO"{return;}
                let caption=if kind=="QUANTITY_MISSING" {"수량 수정"}
                    else if kind=="UNASSIGNED_EVIDENCE" {"수동 분류에서 확인"}
                    else if is_client {"거래처 확인"}
                    else if ["MODEL","MODEL_CHOICE","ALIAS_CONFLICT",
                        "CATEGORY_CONFLICT","QUANTITY_LINK","OPTION"].contains(&kind){
                        "해결 절차 보기"
                    }else{"입력 위치로 이동"};
                if theme::review_action(buttons,caption,ButtonRole::Standard,enabled).clicked(){
                    *selected=Some(Action::Navigate{
                        kind:kind.to_owned(),recipient_id:rid.to_owned(),
                        item_id:iid.map(str::to_owned),
                        link_key:link_key.map(str::to_owned)
                    });
                }
            });
        });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_review_caption_uses_full_body_center_and_avoids_count_overlap(){
        let body=egui::Rect::from_min_size(egui::pos2(26.0,76.0),egui::vec2(580.0,152.0));
        let normal=review_empty_overlay_rect(body,101.0).unwrap();
        assert_eq!(normal,body);
        assert_eq!(normal.center(),body.center());
        let short=egui::Rect::from_min_size(body.min,egui::vec2(580.0,60.0));
        let fallback=review_empty_overlay_rect(short,101.0).unwrap();
        assert_eq!(fallback.center().x,short.center().x);
        assert!(fallback.top()>=101.0+theme::SPACE_4);
        assert!(fallback.bottom()<=short.bottom());
        let tiny=egui::Rect::from_min_size(body.min,egui::vec2(580.0,32.0));
        assert!(review_empty_overlay_rect(tiny,101.0).is_none());
    }
    #[test]
    fn empty_review_caption_keeps_the_shared_overlay_and_metadata_font(){
        let source=include_str!("review_panel.rs").split("#[cfg(test)]").next().unwrap();
        assert!(source.contains("management_body_state_overlay("));
        let shared=include_str!("components.rs").split("pub fn management_body_state_overlay(")
            .nth(1).unwrap().split("pub fn fixed_row").next().unwrap();
        assert!(shared.contains("egui::FontId::proportional(typography::FONT_METADATA)"));
        assert!(shared.contains("egui::Align2::CENTER_CENTER"));
        assert!(shared.contains("theme::MUTED"));
    }
    #[test]
    fn review_tones_follow_native_severity_without_changing_actions(){
        assert_eq!(issue_tone(&json!({"severity":"ACTION"})),ReviewTone::Action);
        assert_eq!(issue_tone(&json!({"severity":"INFO","resolution":"AUTO_APPLIED"})),ReviewTone::Info);
        assert_eq!(issue_tone(&json!({"severity":"INFO","resolution":"PROVISIONALLY_APPROVED"})),ReviewTone::Info);
        assert_eq!(issue_tone(&json!({"severity":"INFO","resolution":"RESOLVED"})),ReviewTone::Complete);
        assert_eq!(issue_tone(&json!({"severity":"ACTION","kind":"CLIENT_MATCH_ERROR"})),ReviewTone::Error);
    }
    #[test]
    fn review_display_partition_is_rebuilt_on_replacement_and_clear(){
        let mut panel=ReviewPanel::default();
        panel.replace_issues(vec![json!({"severity":"ACTION"}),json!({"severity":"INFO"}),
            json!({"severity":"INFO","kind":"APPLIED","resolutionSource":"코어","title":"공통 상품 자동 적용"})]);
        assert_eq!(panel.action_indices,vec![0]);
        assert_eq!(panel.fact_indices,vec![1]);
        assert_eq!(panel.inherited_indices,vec![2]);
        panel.replace_issues(vec![json!({"severity":"INFO"})]);
        assert!(panel.action_indices.is_empty() && panel.inherited_indices.is_empty());
        assert_eq!(panel.fact_indices,vec![0]);
        panel.clear();
        assert!(panel.fact_indices.is_empty());
    }
    #[test]
    fn same_source_receipt_key_changes_after_user_edit(){
        let first=ReviewKey{
            id:"a".into(),raw:"원문".into(),order_type:"AGENCY".into(),
            client_id:Some("c1".into()),
            recipients:json!([{"id":"r1","name":"원본"}]),
            client_match:Value::Null,price:None
        };
        let mut changed=first.clone();
        changed.recipients[0]["name"]=json!("수정");
        assert_ne!(first,changed);
        changed=first.clone();
        changed.client_id=Some("c2".into());
        assert_ne!(first,changed);
    }
}
