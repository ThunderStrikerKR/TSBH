//! Authoritative v2 relational writer shared with the original Tauri host.
//! Taken from src-tauri/src/relational_store.rs. The caller provides a transaction.

use rusqlite::{params, Transaction};
use serde_json::Value;
use std::collections::HashSet;
fn err<E:std::fmt::Display>(e:E)->String{e.to_string()}

fn source_list<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>, String> {
    value.get(key).and_then(Value::as_array)
        .ok_or_else(|| format!("{key} 목록 데이터가 없습니다."))
}

fn required_id<'a>(entry: &'a Value, label: &str) -> Result<&'a str, String> {
    entry.get("id").and_then(Value::as_str).filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("{label} ID가 비어 있습니다."))
}

fn require_unique(seen: &mut HashSet<String>, id: &str, label: &str) -> Result<(), String> {
    if !seen.insert(id.to_string()) { return Err(format!("{label} ID 중복: {id}")); }
    Ok(())
}

fn payload(value: &Value, child: Option<&str>) -> Result<String, String> {
    let mut stripped = value.clone();
    if let Some(child) = child {
        stripped.as_object_mut()
            .ok_or("업무 데이터가 객체가 아닙니다.")?.remove(child);
    }
    serde_json::to_string(&stripped).map_err(err)
}

fn sync_prune(tx: &Transaction<'_>, table: &str, retain: &HashSet<String>) -> Result<usize, String> {
    // Only called with fixed compile-time table names; no user SQL identifiers.
    let key = if table=="core_product_prices" { "product_id" } else { "id" };
    let select = format!("SELECT {key} FROM {table}");
    let mut read = tx.prepare(&select).map_err(err)?;
    let old = read.query_map([], |r| r.get::<_, String>(0)).map_err(err)?
        .collect::<Result<Vec<_>, _>>().map_err(err)?;
    drop(read);
    let sql = format!("DELETE FROM {table} WHERE {key}=?1");
    let mut direct=0;
    for id in old { if !retain.contains(&id) { direct+=tx.execute(&sql, [id]).map_err(err)?; } }
    Ok(direct)
}

/// Full sync remains the bootstrap/migration authority: there may not be
/// an authoritative prior snapshot during schema initialization.
pub fn store_v2_relational(tx: &Transaction<'_>, state: &Value) -> Result<(), String> {
    store_relational(tx,state,None).map(|_|())
}

/// Compare the transaction's hydrated before-image, not a UI-provided draft.
/// Unchanged domains need no SQL. Stable lists need neither redundant upserts
/// nor prune scans. Structural changes use the proven full sync for that domain
/// (preserves ownership, positions, uniqueness and child-before-parent deletes).
pub(crate) fn store_v2_relational_delta(tx:&Transaction<'_>,before:&Value,state:&Value)
    ->Result<usize,String>{
    store_relational(tx,state,Some(before))
}
fn same_ids(old:&[Value],new:&[Value])->bool{
    old.len()==new.len() && old.iter().zip(new).all(|(a,b)|
        a["id"].as_str().is_some() && a["id"]==b["id"])
}
fn stable_products(old:&Value,new:&Value)->bool{
    let (Some(old),Some(new))=(old.as_array(),new.as_array())else{return false};
    same_ids(old,new) && old.iter().zip(new).all(|(a,b)|
        match (a["variants"].as_array(),b["variants"].as_array()){
            (Some(a),Some(b))=>same_ids(a,b),_=>false
        })
}
pub(crate) fn stable_orders(old:&Value,new:&Value)->bool{
    let (Some(old),Some(new))=(old.as_array(),new.as_array())else{return false};
    same_ids(old,new) && old.iter().zip(new).all(|(a,b)|
        match (a["recipients"].as_array(),b["recipients"].as_array()){
            (Some(a),Some(b))=>same_ids(a,b) && a.iter().zip(b).all(|(r,s)|
                match (r["items"].as_array(),s["items"].as_array()){
                    (Some(a),Some(b))=>same_ids(a,b),_=>false
                }),
            _=>false
        })
}
fn same_payload(old:&Value,new:&Value,child:&str)->bool{
    let (Some(old),Some(new))=(old.as_object(),new.as_object())else{return false};
    old.iter().filter(|(k,_)|k.as_str()!=child)
        .eq(new.iter().filter(|(k,_)|k.as_str()!=child))
}
fn store_relational(tx:&Transaction<'_>,state:&Value,before:Option<&Value>)->Result<usize,String>{
    crate::valid_state(state)?;
    let mut direct=0;
    let products_changed=before.is_none_or(|old|old["products"]!=state["products"]);
    let clients_changed=before.is_none_or(|old|old["clients"]!=state["clients"]);
    let orders_changed=before.is_none_or(|old|old["orders"]!=state["orders"]);
    let stable_product=before.filter(|old|stable_products(&old["products"],&state["products"]));
    let stable_client=before.filter(|old|match (old["clients"].as_array(),state["clients"].as_array()){
        (Some(a),Some(b))=>same_ids(a,b),_=>false
    });
    let stable_order=before.filter(|old|stable_orders(&old["orders"],&state["orders"]));
    let mut product_ids = HashSet::new();
    let mut price_ids = HashSet::new();
    let mut client_ids = HashSet::new();
    let mut order_ids = HashSet::new();
    let mut recipient_ids = HashSet::new();
    let mut item_ids = HashSet::new();

    if products_changed {
        let mut statement = tx.prepare(
            "INSERT INTO core_products(id,position,code,name,payload) VALUES (?1,?2,?3,?4,?5)
             ON CONFLICT(id) DO UPDATE SET position=excluded.position,code=excluded.code,
             name=excluded.name,payload=excluded.payload
             WHERE core_products.position<>excluded.position OR core_products.payload<>excluded.payload"
        ).map_err(err)?;
        for (i, p) in source_list(state, "products")?.iter().enumerate() {
            let id = required_id(p,"상품")?;
            require_unique(&mut product_ids,id,"상품")?;
            let code = p["code"].as_str().ok_or("상품 모델코드 누락")?;
            let name = p["name"].as_str().ok_or("상품명 누락")?;
            if stable_product.is_some_and(|old|
                same_payload(&old["products"][i],p,"variants")){continue;}
            direct+=statement.execute(params![id,i as i64,code,name,payload(p,Some("variants"))?]).map_err(err)?;
        }
    }
    if products_changed {
        let mut statement = tx.prepare(
            "INSERT INTO core_product_prices(product_id,price_record_id,color,wholesale,semi,retail,payload)
             VALUES (?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(product_id) DO UPDATE SET price_record_id=excluded.price_record_id,
             color=excluded.color,wholesale=excluded.wholesale,semi=excluded.semi,
             retail=excluded.retail,payload=excluded.payload
             WHERE core_product_prices.price_record_id<>excluded.price_record_id OR
             core_product_prices.payload<>excluded.payload"
        ).map_err(err)?;
        for (pi,p) in source_list(state,"products")?.iter().enumerate() {
            let owner=required_id(p,"상품")?;
            let prices=source_list(p,"variants")?;
            if prices.len()!=1 {
                return Err(format!("독립 상품 {owner}에 가격 기록이 {}개입니다. 각 판매 구성을 개별 상품으로 등록하세요.",prices.len()));
            }
            let v=&prices[0];
            let id=required_id(v,"기존 가격 식별자")?;
            require_unique(&mut price_ids,id,"가격 기록")?;
            if stable_product.is_some_and(|old|old["products"][pi]["variants"][0]==*v){
                continue;
            }
            direct+=statement.execute(params![owner,id,v["color"].as_str().unwrap_or(""),
                v["wholesale"].as_i64(),v["semi"].as_i64(),v["retail"].as_i64(),
                payload(v,None)?]).map_err(err)?;
        }
    }
    if clients_changed {
        let mut statement=tx.prepare(
            "INSERT INTO core_clients(id,position,name,tier,address,payload) VALUES (?1,?2,?3,?4,?5,?6)
             ON CONFLICT(id) DO UPDATE SET position=excluded.position,name=excluded.name,
             tier=excluded.tier,address=excluded.address,payload=excluded.payload
             WHERE core_clients.position<>excluded.position OR core_clients.payload<>excluded.payload"
        ).map_err(err)?;
        for (i,c) in source_list(state,"clients")?.iter().enumerate() {
            let id=required_id(c,"거래처")?;
            require_unique(&mut client_ids,id,"거래처")?;
            if stable_client.is_some_and(|old|old["clients"][i]==*c){continue;}
            direct+=statement.execute(params![id,i as i64,c["name"].as_str().unwrap_or(""),
                c["tier"].as_str().unwrap_or(""),c["address"].as_str().unwrap_or(""),payload(c,None)?]).map_err(err)?;
        }
    }
    if orders_changed {
        let mut statement=tx.prepare(
            "INSERT INTO core_orders(id,position,client_id,created_at,confirmed_at,status,payload)
             VALUES (?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(id) DO UPDATE SET position=excluded.position,client_id=excluded.client_id,
             created_at=excluded.created_at,confirmed_at=excluded.confirmed_at,
             status=excluded.status,payload=excluded.payload
             WHERE core_orders.position<>excluded.position OR core_orders.payload<>excluded.payload"
        ).map_err(err)?;
        for (i,o) in source_list(state,"orders")?.iter().enumerate() {
            let id=required_id(o,"주문")?;
            require_unique(&mut order_ids,id,"주문")?;
            if stable_order.is_some_and(|old|
                same_payload(&old["orders"][i],o,"recipients")){continue;}
            direct+=statement.execute(params![id,i as i64,o["clientId"].as_str(),
                o["createdAt"].as_str(),o["confirmedAt"].as_str(),
                o["status"].as_str(),payload(o,Some("recipients"))?]).map_err(err)?;
        }
    }
    if orders_changed {
        let mut statement=tx.prepare(
            "INSERT INTO core_recipients(id,order_id,position,name,payload) VALUES (?1,?2,?3,?4,?5)
             ON CONFLICT(id) DO UPDATE SET order_id=excluded.order_id,position=excluded.position,
             name=excluded.name,payload=excluded.payload
             WHERE core_recipients.order_id<>excluded.order_id OR
             core_recipients.position<>excluded.position OR core_recipients.payload<>excluded.payload"
        ).map_err(err)?;
        for (oi,o) in source_list(state,"orders")?.iter().enumerate() {
            let owner=required_id(o,"주문")?;
            for (i,r) in source_list(o,"recipients")?.iter().enumerate() {
                let id=required_id(r,"수취인")?;
                require_unique(&mut recipient_ids,id,"수취인")?;
                if stable_order.is_some_and(|old|
                    same_payload(&old["orders"][oi]["recipients"][i],r,"items")){
                    continue;
                }
                direct+=statement.execute(params![id,owner,i as i64,r["name"].as_str().unwrap_or(""),
                    payload(r,Some("items"))?]).map_err(err)?;
            }
        }
    }
    if orders_changed {
        let mut statement=tx.prepare(
            "INSERT INTO core_items(id,recipient_id,position,product_id,variant_id,payload)
             VALUES (?1,?2,?3,?4,?5,?6)
             ON CONFLICT(id) DO UPDATE SET recipient_id=excluded.recipient_id,
             position=excluded.position,product_id=excluded.product_id,
             variant_id=excluded.variant_id,payload=excluded.payload
             WHERE core_items.recipient_id<>excluded.recipient_id OR
             core_items.position<>excluded.position OR core_items.payload<>excluded.payload"
        ).map_err(err)?;
        for (oi,o) in source_list(state,"orders")?.iter().enumerate() {
            for (ri,r) in source_list(o,"recipients")?.iter().enumerate() {
                let owner=required_id(r,"수취인")?;
                for (i,item) in source_list(r,"items")?.iter().enumerate() {
                    let id=required_id(item,"주문 품목")?;
                    require_unique(&mut item_ids,id,"주문 품목")?;
                    if stable_order.is_some_and(|old|
                        old["orders"][oi]["recipients"][ri]["items"][i]==*item){continue;}
                    direct+=statement.execute(params![id,owner,i as i64,
                        item["productId"].as_str(),item["variantId"].as_str(),
                        payload(item,None)?]).map_err(err)?;
                }
            }
        }
    }
    // Remove children before parents so the FK check cannot observe orphans.
    if orders_changed && stable_order.is_none(){
        direct+=sync_prune(tx,"core_items",&item_ids)?;
        direct+=sync_prune(tx,"core_recipients",&recipient_ids)?;
        direct+=sync_prune(tx,"core_orders",&order_ids)?;
    }
    if products_changed && stable_product.is_none(){
        direct+=sync_prune(tx,"core_product_prices",&product_ids)?;
        direct+=sync_prune(tx,"core_products",&product_ids)?;
    }
    if clients_changed && stable_client.is_none(){
        direct+=sync_prune(tx,"core_clients",&client_ids)?;
    }
    Ok(direct)
}

