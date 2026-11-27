//! Staff review: the same Rust issue projection consumed by React StaffChecks.
//! No local validation or approval decisions. Each input has a source-bound
//! background request; older results cannot replace a changed order or edit.
use std::sync::Arc;
use eframe::egui::{self,Ui};
use serde_json::{json,Value};
use crate::{
    gateway::{BandoGateway,ReadOperation},
    manual_workspace,
    preview_store::WorkingReception,
    reception_service::ReceptionAnalysis,
    theme::{ButtonRole,ReviewTone},typography,
};

fn text<'a>(row:&'a Value,key:&str)->&'a str {
    row.get(key).and_then(Value::as_str).unwrap_or("")
}
/// The summary lives in the header, leaving the entire body for the empty state.
fn review_empty_overlay_rect(body:egui::Rect)->Option<egui::Rect>{
    (body.is_positive() && body.height()>=typography::METADATA_LINE_HEIGHT)
        .then_some(body)
}
fn remote_notice(analysis:&ReceptionAnalysis,selected_recipient:usize)->Option<&Value>{
    analysis.reference.get(selected_recipient)
        .and_then(|row|row.remote.as_ref()).filter(|decision|
            ["auto","review"].contains(&text(decision,"status")))
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
    pending:crate::screen_refresh::ReadRefresh<(Vec<Value>,Result<String,String>)>,
    draft_message:Option<Result<String,String>>,
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
    ValidationApproval{recipient_id:String,request:Value,cancel:bool},
    CustomerInquiry{recipient_id:String,request:Value,cancel:bool},
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
        let direct=source.raw.trim().is_empty() && source.manual_model.as_ref()
            .is_some_and(|saved|text(saved,"mode")=="DIRECT");
        if !direct && (source.analysis_raw.as_deref()!=Some(source.raw.as_str())
            ||source.analysis_evidence.is_none()){
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
            let matched_name=analysis.matched_client_name.clone();
            self.draft_message=None;
            self.pending.submit_required(ctx.clone(),move||->Result<(Vec<Value>,Result<String,String>),String>{
                    let mut order=if direct{
                        manual_workspace::native_direct_order(gateway.as_ref(),&saved)?
                    }else{
                        let evidence=saved.analysis_evidence.as_ref()
                            .ok_or("발주서 분석 결과를 확인할 수 없습니다.")?;
                        manual_workspace::native_order(gateway.as_ref(),&saved,evidence)?
                    };
                    order["recipients"]=key.recipients;
                    order["clientMatch"]=key.client_match;
                    // The actual employee choice supersedes advisory evidence.
                    if saved.client_selection.as_deref()==Some("MANUAL"){
                        order["clientMatch"]=json!({"source":"MANUAL","clientId":saved.client_id});
                    }else if let Some(name)=matched_name.filter(|name|!name.trim().is_empty()){
                        if order["clientMatch"].is_object() &&
                            text(&order["clientMatch"],"clientId")==saved.client_id.as_deref().unwrap_or(""){
                            order["clientMatch"]["clientName"]=json!(name);
                        }
                    }
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
                    let mut request=json!({"order":&order});
                    // Sale status requires the live catalog; do not infer it
                    // from a previously recognized product label.
                    if let Ok(catalog)=gateway.load_catalog(){request["state"]=catalog;}
                    let response=gateway.execute(ReadOperation::OrderIssuesWithActions,request)?;
                    let issues=response.get("issues").and_then(Value::as_array)
                        .cloned().ok_or("확인할 내용을 불러오지 못했습니다.")?;
                    let message=(||->Result<String,String>{
                        let settings=gateway.load_settings()?;
                        let settings=settings.get("settings").filter(|v|v.is_object())
                            .ok_or("자동 메시지 설정을 확인할 수 없습니다.")?;
                        let rendered=gateway.execute(ReadOperation::RenderAutomaticRequest,json!({
                            "settings":settings,"order":&order,"issues":&issues,"step":0
                        }))?;
                        rendered["requestText"].as_str().map(str::to_owned)
                            .ok_or_else(||"문의 메시지 생성 결과를 확인할 수 없습니다.".into())
                    })();
                    Ok((issues,message))
            });
        }
        match self.pending.poll(){
            Some(Ok((items,message)))=>{
                self.replace_issues(items);self.draft_message=Some(message);
                self.pending.verified();self.ready=true;self.error=None;
            }
            Some(Err(error))=>{
                // A failed projection cannot authorize an action against old issues.
                self.ready=false;self.error=Some(error);
            }
            None=>{}
        }
    }
    pub fn ready(&self)->bool{self.ready && !self.pending.required() && self.error.is_none()}
    pub fn draft_message(&self,source:&WorkingReception,analysis:&ReceptionAnalysis)
        ->Option<&Result<String,String>>{
        if !self.ready() || !self.key.as_ref().is_some_and(|key|key.matches(source,analysis)){return None;}
        self.draft_message.as_ref()
    }
    /// None means the exact current native projection is not ready.
    pub fn input_targets(&self,source:&WorkingReception,
        analysis:&ReceptionAnalysis,error:Option<&str>)->Option<Vec<(String,Option<String>,&'static str,String)>>{
        if !self.ready() || !self.key.as_ref().is_some_and(|key|key.matches(source,analysis)){
            return None;
        }
        let recipients=analysis.recognition.get("recipients")
            .and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
        Some(ordered_input_targets(recipients,&self.issues,error))
    }
    pub fn refresh_failed(&self)->bool{self.error.is_some()}
    /// The same authoritative issue snapshot drives persistent field accents
    /// independently of the single pending focus/balloon.
    pub fn required_fields(&self,source:&WorkingReception,analysis:&ReceptionAnalysis,recipient:&str)->RequiredFields{
        if !self.ready() || !self.key.as_ref().is_some_and(|key|key.matches(source,analysis)){
            return RequiredFields::default();
        }
        RequiredFields::from_issues(&self.issues,&source.id,recipient)
    }
    pub fn show_header_summary(&self,ui:&mut Ui,analysis:&ReceptionAnalysis,
        selected_recipient:usize){
        if self.error.is_some() || (self.pending.pending() && self.issues.is_empty()){
            return;
        }
        let info_count=self.fact_indices.len()+self.inherited_indices.len()
            +usize::from(remote_notice(analysis,selected_recipient).is_some());
        ui.add(crate::ui_primitives::truncated_label(typography::caption(&format!(
            "조치 {}건 / 안내 {}건",self.action_indices.len(),info_count
        ))));
    }
    /// Keep model resolution outside the scrolling issue body.
    pub fn show_model_shortcut(&self,ui:&mut Ui,analysis:&ReceptionAnalysis,
        selected_recipient:usize,enabled:bool)->(bool,Option<Action>){
        let rid=analysis.recognition.get("recipients").and_then(Value::as_array)
            .and_then(|rows|rows.get(selected_recipient)).map(|row|text(row,"id")).unwrap_or("");
        let Some(issue)=preferred_model_issue(&self.issues,&self.action_indices,rid) else{return (false,None);};
        let clicked=crate::button::review_action(ui,"상품 확인",ButtonRole::Standard,
            self.ready()&&enabled).clicked();
        (true,clicked.then(||model_navigation(issue)))
    }
    /// A client action may be queued while an unrelated delivery edit commits.
    /// Other issue actions still require the ordinary edit admission policy.
    pub fn show_with_client_actions(&self,ui:&mut Ui,analysis:&ReceptionAnalysis,
                selected_recipient:usize,enabled:bool,client_enabled:bool)->Option<Action>{
        let mut selected=None;
        let interactive=self.ready() && enabled;
        let client_interactive=self.ready() && client_enabled;
        if let Some(error)=self.error.as_deref(){
            crate::status_notice::body_error_overlay(ui,ui.available_rect_before_wrap(),
                "reception.review",error);
            return None;
        }
        if self.pending.pending() && self.issues.is_empty(){
            return None;
        }
        let actions=&self.action_indices;
        let facts=&self.fact_indices;
        let inherited=&self.inherited_indices;
        let remote=remote_notice(analysis,selected_recipient);
        let body=ui.available_rect_before_wrap().intersect(ui.clip_rect());
        if actions.is_empty() && facts.is_empty() && inherited.is_empty() && remote.is_none(){
            if let Some(viewport)=review_empty_overlay_rect(body){
                crate::status_notice::management_body_state_overlay(
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
            crate::card::review_issue_card(ui,tone,|content|{
                    content.add(egui::Label::new(typography::review_title(
                        if text(remote,"status")=="auto"{"도서·산간 지역 자동 판정"}else{"도서·산간 지역 확인 필요"},tone)).wrap());
                    content.add(egui::Label::new(typography::review_detail(
                        if text(remote,"status")=="auto" {
                            "주소가 도서·산간 지역으로 판정되었습니다. 주소와 도서/산간 체크 상태를 확인하세요."
                        }else{
                            "도서·산간 지역 여부가 불명확합니다. 주소를 확인하고 해당하면 도서/산간을 체크하세요."
                        },tone
                )).wrap());
                });
        }
        for (caption,indices) in inherited_message_groups(
            &self.issues,inherited,&analysis.recognition
        ){
            crate::button::collapsing_header(caption).show(ui,|details|{
                for index in indices {
                    issue_card(details,&self.issues[index],analysis,interactive,&mut selected);
                }
            });
        }
        for &index in facts {
            issue_card(ui,&self.issues[index],analysis,interactive,&mut selected);
        }
        selected
    }
}
/// Paint targets only; no new validation or approval criteria.
#[derive(Default)]
pub struct RequiredFields {
    fields:std::collections::HashSet<(String,String)>,
}
impl RequiredFields {
    pub fn from_issues(issues:&[Value],order:&str,recipient:&str)->Self{
        let mut result=Self::default();
        if recipient.is_empty(){return result;}
        for issue in issues{
            if text(issue,"severity")!="ACTION" || text(issue,"recipientId")!=recipient
                || (!text(issue,"orderId").is_empty() && text(issue,"orderId")!=order){continue;}
            let item=text(issue,"itemId");
            let owner=if item.is_empty(){recipient}else{item};
            let linked=text(issue,"linkKey").rsplit_once(':');
            let field=match (text(issue,"kind"),item.is_empty(),linked){
                ("NAME",true,Some((id,"name"))) if id==owner=>"name",
                ("ADDRESS",true,Some((id,"address"))) if id==owner=>"address",
                ("CONTACT",true,Some((id,"mobile"))) if id==owner=>"mobile",
                ("CONTACT",true,Some((id,"phone"))) if id==owner=>"phone",
                ("QUANTITY_MISSING"|"QUANTITY_INVALID",false,Some((id,"quantity"))) if id==owner=>"quantity",
                ("PRICE",false,Some((id,"price"|"unitPrice"))) if id==owner=>"price",
                ("MODEL"|"MODEL_CHOICE"|"ALIAS_CONFLICT"|"CATEGORY_CONFLICT"|"QUANTITY_LINK",false,Some((id,"product"))) if id==owner=>"product",
                ("ITEM",true,Some((id,"product"))) if id==owner=>"addItem",
                _=>continue,
            };
            result.fields.insert((owner.to_owned(),field.to_owned()));
        }
        result
    }
    pub fn contains(&self,owner:&str,field:&str)->bool{
        self.fields.iter().any(|(id,key)|id==owner && key==field)
    }
}
/// The native stage and resolution are authoritative; UI only routes exact
/// existing cells. Do not turn product/customer/account decisions into inputs.
fn ordered_input_targets(recipients:&[Value],issues:&[Value],error:Option<&str>)
    ->Vec<(String,Option<String>,&'static str,String)>{
    let mut result:Vec<(String,Option<String>,&'static str,String)>=Vec::new();
    for recipient in recipients{
        let rid=text(recipient,"id");
        if rid.is_empty(){continue;}
        let items=recipient["items"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        let start=result.len();
        for issue in issues{
            if text(issue,"recipientId")!=rid || text(issue,"severity")!="ACTION"
                || issue["blockingStage"].as_i64()!=Some(1){continue;}
            let item=issue.get("itemId").and_then(Value::as_str).filter(|v|!v.is_empty());
            let Some((owner,field))=text(issue,"linkKey").rsplit_once(':')else{continue;};
            if owner!=item.unwrap_or(rid) || item.is_some_and(|id|
                !items.iter().any(|row|text(row,"id")==id)){continue;}
            let field=match (text(issue,"kind"),item.is_some(),field){
                ("NAME",false,"name")=>"name",
                ("ADDRESS",false,"address")=>"address",
                ("CONTACT",false,"mobile")=>"mobile",
                ("CONTACT",false,"phone")=>"phone",
                ("QUANTITY_MISSING"|"QUANTITY_INVALID",true,"quantity")=>"quantity",
                ("PRICE",true,"unitPrice"|"price")=>"unitPrice",
                _=>continue,
            };
            let title=text(issue,"title");
            let detail=text(issue,"detail");
            let name=text(recipient,"name");
            let prefix=if name.is_empty(){"수취인"}else{name};
            if error.is_some_and(|e|!failure_matches_issue(e,prefix,title,detail)){continue;}
            if !result.iter().any(|(r,i,f,_)|r==rid && i.as_deref()==item && *f==field){
                result.push((rid.to_owned(),item.map(str::to_owned),field,
                    if title.is_empty(){detail.to_owned()}else{title.to_owned()}));
            }
        }
        result[start..].sort_by_key(|(_,item,field,_)|{
            match *field{
                "name"=>(0,0),"address"=>(0,1),"mobile"=>(0,2),"phone"=>(0,3),
                _=>(1+item.as_deref().and_then(|id|items.iter().position(|row|text(row,"id")==id))
                    .unwrap_or(0),usize::from(*field=="unitPrice")),
            }
        });
    }
    result
}
/// Compare complete native message lines, never a loose substring of an
/// unrelated technical error or a differently named recipient's issue.
fn failure_matches_issue(error:&str,name:&str,title:&str,detail:&str)->bool{
    let normalize=|s:&str|s.trim().trim_end_matches('.').trim_end().replace(" / ",": ");
    let title=normalize(title);
    let detail=normalize(detail);
    let named=normalize(&format!("{name}: {title}"));
    error.lines().map(normalize).any(|line|
        (!title.is_empty()&&(line==title||line==named)) ||
        (!detail.is_empty()&&line==detail))
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

/// Shared reception/workflow guidance. The existing direct input remains
/// available for a one-off amount; registered prices/rates are reusable.
pub(crate) fn pricing_registration_hint(kind:&str)->Option<&'static str>{
    match kind{
        "PRICE"=>Some("상품 관리에서 해당 상품·옵션의 거래처 등급별 판매가를 등록하세요. 이번 주문만 변경하려면 단가를 직접 입력하세요."),
        "FREIGHT"=>Some("운임 관리에서 배송 그룹 및 수량별 운임을 확인·등록하세요. 이번 주문만 변경하려면 운임을 직접 입력하세요."),
        _=>None
    }
}

fn model_resolution_kind(kind:&str)->bool{
    matches!(kind,"MODEL"|"MODEL_CHOICE"|"ALIAS_CONFLICT"|"CATEGORY_CONFLICT"|"MODEL_CONFIRM")
}
fn preferred_model_issue<'a>(issues:&'a [Value],indices:&[usize],rid:&str)->Option<&'a Value>{
    let candidates=||indices.iter().filter_map(|i|issues.get(*i)).filter(|issue|
        model_resolution_kind(text(issue,"kind"))&&!text(issue,"recipientId").is_empty()
        && !text(issue,"itemId").is_empty());
    candidates().find(|issue|text(issue,"recipientId")==rid).or_else(||candidates().next())
}
fn model_navigation(issue:&Value)->Action{
    Action::Navigate{kind:text(issue,"kind").to_owned(),
        recipient_id:text(issue,"recipientId").to_owned(),
        item_id:issue.get("itemId").and_then(Value::as_str).map(str::to_owned),
        link_key:issue.get("linkKey").and_then(Value::as_str).map(str::to_owned)}
}
pub(crate) const MODEL_RESOLUTION_GUIDANCE:&str=
    "품목 클릭 → 상품 변경 → 선택 → 적용 → 초안에 적용";

pub(crate) fn validation_approval_request(issue:&Value)->Value{
    json!({"issueId":issue["issueId"],"approvalBasis":issue["approvalBasis"]})
}
pub(crate) fn show_validation_approval_action(ui:&mut Ui,issue:&Value,enabled:bool)->Option<bool>{
    let cancel=issue["canCancelCurrent"]==true;
    if !cancel && issue["canApproveCurrent"]!=true{return None;}
    crate::button::review_action(ui,if cancel{"승인 취소"}else if text(issue,"kind")=="UNASSIGNED_EVIDENCE"{"주문 정보 아님"}else{"현재 정보 승인"},
        ButtonRole::Standard,enabled).clicked().then_some(cancel)
}
pub(crate) fn inquiry_request(issue:&Value)->Value{
    json!({"issueId":issue["issueId"],"inquiryBasis":issue["inquiryBasis"]})
}
pub(crate) fn show_inquiry_action(ui:&mut Ui,issue:&Value,enabled:bool)->Option<bool>{
    let cancel=issue["canCancelInquiry"]==true;
    if !cancel && issue["canRequestInquiry"]!=true{return None;}
    crate::button::review_action(ui,if cancel{"문의 취소"}else{"거래처에 문의"},
        ButtonRole::Standard,enabled).clicked().then_some(cancel)
}
pub(crate) const INQUIRY_GUIDANCE:&str="복사용 자동 메시지를 전달하고 회신 내용을 입력하세요.";
// Presentation only: retain native issue identity, approval and routing.
fn review_item_context<'a>(issue:&Value,recognition:&'a Value)->Option<(&'a Value,&'a Value)>{
    let rid=text(issue,"recipientId");
    let iid=text(issue,"itemId");
    if rid.is_empty()||iid.is_empty(){return None;}
    let row=recognition.get("recipients")?.as_array()?.iter()
        .find(|r|text(r,"id")==rid)?;
    let item=row.get("items")?.as_array()?.iter().find(|i|text(i,"id")==iid)?;
    Some((row,item))
}
fn unidentified_product_caption(issue:&Value,recognition:&Value)->Option<String>{
    if text(issue,"kind")!="MODEL"||text(issue,"severity")=="INFO"{return None;}
    let (row,item)=review_item_context(issue,recognition)?;
    let who=text(row,"name").trim();
    let product=text(item,"name").trim();
    let raw=text(item,"raw").trim();
    let product=if product.is_empty(){raw}else{product};
    if product.is_empty(){return None;}
    Some(format!("{} \"{product}\" 제품 선택 필요",if who.is_empty(){"수취인"}else{who}))
}
// Korean particles follow the displayed source/model, not its quotation mark.
fn final_consonant(value:&str)->Option<u32>{
    let c=value.trim().chars().last()?;
    let n=c as u32;
    if (0xac00..=0xd7a3).contains(&n){return Some((n-0xac00)%28);}
    match c {
        '0'=>Some(21),'1'|'7'|'8'=>Some(8),'3'=>Some(16),'6'=>Some(1),
        '2'|'4'|'5'|'9'=>Some(0),_=>None,
    }
}
fn inherited_message_groups(issues:&[Value],indices:&[usize],recognition:&Value)
    ->Vec<(String,Vec<usize>)>{
    let mut groups:Vec<(Option<(String,String)>,Vec<usize>)>=Vec::new();
    for &index in indices {
        let issue=&issues[index];
        let key=review_item_context(issue,recognition).and_then(|(_,item)|{
            let term=text(&item["match"],"term").trim();
            let model=text(item,"name").split('/').next().unwrap_or("").trim();
            if term.is_empty()||model.is_empty(){None}
            else{Some((term.to_owned(),model.to_owned()))}
        });
        if let Some((_,members))=groups.iter_mut().find(|(k,_)|*k==key){
            members.push(index);
        }else{groups.push((key,vec![index]));}
    }
    groups.into_iter().map(|(key,members)|{
        let deliveries=members.iter().map(|&i|text(&issues[i],"recipientId"))
            .filter(|id|!id.is_empty()).collect::<std::collections::HashSet<_>>().len();
        let caption=match key{
            Some((term,model))=>{
                let object=if final_consonant(&term).unwrap_or(0)==0{"를"}else{"을"};
                let direction=if matches!(final_consonant(&model),None|Some(0|8)){"로"}else{"으로"};
                format!("'{term}'{object} {model}{direction} 잠정 적용 (배송 {deliveries}건)")
            },
            None=>format!("공통 상품 자동 적용 / {}개 항목",members.len()),
        };
        (caption,members)
    }).collect()
}
// Display labels are independent of the native validation messages used for
// failure matching, approval evidence and stage gates.
pub(crate) fn review_caption<'a>(issue:&'a Value,recognition:&Value)->std::borrow::Cow<'a,str>{
    let kind=text(issue,"kind");
    let original=text(issue,"title");
    if kind=="APPLIED" && matches!(original,"거래처 별칭 적용"|"공통 별칭 적용"){
        if let Some((_,item))=review_item_context(issue,recognition){
            let term=text(&item["match"],"term").trim();
            let model=text(item,"name").split('/').next().unwrap_or("").trim();
            if !term.is_empty()&&!model.is_empty(){
                let object=if final_consonant(term).unwrap_or(0)==0{"를"}else{"을"};
                let direction=if matches!(final_consonant(model),None|Some(0|8)){"로"}else{"으로"};
                return std::borrow::Cow::Owned(format!("'{term}'{object} {model}{direction} 적용 ({original})"));
            }
        }
    }
    let label=match kind{
        "CLIENT"=>"거래처 선택 필요",
        "CLIENT_MATCH_CONFLICT"=>"거래처 정보 불일치",
        "CLIENT_MATCH_ERROR"=>"거래처 자동선택 실패",
        "RECIPIENT"=>"수취인 정보 없음",
        "NAME"=>"수취인명 누락",
        "ADDRESS"=>{
            let address=recognition.get("recipients").and_then(Value::as_array)
                .and_then(|rows|rows.iter().find(|row|text(row,"id")==text(issue,"recipientId")))
                .map(|row|text(row,"address").trim());
            if address==Some(""){"배송주소 누락"}else{"상세주소 확인 필요"}
        },
        "CONTACT" if original=="연락처가 없습니다"=>"연락처 누락",
        "CANDIDATE_name"=>"수취인명 후보 선택 필요",
        "CANDIDATE_address"=>"배송주소 후보 선택 필요",
        "CANDIDATE_phone"=>"전화번호 후보 선택 필요",
        "CANDIDATE_mobile"=>"휴대폰 번호 후보 선택 필요",
        "ITEM"=>"상품 정보 누락",
        "UNASSIGNED_EVIDENCE"=>"연결되지 않은 상품·수량 정보",
        "MODEL" if original=="등록된 상품과 옵션을 확인하세요"=>"상품·옵션 연결 오류",
        "MODEL_CHOICE"=>"상품 모델 선택 필요",
        "ALIAS_CONFLICT"=>"상품 별칭 충돌",
        "CATEGORY_CONFLICT"=>"상품 종류와 모델 불일치",
        "QUANTITY_LINK"=>"수량 연결 확인 필요",
        "OPTION"=>"상품 옵션 확인 필요",
        "MODEL_CONFIRM"=>"추천 상품 확인 필요",
        "ORIGIN"=>"발송지 미확정",
        "ACCOUNT"=>"입금 계좌 확인 필요",
        "NOTICE"=>"금액·계좌 재안내 필요",
        "QUANTITY_MISSING" if text(issue,"resolution")=="PROVISIONALLY_APPROVED"=>"수량 잠정 처리 완료",
        "QUANTITY_MISSING"=>"수량 미기재",
        "QUANTITY_INVALID"=>"수량 오류",
        "PRICE"=>"판매가 미등록",
        "FREIGHT"=>"운임 미확정",
        _=>original,
    };
    std::borrow::Cow::Borrowed(label)
}
fn review_instruction(kind:&str)->Option<&'static str>{
    match kind{
        "CLIENT"|"CLIENT_MATCH_CONFLICT"|"CLIENT_MATCH_ERROR"=>Some("적용할 거래처를 선택하세요."),
        "RECIPIENT"=>Some("수취인과 상품을 추가하세요."),
        "NAME"=>Some("수취인명을 입력하세요."),
        "ADDRESS"=>Some("주소를 확인하고 수정하세요. 현재 주소가 정확하면 승인할 수 있습니다."),
        "CONTACT"=>Some("연락처를 확인하고 입력 또는 수정하세요."),
        "QUANTITY_MISSING"=>Some("상품 수량을 확인하고 입력하세요."),
        "QUANTITY_INVALID"=>Some("1개 이상의 올바른 수량을 입력하세요."),
        "CANDIDATE_name"|"CANDIDATE_address"|"CANDIDATE_phone"|"CANDIDATE_mobile"=>
            Some("올바른 후보를 적용하거나 입력 위치에서 직접 수정하세요."),
        "ITEM"=>Some("배송할 상품과 수량을 추가하세요."),
        "MODEL_CHOICE"=>Some("발주원문에 맞는 상품 모델을 선택하세요."),
        "ALIAS_CONFLICT"=>Some("서로 다른 상품으로 연결된 별칭입니다. 실제 주문 상품을 선택하세요."),
        "CATEGORY_CONFLICT"=>Some("발주서의 상품 종류와 인식된 모델이 다릅니다. 실제 상품을 선택하세요."),
        "QUANTITY_LINK"=>Some("원문의 수량이 어느 상품에 해당하는지 확인하고 연결하세요."),
        "OPTION"=>Some("색상과 옵션을 확인하고 실제 주문 정보로 수정하세요."),
        "MODEL_CONFIRM"=>Some("추천 상품이 실제 주문 상품과 일치하는지 확인하세요."),
        "SALE_STATUS"=>Some("상품 관리에서 판매 상태를 확인하세요."),
        "ORIGIN"=>Some("품목의 발송지를 매장 또는 공장으로 지정하세요."),
        "ACCOUNT"=>Some("사용할 입금 계좌를 등록하거나 선택하세요."),
        "NOTICE"=>Some("변경된 금액과 계좌를 확인하고 복사용 메시지로 다시 안내하세요."),
        _=>None
    }
}
// Shared UI projection for confirmed orders. Never change the native issue
// object or its original detail used by error deduplication.
pub(crate) fn workflow_review_text(issue:&Value,label:&str,recognition:&Value)->String{
    if text(issue,"resolution")=="USER_APPROVED"||text(issue,"severity")=="INFO"{
        return label.to_owned();
    }
    let old=text(issue,"title");
    let caption=review_caption(issue,recognition);
    if old.is_empty()||caption.as_ref()==old{return label.to_owned();}
    if label==old{return caption.into_owned();}
    if let Some(prefix)=label.strip_suffix(old){
        if prefix.ends_with(" / ")||prefix.ends_with(": "){
            return format!("{prefix}{caption}");
        }
    }
    label.to_owned()
}
pub(crate) fn issue_instruction(issue:&Value)->Option<&'static str>{
    if text(issue,"severity")=="INFO"{return None;}
    let kind=text(issue,"kind");
    if kind=="STAGE_BLOCKER"{
        let detail=text(issue,"detail").trim().trim_end_matches('.');
        let problem=detail.rsplit_once(": ").map(|(_,problem)|problem).unwrap_or(detail);
        return match problem{
            "청구액 미확정"=>Some("품목 단가, 수량 및 운임을 확인하여 청구액을 확정하세요."),
            "운임 적용 필요"=>Some("배송 운임을 확인하고 운임 적용 단계를 완료하세요."),
            "계좌 안내 완료 필요"=>Some("입금 계좌와 청구액을 확인하고 계좌 안내 단계를 완료하세요."),
            _=>None,
        };
    }
    if kind=="ADDRESS"&&issue["canApproveCurrent"]!=true{
        return Some("배송주소를 확인하고 입력 또는 수정하세요.");
    }
    if kind=="SALE_STATUS"&&issue["canApproveCurrent"]==true{
        return Some("상품 관리에서 판매 상태를 확인하세요. 현재 상품을 사용해도 된다면 현재 정보 승인으로 처리하세요.");
    }
    review_instruction(kind)
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
    let product_caption=unidentified_product_caption(issue,&analysis.recognition);
    let caption=review_caption(issue,&analysis.recognition);
    crate::card::review_issue_card(ui,tone,|cell|{
            if text(issue,"resolution")=="USER_APPROVED"{
                cell.add(egui::Label::new(typography::review_title(detail,tone)).wrap());
                if let Some(cancel)=show_validation_approval_action(cell,issue,enabled){
                    *selected=Some(Action::ValidationApproval{recipient_id:rid.to_owned(),
                        request:validation_approval_request(issue),cancel});
                }
                return;
            }
            if kind=="CLIENT_MATCH_EVIDENCE" && severity=="INFO"{
                cell.add(egui::Label::new(typography::review_title(title,tone)).wrap());
                return;
            }
            cell.add(egui::Label::new(typography::review_title(
                product_caption.as_deref().unwrap_or(caption.as_ref()),tone
            )).wrap());
            let model_action=model_resolution_kind(kind)&&severity!="INFO"&&!rid.is_empty()&&iid.is_some_and(|id|!id.is_empty());
            if model_action{
                if crate::button::review_action(cell,"상품 확인",ButtonRole::Standard,enabled).clicked(){
                    *selected=Some(model_navigation(issue));
                }
            }
            if product_caption.is_none() && !detail.is_empty() && detail!=title {
                // Native recipient-only details repeat the old title; keep
                // identity and actual matching evidence without repeating it.
                let shown=if caption.as_ref()!=title {
                    detail.strip_suffix(title).and_then(|prefix|prefix.strip_suffix(" / "))
                        .unwrap_or(detail)
                }else{detail};
                if !shown.is_empty(){
                    cell.add(egui::Label::new(typography::review_detail(shown,tone)).wrap());
                }
            }
            if let Some(instruction)=issue_instruction(issue){
                cell.add(egui::Label::new(typography::review_detail(instruction,tone)).wrap());
            }
            if kind=="UNASSIGNED_EVIDENCE" {
                cell.add(egui::Label::new(typography::review_detail(
                    "상품 또는 수량으로 인식된 문구입니다. 수동 분류에서 실제 주문 내용과 연결할지 확인하세요.",tone
                )).wrap());
            }
            if issue["inquiryRequested"]==true{
                cell.add(egui::Label::new(typography::review_detail(
                    "거래처 회신 대기",tone)).wrap());
                cell.add(egui::Label::new(typography::review_detail(INQUIRY_GUIDANCE,tone)).wrap());
            }else if text(issue,"resolution")=="PENDING_CUSTOMER" {
                cell.add(egui::Label::new(typography::review_detail(
                    "거래처 확인 필요",tone
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
                if kind=="CLIENT_MATCH_CONFLICT" && severity!="INFO"{
                    if let Some(id)=issue.get("selectedClientId").and_then(Value::as_str).filter(|id|!id.is_empty()){
                        if crate::button::review_action(buttons,"현재 정보 승인",ButtonRole::Standard,enabled).clicked(){
                            *selected=Some(Action::ChooseClient(id.to_owned()));
                        }
                    }
                }
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
                            &&crate::button::review_action(buttons,name,ButtonRole::Quiet,enabled).clicked(){
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
                            if !value.is_empty() &&crate::button::review_action(
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
                        if crate::button::review_action(
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
                        if crate::button::review_action(
                            buttons,"잠정 승인 취소",ButtonRole::Standard,enabled
                        ).clicked(){
                            *selected=Some(Action::CancelApproval{
                                recipient_id:rid.to_owned(),item_id:item_id.to_owned()
                            });
                        }
                    }
                }
                // React's informational client-match evidence has no action.
                if model_action || (kind=="CLIENT_MATCH_EVIDENCE" && severity=="INFO"){return;}
                let caption=if kind=="QUANTITY_MISSING" {"수량 수정"}
                    else if kind=="PRICE" {"단가 입력"}
                    else if kind=="FREIGHT" {"운임 입력"}
                    else if kind=="UNASSIGNED_EVIDENCE" {"수동 분류에서 확인"}
                    else if is_client {"거래처 선택"}
                    else if ["MODEL","MODEL_CHOICE","ALIAS_CONFLICT",
                        "CATEGORY_CONFLICT","QUANTITY_LINK","OPTION"].contains(&kind){
                        "해결 절차 보기"
                    }else{"입력 위치로 이동"};
                if crate::button::review_action(buttons,caption,ButtonRole::Standard,enabled).clicked(){
                    *selected=Some(Action::Navigate{
                        kind:kind.to_owned(),recipient_id:rid.to_owned(),
                        item_id:iid.map(str::to_owned),
                        link_key:link_key.map(str::to_owned)
                    });
                }
            });
            if let Some(cancel)=show_validation_approval_action(cell,issue,enabled){
                *selected=Some(Action::ValidationApproval{recipient_id:rid.to_owned(),
                    request:validation_approval_request(issue),cancel});
            }
            if let Some(cancel)=show_inquiry_action(cell,issue,enabled){
                *selected=Some(Action::CustomerInquiry{recipient_id:rid.to_owned(),
                    request:inquiry_request(issue),cancel});
            }
            // Keep the immediate action ahead of the secondary reference
            // guidance so a narrow panel does not hide the input button.
            if severity!="INFO"{
                if model_action{
                    cell.add(egui::Label::new(typography::review_detail(MODEL_RESOLUTION_GUIDANCE,tone)).wrap());
                }
                if let Some(hint)=pricing_registration_hint(kind){
                    cell.add(egui::Label::new(typography::review_detail(hint,tone)).wrap());
                }
            }
        });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workflow_review_projection_keeps_original_data_and_approved_state(){
        let issue=json!({"kind":"ACCOUNT","severity":"ACTION","title":"입금 계좌를 등록 / 선택하세요"});
        assert_eq!(workflow_review_text(&issue,"입금 계좌를 등록 / 선택하세요",&json!({})),"입금 계좌 확인 필요");
        assert_eq!(issue["title"],"입금 계좌를 등록 / 선택하세요");
        let origin=json!({"kind":"ORIGIN","severity":"ACTION","title":"발송지를 확인하세요"});
        assert_eq!(workflow_review_text(&origin,"가상 수취인 / 발송지를 확인하세요",&json!({})),"가상 수취인 / 발송지 미확정");
        let approved=json!({"kind":"NOTICE","severity":"INFO","resolution":"USER_APPROVED"});
        assert!(issue_instruction(&approved).is_none());
        assert_eq!(workflow_review_text(&approved,"변경된 금액 / 계좌 재안내 사용자 승인",&json!({})),"변경된 금액 / 계좌 재안내 사용자 승인");
        let unapproved=json!({"kind":"SALE_STATUS","severity":"ACTION"});
        assert!(!issue_instruction(&unapproved).unwrap().contains("현재 정보 승인"));
        let stage=json!({"kind":"STAGE_BLOCKER","severity":"ACTION","detail":"수취인: 청구액 미확정"});
        assert!(issue_instruction(&stage).unwrap().contains("청구액을 확정"));
        let unknown=json!({"kind":"STAGE_BLOCKER","severity":"ACTION","detail":"다른 오류"});
        assert!(issue_instruction(&unknown).is_none());
    }
    #[test]
    fn recovered_review_labels_keep_native_issue_identity_and_approval(){
        for (kind,old,expected) in [
            ("CLIENT","거래처를 선택하면 단가가 적용됩니다","거래처 선택 필요"),
            ("CLIENT_MATCH_CONFLICT","발주처 매칭 충돌","거래처 정보 불일치"),
            ("CLIENT_MATCH_ERROR","거래처 자동 매칭 오류","거래처 자동선택 실패"),
            ("RECIPIENT","수취인과 상품을 추가하세요","수취인 정보 없음"),
            ("NAME","수취인 이름을 확인하세요","수취인명 누락"),
            ("CONTACT","연락처가 없습니다","연락처 누락"),
            ("QUANTITY_MISSING","상품에 귀속된 수량을 확정하세요.","수량 미기재"),
            ("QUANTITY_INVALID","수량을 확인하세요","수량 오류"),
            ("PRICE","판매가를 등록하세요","판매가 미등록"),
            ("FREIGHT","배송 운임을 확인하세요","운임 미확정"),
        ]{
            let issue=json!({"kind":kind,"title":old,"canApproveCurrent":false});
            assert_eq!(review_caption(&issue,&json!({})).as_ref(),expected);
            assert_eq!(issue["title"],old);
            assert_eq!(issue["canApproveCurrent"],false);
        }
        let address=json!({"kind":"ADDRESS","recipientId":"r","title":"상세 배송주소가 부족합니다"});
        let recognition=json!({"recipients":[{"id":"r","address":""}]});
        assert_eq!(review_caption(&address,&recognition).as_ref(),"배송주소 누락");
        assert_eq!(review_caption(&address,&json!({})).as_ref(),"상세주소 확인 필요");
        let quantity=json!({"kind":"QUANTITY_MISSING","resolution":"PROVISIONALLY_APPROVED"});
        assert_eq!(review_caption(&quantity,&json!({})).as_ref(),"수량 잠정 처리 완료");
        let phone=json!({"kind":"CONTACT","title":"휴대폰 번호가 6자리입니다."});
        assert_eq!(review_caption(&phone,&json!({})).as_ref(),"휴대폰 번호가 6자리입니다.");
    }
    #[test]
    fn unidentified_product_caption_names_recipient_and_product(){
        let recognition=json!({"recipients":[{"id":"r","name":"가상 수취인","items":[{"id":"i","name":"709"}]}]});
        let issue=json!({"kind":"MODEL","severity":"ACTION","recipientId":"r","itemId":"i"});
        assert_eq!(unidentified_product_caption(&issue,&recognition).as_deref(),
            Some("가상 수취인 \"709\" 제품 선택 필요"));
        assert!(unidentified_product_caption(&issue,&json!({})).is_none());
    }
    #[test]
    fn inherited_caption_counts_distinct_deliveries_and_preserves_issue_indices(){
        let recognition=json!({"recipients":[
            {"id":"a","items":[{"id":"i1","name":"709 / 보면대","match":{"term":"보면대"}},
                {"id":"i2","name":"709 / 보면대","match":{"term":"보면대"}}]},
            {"id":"b","items":[{"id":"i3","name":"709 / 보면대","match":{"term":"보면대"}}]},
            {"id":"c","items":[{"id":"i4","name":"709 / 보면대","match":{"term":"보면대"}},
                {"id":"i5","name":"710 / 보면대","match":{"term":"보면대"}}]}
        ]});
        let issues=vec![json!({"recipientId":"a","itemId":"i1"}),
            json!({"recipientId":"a","itemId":"i2"}),json!({"recipientId":"b","itemId":"i3"}),
            json!({"recipientId":"c","itemId":"i4"}),json!({"recipientId":"c","itemId":"i5"})];
        let groups=inherited_message_groups(&issues,&[0,1,2,3,4],&recognition);
        assert_eq!(groups,vec![("'보면대'를 709로 잠정 적용 (배송 3건)".into(),vec![0,1,2,3]),
            ("'보면대'를 710으로 잠정 적용 (배송 1건)".into(),vec![4])]);
    }
    #[test]
    fn fixed_model_shortcut_prefers_visible_recipient_and_requires_item_identity(){
        let issues=vec![
            json!({"kind":"PRICE","recipientId":"r2","itemId":"p"}),
            json!({"kind":"MODEL","recipientId":"r1","itemId":"i1"}),
            json!({"kind":"MODEL_CHOICE","recipientId":"r2","itemId":"i2"}),
            json!({"kind":"MODEL","recipientId":"r3"})
        ];
        let indices=[0,1,2,3];
        assert_eq!(text(preferred_model_issue(&issues,&indices,"r2").unwrap(),"itemId"),"i2");
        assert_eq!(text(preferred_model_issue(&issues,&indices,"r3").unwrap(),"itemId"),"i1");
        assert!(preferred_model_issue(&issues,&[0,3],"r3").is_none());
        assert!(matches!(model_navigation(&issues[2]),Action::Navigate{recipient_id,item_id,..}
            if recipient_id=="r2"&&item_id.as_deref()==Some("i2")));
    }
    #[test]
    fn registration_guidance_names_the_management_path_and_keeps_manual_input(){
        let price=pricing_registration_hint("PRICE").unwrap();
        assert!(price.contains("상품 관리")&&price.contains("거래처 등급별 판매가")&&price.contains("단가를 직접 입력"));
        let freight=pricing_registration_hint("FREIGHT").unwrap();
        assert!(freight.contains("운임 관리")&&freight.contains("수량별 운임")&&freight.contains("운임을 직접 입력"));
        for kind in ["MODEL","CLIENT","QUANTITY_MISSING","NOTICE","ACCOUNT"]{
            assert!(pricing_registration_hint(kind).is_none());
        }
    }
    #[test]
    fn required_fields_preserve_all_actionable_cells_and_exact_recipient_identity(){
        let issue=|r:&str,i:&str,k:&str,f:&str,severity:&str|json!({
            "orderId":"o","recipientId":r,"itemId":i,"kind":k,"severity":severity,
            "linkKey":format!("{}:{f}",if i.is_empty(){r}else{i})
        });
        let issues=vec![issue("r","","NAME","name","ACTION"),
            issue("r","","CONTACT","mobile","ACTION"),
            issue("r","i","QUANTITY_MISSING","quantity","ACTION"),
            issue("r","i","PRICE","price","ACTION"),
            issue("r","i","MODEL","product","INFO"),
            issue("other","","ADDRESS","address","ACTION")];
        let fields=RequiredFields::from_issues(&issues,"o","r");
        for (id,field) in [("r","name"),("r","mobile"),("i","quantity"),("i","price")]{
            assert!(fields.contains(id,field));
        }
        assert!(!fields.contains("r","address")&&!fields.contains("r","phone"));
        assert!(!fields.contains("i","product"));
        assert!(!RequiredFields::from_issues(&issues,"different","r").contains("r","name"));
    }
    #[test]
    fn ordered_guidance_excludes_wrong_stage_and_noncell_issues(){
        let recipients=vec![json!({"id":"a","name":"A","items":[{"id":"i"}]}),
            json!({"id":"b","name":"B","items":[]})];
        let issue=|rid:&str,item:Option<&str>,kind:&str,field:&str,stage:i64|json!({
            "recipientId":rid,"itemId":item,"kind":kind,"linkKey":format!("{}:{field}",item.unwrap_or(rid)),
            "blockingStage":stage,"severity":"ACTION","title":kind
        });
        let issues=vec![issue("b",None,"NAME","name",1),
            issue("a",Some("i"),"QUANTITY_MISSING","quantity",1),
            issue("a",None,"ADDRESS","address",1),
            issue("a",None,"CONTACT","mobile",2),
            issue("a",Some("i"),"MODEL","product",1)];
        let targets=ordered_input_targets(&recipients,&issues,None);
        assert_eq!(targets.iter().map(|(r,_,f,_)|(r.as_str(),*f)).collect::<Vec<_>>(),
            vec![("a","address"),("a","quantity"),("b","name")]);
        assert_eq!(ordered_input_targets(&recipients,&issues,Some("A: ADDRESS")).len(),1);
        assert!(ordered_input_targets(&recipients,&issues,Some("database failure")).is_empty());
    }
    #[test]
    fn input_failure_matching_rejects_unrelated_recipient_and_technical_error(){
        assert!(failure_matches_issue("김씨: 수량을 입력하세요.","김씨","수량을 입력하세요","김씨 / 수량을 입력하세요"));
        assert!(!failure_matches_issue("박씨: 수량을 입력하세요","김씨","수량을 입력하세요","김씨 / 수량을 입력하세요"));
        assert!(!failure_matches_issue("저장 실패: 수량을 입력하세요","김씨","수량을 입력하세요","김씨 / 수량을 입력하세요"));
    }
    #[test]
    fn empty_review_caption_uses_full_body_center_without_count_reservation(){
        let body=egui::Rect::from_min_size(egui::pos2(26.0,76.0),egui::vec2(580.0,152.0));
        assert_eq!(review_empty_overlay_rect(body),Some(body));
        let short=egui::Rect::from_min_size(body.min,egui::vec2(580.0,60.0));
        assert_eq!(review_empty_overlay_rect(short),Some(short));
        let tiny=egui::Rect::from_min_size(body.min,egui::vec2(580.0,1.0));
        assert!(review_empty_overlay_rect(tiny).is_none());
    }
    #[test]
    fn empty_review_caption_keeps_the_shared_overlay_and_metadata_font(){
        let source=include_str!("review_panel.rs").split("#[cfg(test)]").next().unwrap();
        assert!(source.contains("management_body_state_overlay("));
        let shared=include_str!("status_notice/mod.rs").split("fn management_body_state_overlay(")
            .nth(1).unwrap().split("fn body_error_overlay(").next().unwrap();
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
