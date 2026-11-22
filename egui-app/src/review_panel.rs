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
    theme::{self,ButtonRole},typography,
};

fn text<'a>(row:&'a Value,key:&str)->&'a str {
    row.get(key).and_then(Value::as_str).unwrap_or("")
}
#[derive(Clone,PartialEq)]
struct ReviewKey {
    id:String,raw:String,order_type:String,client_id:Option<String>,
    recipients:Value,client_match:Value,price:Option<Value>,
}
#[derive(Default)]
pub struct ReviewPanel {
    key:Option<ReviewKey>,
    pending:Option<Receiver<Result<Vec<Value>,String>>>,
    issues:Vec<Value>,
    ready:bool,
    error:Option<String>,
}
#[derive(Debug)]
pub enum Action {
    Navigate{kind:String,recipient_id:String,item_id:Option<String>,
        link_key:Option<String>},
    ChooseClient(String),
    Candidate{recipient_id:String,field:String,value:String},
}
impl ReviewPanel {
    pub fn clear(&mut self){*self=Self::default();}
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
        let key=ReviewKey{
            id:source.id.clone(),raw:source.raw.clone(),
            order_type:source.order_type.clone(),client_id:source.client_id.clone(),
            recipients:analysis.recognition.get("recipients").cloned().unwrap_or(Value::Null),
            client_match:source.client_match.clone().unwrap_or_else(||analysis.client_match.clone()),
            price:source.pricing_preview.clone(),
        };
        if self.key.as_ref()!=Some(&key){
            let same_source=self.key.as_ref().is_some_and(|last|
                last.id==key.id && last.raw==key.raw);
            if !same_source{self.issues.clear();}
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
                        .ok_or("Rust 발주 인식 근거가 없습니다.")?;
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
                            "Rust 확인 항목 결과의 형식이 올바르지 않습니다.".into())
                })();
                let _=sender.send(result);
                context.request_repaint();
            });
        }
        let done=self.pending.as_ref().map(|rx|rx.try_recv());
        match done{
            Some(Ok(Ok(items)))=>{
                self.pending=None;self.issues=items;self.ready=true;self.error=None;
            }
            Some(Ok(Err(error)))=>{
                self.pending=None;self.issues.clear();self.ready=false;self.error=Some(error);
            }
            Some(Err(mpsc::TryRecvError::Disconnected))=>{
                self.pending=None;self.issues.clear();self.ready=false;
                self.error=Some("Rust 확인 항목 작업이 중단됐습니다.".into());
            }
            _=>{}
        }
    }
    pub fn ready(&self)->bool{self.ready && self.error.is_none()}
    pub fn show(&self,ui:&mut Ui,analysis:&ReceptionAnalysis,
                selected_recipient:usize,enabled:bool)->Option<Action>{
        let mut selected=None;
        let interactive=self.ready() && enabled;
        if let Some(error)=self.error.as_deref(){
            ui.label(typography::status(
                &format!("Rust 확인 항목 조회 실패: {error}"),theme::DANGER
            ));
            return None;
        }
        if self.pending.is_some() && self.issues.is_empty(){
            ui.label(typography::caption("확인 항목 분석 중"));
            return None;
        }
        let actions=self.issues.iter().filter(|issue|text(issue,"severity")!="INFO")
            .collect::<Vec<_>>();
        let facts=self.issues.iter().filter(|issue|text(issue,"severity")=="INFO")
            .collect::<Vec<_>>();
        let remote=analysis.reference.get(selected_recipient)
            .and_then(|row|row.remote.as_ref()).filter(|decision|
                ["auto","review"].contains(&text(decision,"status")));
        let info_count=facts.len()+usize::from(remote.is_some());
        ui.label(typography::caption(&format!(
            "조치 {}건 / 안내 {}건",actions.len(),info_count
        )));
        if actions.is_empty(){
            ui.label(typography::body("필수 확인 사항 없음"));
        }
        for issue in actions {
            issue_card(ui,issue,analysis,interactive,&mut selected);
        }
        if let Some(remote)=remote{
            egui::Frame::new()
                .fill(theme::TABLE_ROW_ALT_BG)
                .corner_radius(theme::CONTROL_RADIUS)
                .inner_margin(egui::Margin::symmetric(
                    theme::SPACE_8 as i8,theme::SPACE_8 as i8
                ))
                .show(ui,|content|{
                    content.label(typography::body("도서·산간 지역 확인").strong());
                    content.label(typography::caption(
                        if text(remote,"status")=="auto" {
                            "도서·산간 지역으로 판단했습니다. 확인 후 체크 여부를 결정하세요."
                        }else{
                            "도서·산간 지역일 수 있습니다. 주소를 확인 후 체크 여부를 결정하세요."
                        }
                    ));
                });
        }
        let inherited=facts.iter().copied().filter(|issue|
            text(issue,"kind")=="APPLIED"
                &&text(issue,"resolutionSource")=="코어"
                &&text(issue,"title")=="공통 상품 자동 적용"
        ).collect::<Vec<_>>();
        if !inherited.is_empty(){
            ui.collapsing(
                format!("공통 상품 자동 적용 / {}개 항목",inherited.len()),
                |details|{
                    for issue in &inherited {
                        issue_card(details,issue,analysis,interactive,&mut selected);
                    }
                }
            );
        }
        for issue in facts {
            if inherited.contains(&issue){continue;}
            issue_card(ui,issue,analysis,interactive,&mut selected);
        }
        selected
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
    egui::Frame::new()
        .fill(theme::TABLE_ROW_ALT_BG)
        .corner_radius(theme::CONTROL_RADIUS)
        .inner_margin(egui::Margin::symmetric(
            theme::SPACE_8 as i8,theme::SPACE_8 as i8
        ))
        .show(ui,|cell|{
            cell.label(typography::body(
                if kind=="QUANTITY_MISSING"
                    &&text(issue,"resolution")=="PROVISIONALLY_APPROVED"{
                    "수량 1개 잠정 처리됨"
                }else{title}
            ).strong());
            if !detail.is_empty() && detail!=title {
                cell.add(egui::Label::new(typography::caption(detail)).wrap());
            }
            if kind=="UNASSIGNED_EVIDENCE" {
                cell.label(typography::caption(
                    "상품 또는 수량으로 인식된 문구입니다. 수동 분류에서 실제 주문 내용과 연결할지 확인하세요."
                ));
            }
            if text(issue,"resolution")=="PENDING_CUSTOMER" {
                cell.label(typography::caption(
                    "거래처 확인 대기 / 문의 복사만으로 해결되지 않습니다."
                ));
            }
            if kind=="QUANTITY_MISSING"
                &&text(issue,"resolution")=="PROVISIONALLY_APPROVED"{
                cell.label(typography::caption(
                    "1개 발송 예정 안내가 복사용 메시지에 포함됩니다. 실제 수량이 확인되면 수량을 수정하세요."
                ));
            }
            let is_client=kind=="CLIENT" || kind.starts_with("CLIENT_MATCH_");
            let is_candidate=kind.starts_with("CANDIDATE_");
            let recipient=analysis.recognition.get("recipients")
                .and_then(Value::as_array)
                .and_then(|rows|rows.iter().find(|row|text(row,"id")==rid));
            cell.horizontal_wrapped(|buttons|{
                if is_client && severity!="INFO"{
                    for candidate in &analysis.client_candidates {
                        let id=text(candidate,"clientId");
                        let name=text(candidate,"name");
                        if !id.is_empty() && !name.is_empty()
                            &&theme::action(buttons,name,ButtonRole::Quiet,enabled).clicked(){
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
                            if !value.is_empty() &&theme::action(
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
                // React's informational client-match evidence has no action.
                if kind=="CLIENT_MATCH_EVIDENCE" && severity=="INFO"{return;}
                let caption=if kind=="QUANTITY_MISSING" {"수량 수정"}
                    else if kind=="UNASSIGNED_EVIDENCE" {"수동 분류에서 확인"}
                    else if is_client {"거래처 확인"}
                    else if ["MODEL","MODEL_CHOICE","ALIAS_CONFLICT",
                        "CATEGORY_CONFLICT","QUANTITY_LINK","OPTION"].contains(&kind){
                        "해결 절차 보기"
                    }else{"입력 위치로 이동"};
                if theme::action(buttons,caption,ButtonRole::Standard,enabled).clicked(){
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
