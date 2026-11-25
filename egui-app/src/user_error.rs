//! User-facing presentation boundary for legacy String errors.
//! Never use translated text for business decisions, persistence or retry logic.
use std::{borrow::Cow, collections::{VecDeque,hash_map::DefaultHasher}, hash::{Hash,Hasher}, sync::{Mutex,OnceLock}};
const RECENT_LIMIT:usize=64;
static RECENT:OnceLock<Mutex<VecDeque<u64>>>=OnceLock::new();

fn diagnostic(context:&str,raw:&str){
    // Use the existing local diagnostic stream. No new files or network
    // transmission. Deduplicate persistent errors rendered every frame.
    let mut hasher=DefaultHasher::new();
    context.hash(&mut hasher);
    raw.hash(&mut hasher);
    let key=hasher.finish();
    let Ok(mut recent)=RECENT.get_or_init(||Mutex::new(VecDeque::new())).lock() else{return};
    if recent.contains(&key){return;}
    if recent.len()>=RECENT_LIMIT{recent.pop_front();}
    recent.push_back(key);
    drop(recent);
    eprintln!("[BANDO HUB] UI diagnostic context={context:?} error={raw:?}");
}
fn technical(raw:&str)->bool{
    let lower=raw.to_ascii_lowercase();
    if lower.split(|ch:char|!ch.is_ascii_alphanumeric()&&ch!='_').any(|word|matches!(word,
        "rust"|"sqlite"|"sqlite3"|"egui"|"tauri"|"react"|"typescript"|
        "revision"|"json"|"schema"|"transaction"|"rollback"|"rusqlite"|
        "serde"|"serde_json"|"panic"|"backtrace"|"stacktrace"|"sql"|
        "db"|"api"|"http"|"https"|"gateway"|"unwrap"|"errno"|"oserror"|
        "appdata"|"debug"|"release"|"localhost"|"cas"|"sha256")){
        return true;
    }
    if ["리비전","스냅샷","트랜잭션","마이그레이션","스키마","직렬화","역직렬화",
        "게이트웨이","롤백","개발자 모드","개발용 db","내부 id","배정 id","파일 시스템 오류",
        "database is locked","database is busy","sqlite_busy","sqlite_locked","database disk image","constraint failed","no such table",
        "no such column","permission denied","access is denied","os error",
        "thread '","failed to","connection refused","unexpected eof","syntax error",
        "src/","src\\",".rs:",".sqlite","\\users\\","/home/"]
        .iter().any(|term|lower.contains(term)){
        return true;
    }
    let trimmed=raw.trim();
    trimmed.starts_with('{')||trimmed.starts_with("[{")||
        lower.as_bytes().windows(3).any(|w|w[0].is_ascii_alphabetic()&&w[1]==b':'&&matches!(w[2],b'\\'|b'/'))||
        (!trimmed.is_empty()&&!trimmed.chars().any(|c|matches!(c,'\u{ac00}'..='\u{d7a3}'|'\u{1100}'..='\u{11ff}'|'\u{3130}'..='\u{318f}')))
}
fn fallback(context:&str)->&'static str{
    if context.starts_with("reception.review"){"확인할 내용을 불러오지 못했습니다. 다시 시도해 주세요."}
    else if context.starts_with("reception.remote"){"도서/산간 여부를 확인하지 못했습니다. 주소를 확인해 주세요."}
    else if context.starts_with("reception.manual"){"수동 분류를 처리하지 못했습니다. 입력 내용을 확인하고 다시 시도해 주세요."}
    else if context.starts_with("reception.item")||context.starts_with("reception.product"){"상품 정보를 처리하지 못했습니다. 다시 시도해 주세요."}
    else if context.starts_with("reception"){"발주서를 처리하지 못했습니다. 다시 시도해 주세요."}
    else if context.starts_with("excel.reply"){"공장 회신을 처리하지 못했습니다. 회신 파일을 확인해 주세요."}
    else if context.starts_with("excel"){"엑셀 작업을 완료하지 못했습니다. 다시 시도해 주세요."}
    else if context.starts_with("product"){"상품 정보를 처리하지 못했습니다. 다시 시도해 주세요."}
    else if context.starts_with("client"){"거래처 정보를 처리하지 못했습니다. 다시 시도해 주세요."}
    else if context.starts_with("freight"){"운임 정보를 처리하지 못했습니다. 다시 시도해 주세요."}
    else if context.starts_with("history"){"주문내역을 처리하지 못했습니다. 다시 시도해 주세요."}
    else if context.starts_with("settlement"){"결산 정보를 불러오지 못했습니다. 다시 시도해 주세요."}
    else if context.starts_with("settings"){"설정 또는 데이터 관리 작업을 완료하지 못했습니다. 다시 시도해 주세요."}
    else{"작업을 완료하지 못했습니다. 다시 시도해 주세요."}
}
fn translated(context:&str,raw:&str)->&'static str{
    let lower=raw.to_ascii_lowercase();
    if ["revision","리비전","cas"].iter().any(|term|lower.contains(term))&&
       ["충돌","변경","불일치","stale","conflict","mismatch"].iter().any(|term|lower.contains(term)){
        "다른 작업에서 정보가 변경되었습니다. 최신 정보를 불러온 뒤 다시 시도해 주세요."
    }else if ["database is locked","database is busy","sqlite_busy","sqlite_locked"].iter().any(|term|lower.contains(term)){
        "다른 작업에서 데이터를 사용 중입니다. 잠시 후 다시 시도해 주세요."
    }else if ["disk full","database or disk is full","no space left","디스크 공간 부족"].iter().any(|term|lower.contains(term)){
        "저장 공간이 부족합니다. 공간을 확보한 뒤 다시 시도해 주세요."
    }else if ["permission denied","access is denied","read-only","readonly","읽기 전용","권한"].iter().any(|term|lower.contains(term)){
        "현재 작업을 수행할 권한이 없습니다. 저장 위치와 사용 권한을 확인해 주세요."
    }else if ["database disk image is malformed","file is not a database","데이터베이스 손상","db 손상"].iter().any(|term|lower.contains(term)){
        "저장된 데이터를 읽을 수 없습니다. 기존 데이터는 그대로 두고 백업 및 복구 상태를 확인해 주세요."
    }else{fallback(context)}
}
pub fn display<'a>(context:&str,raw:&'a str)->Cow<'a,str>{
    if !technical(raw)&&!raw.lines().any(technical){return Cow::Borrowed(raw);}
    diagnostic(context,raw);
    let lower=raw.to_ascii_lowercase();
    // Do not echo continuation lines from raw structured payloads or traces.
    if raw.trim_start().starts_with('{')||raw.trim_start().starts_with("[{")||
        lower.contains("stacktrace")||lower.contains("backtrace")||lower.contains("thread '"){
        return Cow::Borrowed(translated(context,raw));
    }
    let mut lines:Vec<&str>=Vec::new();
    for line in raw.lines().filter(|l|!l.trim().is_empty()){
        let visible=if technical(line){translated(context,line)}else{line};
        if !lines.contains(&visible){lines.push(visible);}
    }
    if lines.is_empty(){Cow::Borrowed(translated(context,raw))}
    else{Cow::Owned(lines.join("\n"))}
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn business_guidance_survives_unchanged(){
        for raw in ["배송 운임을 확인하세요","수취인 전화번호를 입력하세요.","입금액이 부족합니다.",
            "운임구분은 010(선불) 또는 020(착불)이어야 합니다.",
            "대리발송 송하인의 상호, 주소, 연락처가 누락되었습니다.",
            "부가세율을 확인하세요.\n상품 수량을 확인하세요."]{
            assert_eq!(display("reception.workflow",raw).as_ref(),raw);
        }
    }
    #[test]
    fn technical_failures_use_context_and_known_recovery(){
        assert_eq!(display("reception.workflow","Rust 주문 변경 결과의 JSON 형식 오류").as_ref(),
            "발주서를 처리하지 못했습니다. 다시 시도해 주세요.");
        assert_eq!(display("excel.reply","SQLite: database is locked").as_ref(),
            "다른 작업에서 데이터를 사용 중입니다. 잠시 후 다시 시도해 주세요.");
        assert!(display("settings","DB revision 충돌").contains("최신 정보"));
        assert!(display("products","permission denied").contains("권한"));
        assert!(display("settlement.query","unexpected EOF").contains("결산"));
    }
    #[test]
    fn mixed_lists_keep_business_blockers_without_technical_details(){
        let raw="배송 운임을 확인하세요.\nSQLite: no such table: core_orders\n상품 수량을 확인하세요.";
        let result=display("reception.workflow",raw);
        assert!(result.contains("배송 운임을 확인하세요."));
        assert!(result.contains("상품 수량을 확인하세요."));
        assert!(!result.contains("SQLite"));
        assert!(!result.contains("core_orders"));
        assert_eq!(display("status",result.as_ref()).as_ref(),result.as_ref());
        assert!(!display("status","배송 운임을 확인하세요.\nSomething went wrong").contains("Something"));
    }
    #[test]
    fn raw_debug_and_path_details_are_not_echoed(){
        for raw in ["{\"secret\":\"내부 자료\"}","panic: backtrace\n내부 자료","파일 접근 실패 C:\\Users\\name\\data.sqlite",
            "called Result::unwrap() on an Err value"]{
            assert_eq!(display("status",raw).as_ref(),"작업을 완료하지 못했습니다. 다시 시도해 주세요.");
        }
    }
}
