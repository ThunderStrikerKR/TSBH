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
// Resource ceilings apply to untrusted input only; internal export self-checks
// retain their existing path. No extraction to disk, no formula execution.
const MAX_XLSX_ENTRIES:usize=4096;
const MAX_XLSX_PART_BYTES:u64=64*1024*1024;
const MAX_XLSX_EXPANDED_BYTES:u64=128*1024*1024;

// Excel coordinate limits are format limits, not additional business row limits.
// The dense-area ceiling protects Calamine Range/StyleRange allocations.
const XLSX_ROWS:u32=1_048_576;
const XLSX_COLUMNS:u32=16_384;
const MAX_XLSX_DENSE_CELLS:u64=1_000_000;
const COORD_ERROR:&str="XLSX 셀 좌표 또는 범위가 올바르지 않습니다.";
const AREA_ERROR:&str="XLSX 셀 범위가 너무 넓어 안전하게 읽을 수 없습니다.";

#[derive(Default)]
struct SheetBudget{
    min:Option<(u32,u32)>,
    max:Option<(u32,u32)>,
    count:u64,
}
impl SheetBudget{
    fn record(&mut self,pos:Option<(u32,u32)>)->Result<(),String>{
        self.count+=1;
        if self.count>MAX_XLSX_DENSE_CELLS{return Err(AREA_ERROR.into())}
        if let Some((r,c))=pos{
            if r>=XLSX_ROWS||c>=XLSX_COLUMNS{return Err(COORD_ERROR.into())}
            let lo=self.min.map_or((r,c),|(a,b)|(a.min(r),b.min(c)));
            let hi=self.max.map_or((r,c),|(a,b)|(a.max(r),b.max(c)));
            let area=(u64::from(hi.0-lo.0)+1)*(u64::from(hi.1-lo.1)+1);
            if area>MAX_XLSX_DENSE_CELLS{return Err(AREA_ERROR.into())}
            self.min=Some(lo);self.max=Some(hi);
        }
        Ok(())
    }
}
fn row_coordinate(raw:&[u8])->Result<u32,String>{
    if raw.is_empty(){return Err(COORD_ERROR.into())}
    let mut row=0u32;
    for &b in raw{
        if !b.is_ascii_digit(){return Err(COORD_ERROR.into())}
        row=row.checked_mul(10).and_then(|v|v.checked_add(u32::from(b-b'0')))
            .filter(|v|*v<=XLSX_ROWS).ok_or(COORD_ERROR)?;
    }
    row.checked_sub(1).ok_or_else(||COORD_ERROR.into())
}
fn cell_coordinate(raw:&[u8])->Result<(u32,u32),String>{
    let split=raw.iter().position(|b|!b.is_ascii_alphabetic()).unwrap_or(raw.len());
    if split==0||split==raw.len(){return Err(COORD_ERROR.into())}
    let mut col=0u32;
    for &b in &raw[..split]{
        col=col.checked_mul(26).and_then(|v|v.checked_add(u32::from(b.to_ascii_uppercase()-b'A')+1))
            .filter(|v|*v<=XLSX_COLUMNS).ok_or(COORD_ERROR)?;
    }
    Ok((row_coordinate(&raw[split..])?,col-1))
}
fn validate_reference(raw:&[u8])->Result<(),String>{
    let mut parts=raw.split(|b|*b==b':');
    let start=cell_coordinate(parts.next().ok_or(COORD_ERROR)?)?;
    if let Some(end)=parts.next(){
        let end=cell_coordinate(end)?;
        if end.0<start.0||end.1<start.1||parts.next().is_some(){return Err(COORD_ERROR.into())}
    }
    Ok(())
}
// Inspect worksheet XML before Calamine parses numeric references. The part
// name is deliberately not used: a workbook relationship can point elsewhere.
fn validate_sheet_xml<R:std::io::BufRead>(input:R)->Result<(),String>{
    use quick_xml::{Reader,events::Event};
    let mut xml=Reader::from_reader(input);
    let mut buf=Vec::new();
    let mut root_seen=false;
    let mut budget=SheetBudget::default();
    let invalid=||"XLSX 시트 XML을 읽을 수 없습니다.".to_owned();
    loop{
        buf.clear();
        match xml.read_event_into(&mut buf).map_err(|_|invalid())?{
            Event::Start(e)|Event::Empty(e)=>{
                let tag=e.local_name();
                if !root_seen{
                    root_seen=true;
                    if tag.as_ref()!=b"worksheet"{return Ok(())}
                }
                let mut pos=None;
                for attr in e.attributes(){
                    let attr=attr.map_err(|_|invalid())?;
                    match (tag.as_ref(),attr.key.as_ref()){
                        (b"row",b"r")=>{row_coordinate(attr.value.as_ref())?;},
                        (b"c",b"r")=>{pos=Some(cell_coordinate(attr.value.as_ref())?);},
                        (b"dimension"|b"mergeCell"|b"f",b"ref")=>validate_reference(attr.value.as_ref())?,
                        _=>{}
                    }
                }
                if tag.as_ref()==b"c"{budget.record(pos)?;}
            }
            Event::Eof=>return Ok(()),
            _=>{}
        }
    }
}
// Also validate the actual parser's positions (including implicit references).
// Call only for worksheets that are about to be converted to a dense range.
pub(crate) fn guard_sheet(book:&mut Xlsx<Cursor<&[u8]>>,name:&str)->Result<(),String>{
    let mut cells=book.worksheet_cells_reader(name)
        .map_err(|_|"XLSX 시트 좌표를 확인할 수 없습니다.".to_owned())?;
    let mut budget=SheetBudget::default();
    while let Some((cell,_))=cells.next_cell_with_style_id()
        .map_err(|_|"XLSX 셀 좌표를 확인할 수 없습니다.".to_owned())?{
        budget.record(Some(cell.get_position()))?;
    }
    Ok(())
}

fn validate_archive(bytes:&[u8],entry_limit:usize,part_limit:u64,total_limit:u64)->Result<(),String>{
    let invalid=||"XLSX 압축 구조가 손상되었거나 읽을 수 없습니다.".to_owned();
    let mut zip=zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_|invalid())?;
    if zip.is_empty()||zip.len()>entry_limit{
        return Err("XLSX 내부 파일 수가 허용 범위를 초과했습니다.".into());
    }
    if zip.has_duplicate_names(){
        return Err("XLSX 내부 파일 이름이 중복되었습니다.".into());
    }
    // First pass: check declared sizes before decompressing any part.
    let mut declared=0u64;
    for i in 0..zip.len(){
        let entry=zip.by_index_data(i).map_err(|_|invalid())?;
        if entry.size()>part_limit{
            return Err("XLSX 내부 파일의 압축 해제 크기가 허용 한도를 초과했습니다.".into());
        }
        declared=declared.checked_add(entry.size()).filter(|n|*n<=total_limit)
            .ok_or("XLSX 전체 압축 해제 크기가 허용 한도를 초과했습니다.")?;
    }
    // Second pass: count real decompressed bytes with constant-memory sink.
    // Do not trust the central directory's size; read up to declared + 1.
    let mut actual=0u64;
    for i in 0..zip.len(){
        let mut entry=zip.by_index(i).map_err(|_|invalid())?;
        if entry.encrypted()||entry.is_symlink(){
            return Err("암호화되거나 링크가 포함된 XLSX 파일은 읽을 수 없습니다.".into());
        }
        let expected=entry.size();
        let remaining=total_limit.checked_sub(actual).ok_or("XLSX 전체 압축 해제 한도 초과")?;
        let bound=expected.min(part_limit).min(remaining);
        let count=std::io::copy(&mut std::io::Read::take(&mut entry,bound+1),&mut std::io::sink())
            .map_err(|_|invalid())?;
        if count!=expected||count>bound{
            return Err("XLSX 압축 해제 크기가 선언된 정보와 일치하지 않습니다.".into());
        }
        actual=actual.checked_add(count).ok_or("XLSX 전체 압축 해제 한도 초과")?;
    }
    // Check XML worksheet coordinate syntax only after the byte limits passed.
    // Non-worksheet parts and binary attachments are not subjected to cell rules.
    use std::io::{BufRead,BufReader};
    for i in 0..zip.len(){
        let entry=zip.by_index(i).map_err(|_|invalid())?;
        let mut input=BufReader::new(entry);
        let head=input.fill_buf().map_err(|_|invalid())?;
        let head=head.strip_prefix(b"\xef\xbb\xbf").unwrap_or(head);
        if head.iter().copied().find(|b|!b.is_ascii_whitespace())==Some(b'<'){
            validate_sheet_xml(input)?;
        }
    }
    Ok(())
}

pub(crate) fn open_book(bytes:&[u8],limit:usize)->Result<Xlsx<Cursor<&[u8]>>,String>{
    if bytes.is_empty()||bytes.len()>limit||!bytes.starts_with(b"PK"){
        return Err("허용된 크기의 XLSX 파일을 선택하세요.".into());
    }
    validate_archive(bytes,MAX_XLSX_ENTRIES,MAX_XLSX_PART_BYTES,MAX_XLSX_EXPANDED_BYTES)?;
    Xlsx::new(Cursor::new(bytes)).map_err(|e|format!("XLSX 파일을 읽지 못했습니다: {e}"))
}

#[cfg(test)]
mod xlsx_resource_tests{
    use super::*;
    use std::io::Write;
    fn archive(parts:&[(&str,usize)])->Vec<u8>{
        let mut writer=zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name,size) in parts{
            let options=zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            writer.start_file(*name,options).unwrap();
            writer.write_all(&vec![b'x';*size]).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }
    #[test]
    fn rejects_single_expanded_part_before_workbook_parse(){
        let bytes=archive(&[("xl/worksheets/sheet1.xml",4096)]);
        assert!(validate_archive(&bytes,10,2048,8192).unwrap_err().contains("내부 파일의 압축 해제"));
    }
    #[test]
    fn rejects_total_expanded_size_even_when_parts_fit(){
        let bytes=archive(&[("a.xml",1024),("b.xml",1024)]);
        assert!(validate_archive(&bytes,10,2048,1500).unwrap_err().contains("전체 압축 해제"));
    }
    #[test]
    fn rejects_excess_entries(){
        let bytes=archive(&[("a.xml",1),("b.xml",1)]);
        assert!(validate_archive(&bytes,1,2048,4096).unwrap_err().contains("파일 수"));
    }
    #[test]
    fn accepts_high_compression_when_absolute_sizes_are_safe(){
        let bytes=archive(&[("a.xml",4096),("b.xml",4096)]);
        assert!(validate_archive(&bytes,2,4096,8192).is_ok());
    }
    #[test]
    fn malformed_zip_fails_before_workbook_parse(){
        assert!(open_book(b"PK invalid",1024).is_err());
    }
    #[test]
    fn rejects_sparse_wide_workbook_before_dense_range(){
        let mut workbook=Workbook::new();
        let sheet=workbook.add_worksheet();
        sheet.write_string(0,0,"header").unwrap();
        sheet.write_string(10000,16383,"far").unwrap();
        let bytes=workbook.save_to_buffer().unwrap();
        assert!(read_factory_reply("synthetic.xlsx",&bytes).unwrap_err().contains("셀 범위가 너무 넓어"));
    }
    #[test]
    fn rejects_invalid_coordinates_and_reversed_range(){
        for raw in [b"A0".as_slice(),b"XFE1",b"A1048577",b"A42949672960",b"ZZZZZZZZZZZ1",b"1A"]{
            assert!(cell_coordinate(raw).is_err());
        }
        assert_eq!(cell_coordinate(b"XFD1048576").unwrap(),(1048575,16383));
        assert!(validate_reference(b"B2:A1").is_err());
        assert!(validate_reference(b"A1:B2:C3").is_err());
    }
    #[test]
    fn rejects_malformed_raw_sheet_coordinates_without_parser_allocation(){
        for xml in [
            r#"<worksheet><dimension ref="A42949672960"/></worksheet>"#,
            r#"<worksheet><sheetData><row r="0"><c r="A1"/></row></sheetData></worksheet>"#,
            r#"<worksheet><sheetData><row r="1"><c r="XFE1"/></row></sheetData></worksheet>"#,
            r#"<worksheet><mergeCells><mergeCell ref="B2:A1"/></mergeCells></worksheet>"#,
        ]{
            assert!(validate_sheet_xml(Cursor::new(xml.as_bytes())).is_err());
        }
    }
    #[test]
    fn accepts_full_sheet_dimension_when_real_cells_are_small(){
        let xml=br#"<worksheet><dimension ref="A1:XFD1048576"/><sheetData><row r="1"><c r="A1"/><c r="Q1"/></row></sheetData></worksheet>"#;
        assert!(validate_sheet_xml(Cursor::new(xml)).is_ok());
    }
    #[test]
    fn rejects_implicit_and_style_only_coordinate_span_via_budget(){
        let mut budget=SheetBudget::default();
        budget.record(Some((0,0))).unwrap();
        assert!(budget.record(Some((9999,16383))).is_err());
        assert!(SheetBudget::default().record(Some((XLSX_ROWS,0))).is_err());
    }
    #[test]
    fn ordinary_reply_with_formula_and_merge_remains_readable(){
        let mut workbook=Workbook::new();
        let sheet=workbook.add_worksheet();
        sheet.write_string(0,0,"수하인").unwrap();
        sheet.write_string(1,0,"합성 수하인").unwrap();
        sheet.write_formula(1,6,"=3000").unwrap();
        sheet.write_string(1,16,"0000123456").unwrap();
        sheet.merge_range(0,8,0,9,"품목",&Format::new()).unwrap();
        let bytes=workbook.save_to_buffer().unwrap();
        assert!(read_factory_reply("synthetic.xlsx",&bytes).is_ok());
    }
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
    guard_sheet(&mut book,META)?;
    guard_sheet(&mut book,CLIENT_SHEET)?;
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

/// Resource ceiling for one incoming workbook, not a document-format rule.
pub const FACTORY_REPLY_MAX_BYTES:usize=50*1024*1024;
pub fn read_factory_reply(filename:&str,bytes:&[u8])->Result<Value,String>{
    if !filename.to_ascii_lowercase().ends_with(".xlsx")||
        filename.contains('/')||filename.contains('\\')||filename.contains('\0'){
        return Err("공장 회신 파일명 오류".into());
    }
    let mut book=open_book(bytes,FACTORY_REPLY_MAX_BYTES)?;
    let name=book.sheet_names().first().cloned().ok_or("회신 시트 누락")?;
    guard_sheet(&mut book,&name)?;
    // Merged cells outside shipping data are irrelevant. Do not propagate
    // merged values into another shipment; validate each row's saved values.
    let sheet=book.worksheet_range(&name).map_err(|e|e.to_string())?;
    let last=sheet.end().map(|(r,_)|r).unwrap_or(0);
    if last>10_000{return Err("공장 회신 최대 10,000행".into())}
    // Read number formats only when numeric waybills require a precision check.
    // A cell's data type is not itself a reason to hold a reply.
    let styles=if (1..=last).any(|r|numeric(sheet.get_value((r,16)).unwrap_or(&Data::Empty))){
        book.worksheet_style(&name).ok()
    }else{None};
    let mut rows=Vec::new();
    for r in 1..=last{
        // Read saved values, including cached formula results. Never execute
        // formulas or external links. Unreadable values become missing business
        // fields and are handled by the existing name/freight/waybill checks.
        let value=|col|text(sheet.get_value((r,col)).unwrap_or(&Data::Empty)).unwrap_or_default();
        let tracking_numeric=numeric(sheet.get_value((r,16)).unwrap_or(&Data::Empty));
        let tracking_format=if tracking_numeric{
            styles.as_ref().and_then(|styles|styles.get((r,16)))
                .and_then(|style|style.number_format.as_ref())
                .map(|format|format.format_code.as_str()).unwrap_or("General")
        }else{""};
        let mut row=json!({"row":r+1,"name":value(0),"goods":value(8),
            "address":value(2),"phone":value(3),"mobile":value(4),
            "rawFreight":value(6),"tracking":value(16),
            "trackingNumeric":tracking_numeric,"trackingNumberFormat":tracking_format,
            "reservedJ":false,"extraAfterQ":false});
        // An unreadable format cannot prove a numeric identifier is exact.
        // Hold that row only; text waybills do not depend on numeric styles.
        if tracking_numeric&&styles.is_none(){
            row["trackingFormatUnreadable"]=json!(true);
        }
        let aggregate_values:Vec<String>=(0..17).map(value).collect();
        row["aggregateValues"]=json!(aggregate_values);
        rows.push(row);
    }
    Ok(json!({"id":format!("factory-reply-{:x}",Sha256::digest(bytes)),
        "filename":filename,"sourceRowCount":last,"sourceRows":rows}))
}
