//! Fixed baseline configuration and normalized evidence scores.
//! Experimental profiles are only selectable by the native test harness.
//! Hard business constraints must be applied before these soft scores.
pub(super) const SCALE:i64=1000;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(super) enum Decay{Linear,Reciprocal,Exponential}
#[derive(Clone,Copy,Debug)]
pub(super) struct Profile{
    pub(super) id:&'static str,
    pub(super) version:u16,
    pub(super) decay:Decay,
    pub(super) direction:i64,
    pub(super) distance:i64,
    pub(super) paragraph:i64,
    pub(super) repetition:i64,
    pub(super) type_priority:i64,
    pub(super) type_order:i64,
    pub(super) type_length:i64,
    pub(super) sku_length:i64,
    pub(super) sku_option:i64,
    pub(super) quantity_distance:i64,
    pub(super) quantity_local:i64,
}
pub(super) const BASELINE:Profile=Profile{id:"baseline-v1",version:1,decay:Decay::Reciprocal,
    direction:1,distance:1,paragraph:0,repetition:0,type_priority:1,type_order:1,type_length:0,
    sku_length:1,sku_option:1,quantity_distance:1,quantity_local:1};
impl Profile{
    pub(super) fn baseline(self)->bool{self.id==BASELINE.id}
    pub(super) fn ownership(self,preferred:bool,line_distance:usize,char_distance:usize,
        same_paragraph:bool,same_cycle:Option<bool>)->i64{
        self.direction*flag(preferred)+self.distance*proximity(self.decay,line_distance,char_distance)
            +self.paragraph*flag(same_paragraph)+self.repetition*flag(same_cycle==Some(true))
    }
    pub(super) fn name(self,priority:u8,index:usize,length:usize,line_length:usize)->i64{
        self.type_priority*ratio(priority as usize,3)+self.type_order*decay(Decay::Reciprocal,index,1)
            +self.type_length*ratio(length,line_length)
    }
    pub(super) fn quantity_link(self,line_distance:usize,char_distance:usize,same_paragraph:bool,same_cycle:Option<bool>)->i64{
        self.quantity_distance*proximity(self.decay,line_distance,char_distance)
            +self.quantity_local*flag(same_paragraph)+self.repetition*flag(same_cycle==Some(true))
    }
    pub(super) fn sku(self,length:usize,options:usize,full_length:usize,option_count:usize)->i64{
        self.sku_length*ratio(length,full_length)+self.sku_option*ratio(options,option_count)
    }
}
pub(super) fn flag(value:bool)->i64{if value{SCALE}else{0}}
pub(super) fn ratio(value:usize,total:usize)->i64{
    if total==0{return 0;}
    ((value.min(total) as u128*SCALE as u128)/total as u128) as i64
}
pub(super) fn decay(kind:Decay,distance:usize,scale:usize)->i64{
    let scale=scale.max(1);
    match kind{
        Decay::Linear=>SCALE-ratio(distance,scale.saturating_mul(4)),
        Decay::Reciprocal=>((scale as u128*SCALE as u128)/(scale as u128+distance as u128)) as i64,
        Decay::Exponential=>((-(distance as f64)/(scale as f64)).exp()*SCALE as f64).round() as i64,
    }
}
pub(super) fn proximity(kind:Decay,lines:usize,characters:usize)->i64{
    (decay(kind,lines,3)+decay(kind,characters,40))/2
}
#[cfg(test)]
pub(super) const EXPERIMENTS:[Profile;4]=[
    BASELINE,
    Profile{id:"linear-v1",decay:Decay::Linear,direction:4,distance:2,paragraph:1,repetition:1,
        type_priority:4,type_order:2,type_length:1,sku_length:4,sku_option:1,quantity_distance:3,quantity_local:1,..BASELINE},
    Profile{id:"reciprocal-v1",decay:Decay::Reciprocal,direction:2,distance:3,paragraph:2,repetition:2,
        type_priority:3,type_order:1,type_length:2,sku_length:2,sku_option:2,quantity_distance:2,quantity_local:2,..BASELINE},
    Profile{id:"exponential-v1",decay:Decay::Exponential,direction:2,distance:3,paragraph:1,repetition:3,
        type_priority:2,type_order:2,type_length:1,sku_length:1,sku_option:3,quantity_distance:1,quantity_local:3,..BASELINE},
];
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn normalized_weight_features_are_bounded_and_distance_decays(){
        for kind in [Decay::Linear,Decay::Reciprocal,Decay::Exponential]{
            assert_eq!(decay(kind,0,3),SCALE);
            let values=(0..200).map(|n|decay(kind,n,3)).collect::<Vec<_>>();
            assert!(values.iter().all(|v|(0..=SCALE).contains(v)));
            assert!(values.windows(2).all(|v|v[0]>=v[1]));
        }
        assert_eq!(ratio(0,0),0);
        assert_eq!(ratio(usize::MAX,usize::MAX),SCALE);
        assert_eq!(ratio(1,2),500);
        assert_ne!(decay(Decay::Linear,3,3),decay(Decay::Reciprocal,3,3));
    }
    #[test]
    fn experimental_weight_profiles_keep_version_and_component_bounds(){
        for p in EXPERIMENTS{
            assert_eq!(p.version,1);
            assert!(p.ownership(true,0,0,true,Some(true))>=p.ownership(false,8,100,false,Some(false)));
            assert!(p.name(3,0,10,10)>=p.name(1,3,2,10));
            assert!(p.sku(8,2,8,2)>=p.sku(1,0,8,2));
            assert!(p.quantity_link(0,3,true,Some(true))>p.quantity_link(4,50,false,Some(false)));
        }
        let ids=EXPERIMENTS.iter().map(|p|p.id).collect::<std::collections::HashSet<_>>();
        assert_eq!(ids.len(),EXPERIMENTS.len());
    }
}
