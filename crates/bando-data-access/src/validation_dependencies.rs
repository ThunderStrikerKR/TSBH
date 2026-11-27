//! Proposed validation dependencies for complete authoritative snapshots.
//! Rule IDs are requirements, NOT evidence that an executable validator
//! exists. The next stage must explicitly bind each requirement or reject
//! an unsupported plan. No save path uses this registry yet.
use crate::change_set::{ChangeSet,FieldChange,Segment};
use std::collections::BTreeSet;

#[derive(Clone,Copy,Debug,PartialEq,Eq,PartialOrd,Ord)]
pub enum Validator {
    Shape, Product, Price, PriceHistory, Category, CategoryTopology,
    Brand, BrandLink, Alias, ModelGroup, Client, Account, Shipping,
    ShippingHistory, Reception, Workflow, FactoryReply, Message,
    ProvisionalNotice, Registration, Mapping,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum FullReason { AnalysisFallback, UnregisteredPath }
#[derive(Clone,Debug,PartialEq,Eq)]
pub enum Dependencies {
    Required(BTreeSet<Validator>),
    Full(FullReason),
}
use Validator::*;
use Segment::{Field,RecordId};

// Broad changes (insert/delete, replacement, unknown array identity) cover
// the entire registered subtree. No historical order repricing is implied.
const PRODUCT_ALL:&[Validator]=&[Product,Price,PriceHistory,Category,BrandLink,Alias,ModelGroup,Shipping,ShippingHistory,Reception,Workflow,ProvisionalNotice,Registration,Mapping];
const PRODUCT_IDENTITY:&[Validator]=&[Product,Alias,ModelGroup,Shipping,Reception,Workflow,Registration,Mapping];
const VARIANT_ALL:&[Validator]=&[Product,Price,PriceHistory,Alias,ModelGroup,Shipping,Reception,Workflow,Registration,Mapping];
const PRICE_ONLY:&[Validator]=&[Product,Price,PriceHistory];
const CATEGORY_RULES:&[Validator]=&[Category,CategoryTopology,Product,Shipping];
const CLIENT_RULES:&[Validator]=&[Client,Account,Alias,Reception,Workflow];
const SHIPPING_RULES:&[Validator]=&[Shipping,ShippingHistory,Reception,Workflow];
const WORKFLOW_RULES:&[Validator]=&[Reception,Workflow,FactoryReply];
const SETTINGS_ALL:&[Validator]=&[Shape,Account,Client,ModelGroup,Message,ProvisionalNotice,Shipping,Reception,Workflow,FactoryReply];
const BRAND_RULES:&[Validator]=&[Brand,BrandLink,Product];
const REGISTRATION_RULES:&[Validator]=&[Registration,Product,Price,PriceHistory,Category,BrandLink,Alias,ModelGroup,Shipping,Reception,Workflow,Mapping];
const MAPPING_RULES:&[Validator]=&[Mapping,Product,Alias,ModelGroup,Shipping,Reception,Workflow,Registration];
const PRICE_HISTORY_ONLY:&[Validator]=&[Price,PriceHistory];

fn product_field(name:&str)->Option<&'static [Validator]>{
    Some(match name{
        "id"|"catalog"|"active"|"deletedAt"|"supersededBy"|"saleStatus"=>PRODUCT_ALL,
        "name"|"code"|"modelCodes"|"modelKey"|"familyKey"|"defaultColor"|
        "originalName"|"subtitle"|"specification"|"sellingUnit"|"siteId"|
        "siteCode"|"siteRefs"|"saleUnit"|"requiresOption"=>PRODUCT_IDENTITY,
        "categories"|"category"=>&[Product,Category,Shipping],
        "brandId"=>&[Product,BrandLink,Alias,Reception,Workflow],
        "origin"|"shippingGroupId"=>&[Product,Shipping,ShippingHistory,Reception,Workflow],
        "baseRetail"=>PRICE_ONLY,
        "priceHistory"=>PRICE_HISTORY_ONLY,
        "createdAt"|"updatedAt"=>&[Product],
        "variants"=>VARIANT_ALL,
        _=>return None,
    })
}
fn variant_field(name:&str)->Option<&'static [Validator]>{
    Some(match name{
        "wholesale"|"semi"|"retail"|"officialRetail"=>PRICE_ONLY,
        "id"|"color"|"option"|"source"|"siteProductId"|"siteOptionId"=>VARIANT_ALL,
        "url"|"collectedAt"=>&[Product,Registration],
        _=>return None,
    })
}
fn product_path(path:&[Segment])->Option<&'static [Validator]>{
    match path {
        []|[RecordId(_)]=>Some(PRODUCT_ALL),
        [RecordId(_),Field(name)]=>product_field(name),
        [RecordId(_),Field(name),rest @ ..] if name=="variants"=>{
            match rest{
                [RecordId(_)]=>Some(VARIANT_ALL),
                [RecordId(_),Field(name)]=>variant_field(name),
                // Unknown nested fields cannot be considered harmless.
                _=>None,
            }
        }
        [RecordId(_),Field(name),..] if name=="priceHistory"=>Some(PRICE_HISTORY_ONLY),
        _=>None,
    }
}
fn settings_path(path:&[Segment])->Option<&'static [Validator]>{
    match path {
        []=>Some(SETTINGS_ALL),
        [Field(name),rest @ ..]=>Some(match name.as_str(){
            "accounts"|"defaultAccountId"|"brandAccounts"=>&[Account,Client,Reception,Workflow],
            "colorGuidanceGroups"=>&[ModelGroup],
            "automaticMessageTemplates"=>&[Message,Reception,Workflow],
            "provisionalProductNotices"=>&[ProvisionalNotice,Reception,Workflow],
            "senderPostcode"|"senderName"|"senderAddress"|"senderPhone"|"senderMobile"|
            "rounding"|"specialPolicy"=>&[Reception,Workflow,Shipping],
            "kakaoTitleMappings"|"kakaoTitleEnabled"=>&[Client,Reception],
            "excelDirectory"|"factoryReplyDirectory"|"orderImageInboxDirectory"|
            "operatorName"=>&[Reception,Workflow,FactoryReply],
            "panelRatios"|"receptionVerticalRatio"|"uiScale" if rest.is_empty()=>&[Shape],
            // These exact named scopes intentionally include all children
            // only where their validator is responsible for the whole value.
            "panelRatios"=>&[Shape],
            _=>return None,
        }),
        _=>None,
    }
}
/// Registry is exhaustive only for registered scopes. Broad owners (e.g.
/// orders, workflow) take responsibility for their complete subtree; the
/// planner must never mistake a broad match for a single-row validation.
pub fn for_change(change:&FieldChange)->Option<&'static [Validator]>{
    let [Field(root),rest @ ..]=change.path.as_slice() else{return None;};
    match root.as_str(){
        "products"=>product_path(rest),
        "settings"=>settings_path(rest),
        "categories"=>Some(CATEGORY_RULES),
        "brands"=>Some(BRAND_RULES),
        "aliases"=>Some(&[Alias]),
        "clients"=>Some(CLIENT_RULES),
        "shipping"=>Some(SHIPPING_RULES),
        "orders"=>Some(WORKFLOW_RULES),
        "workflow"=>Some(WORKFLOW_RULES),
        "factoryReplies"=>Some(WORKFLOW_RULES),
        "queue"|"exports"|"sourceLinks"=>Some(WORKFLOW_RULES),
        "registrationBatches"|"deletedProductKeys"=>Some(REGISTRATION_RULES),
        "productMappings"|"variantMappings"=>Some(MAPPING_RULES),
        "revision" if rest.is_empty()=>Some(&[Shape]),
        _=>None,
    }
}
/// Union and deduplicate requirements. A single unknown path or imprecise
/// analysis yields Full, never an incomplete partial set of requirements.
/// Shape is always required for a nonempty precise change set. A no-op is
/// empty; pre-existing storage preconditions remain outside this registry.
pub fn resolve(changes:&ChangeSet)->Dependencies{
    let ChangeSet::Precise(changes)=changes else{return Dependencies::Full(FullReason::AnalysisFallback);};
    let mut required=BTreeSet::new();
    for change in changes{
        let Some(rules)=for_change(change)else{return Dependencies::Full(FullReason::UnregisteredPath);};
        required.insert(Shape);
        required.extend(rules.iter().copied());
    }
    Dependencies::Required(required)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change_set::{analyze,Fallback};
    use serde_json::{json,Value};
    fn requirements(before:&Value,after:&Value)->BTreeSet<Validator>{
        match resolve(&analyze(before,after)){
            Dependencies::Required(set)=>set,
            other=>panic!("unexpected {other:?}"),
        }
    }
    fn set(rules:&[Validator])->BTreeSet<Validator>{rules.iter().copied().collect()}
    #[test]
    fn price_changes_do_not_activate_unrelated_master_validators(){
        let a=json!({"products":[{"id":"p","variants":[{"id":"v","retail":1}]}]});
        let mut b=a.clone();b["products"][0]["variants"][0]["retail"]=json!(2);
        assert_eq!(requirements(&a,&b),set(&[Shape,Product,Price,PriceHistory]));
    }
    #[test]
    fn product_removal_and_archival_include_reference_dependents(){
        let a=json!({"products":[{"id":"p"}]});
        let removed=json!({"products":[]});
        let mut archived=a.clone();archived["products"][0]["deletedAt"]=json!("now");
        for b in [removed,archived]{
            let required=requirements(&a,&b);
            for rule in [Alias,ModelGroup,Category,BrandLink,Shipping,Reception,Workflow,ProvisionalNotice,Mapping]{
                assert!(required.contains(&rule),"{rule:?}");
            }
        }
    }
    #[test]
    fn assignment_and_topology_are_distinct(){
        let a=json!({"products":[{"id":"p","categories":["a"]}],"categories":[{"id":"a","name":"A"}]});
        let mut b=a.clone();b["products"][0]["categories"]=json!(["b"]);
        assert_eq!(requirements(&a,&b),set(&[Shape,Product,Category,Shipping]));
        b["categories"][0]["name"]=json!("B");
        assert!(requirements(&a,&b).contains(&CategoryTopology));
    }
    #[test]
    fn account_deletion_includes_clients_and_workflow(){
        let a=json!({"settings":{"accounts":[{"id":"a"}]}});
        let b=json!({"settings":{"accounts":[]}});
        assert_eq!(requirements(&a,&b),set(&[Shape,Account,Client,Reception,Workflow]));
    }
    #[test]
    fn variant_identity_and_opaque_product_arrays_have_broad_coverage(){
        let a=json!({"products":[{"id":"p","variants":[{"id":"v","color":"black"}]}]});
        let mut b=a.clone();b["products"][0]["variants"][0]["color"]=json!("white");
        let rules=requirements(&a,&b);
        assert!(rules.is_superset(&set(&[Alias,ModelGroup,Price,PriceHistory,Reception])));
        let b=json!({"products":[{"id":"p"},{"id":"p"}]});
        assert!(requirements(&a,&b).is_superset(&set(PRODUCT_ALL)));
    }
    #[test]
    fn unknown_paths_and_imprecise_analysis_never_skip_validators(){
        let a=json!({"products":[{"id":"p","future":1}],"settings":{"future":1}});
        for b in [json!({"products":[{"id":"p","future":2}],"settings":{"future":1}}),
            json!({"products":[{"id":"p","future":1}],"settings":{"future":2}}),
            json!({"products":[{"id":"p","future":1}],"settings":{"future":1},"newDomain":true})]{
            assert_eq!(resolve(&analyze(&a,&b)),Dependencies::Full(FullReason::UnregisteredPath));
        }
        assert_eq!(resolve(&ChangeSet::Full(Fallback::DepthLimit)),Dependencies::Full(FullReason::AnalysisFallback));
    }
    #[test]
    fn multi_domain_changes_are_unioned_and_deduplicated(){
        let a=json!({"settings":{"accounts":[],"colorGuidanceGroups":{}},"workflow":{"payments":[{"id":"p","amount":1}]}});
        let mut b=a.clone();b["settings"]["accounts"]=json!([{"id":"a"}]);b["settings"]["colorGuidanceGroups"]["g"]=json!({});b["workflow"]["payments"][0]["amount"]=json!(2);
        assert_eq!(requirements(&a,&b),set(&[Shape,Account,Client,ModelGroup,Reception,Workflow,FactoryReply]));
    }
    #[test]
    fn no_op_and_revision_only_are_distinct(){
        let a=json!({"revision":1});let b=json!({"revision":2});
        assert!(requirements(&a,&a).is_empty());
        assert_eq!(requirements(&a,&b),set(&[Shape]));
    }
    #[test]
    fn registered_seed_roots_have_explicit_coverage(){
        use crate::change_set::ChangeKind;
        for root in ["revision","products","clients","aliases","orders","queue","exports","categories","brands","shipping","workflow","factoryReplies","sourceLinks","registrationBatches","deletedProductKeys","productMappings","variantMappings","settings"]{
            assert!(for_change(&FieldChange{path:vec![Field(root.into())],kind:ChangeKind::Modified}).is_some(),"{root}");
        }
    }
}
