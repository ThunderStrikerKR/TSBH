//! Native XLSX boundary for the existing React "unified product workbook" V2.
//! The file format (not the mutation rules) lives here. Preview and commit
//! always pass through the existing Rust catalog_clients business engine.
#[cfg(any(windows,test))]
use calamine::{Data,Reader};
use rust_xlsxwriter::{Color,Format,Workbook};
use serde_json::Value;
#[cfg(any(windows,test))]
use serde_json::json;
#[cfg(any(windows,test))]
use std::path::Path;

pub const MAX_BYTES:u64=20*1024*1024;
const MAX_ROWS:u32=20_000;
const FORMAT:&str="BANDO-HUB-PRODUCTS-UNIFIED-V2";
#[cfg(any(windows,test))]
const OLD_FORMAT:&str="BANDO-HUB-PRODUCTS-UNIFIED-V1";
const HEADERS:[&str;20]=[
    "__상품ID","신규등록키","상품명","상품설명","색상",
    "도매가","반도매가","소매가","상품군","출고구분",
    "카테고리 ID 경로","브랜드 ID","규격","판매 단위","거래 상태",
    "__보관 상태","__가격기록ID","공식 원본 구성(읽기 전용)",
    "__가격 원본","__공식 사이트 옵션 ID"
];
const WIDTHS:[f64;20]=[
    38.,22.,44.,50.,19.,17.,17.,17.,18.,13.,
    32.,34.,22.,20.,15.,16.,38.,32.,19.,35.
];
const GUIDE:[(&str,&str);12]=[
    ("항목","작성 규칙"),
    ("작업 방법","독립 상품 1개가 한 행입니다. 상품명, 설명, 색상, 세 종류의 가격을 같은 행에서 수정하세요."),
    ("기존 상품","__상품ID 및 __가격기록ID, 출고구분과 상품군을 변경하지 마세요."),
    ("신규 상품","__상품ID와 __가격기록ID는 비우고 신규등록키, 상품명, 상품군, 색상을 입력하세요."),
    ("색상 구분 없음","색상 칸에 '단일'로 입력하세요. 다른 색상이나 구성은 별도의 상품 행으로 등록하세요."),
    ("상품명과 설명","상품명은 거래처, 매장, 공장 공통 표시입니다. 가방 포함 여부와 바퀴 구성 등은 미포함도 생략하지 말고 명시하세요. 설명은 매장 내부 참고입니다."),
    ("상품 식별","상품명에 모델명, 색상 및 판매 구성을 기재하고 내부 상품 ID는 변경하지 않습니다."),
    ("가격","도매가, 반도매가, 소매가는 각기 다른 독립 상품에 직접 입력합니다. 공란은 기존 값 유지, [비우기]는 값 제거입니다."),
    ("읽기 전용","__상품ID, __가격기록ID, 공식 원본 구성, __가격 원본, __공식 사이트 옵션 ID, __보관 상태는 수정하지 마세요."),
    ("누락 행","Excel에서 누락된 기존 상품은 삭제하거나 중지하지 않습니다."),
    ("검증","수식, 병합 셀, 중복 ID, 미확인 신규등록키, 오래된 DB 버전과 잘못된 참조는 거부합니다."),
    ("반영","미리보기 검토 및 명시적 승인 후 백업을 만들고 변경사항을 저장합니다."),
];
fn field<'a>(value:&'a Value,key:&str)->&'a str{
    value.get(key).and_then(Value::as_str).unwrap_or("")
}
fn list<'a>(value:&'a Value,key:&str)->&'a [Value]{
    value.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}
fn maybe_string(value:&Value)->String{
    match value{
        Value::Null=>String::new(),
        Value::String(s)=>s.clone(),
        Value::Number(n)=>n.to_string(),
        Value::Bool(b)=>b.to_string(),
        _=>String::new(),
    }
}
fn exported_row(product:&Value)->Result<Vec<String>,String>{
    let variants=list(product,"variants");
    if variants.len()!=1 {
        return Err(format!("독립 상품 규칙 위반: {}의 가격 기록이 하나가 아닙니다.",field(product,"name")));
    }
    let v=&variants[0];
    if field(v,"color")!=field(product,"defaultColor"){
        return Err(format!("상품 색상과 가격 기록 색상이 일치하지 않습니다: {}",field(product,"name")));
    }
    let category=product["categories"].as_array().map(|items|{
        items.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" / ")
    }).unwrap_or_default();
    Ok(vec![
        field(product,"id").to_owned(),String::new(),field(product,"name").to_owned(),
        field(product,"subtitle").to_owned(),field(v,"color").to_owned(),
        maybe_string(&v["wholesale"]),maybe_string(&v["semi"]),maybe_string(&v["retail"]),
        if field(product,"catalog")=="BANDOMALL"{"반도몰"}else{"반도스탠드"}.into(),
        if field(product,"origin")=="STORE"{"매장"}else{"공장"}.into(),
        category,field(product,"brandId").to_owned(),field(product,"specification").to_owned(),
        field(product,"sellingUnit").to_owned(),
        if product["active"]==Value::Bool(false){"중지"}else{"사용"}.into(),
        if !field(product,"deletedAt").is_empty()||!field(product,"supersededBy").is_empty(){
            "보관"
        }else{"정상"}.into(),
        field(v,"id").to_owned(),field(v,"option").to_owned(),
        field(v,"source").to_owned(),field(v,"siteOptionId").to_owned(),
    ])
}
pub fn export(state:&Value)->Result<Vec<u8>,String>{
    let mut book=Workbook::new();
    let header=Format::new().set_bold().set_font_color(Color::White)
        .set_background_color(Color::RGB(0x303030));
    let numbers=Format::new().set_num_format("#,##0");
    {
        let sheet=book.add_worksheet();
        sheet.set_name("상품 목록").map_err(|e|e.to_string())?;
        for (i,title) in HEADERS.iter().enumerate(){
            sheet.write_with_format(0,i as u16,*title,&header)
                .map_err(|e|e.to_string())?;
            sheet.set_column_width(i as u16,WIDTHS[i])
                .map_err(|e|e.to_string())?;
        }
        for (i,product) in list(state,"products").iter().enumerate(){
            if i>=MAX_ROWS as usize{
                return Err("상품 Excel은 20,000행까지 지원합니다.".into());
            }
            let cells=exported_row(product)?;
            for (j,value) in cells.iter().enumerate(){
                if value.is_empty(){continue;}
                if (5..=7).contains(&j) {
                    if let Ok(amount)=value.parse::<f64>(){
                        sheet.write_number_with_format((i+1) as u32,j as u16,amount,&numbers)
                            .map_err(|e|e.to_string())?;
                        continue;
                    }
                }
                sheet.write_string((i+1) as u32,j as u16,value)
                    .map_err(|e|e.to_string())?;
            }
        }
        sheet.set_freeze_panes(1,0).map_err(|e|e.to_string())?;
        sheet.autofilter(0,0,0,19).map_err(|e|e.to_string())?;
    }
    {
        let sheet=book.add_worksheet();
        sheet.set_name("카테고리 및 브랜드 참고").map_err(|e|e.to_string())?;
        for (j,title) in ["구분","식별자","이름","상위 카테고리 ID"].iter().enumerate(){
            sheet.write_string(0,j as u16,*title).map_err(|e|e.to_string())?;
        }
        let mut row=1;
        for cat in list(state,"categories"){
            for (j,val) in ["카테고리",field(cat,"id"),field(cat,"name"),field(cat,"parentId")]
                .iter().enumerate(){
                sheet.write_string(row,j as u16,*val).map_err(|e|e.to_string())?;
            }
            row+=1;
        }
        for brand in list(state,"brands"){
            for (j,val) in ["브랜드",field(brand,"id"),field(brand,"name"),""]
                .iter().enumerate(){
                sheet.write_string(row,j as u16,*val).map_err(|e|e.to_string())?;
            }
            row+=1;
        }
    }
    {
        let sheet=book.add_worksheet();
        sheet.set_name("작성 안내").map_err(|e|e.to_string())?;
        sheet.set_column_width(0,27.).map_err(|e|e.to_string())?;
        sheet.set_column_width(1,118.).map_err(|e|e.to_string())?;
        for (i,(key,value)) in GUIDE.iter().enumerate(){
            sheet.write_string(i as u32,0,*key).map_err(|e|e.to_string())?;
            sheet.write_string(i as u32,1,*value).map_err(|e|e.to_string())?;
        }
    }
    {
        let sheet=book.add_worksheet();
        sheet.set_name("__반도허브_관리정보").map_err(|e|e.to_string())?;
        sheet.write_string(0,0,"종류").map_err(|e|e.to_string())?;
        sheet.write_string(0,1,FORMAT).map_err(|e|e.to_string())?;
        sheet.write_string(1,0,"DB버전").map_err(|e|e.to_string())?;
        sheet.write_number(1,1,state["revision"].as_u64().unwrap_or(0) as f64)
            .map_err(|e|e.to_string())?;
        sheet.set_very_hidden(true);
    }
    book.save_to_buffer().map_err(|e|e.to_string())
}

#[cfg(any(windows,test))]
fn cell_value(value:&Data)->Result<String,String>{
    let text=match value {
        Data::Empty=>String::new(),
        Data::String(s)=>s.trim().to_owned(),
        Data::Int(n)=>n.to_string(),
        Data::Float(n)=>{
            if !n.is_finite(){return Err("유효하지 않은 Excel 숫자입니다.".into())}
            n.to_string()
        }
        Data::Bool(b)=>b.to_string(),
        _=>return Err("문자 또는 숫자 셀을 사용하세요.".into()),
    };
    Ok(text)
}
#[cfg(any(windows,test))]
pub fn read(path:&Path)->Result<Value,String>{
    if !path.is_file() || path.extension().is_none_or(|ext|!ext.eq_ignore_ascii_case("xlsx")){
        return Err("20MB 이하의 .xlsx 상품 전체 목록을 선택하세요.".into());
    }
    let size=std::fs::metadata(path).map_err(|e|e.to_string())?.len();
    if size==0||size>MAX_BYTES{
        return Err("20MB 이하의 .xlsx 상품 전체 목록을 선택하세요.".into());
    }
    use std::io::Read;
    let mut bytes=Vec::new();
    std::fs::File::open(path).map_err(|e|e.to_string())?
        .take(MAX_BYTES+1).read_to_end(&mut bytes).map_err(|e|e.to_string())?;
    // Validate and parse exactly the same bounded byte snapshot.
    let mut book=crate::native_workbooks::open_book(&bytes,MAX_BYTES as usize)?;
    if !book.sheet_names().iter().any(|n|n=="상품 목록") ||
       !book.sheet_names().iter().any(|n|n=="__반도허브_관리정보"){
        return Err("상품 Excel 형식이 다릅니다. 통합 상품 목록을 새로 다운로드하여 사용하세요.".into());
    }
    crate::native_workbooks::guard_sheet(&mut book,"__반도허브_관리정보")?;
    crate::native_workbooks::guard_sheet(&mut book,"상품 목록")?;
    let meta=book.worksheet_range("__반도허브_관리정보")
        .map_err(|_|"상품 Excel 관리정보를 확인할 수 없습니다.".to_owned())?;
    let fmt=meta.get_value((0,1))
        .map(cell_value).transpose()?.unwrap_or_default();
    if fmt!=FORMAT&&fmt!=OLD_FORMAT{
        return Err("상품 Excel 형식이 다릅니다. 통합 상품 목록을 새로 다운로드하여 사용하세요.".into());
    }
    let revision_text=meta.get_value((1,1))
        .map(cell_value).transpose()?.unwrap_or_default();
    let revision=revision_text.parse::<u64>()
        .map_err(|_|"상품 Excel DB 버전이 잘못되었습니다.".to_owned())?;
    if revision>9_007_199_254_740_991{
        return Err("상품 Excel DB 버전이 잘못되었습니다.".into());
    }
    if !book.merge_cells_by_sheet_name("상품 목록")
        .map_err(|_|"상품 Excel 병합 셀 검사에 실패했습니다.".to_owned())?.is_empty(){
        return Err("병합된 셀이나 수식은 허용되지 않습니다.".into());
    }
    let formulas=book.worksheet_formula("상품 목록")
        .map_err(|_|"상품 Excel 수식 검사에 실패했습니다.".to_owned())?;
    if formulas.used_cells().any(|(_,_,value)|!value.trim().is_empty()){
        return Err("병합된 셀이나 수식은 허용되지 않습니다.".into());
    }
    let sheet=book.worksheet_range("상품 목록")
        .map_err(|_|"상품 Excel 상품 목록을 읽지 못했습니다.".to_owned())?;
    let mut headers=HEADERS.iter().map(|v|v.to_string()).collect::<Vec<_>>();
    if fmt==OLD_FORMAT{headers.insert(8,"신규 모델코드".to_owned());}
    for (index,header) in headers.iter().enumerate(){
        let found=sheet.get_value((0,index as u32))
            .map(cell_value).transpose()?.unwrap_or_default();
        if &found!=header{
            return Err(format!("상품 Excel 열 이름 또는 순서가 다릅니다: {header}"));
        }
    }
    let last=sheet.end().map(|(row,_)|row).unwrap_or(0);
    if last>MAX_ROWS{
        return Err("상품 Excel은 20,000행까지 지원합니다.".into());
    }
    let mut products=Vec::new();
    let mut options=Vec::new();
    for sheet_row in 1..=last{
        let mut values=Vec::with_capacity(headers.len());
        for col in 0..headers.len(){
            let value=sheet.get_value((sheet_row,col as u32)).unwrap_or(&Data::Empty);
            let identity=col==0||col==1||col==if fmt==FORMAT{16}else{17};
            if identity&&matches!(value,Data::Int(_)|Data::Float(_)){
                return Err(format!("{}행의 ID는 텍스트로 입력해야 합니다.",sheet_row+1));
            }
            values.push(cell_value(value)?);
        }
        if values.iter().all(|s|s.is_empty()){continue;}
        let v=if fmt==FORMAT{
            let mut copy=values.clone();
            copy.insert(8,String::new());
            copy
        }else{values};
        let a=vec![0,1,2,3,8,9,10,11,12,13,14,4,15,16]
            .into_iter().map(|i|v[i].clone()).collect::<Vec<_>>();
        let b=vec![0,1,17,4,18,5,6,7,19,20]
            .into_iter().map(|i|v[i].clone()).collect::<Vec<_>>();
        products.push(json!({"row":sheet_row+1,"cells":a}));
        options.push(json!({"row":sheet_row+1,"cells":b}));
    }
    Ok(json!({
        "revision":revision,
        "products":products,
        "options":options,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unified_v2_roundtrip_keeps_identity_and_prices() {
        let state=json!({"revision":7,"products":[{
            "id":"p7","name":"BD 709","subtitle":"보면대","catalog":"BANDOSTAND",
            "origin":"FACTORY","active":true,"defaultColor":"검정","variants":[{
                "id":"v7","color":"검정","option":"가방 미포함","source":"MANUAL",
                "wholesale":5000,"semi":6000,"retail":8000
            }]
        }],"categories":[],"brands":[]});
        let bytes=export(&state).unwrap();
        assert!(bytes.len()<=MAX_BYTES as usize);
        let filename=std::env::temp_dir().join(format!(
            "bando-product-excel-test-{}-{}.xlsx",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::write(&filename,bytes).unwrap();
        let parsed=read(&filename);
        let _=std::fs::remove_file(filename);
        let value=parsed.unwrap();
        assert_eq!(value["revision"],7);
        assert_eq!(value["products"][0]["cells"][0],"p7");
        assert_eq!(value["options"][0]["cells"][2],"v7");
        assert_eq!(value["options"][0]["cells"][5],"5000");
        // Check the actual exported XLSX -> parser -> native business preview,
        // not merely the visible IDs and numbers in the parsed cells.
        let review=bando_data_access::preview_unified_product_workbook(&state,&value).unwrap();
        assert_eq!(review["products"],state["products"]);
        assert_eq!(review["updatedProducts"],0);
        assert_eq!(review["updatedOptions"],0);
        assert_eq!(review["rows"][0]["status"],"UNCHANGED");
        let mut edited=value;
        edited["options"][0]["cells"][5]=json!("5,500");
        let changed=bando_data_access::preview_unified_product_workbook(&state,&edited).unwrap();
        assert_eq!(changed["updatedProducts"],1);
        assert_eq!(changed["products"][0]["variants"][0]["wholesale"],5500);
        assert_eq!(changed["products"][0]["defaultColor"],"검정");
        assert_eq!(changed["products"][0]["variants"][0]["color"],"검정");
        assert_eq!(changed["products"][0]["variants"][0]["retail"],8000);
    }
}
