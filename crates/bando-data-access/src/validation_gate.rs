//! Stage 4: read-only validation executor. No production save path calls it.
//! Every requirement must be supported before a plan can pass. Incomplete
//! coverage is an explicit non-success, never a permissive default.
use crate::{change_set,validation,validation_dependencies::{self,Dependencies,FullReason,Validator}};
use serde_json::Value;
use std::collections::BTreeSet;
use Validator::*;

/// Exhaustive requirement universe. Adding an enum variant requires updating
/// binding() (exhaustive match) and this inventory.
pub const ALL:[Validator;21]=[
    Shape,Product,Price,PriceHistory,Category,CategoryTopology,Brand,BrandLink,
    Alias,ModelGroup,Client,Account,Shipping,ShippingHistory,Reception,Workflow,
    FactoryReply,Message,ProvisionalNotice,Registration,Mapping,
];
type Check=fn(&Value,&Value)->Result<(),String>;

fn shape(_: &Value,next:&Value)->Result<(),String>{validation::validate_shape(next)}
fn topology(_: &Value,next:&Value)->Result<(),String>{crate::product_commands::validate_native_category_topology(next)}
fn model_group(_: &Value,next:&Value)->Result<(),String>{crate::validate_native_model_groups(next)}
fn account(_: &Value,next:&Value)->Result<(),String>{validation::validate_accounts(next)}
fn workflow(before:&Value,next:&Value)->Result<(),String>{crate::rules::validate_workflow_transition(next,before)}
fn message(_: &Value,next:&Value)->Result<(),String>{crate::validate_message_templates(next)}
fn provisional(_: &Value,next:&Value)->Result<(),String>{crate::validate_provisional_product_notices(next)}

/// Only bind genuinely available pure checks for the advertised requirement.
/// Command-specific mutation or validation of a single input is insufficient
/// to certify a whole snapshot transition.
fn binding(rule:Validator)->Option<Check>{
    Some(match rule{
        Shape=>shape,
        Category=>crate::validate_category_transition,
        CategoryTopology=>topology,
        Brand=>crate::validate_brands,
        ModelGroup=>model_group,
        Client=>crate::validate_clients,
        Account=>account,
        Reception=>crate::storage::reception::validate_reception,
        Workflow=>workflow,
        Message=>message,
        ProvisionalNotice=>provisional,
        Product=>crate::validation_transitions::product,
        Price=>crate::validation_transitions::price,
        PriceHistory=>crate::validation_transitions::price_history,
        BrandLink=>crate::validation_transitions::brand_link,
        Alias=>crate::validation_transitions::alias,
        Shipping=>crate::validation_transitions::shipping,
        ShippingHistory=>crate::validation_transitions::shipping_history,
        FactoryReply=>crate::validation_transitions::factory_reply,
        Registration=>crate::validation_transitions::registration,
        Mapping=>crate::validation_transitions::mapping,
    })
}
pub fn supported(rule:Validator)->bool{binding(rule).is_some()}

#[derive(Clone,Debug,PartialEq,Eq)]
pub struct ValidationFailure{
    pub rule:Validator,
    pub message:String,
}
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct ValidationReport{
    pub required:BTreeSet<Validator>,
    pub checked:BTreeSet<Validator>,
    pub unavailable:BTreeSet<Validator>,
    pub failures:Vec<ValidationFailure>,
    pub fallback:Option<FullReason>,
}
impl ValidationReport{
    /// Fallback remains non-authorizing until a full validation policy for
    /// unregistered paths and imprecise analysis has been approved.
    pub fn is_complete_and_valid(&self)->bool{
        self.fallback.is_none()&&self.unavailable.is_empty()&&self.failures.is_empty()
            &&self.checked==self.required
    }
}

fn execute_plan(before:&Value,next:&Value,plan:Dependencies)->ValidationReport{
    let (mut required,fallback)=match plan{
        Dependencies::Required(rules)=>(rules,None),
        Dependencies::Full(reason)=>(ALL.into_iter().collect(),Some(reason)),
    };
    // Always retain the snapshot shape precondition, even for a no-op.
    required.insert(Shape);
    // Existing valid_state checks these two rules on every save. Stage 4
    // must not quietly weaken that contract. They run only once each.
    required.insert(Message);
    required.insert(ProvisionalNotice);
    let mut report=ValidationReport{required,checked:BTreeSet::new(),unavailable:BTreeSet::new(),failures:Vec::new(),fallback};
    for rule in report.required.iter().copied(){
        let Some(check)=binding(rule)else{report.unavailable.insert(rule);continue;};
        report.checked.insert(rule);
        if let Err(message)=check(before,next){
            report.failures.push(ValidationFailure{rule,message});
            // Malformed root data cannot be safely passed to downstream
            // checks. Missing requirements remain visible via checked.
            if rule==Shape{break;}
        }
    }
    // Report all unsupported requirements even when a prior check failed.
    report.unavailable.extend(report.required.iter().copied().filter(|r|!supported(*r)));
    report
}

/// Derive requirements internally from the same two complete snapshots.
/// Callers cannot supply a narrower plan. Partial/scoped snapshots are not
/// supported. A report is diagnostic only: it cannot commit a transaction.
pub fn inspect(before:&Value,next:&Value)->ValidationReport{
    execute_plan(before,next,validation_dependencies::resolve(&change_set::analyze(before,next)))
}

#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    fn state()->Value{
        json!({"products":[],"orders":[],"clients":[],"aliases":[],"queue":[],"exports":[],
            "categories":[],"brands":[],"settings":{"accounts":[]},"revision":1})
    }
    #[test]
    fn every_requirement_has_an_explicit_binding_status(){
        let all:BTreeSet<_>=ALL.into_iter().collect();
        assert_eq!(all.len(),21);
        assert_eq!(all.iter().filter(|r|supported(**r)).count(),21);
        assert_eq!(all.iter().filter(|r|!supported(**r)).count(),0);
    }
    #[test]
    fn no_op_still_executes_existing_basic_checks(){
        let s=state();let report=inspect(&s,&s);
        assert!(report.is_complete_and_valid());
        assert_eq!(report.checked,[Shape,Message,ProvisionalNotice].into_iter().collect());
    }
    #[test]
    fn category_rules_delegate_to_actual_validator(){
        let mut a=state();
        a["categories"]=json!([{"id":"a","name":"A"},{"id":"b","name":"B","parentId":"a"}]);
        let mut b=a.clone();
        b["products"]=json!([{"id":"p","name":"P","categories":["a"]}]);
        let report=inspect(&a,&b);
        assert!(report.failures.iter().any(|f|f.rule==Category));
        assert!(report.checked.contains(&Product));
        assert!(!report.is_complete_and_valid());
    }
    #[test]
    fn price_change_without_history_never_passes(){
        let mut a=state();a["products"]=json!([{"id":"p","variants":[{"id":"v","retail":1}]}]);
        let mut b=a.clone();b["products"][0]["variants"][0]["retail"]=json!(2);
        let report=inspect(&a,&b);
        assert!(report.unavailable.is_empty());
        assert!(report.failures.iter().any(|f|f.rule==PriceHistory));
        assert!(!report.is_complete_and_valid());
    }
    #[test]
    fn account_rule_can_reject_actual_missing_reference(){
        let mut a=state();a["settings"]["accounts"]=json!([{"id":"a","bank":"은행","number":"1","holder":"예금주"}]);
        a["settings"]["defaultAccountId"]=json!("a");
        let mut b=a.clone();b["settings"]["accounts"]=json!([]);
        let report=inspect(&a,&b);
        assert!(report.failures.iter().any(|f|f.rule==Account));
        assert!(!report.is_complete_and_valid());
    }
    #[test]
    fn unknown_paths_require_full_and_never_authorize(){
        let a=state();let mut b=a.clone();b["future"]=json!(true);
        let report=inspect(&a,&b);
        assert_eq!(report.fallback,Some(FullReason::UnregisteredPath));
        assert_eq!(report.required.len(),21);
        assert!(!report.is_complete_and_valid());
    }
    #[test]
    fn shape_failure_stops_downstream_checks_and_exposes_incomplete_coverage(){
        let a=state();let b=json!({"products":false});
        let report=inspect(&a,&b);
        assert_eq!(report.checked,[Shape].into_iter().collect());
        assert!(report.failures.iter().any(|f|f.rule==Shape));
        assert!(!report.is_complete_and_valid());
    }
    #[test]
    fn model_group_change_has_no_duplicate_invocations(){
        let a=state();let mut b=a.clone();b["settings"]["colorGuidanceGroups"]=json!({});
        let report=inspect(&a,&b);
        assert_eq!(report.checked,[Shape,ModelGroup,Message,ProvisionalNotice].into_iter().collect());
        assert!(report.is_complete_and_valid());
    }
}
