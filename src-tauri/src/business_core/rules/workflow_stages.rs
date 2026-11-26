//! Independent validity for the four operational approvals. The legacy
//! numeric step is a derived contiguous prefix, never the source of truth
//! once a shipment is written by this module.
use serde_json::{json,Value};
use super::{field,set};

pub(super) type Stages=[bool;4];
pub(super) fn prefix(stages:Stages)->i64{
    stages.iter().take_while(|done|**done).count() as i64
}
pub(super) fn from_prefix(step:i64)->Stages{
    std::array::from_fn(|i|(i as i64)<step.clamp(0,4))
}
pub(super) fn explicit(ship:&Value)->Option<Stages>{
    let values=field(ship,"stageValidity").as_array()?;
    if values.len()!=4{return None;}
    Some([values[0].as_bool()?,values[1].as_bool()?,values[2].as_bool()?,values[3].as_bool()?])
}
pub(super) fn read(ship:&Value)->Stages{
    let step=field(ship,"step").as_i64().unwrap_or(0);
    // Older snapshots have no mask. A mismatching legacy step must never
    // resurrect completed approvals that a legacy writer has reset.
    explicit(ship).filter(|s|prefix(*s)==step).unwrap_or_else(||from_prefix(step))
}
pub(super) fn write(ship:&mut Value,stages:Stages)->Result<(),String>{
    set(ship,"stageValidity",json!(stages))?;
    set(ship,"step",json!(prefix(stages)))
}
pub(super) fn reset(ship:&mut Value,step:i64)->Result<(),String>{
    write(ship,from_prefix(step))
}
pub(super) fn complete(ship:&mut Value,stage:i64)->Result<bool,String>{
    if !(1..=4).contains(&stage){return Err("잘못된 업무 단계".into());}
    let mut stages=read(ship);
    let index=(stage-1) as usize;
    let changed=!stages[index];
    stages[index]=true;
    write(ship,stages)?;
    Ok(changed)
}
pub(super) fn aggregate<'a>(ships:impl Iterator<Item=&'a Value>)->Stages{
    let mut result=[true;4];
    let mut any=false;
    for ship in ships{
        any=true;
        for (done,other) in result.iter_mut().zip(read(ship)){*done&=other;}
    }
    if any{result}else{[false;4]}
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn legacy_steps_migrate_without_inventing_approvals(){
        for step in 0..=4{
            let mut ship=json!({"step":step});
            assert_eq!(prefix(read(&ship)),step);
            let before=read(&ship);
            write(&mut ship,before).unwrap();
            assert_eq!(ship["stageValidity"],json!(from_prefix(step)));
        }
    }
    #[test]
    fn completing_earlier_review_preserves_later_independent_approvals(){
        let mut ship=json!({"step":0,"stageValidity":[false,false,true,true]});
        assert!(complete(&mut ship,1).unwrap());
        assert_eq!(ship["step"],1);
        assert!(complete(&mut ship,2).unwrap());
        assert_eq!(ship["step"],4);
        assert_eq!(ship["stageValidity"],json!([true,true,true,true]));
        reset(&mut ship,0).unwrap();
        assert_eq!(read(&ship),[false;4]);
    }
    #[test]
    fn inconsistent_legacy_step_never_resurrects_approvals(){
        let ship=json!({"step":1,"stageValidity":[true,true,true,true]});
        assert_eq!(read(&ship),[true,false,false,false]);
    }
}
