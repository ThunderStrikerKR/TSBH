//! Native XLSX boundary for client masters and incoming Logen replies.
//! No UI, filesystem writes, automatic monitoring or DB mutation here.
use calamine::{Data,Reader,Xlsx};
use rust_xlsxwriter::{Format,Workbook};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::io::Cursor;

const CLIENT_FORMAT:&str="BANDO-HUB-CLIENTS-V2";
#[cfg(any(windows,test))]
const CLIENT_LEGACY:&str="BANDO-HUB-CLIENTS-V1";
const CLIENT_SHEET:&str="거래처 목록";
const META:&str="__반도허브_관리정보";
const HEADERS:[&str;17]=["__거래처ID","거래처명","주소","우편번호",
    "전화번호","휴대폰","담당자","연락처 메모","가격등급","결제방식",
    "운임 청구 제외","부가세 기본 적용","거래 상태","거래처 별칭",
    "검색 식별자","주 사용 계좌 ID","보관 상태"];
fn field<'a>(v:&'a Value,k:&str)->&'a str{v[k].as_str().unwrap_or("")}
fn list<'a>(v:&'a Value,k:&str)->&'a [Value]{v[k].as_array().map(Vec::as_slice).unwrap_or(&[])}
fn text(v:&Data)->Result<String,String>{
    match v{
        Data::Empty=>Ok(String::new()),
        Data::String(s)=>Ok(s.trim().to_owned()),
        Data::Int(n)=>Ok(n.to_string()),
        Data::Float(n) if n.is_finite()=>Ok(n.to_string()),
        Data::Bool(b)=>Ok(b.to_string()),
        _=>Err("문자 또는 숫자 셀을 사용하세요.".into())
    }
}
fn numeric(v:&Data)->bool{matches!(v,Data::Int(_)|Data::Float(_))}
fn open_book(bytes:&[u8],limit:usize)->Result<Xlsx<Cursor<&[u8]>>,String>{
    if bytes.is_empty()||bytes.len()>limit||!bytes.starts_with(b"PK"){
        return Err("허용된 크기의 XLSX 파일을 선택하세요.".into());
    }
    Xlsx::new(Cursor::new(bytes)).map_err(|e|format!("XLSX 파일을 읽지 못했습니다: {e}"))
}
#[cfg(any(windows,test))]
fn safe_sheet(book:&mut Xlsx<Cursor<&[u8]>>,name:&str)->Result<(),String>{
    if !book.merge_cells_by_sheet_name(name).map_err(|e|e.to_string())?.is_empty(){
        return Err("병합된 셀은 허용되지 않습니다.".into());
    }
    let formulas=book.worksheet_formula(name).map_err(|e|e.to_string())?;
    if formulas.used_cells().any(|(_,_,s)|!s.trim().is_empty()){
        return Err("Excel 수식은 허용되지 않습니다.".into());
    }
    Ok(())
}
fn row(c:&Value)->Result<Vec<String>,String>{
    let tier=match field(c,"tier"){
        "wholesale"=>"도매","semi"=>"반도매","retail"=>"소매","special"=>"특수",
        _=>return Err("거래처 가격등급이 올바르지 않습니다.".into())
    };
    let payment=match field(c,"payment"){
        "PREPAID"=>"선입금","POSTPAID"=>"후불",
        _=>return Err("거래처 결제방식이 올바르지 않습니다.".into())
    };
    let vat=match c["defaultApplyVat"].as_bool(){Some(true)=>"예",Some(false)=>"아니오",None=>""};
    Ok([field(c,"id"),field(c,"name"),field(c,"address"),field(c,"postcode"),
        field(c,"phone"),field(c,"mobile"),field(c,"person"),field(c,"contact"),
        tier,payment,if c["shippingFeeExcluded"]==true{"예"}else{"아니오"},vat,
        if c["active"]==true{"거래중"}else{"거래중지"},field(c,"shortNames"),
        field(c,"identifiers"),field(c,"preferredAccountId"),
        if c["deletedAt"].is_null()||c["deletedAt"]==""{"사용"}else{"보관"}]
        .into_iter().map(str::to_owned).collect())
}
pub fn export_clients(state:&Value)->Result<Vec<u8>,String>{
    let revision=state["revision"].as_u64().filter(|r|*r<=9_007_199_254_740_991)
        .ok_or("DB 버전 오류")?;
    let clients=state["clients"].as_array().ok_or("거래처 목록 누락")?;
    if clients.len()>20_000{return Err("거래처 최대 20,000행".into())}
    let mut book=Workbook::new();
    let header=Format::new().set_bold();
    {
        let sheet=book.add_worksheet();
        sheet.set_name(CLIENT_SHEET).map_err(|e|e.to_string())?;
        for (i,h) in HEADERS.iter().enumerate(){
            sheet.write_string_with_format(0,i as u16,*h,&header).map_err(|e|e.to_string())?;
            sheet.set_column_width(i as u16,if i==2{44.}else if i==0||i==15{38.}else{23.})
                .map_err(|e|e.to_string())?;
        }
        for (i,c) in clients.iter().enumerate(){
            for (j,v) in row(c)?.iter().enumerate(){
                if !v.is_empty(){sheet.write_string((i+1) as u32,j as u16,v).map_err(|e|e.to_string())?;}
            }
        }
        sheet.set_freeze_panes(1,0).map_err(|e|e.to_string())?;
        sheet.autofilter(0,0,0,16).map_err(|e|e.to_string())?;
    }
    {
        let sheet=book.add_worksheet();
        sheet.set_name("작성 안내").map_err(|e|e.to_string())?;
        for (i,(a,b)) in [("기존 거래처","ID 변경 금지. 공란은 기존 값 유지, [비우기]는 선택 항목 제거."),
            ("신규 거래처","ID를 비우고 새 행 작성. 누락된 행은 삭제되지 않습니다."),
            ("안전","수식·병합·중복·오래된 DB 버전 거부. 미리보기 승인 및 백업 후 저장."),
            ("특수 단가","참고 시트는 읽기 전용. 기존 특수 단가는 유지.")].iter().enumerate(){
            sheet.write_string(i as u32,0,*a).map_err(|e|e.to_string())?;
            sheet.write_string(i as u32,1,*b).map_err(|e|e.to_string())?;
        }
        sheet.set_column_width(1,100.).map_err(|e|e.to_string())?;
    }
    {
        let sheet=book.add_worksheet();
        sheet.set_name("입금 계좌 안내").map_err(|e|e.to_string())?;
        for (j,v) in ["계좌 ID","은행","계좌주"].iter().enumerate(){
            sheet.write_string(0,j as u16,*v).map_err(|e|e.to_string())?;
        }
        for (i,a) in list(&state["settings"],"accounts").iter().enumerate(){
            for (j,v) in ["id","bank","holder"].iter().enumerate(){
                sheet.write_string((i+1) as u32,j as u16,field(a,v)).map_err(|e|e.to_string())?;
            }
        }
    }
    {
        let sheet=book.add_worksheet();
        sheet.set_name("특수 단가 참고").map_err(|e|e.to_string())?;
        for (j,v) in ["거래처 ID","거래처명","상품 옵션 ID","특수 단가 (원)"].iter().enumerate(){
            sheet.write_string(0,j as u16,*v).map_err(|e|e.to_string())?;
        }
        let mut i=1;
        for c in clients{
            if let Some(overrides)=c["overrides"].as_object(){
                for (id,amount) in overrides{
                    for (j,v) in [field(c,"id"),field(c,"name"),id.as_str()].iter().enumerate(){
                        sheet.write_string(i,j as u16,*v).map_err(|e|e.to_string())?;
                    }
                    sheet.write_string(i,3,amount.to_string()).map_err(|e|e.to_string())?;
                    i+=1;
                }
            }
        }
    }
    {
        let sheet=book.add_worksheet();
        sheet.set_name(META).map_err(|e|e.to_string())?;
        sheet.write_string(0,0,"종류").map_err(|e|e.to_string())?;
        sheet.write_string(0,1,CLIENT_FORMAT).map_err(|e|e.to_string())?;
        sheet.write_string(1,0,"DB버전").map_err(|e|e.to_string())?;
        sheet.write_number(1,1,revision as f64).map_err(|e|e.to_string())?;
        sheet.set_very_hidden(true);
    }
    book.save_to_buffer().map_err(|e|e.to_string())
}
#[cfg(any(windows,test))]
pub fn read_clients(bytes:&[u8])->Result<Value,String>{
    let mut book=open_book(bytes,20*1024*1024)?;
    safe_sheet(&mut book,META)?;
    safe_sheet(&mut book,CLIENT_SHEET)?;
    let meta=book.worksheet_range(META).map_err(|e|e.to_string())?;
    let format=text(meta.get_value((0,1)).unwrap_or(&Data::Empty))?;
    let legacy=format==CLIENT_LEGACY;
    if !legacy&&format!=CLIENT_FORMAT{return Err("거래처 전체 목록 형식이 아닙니다.".into())}
    let revision=text(meta.get_value((1,1)).unwrap_or(&Data::Empty))?.parse::<u64>()
        .ok().filter(|r|*r<=9_007_199_254_740_991).ok_or("DB 버전 오류")?;
    let mut headers=HEADERS.to_vec();
    if legacy{headers.insert(2,"송장 표시 상호");}
    let sheet=book.worksheet_range(CLIENT_SHEET).map_err(|e|e.to_string())?;
    for (i,h) in headers.iter().enumerate(){
        if text(sheet.get_value((0,i as u32)).unwrap_or(&Data::Empty))?!=*h{
            return Err(format!("거래처 Excel 열 이름 또는 순서 오류: {h}"));
        }
    }
    let last=sheet.end().map(|(r,_)|r).unwrap_or(0);
    if last>20_000{return Err("거래처 최대 20,000행".into())}
    let mut input=Vec::new();
    for r in 1..=last{
        let mut values=Vec::new();
        for (col,h) in headers.iter().enumerate(){
            let v=sheet.get_value((r,col as u32)).unwrap_or(&Data::Empty);
            if (if legacy{[0,4,5,6,16]}else{[0,3,4,5,15]}).contains(&col)&&numeric(v){
                return Err(format!("{}행 {h}: 텍스트 형식으로 입력하세요.",r+1));
            }
            values.push(text(v)?);
        }
        if values.iter().any(|v|!v.is_empty()){input.push(json!({"row":r+1,"cells":values}));}
    }
    // Preserve 18-column V1 source; existing Rust client_preview owns conversion.
    Ok(json!({"sourceRevision":revision,"input":input}))
}

#[cfg(test)]
mod client_binary_unit_tests{
    use super::*;
    #[test]
    fn client_reader_preserves_text_identity_and_revision(){
        let state=json!({"revision":7,"clients":[{"id":"001","name":"합성 거래처","postcode":"01234","phone":"02-1234-5678","mobile":"010-0000-0000","tier":"special","payment":"PREPAID","active":true}],"settings":{"accounts":[]},"orders":[]});
        let parsed=read_clients(&export_clients(&state).unwrap()).unwrap();
        assert_eq!(parsed["sourceRevision"],7);
        assert_eq!(parsed["input"][0]["cells"][0],"001");
        assert_eq!(parsed["input"][0]["cells"][3],"01234");
    }
}

/// Shared instructions for reply review and contextual help.
pub const FACTORY_REPLY_FORMAT_GUIDANCE:&str="회신 Excel: G열에 출고 운임(0 이상의 정수), Q열에 운송장번호(텍스트 8~30자리)를 입력하세요. A열 수하인명으로 최근 출력 주문을 찾고, 같은 날짜에 이름이 중복되면 I열 품목을 비교합니다. R열 등 다른 열의 운임은 반영되지 않습니다.";

pub fn read_factory_reply(filename:&str,bytes:&[u8])->Result<Value,String>{
    if !filename.to_ascii_lowercase().ends_with(".xlsx")||
        filename.contains('/')||filename.contains('\\')||filename.contains('\0'){
        return Err("공장 회신 파일명 오류".into());
    }
    let mut book=open_book(bytes,10*1024*1024)?;
    let name=book.sheet_names().first().cloned().ok_or("회신 시트 누락")?;
    // Reject a merged sheet before interpretation; formula errors in relevant
    // columns are held per row, unrelated columns do not affect matching.
    if !book.merge_cells_by_sheet_name(&name).map_err(|e|e.to_string())?.is_empty(){
        return Err("병합된 회신 셀은 허용되지 않습니다.".into());
    }
    let formulas=book.worksheet_formula(&name).map_err(|e|e.to_string())?;
    let sheet=book.worksheet_range(&name).map_err(|e|e.to_string())?;
    let last=sheet.end().map(|(r,_)|r).unwrap_or(0);
    if last>1000{return Err("공장 회신 최대 1,000행".into())}
    let mut rows=Vec::new();
    for r in 1..=last{
        let mut values=Vec::new();
        let mut problems=Vec::new();
        for (col,label) in [(0,"A"),(6,"G"),(16,"Q")]{
            let v=sheet.get_value((r,col)).unwrap_or(&Data::Empty);
            if formulas.get_value((r,col)).is_some_and(|s|!s.trim().is_empty()){
                problems.push(format!("{label}열 수식"));
            }
            match text(v){Ok(s)=>values.push(s),Err(e)=>{problems.push(format!("{label}열 {e}"));values.push(String::new());}}
        }
        let tracking_numeric=numeric(sheet.get_value((r,16)).unwrap_or(&Data::Empty));
        // Calamine's cell value alone cannot prove that a numeric tracking
        // cell had no leading-zero number format. Hold it rather than guess.
        if tracking_numeric{problems.push("Q열 숫자형 운송장번호 원본 서식 확인 필요".into());}
        // I is optional corroboration only. An unreadable product cell
        // cannot block an otherwise uniquely named dispatch.
        let goods=if formulas.get_value((r,8)).is_some_and(|s|!s.trim().is_empty()){
            String::new()
        }else{text(sheet.get_value((r,8)).unwrap_or(&Data::Empty)).unwrap_or_default()};
        // Existing C/D/E are read only as optional notification evidence.
        // Invalid, empty, or formula cells do not affect matching or import.
        let evidence=|col|if formulas.get_value((r,col)).is_some_and(|s|!s.trim().is_empty()){
            String::new()
        }else{text(sheet.get_value((r,col)).unwrap_or(&Data::Empty)).unwrap_or_default()};
        let mut row=json!({"row":r+1,"name":values[0],"goods":goods,
            "address":evidence(2),"phone":evidence(3),"mobile":evidence(4),
            "rawFreight":values[1],"tracking":values[2],
            "trackingNumeric":tracking_numeric,"trackingNumberFormat":"",
            "reservedJ":false,"extraAfterQ":false});
        if !problems.is_empty(){row["sourceIssue"]=json!(format!("{}행: {}",r+1,problems.join("; ")));}
        rows.push(row);
    }
    Ok(json!({"id":format!("factory-reply-{:x}",Sha256::digest(bytes)),
        "filename":filename,"sourceRowCount":last,"sourceRows":rows}))
}
