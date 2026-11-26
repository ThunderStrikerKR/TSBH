//! Factory reply notification inbox. Read state is independent of business revisions.
//! The existing hub_notifications table is reused; only factory reply events enter this path.
use rusqlite::{params,Connection};
use serde_json::{json,Value};
fn err(e:impl std::fmt::Display)->String{e.to_string()}
fn text<'a>(v:&'a Value,k:&str)->&'a str{v[k].as_str().unwrap_or("")}
fn rows(v:&Value)->&[Value]{v.as_array().map(Vec::as_slice).unwrap_or(&[])}
fn initialize(conn:&Connection)->Result<(),String>{
    conn.execute_batch("CREATE TABLE IF NOT EXISTS hub_notifications(
        id TEXT PRIMARY KEY NOT NULL, at TEXT NOT NULL,
        read INTEGER NOT NULL DEFAULT 0 CHECK(read IN (0,1)), payload TEXT NOT NULL);").map_err(err)
}
fn insert(conn:&Connection,notice:&Value)->Result<(),String>{
    initialize(conn)?;
    conn.execute("INSERT OR IGNORE INTO hub_notifications(id,at,read,payload) VALUES (?1,?2,0,?3)",
        params![text(notice,"id"),text(notice,"at"),notice.to_string()]).map_err(err)?;
    Ok(())
}
pub(crate) fn record_factory_reply(conn:&Connection,state:&Value)->Result<(),String>{
    let Some(record)=rows(&state["factoryReplies"]).last() else{return Ok(())};
    for row in rows(&record["rows"]){
        let applied=row["applied"]==true;
        let changes=rows(&row["notificationChanges"]);
        if applied&&changes.is_empty(){continue;}
        let issue=text(row,"issue").replace("직원 확인 필요","확인 필요");
        let id=format!("{}:{}",text(record,"id"),row["row"]);
        let title=if applied{changes.iter().map(|c|text(c,"field")).collect::<Vec<_>>().join(" / ")}
            else if issue.is_empty(){"공장 회신 반영 보류".into()}else{issue.clone()};
        let mut detail=vec![format!("수하인: {}",text(row,"name")),
            format!("{} / {}행",text(record,"filename"),row["row"]),
            format!("운송장번호: {}",text(row,"tracking")),
            if applied{"자동 반영 완료".into()}else{"반영 보류".into()}];
        for c in changes{detail.push(format!("{}: {} → {}",text(c,"field"),text(c,"before"),text(c,"after")));}
        if !issue.is_empty(){detail.push(issue);}
        insert(conn,&json!({"id":id,"at":record["at"],"source":"공장 회신",
            "tone":if applied{"info"}else{"error"},"title":title,"detail":detail.join("\n"),
            "filename":record["filename"],"orderId":row["orderId"],"shipmentId":row["shipmentId"],
            "replyId":record["id"],"row":row["row"],"changes":changes,"applied":applied}))?;
    }
    Ok(())
}
pub fn record_file_error(conn:&Connection,id:&str,filename:&str,at:&str,detail:&str)->Result<(),String>{
    insert(conn,&json!({"id":format!("{id}:error"),"at":at,"source":"공장 회신",
        "tone":"error","title":"공장 회신 파일 처리 실패","detail":format!("{filename}\n{detail}"),"filename":filename}))
}
pub fn load(conn:&Connection)->Result<Vec<Value>,String>{
    initialize(conn)?;
    let mut stmt=conn.prepare("SELECT payload,read FROM hub_notifications
        WHERE json_extract(payload,'$.source')='공장 회신'
        AND COALESCE(json_extract(payload,'$.dismissed'),0)=0 ORDER BY rowid DESC").map_err(err)?;
    let items=stmt.query_map([],|row|Ok((row.get::<_,String>(0)?,row.get::<_,i64>(1)?))).map_err(err)?;
    let mut result=Vec::new();
    for item in items{let (raw,read)=item.map_err(err)?;
        let mut v:Value=serde_json::from_str(&raw).map_err(err)?;
        v["read"]=json!(read!=0);result.push(v);}
    Ok(result)
}
pub fn mark_read(conn:&Connection,id:Option<&str>)->Result<(),String>{
    initialize(conn)?;
    conn.execute("UPDATE hub_notifications SET read=1 WHERE json_extract(payload,'$.source')='공장 회신'
        AND (?1 IS NULL OR id=?1)",params![id]).map_err(err)?;
    Ok(())
}
pub fn dismiss(conn:&Connection,id:Option<&str>)->Result<(),String>{
    initialize(conn)?;
    // Keep the identity to suppress repeated delivery of the same event.
    conn.execute("UPDATE hub_notifications SET read=1,payload=json_set(payload,'$.dismissed',1)
        WHERE json_extract(payload,'$.source')='공장 회신' AND (?1 IS NULL OR id=?1)",params![id]).map_err(err)?;
    Ok(())
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn normal_replies_are_silent_and_changes_and_holds_are_persistent_and_deduplicated(){
        let conn=Connection::open_in_memory().unwrap();
        let state=json!({"factoryReplies":[{"id":"reply","at":"2026-11-27","filename":"reply.xlsx","rows":[
            {"row":2,"applied":true},{"row":3,"applied":true,"notificationChanges":[{"field":"실제 운임 변경","before":"3000원","after":"4000원"}]},
            {"row":4,"issue":"일치하는 공장 발송 배송 건이 없습니다."},{"row":5,"issue":"운송장번호가 다른 배송 건에 이미 연결되어 있습니다."}]}]});
        record_factory_reply(&conn,&state).unwrap();record_factory_reply(&conn,&state).unwrap();
        assert_eq!(load(&conn).unwrap().len(),3);
        mark_read(&conn,Some("reply:3")).unwrap();
        assert!(load(&conn).unwrap().iter().any(|n|n["id"]=="reply:3"&&n["read"]==true));
        dismiss(&conn,Some("reply:3")).unwrap();
        record_factory_reply(&conn,&state).unwrap();
        assert_eq!(load(&conn).unwrap().len(),2);
    }
}
