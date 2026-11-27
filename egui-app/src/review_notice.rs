//! Shared presentation and button policy for reception review notices.
//! Native issue identity, severity, approval evidence and workflow gates remain authoritative.
use eframe::egui::Ui;
use serde_json::{json,Value};
use crate::theme::{ButtonRole,ReviewTone};

fn text<'a>(row:&'a Value,key:&str)->&'a str {
    row.get(key).and_then(Value::as_str).unwrap_or("")
}

pub(crate) const PRODUCT_ACTION:&str="상품 확인";
pub(crate) const FREIGHT_ACTION:&str="운임 입력";
pub(crate) const APPROVE_ACTION:&str="현재 정보 승인";
pub(crate) const INQUIRY_WAIT:&str="거래처 회신 대기";

pub(crate) fn input_action_caption(field:&str)->&'static str{
    match field{"unitPrice"=>"단가 입력","quantity"=>"수량 수정",_=>"입력 위치로 이동"}
}
pub(crate) fn navigation_caption(kind:&str)->&'static str{
    match kind{
        "QUANTITY_MISSING"=>input_action_caption("quantity"),
        "PRICE"=>input_action_caption("unitPrice"),
        "FREIGHT"=>FREIGHT_ACTION,
        "UNASSIGNED_EVIDENCE"=>"수동 분류에서 확인",
        "CLIENT"=>"거래처 선택",
        k if k.starts_with("CLIENT_MATCH_")=>"거래처 선택",
        "MODEL"|"MODEL_CHOICE"|"ALIAS_CONFLICT"|"CATEGORY_CONFLICT"|"QUANTITY_LINK"|"OPTION"=>"해결 절차 보기",
        _=>input_action_caption(""),
    }
}
pub(crate) fn confirmed_input_field(issue:&Value)->Option<&'static str>{
    if text(issue,"severity")!="ACTION"{return None;}
    match text(issue,"kind"){
        "NAME"|"CANDIDATE_name"=>Some("name"),
        "ADDRESS"|"CANDIDATE_address"=>Some("address"),
        "CANDIDATE_phone"=>Some("phone"),
        "CANDIDATE_mobile"=>Some("mobile"),
        "CONTACT"=>Some(if text(issue,"linkKey").ends_with(":phone"){"phone"}else{"mobile"}),
        "PRICE"=>Some("unitPrice"),
        "QUANTITY_MISSING"|"QUANTITY_INVALID"|"QUANTITY_LINK"=>Some("quantity"),
        _=>None,
    }
}
pub(crate) fn confirmed_product_action(issue:&Value)->bool{
    text(issue,"severity")=="ACTION"&&(model_resolution_kind(text(issue,"kind"))||text(issue,"kind")=="OPTION")
}

// Normal stage waiting notices remain visible even when the order card
// already shows the next action. Only their presentation tone changes.
pub(crate) fn waiting_payment_amount(detail:&str)->bool{
    detail.rsplit_once(" 입금 청구 ").is_some_and(|(dealer,amount)|
        !dealer.trim().is_empty()&&amount.strip_suffix('원').is_some_and(|n|
            !n.is_empty()&&n.chars().any(|c|c.is_ascii_digit())&&
            n.chars().all(|c|c.is_ascii_digit()||c==',')))
}
pub(crate) fn routine_stage_notice(issue:&Value)->bool{
    if text(issue,"kind")!="STAGE_BLOCKER"||text(issue,"severity")!="ACTION"{return false;}
    let detail=text(issue,"detail").trim().trim_end_matches('.');
    let problem=detail.rsplit_once(": ").map(|(_,problem)|problem).unwrap_or(detail);
    matches!(problem,"운임 적용 필요"|"계좌 안내 완료 필요")||waiting_payment_amount(detail)
}
pub(crate) fn issue_tone(issue:&Value)->ReviewTone {
    if routine_stage_notice(issue){return ReviewTone::Info;}
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

pub(crate) fn model_resolution_kind(kind:&str)->bool{
    matches!(kind,"MODEL"|"MODEL_CHOICE"|"ALIAS_CONFLICT"|"CATEGORY_CONFLICT"|"MODEL_CONFIRM")
}
pub(crate) const MODEL_RESOLUTION_GUIDANCE:&str=
    "품목 클릭 → 상품 변경 → 선택 → 적용 → 초안에 적용";

pub(crate) fn validation_approval_request(issue:&Value)->Value{
    json!({"issueId":issue["issueId"],"approvalBasis":issue["approvalBasis"]})
}
pub(crate) fn show_validation_approval_action(ui:&mut Ui,issue:&Value,enabled:bool)->Option<bool>{
    let cancel=issue["canCancelCurrent"]==true;
    if !cancel && issue["canApproveCurrent"]!=true{return None;}
    crate::button::review_action(ui,if cancel{"승인 취소"}else if text(issue,"kind")=="UNASSIGNED_EVIDENCE"{"주문 정보 아님"}else{APPROVE_ACTION},
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
pub(crate) fn review_item_context<'a>(issue:&Value,recognition:&'a Value)->Option<(&'a Value,&'a Value)>{
    let rid=text(issue,"recipientId");
    let iid=text(issue,"itemId");
    if rid.is_empty()||iid.is_empty(){return None;}
    let row=recognition.get("recipients")?.as_array()?.iter()
        .find(|r|text(r,"id")==rid)?;
    let item=row.get("items")?.as_array()?.iter().find(|i|text(i,"id")==iid)?;
    Some((row,item))
}
pub(crate) fn unidentified_product_caption(issue:&Value,recognition:&Value)->Option<String>{
    if text(issue,"kind")!="MODEL"||text(issue,"severity")=="INFO"||
        text(issue,"title")=="등록된 상품과 옵션을 확인하세요"{return None;}
    let (row,item)=review_item_context(issue,recognition)?;
    let who=text(row,"name").trim();
    let product=text(item,"name").trim();
    let raw=text(item,"raw").trim();
    let product=if product.is_empty(){raw}else{product};
    if product.is_empty(){return None;}
    Some(format!("{} \"{product}\" 제품 선택 필요",if who.is_empty(){"수취인"}else{who}))
}
// Korean particles follow the displayed source/model, not its quotation mark.
pub(crate) fn final_consonant(value:&str)->Option<u32>{
    let c=value.trim().chars().last()?;
    let n=c as u32;
    if (0xac00..=0xd7a3).contains(&n){return Some((n-0xac00)%28);}
    match c {
        '0'=>Some(21),'1'|'7'|'8'=>Some(8),'3'=>Some(16),'6'=>Some(1),
        '2'|'4'|'5'|'9'=>Some(0),_=>None,
    }
}
pub(crate) fn inherited_message_groups(issues:&[Value],indices:&[usize],recognition:&Value)
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
pub(crate) fn review_instruction(kind:&str)->Option<&'static str>{
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
        "MODEL"|"MODEL_CHOICE"=>Some("발주원문에 맞는 상품 모델을 선택하세요."),
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
    if text(issue,"kind")=="CHARGE_CALCULATION"{
        let targets=issue["calculationTargets"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        if targets.is_empty(){
            return format!("청구액 계산 불가\n{label}");
        }
        let names=targets.iter().map(|target|{
            let name=text(target,"name");
            if name.is_empty(){"수취인 미지정"}else{name}
        }).collect::<Vec<_>>().join(", ");
        return format!("청구액 계산 불가 (배송 {}건)\n{names}",targets.len());
    }
    if text(issue,"resolution")=="USER_APPROVED"||text(issue,"severity")=="INFO"{
        return label.to_owned();
    }
    if let Some(caption)=unidentified_product_caption(issue,recognition){return caption;}
    let old=text(issue,"title");
    let caption=review_caption(issue,recognition);
    if old.is_empty(){return label.to_owned();}
    if label==old{return caption.into_owned();}
    if let Some(prefix)=label.strip_suffix(old){
        if prefix.ends_with(" / ")||prefix.ends_with(": "){
            return format!("{prefix}{caption}");
        }
    }
    // Item details often contain recipient/product only, not the actual
    // issue title. Keep the evidence and make the reason visible as well.
    if !text(issue,"itemId").is_empty()&&!old.is_empty()&&!label.contains(caption.as_ref()){
        return format!("{label} — {caption}");
    }
    label.to_owned()
}
pub(crate) fn issue_instruction(issue:&Value)->Option<&'static str>{
    if text(issue,"severity")=="INFO"{return None;}
    let kind=text(issue,"kind");
    if kind=="CHARGE_CALCULATION"{
        return Some("해당 배송 건의 상품 단가, 수량, 운임 및 부가세를 확인하세요. 입금 확인 단계에서는 기존 입금 내역도 확인하세요.");
    }
    if kind=="STAGE_BLOCKER"{
        let detail=text(issue,"detail").trim().trim_end_matches('.');
        let problem=detail.rsplit_once(": ").map(|(_,problem)|problem).unwrap_or(detail);
        // The native stage engine aggregates the outstanding balance by
        // dealer, not recipient. Preserve the exact dealer and amount.
        if detail.ends_with(" 입금 청구액 확인 필요")||problem=="입금 청구액 확인 필요"{
            return Some("청구 금액과 기존 입금 내역을 확인한 뒤 입금 확인 단계를 진행하세요.");
        }
        if waiting_payment_amount(detail){
            return Some("거래처의 미입금 금액입니다. 실제 입금액을 확인한 뒤 입금 확인 단계를 진행하세요.");
        }
        return match problem{
            "청구액 미확정"=>Some("품목 단가, 수량 및 운임을 확인하여 청구액을 확정하세요."),
            "운임 적용 필요"=>Some("배송 운임을 확인하고 운임 적용 단계를 완료하세요."),
            "계좌 안내 완료 필요"=>Some("입금 계좌와 청구액을 확인하고 계좌 안내 단계를 완료하세요."),
            "입금 부족"|"입금액이 부족합니다"=>Some("청구액과 실제 입금액을 대조하고 부족한 금액의 입금을 확인하세요."),
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
