//! Shared category assignment invariant. Legacy ancestor IDs describe a
//! path, not a second placement. An empty assignment remains permitted.
use serde_json::Value;
use std::collections::{HashMap,HashSet,BTreeSet};

fn rows<'a>(state:&'a Value,key:&str)->&'a [Value]{
    state[key].as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn field<'a>(value:&'a Value,key:&str)->&'a str{
    value[key].as_str().unwrap_or("")
}
fn archived(p:&Value)->bool{
    !field(p,"deletedAt").is_empty()||!field(p,"supersededBy").is_empty()
}
struct CategoryGraph<'a>{
    by_id:HashMap<&'a str,&'a Value>,
    parents:HashSet<&'a str>,
    duplicates:HashSet<&'a str>,
}
impl<'a> CategoryGraph<'a>{
    fn new(state:&'a Value)->Self{
        let mut graph=Self{by_id:HashMap::new(),parents:HashSet::new(),duplicates:HashSet::new()};
        for c in rows(state,"categories"){
            let id=field(c,"id");
            if graph.by_id.insert(id,c).is_some(){graph.duplicates.insert(id);}
            let parent=field(c,"parentId");
            if !parent.is_empty(){graph.parents.insert(parent);}
        }
        graph
    }
    fn path(&self,id:&str)->Result<Vec<String>,String>{
        let mut current=id;
        let mut result=Vec::new();
        let mut seen=HashSet::new();
        while !current.is_empty(){
            if !seen.insert(current.to_owned())||self.duplicates.contains(current){
                return Err(format!("순환 또는 중복 분류 ID: {current}"));
            }
            result.push(current.to_owned());
            if let Some(c)=self.by_id.get(current){
                if c["active"]==false||!field(c,"deletedAt").is_empty(){
                    return Err(format!("비활성 분류: {current}"));
                }
                current=field(c,"parentId");
            }else if matches!(current,"catalog-root:BANDOSTAND"|"catalog-root:BANDOMALL"){
                break;
            }else{
                return Err(format!("존재하지 않는 분류: {current}"));
            }
        }
        Ok(result)
    }
    fn issues(&self,product:&Value)->BTreeSet<String>{
        let mut issues=BTreeSet::new();
        if archived(product){return issues;}
        let ids=match product.get("categories"){
            None|Some(Value::Null)=>return issues,
            Some(Value::Array(ids))=>ids,
            _=>{issues.insert("카테고리 목록 형식 오류".into());return issues;}
        };
        let mut assignments=HashSet::new();
        let mut ancestors=HashSet::new();
        for raw in ids{
            let Some(id)=raw.as_str().filter(|v|!v.trim().is_empty())else{
                issues.insert("카테고리 ID 형식 오류".into());continue;
            };
            assignments.insert(id);
            match self.path(id){
                Ok(path)=>ancestors.extend(path.into_iter().skip(1)),
                Err(error)=>{issues.insert(error);}
            }
        }
        for id in assignments{
            if !ancestors.contains(id)&&self.parents.contains(id){
                issues.insert(format!("최하위가 아닌 분류: {id}"));
            }
        }
        issues
    }
}
fn reject(product:&Value,issues:&BTreeSet<String>)->Result<(),String>{
    if issues.is_empty(){return Ok(());}
    Err(format!("상품 '{}'의 카테고리를 확인하세요. 활성 최하위 분류에만 배정할 수 있습니다. ({})",
        field(product,"name"),issues.iter().cloned().collect::<Vec<_>>().join(", ")))
}

/// Editor/registration/preview checks use the same rules as persistence.
/// Existing unchanged assignments may be edited in unrelated fields.
pub fn validate_product_assignment(state:&Value,product:&Value)->Result<(),String>{
    let original=rows(state,"products").iter().find(|p|field(p,"id")==field(product,"id"));
    if original.is_some_and(|p|p.get("categories")==product.get("categories")&&archived(p)==archived(product)){
        return Ok(());
    }
    reject(product,&CategoryGraph::new(state).issues(product))
}

/// Authoritative pre-commit guard. New/changed assignments must be valid;
/// a topology change cannot introduce a violation for an unchanged product.
/// Existing legacy defects do not block unrelated orders or price edits.
pub fn validate_category_transition(before:&Value,next:&Value)->Result<(),String>{
    let topology_changed=before.get("categories")!=next.get("categories");
    if !topology_changed&&before.get("products")==next.get("products"){return Ok(());}
    let old_graph=CategoryGraph::new(before);
    let new_graph=CategoryGraph::new(next);
    let originals:HashMap<_,_>=rows(before,"products").iter().map(|p|(field(p,"id"),p)).collect();
    for p in rows(next,"products"){
        if archived(p){continue;}
        let original=originals.get(field(p,"id")).copied();
        let changed=original.is_none_or(|old|old.get("categories")!=p.get("categories")||archived(old));
        if changed{
            reject(p,&new_graph.issues(p))?;
        }else if topology_changed{
            let old=old_graph.issues(original.unwrap());
            let new=new_graph.issues(p);
            let added=new.difference(&old).cloned().collect::<BTreeSet<_>>();
            reject(p,&added)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    fn fixture()->Value{
        json!({"categories":[
            {"id":"parent","parentId":"catalog-root:BANDOSTAND"},
            {"id":"leaf","parentId":"parent"},
            {"id":"sibling","parentId":"parent"},
            {"id":"other","parentId":"catalog-root:BANDOSTAND"},
            {"id":"inactive","active":false},
            {"id":"hidden","parentId":"inactive"}
        ],"products":[{"id":"p","name":"상품","categories":["parent","leaf","other"],"price":1}]})
    }
    #[test]
    fn legacy_path_and_multiple_leaves_are_valid_but_direct_parent_is_not(){
        let s=fixture();
        for ids in [json!(["parent","leaf","other"]),json!(["leaf","sibling"]),json!([])]{
            assert!(validate_product_assignment(&s,&json!({"id":"new","categories":ids})).is_ok());
        }
        for ids in [json!(["parent"]),json!(["parent","other"]),json!(["missing"]),
            json!(["hidden"]),json!(["catalog-root:BANDOSTAND"]),json!([123])]{
            assert!(validate_product_assignment(&s,&json!({"id":"new","categories":ids})).is_err());
        }
    }
    #[test]
    fn topology_cannot_create_nonleaf_assignment_without_transfer(){
        let s=fixture();
        let mut next=s.clone();
        next["categories"].as_array_mut().unwrap().push(json!({"id":"new","parentId":"other"}));
        assert!(validate_category_transition(&s,&next).is_err());
        next["products"][0]["categories"]=json!(["parent","leaf","new"]);
        assert!(validate_category_transition(&s,&next).is_ok());
    }
    #[test]
    fn new_invalid_assignment_and_removed_category_are_rejected_but_price_change_is_allowed(){
        let s=fixture();
        let mut next=s.clone();
        next["products"].as_array_mut().unwrap().push(json!({"id":"bad","categories":["parent"]}));
        assert!(validate_category_transition(&s,&next).is_err());
        let mut next=s.clone();
        next["categories"].as_array_mut().unwrap().retain(|c|c["id"]!="leaf");
        assert!(validate_category_transition(&s,&next).is_err());
        let mut old=s.clone();
        old["products"][0]["categories"]=json!(["parent"]);
        let mut next=old.clone();
        next["products"][0]["price"]=json!(2);
        assert!(validate_category_transition(&old,&next).is_ok());
        next["products"][0]["categories"]=json!(["leaf"]);
        assert!(validate_category_transition(&old,&next).is_ok());
    }
}
