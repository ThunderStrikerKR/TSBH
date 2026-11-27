//! Read-only field-level diff for complete authoritative snapshots.
//! This reports facts, not validation policy. No values are copied or logged.
//! A partial/scoped snapshot must NOT be passed as a complete snapshot.
use serde_json::Value;
use std::collections::{BTreeMap,BTreeSet};

/// Typed segments avoid ambiguity between object keys and record IDs.
#[derive(Clone,Debug,PartialEq,Eq,PartialOrd,Ord)]
pub enum Segment {
    Field(String),
    RecordId(String),
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum ChangeKind {
    Added,
    Removed,
    Modified,
    Reordered,
    /// Array cannot safely be matched by unique record IDs. Inspect the
    /// entire collection, including its order and every element.
    OpaqueArray,
}
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct FieldChange {
    pub path:Vec<Segment>,
    pub kind:ChangeKind,
}
#[derive(Clone,Debug,PartialEq,Eq)]
pub enum ChangeSet {
    Precise(Vec<FieldChange>),
    /// No precise coverage claim. The planner must not skip validators.
    Full(Fallback),
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Fallback {
    InvalidRoot,
    DepthLimit,
    ChangeLimit,
}
const MAX_DEPTH:usize=64;
const MAX_CHANGES:usize=20_000;

fn id_map(rows:&[Value])->Option<BTreeMap<&str,&Value>>{
    let mut map=BTreeMap::new();
    for row in rows{
        let id=row.as_object()?.get("id")?.as_str().filter(|id|!id.trim().is_empty())?;
        if map.insert(id,row).is_some(){return None;}
    }
    Some(map)
}

struct Diff {
    path:Vec<Segment>,
    changes:Vec<FieldChange>,
    max_changes:usize,
}
impl Diff {
    fn record(&mut self,kind:ChangeKind)->Result<(),Fallback>{
        if self.changes.len()>=self.max_changes{return Err(Fallback::ChangeLimit);}
        self.changes.push(FieldChange{path:self.path.clone(),kind});
        Ok(())
    }
    fn at(&mut self,segment:Segment,before:Option<&Value>,after:Option<&Value>)->Result<(),Fallback>{
        self.path.push(segment);
        let result=self.compare(before,after);
        self.path.pop();
        result
    }
    fn compare(&mut self,before:Option<&Value>,after:Option<&Value>)->Result<(),Fallback>{
        if before==after{return Ok(());}
        if self.path.len()>MAX_DEPTH{return Err(Fallback::DepthLimit);}
        let (Some(before),Some(after))=(before,after) else{
            return self.record(if before.is_none(){ChangeKind::Added}else{ChangeKind::Removed});
        };
        match (before,after){
            (Value::Object(a),Value::Object(b))=>{
                let keys:BTreeSet<_>=a.keys().chain(b.keys()).collect();
                for key in keys{
                    self.at(Segment::Field(key.clone()),a.get(key),b.get(key))?;
                }
                Ok(())
            }
            (Value::Array(a),Value::Array(b))=>{
                let (Some(old),Some(new))=(id_map(a),id_map(b)) else{
                    return self.record(ChangeKind::OpaqueArray);
                };
                // The empty array is unambiguous only if the other side
                // consists entirely of unique identified objects.
                // Detect relative order changes among surviving records,
                // independently of inserts/removals.
                let old_order=a.iter().filter_map(|r|r["id"].as_str())
                    .filter(|id|new.contains_key(*id));
                let new_order=b.iter().filter_map(|r|r["id"].as_str())
                    .filter(|id|old.contains_key(*id));
                if !old_order.eq(new_order){self.record(ChangeKind::Reordered)?;}
                let ids:BTreeSet<_>=old.keys().chain(new.keys()).copied().collect();
                for id in ids{
                    self.at(Segment::RecordId(id.to_owned()),old.get(id).copied(),new.get(id).copied())?;
                }
                Ok(())
            }
            _=>self.record(ChangeKind::Modified),
        }
    }
}

/// Compare full before/after snapshots borrowed from their owner.
/// A no-op is Precise(empty). Added/removed containers cover all descendants.
/// Unknown keys are never ignored, and missing is not equivalent to null.
/// Limits cause a full-validation marker, never a silently truncated diff.
pub fn analyze(before:&Value,after:&Value)->ChangeSet{
    analyze_with_limit(before,after,MAX_CHANGES)
}
fn analyze_with_limit(before:&Value,after:&Value,max_changes:usize)->ChangeSet{
    if !before.is_object()||!after.is_object(){return ChangeSet::Full(Fallback::InvalidRoot);}
    let mut diff=Diff{path:Vec::new(),changes:Vec::new(),max_changes};
    match diff.compare(Some(before),Some(after)){
        Ok(())=>ChangeSet::Precise(diff.changes),
        Err(reason)=>ChangeSet::Full(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn field(s:&str)->Segment{Segment::Field(s.into())}
    fn id(s:&str)->Segment{Segment::RecordId(s.into())}
    fn changed(path:Vec<Segment>,kind:ChangeKind)->FieldChange{FieldChange{path,kind}}
    fn precise(before:&Value,after:&Value)->Vec<FieldChange>{
        match analyze(before,after){ChangeSet::Precise(v)=>v,other=>panic!("unexpected {other:?}")}
    }
    #[test]
    fn identical_snapshot_has_no_changes(){
        let s=json!({"revision":3,"products":[{"id":"p","name":"상품"}]});
        assert!(precise(&s,&s).is_empty());
    }
    #[test]
    fn only_changed_price_and_metadata_paths_are_reported(){
        let before=json!({"revision":3,"products":[{"id":"p","name":"상품","variants":[{"id":"v","retail":1000,"wholesale":700}]}],"orders":[{"id":"o"}]});
        let mut after=before.clone();
        after["products"][0]["variants"][0]["retail"]=json!(1200);
        after["revision"]=json!(4);
        assert_eq!(precise(&before,&after),vec![
            changed(vec![field("products"),id("p"),field("variants"),id("v"),field("retail")],ChangeKind::Modified),
            changed(vec![field("revision")],ChangeKind::Modified)
        ]);
    }
    #[test]
    fn inserts_removals_and_relative_reorders_are_distinct(){
        let before=json!({"products":[{"id":"a"},{"id":"b"},{"id":"c"}]});
        let after=json!({"products":[{"id":"c"},{"id":"a"},{"id":"d"}]});
        assert_eq!(precise(&before,&after),vec![
            changed(vec![field("products")],ChangeKind::Reordered),
            changed(vec![field("products"),id("b")],ChangeKind::Removed),
            changed(vec![field("products"),id("d")],ChangeKind::Added),
        ]);
        let after=json!({"products":[{"id":"x"},{"id":"a"},{"id":"b"},{"id":"c"}]});
        assert_eq!(precise(&before,&after),vec![changed(vec![field("products"),id("x")],ChangeKind::Added)]);
    }
    #[test]
    fn nested_order_recipient_and_item_ids_are_preserved(){
        let before=json!({"orders":[{"id":"o","recipients":[{"id":"r","items":[{"id":"i","quantity":1}]}]}]});
        let mut after=before.clone();
        after["orders"][0]["recipients"][0]["items"][0]["quantity"]=json!(2);
        assert_eq!(precise(&before,&after),vec![changed(vec![
            field("orders"),id("o"),field("recipients"),id("r"),field("items"),id("i"),field("quantity")
        ],ChangeKind::Modified)]);
    }
    #[test]
    fn opaque_arrays_cover_duplicates_unidentified_rows_and_scalar_memberships(){
        for (a,b) in [
            (json!([{"id":"a"},{"id":"a"}]),json!([{"id":"a"}])),
            (json!([{"shipmentId":"s","amount":1}]),json!([{"shipmentId":"s","amount":2}])),
            (json!(["a","b"]),json!(["b","a"])),
            (json!([{"id":"a"}]),json!([{"id":null}])),
            (json!([]),json!([1])),
        ]{
            assert_eq!(precise(&json!({"rows":a}),&json!({"rows":b})),
                vec![changed(vec![field("rows")],ChangeKind::OpaqueArray)]);
        }
    }
    #[test]
    fn unknown_fields_and_missing_null_are_not_discarded(){
        let before=json!({"settings":{"x/y":null},"future":1});
        let after=json!({"settings":{"~id":null},"future":2});
        assert_eq!(precise(&before,&after),vec![
            changed(vec![field("future")],ChangeKind::Modified),
            changed(vec![field("settings"),field("x/y")],ChangeKind::Removed),
            changed(vec![field("settings"),field("~id")],ChangeKind::Added),
        ]);
    }
    #[test]
    fn malformed_root_and_budget_never_return_partial_coverage(){
        assert_eq!(analyze(&json!([]),&json!({})),ChangeSet::Full(Fallback::InvalidRoot));
        assert_eq!(analyze_with_limit(&json!({"a":1,"b":1}),&json!({"a":2,"b":2}),1),ChangeSet::Full(Fallback::ChangeLimit));
        let (mut a,mut b)=(json!(1),json!(2));
        for _ in 0..=MAX_DEPTH{a=json!({"x":a});b=json!({"x":b});}
        assert_eq!(analyze(&a,&b),ChangeSet::Full(Fallback::DepthLimit));
    }
    #[test]
    fn keyed_settings_maps_and_workflow_ids_use_same_contract(){
        let before=json!({"settings":{"colorGuidanceGroups":{"g":{"representativeProductId":"p"}}},"workflow":{"payments":[{"id":"pay","amount":1}]}});
        let mut after=before.clone();
        after["settings"]["colorGuidanceGroups"]["g"]["representativeProductId"]=json!("q");
        after["workflow"]["payments"][0]["amount"]=json!(2);
        assert_eq!(precise(&before,&after),vec![
            changed(vec![field("settings"),field("colorGuidanceGroups"),field("g"),field("representativeProductId")],ChangeKind::Modified),
            changed(vec![field("workflow"),field("payments"),id("pay"),field("amount")],ChangeKind::Modified)
        ]);
    }
}
