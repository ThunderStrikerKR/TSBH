//! One admission policy for order-scoped reception work.
//!
//! UI components must never independently decide which native calculations
//! may overlap. This module is deliberately pure: it cannot change invoices,
//! access SQLite or spawn threads. The existing guarded Rust result handlers
//! still validate the original order/source/recipient identity.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Kind {
    ManualClient,
    AutomaticClient,
    PricingBasis,
    InlineEdit,
    VatEdit,
    WorkingSync,
    ManualWorkspace,
    InlineEditor,
    Analysis,
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Admission {
    Start,
    Duplicate,
    Conflict(Kind),
}

impl Kind {
    pub fn label(self)->&'static str {
        match self {
            Self::ManualClient=>"거래처 변경",
            Self::AutomaticClient=>"자동 거래처 연결",
            Self::PricingBasis=>"가격 기준 확인",
            Self::InlineEdit=>"셀 수정",
            Self::VatEdit=>"부가세 변경",
            Self::WorkingSync=>"운영 작업본 저장",
            Self::ManualWorkspace=>"수동 분류",
            Self::InlineEditor=>"셀 입력",
            Self::Analysis=>"발주원문 분석",
        }
    }
}

/// All requests are exclusive except that an explicit employee dealer
/// selection has priority over an advisory automatic match and an in-flight
/// read-only pricing-basis check. Both outdated results are disconnected by
/// the owner; they never commit after the employee's choice.
pub fn admit(request:Kind, active:&[Kind], same_request:bool)->Admission{
    if active.contains(&request) && same_request {
        return Admission::Duplicate;
    }
    let other=active.iter().copied().find(|kind|{
        !matches!(
            (request,*kind),
            (Kind::ManualClient,Kind::AutomaticClient | Kind::PricingBasis)
        )
    });
    match other {
        Some(kind)=>Admission::Conflict(kind),
        None=>Admission::Start,
    }
}

#[cfg(test)]
mod tests {
    use super::{admit,Admission as A,Kind as K};

    #[test]
    fn every_employee_write_pair_is_serialized_in_both_orders(){
        let writes=[
            K::ManualClient,K::InlineEdit,K::VatEdit,
            K::WorkingSync,K::ManualWorkspace,K::InlineEditor
        ];
        for &first in &writes {
            for &second in &writes {
                if first==second {
                    assert_eq!(admit(second,&[first],true),A::Duplicate);
                }else{
                    assert_eq!(admit(second,&[first],false),A::Conflict(first),
                        "overlap {first:?} -> {second:?}");
                }
            }
        }
    }

    #[test]
    fn manual_choice_supersedes_advisory_work_but_not_staff_edits(){
        assert_eq!(admit(K::ManualClient,&[K::AutomaticClient],false),A::Start);
        assert_eq!(admit(K::ManualClient,&[K::PricingBasis],false),A::Start);
        assert_eq!(admit(K::ManualClient,&[K::AutomaticClient,K::PricingBasis],false),A::Start);
        assert_eq!(admit(K::ManualClient,&[K::AutomaticClient,K::WorkingSync],false),
            A::Conflict(K::WorkingSync));
        assert_eq!(admit(K::AutomaticClient,&[K::ManualClient],false),A::Conflict(K::ManualClient));
        assert_eq!(admit(K::PricingBasis,&[K::InlineEdit],false),A::Conflict(K::InlineEdit));
    }

    #[test]
    fn identical_pending_request_is_not_reissued(){
        for kind in [
            K::ManualClient,K::AutomaticClient,K::PricingBasis,
            K::InlineEdit,K::VatEdit,K::WorkingSync,K::ManualWorkspace,K::Analysis
        ] {
            assert_eq!(admit(kind,&[kind],true),A::Duplicate);
            assert_eq!(admit(kind,&[kind],false),A::Conflict(kind));
        }
        assert_eq!(admit(K::ManualClient,&[],false),A::Start);
    }
}
