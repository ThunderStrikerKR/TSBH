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
    /// Read-only recipient locality lookup; a newer address snapshot wins.
    RemoteRecheck,
    /// Clipboard/screen-capture acquisition (before employee approval).
    SourceIntake,
    /// Approved OCR review or pending original-source replacement dialog.
    SourceReview,
    /// A verified source replacement invalidates advisory native reads.
    SourceReplace,
    /// Direct typing owns the source until the input loses focus.
    SourceEntry,
    /// Native operational group write (revision-checked in SQLite).
    WorkflowAction,
    /// Deleting a draft must never race any running reception job.
    DeleteReception,
    /// Navigation may abandon advisory reads, but not employee writes.
    NavigateReception,
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
            Self::RemoteRecheck=>"도서산간 재확인",
            Self::SourceIntake=>"클립보드·이미지 접수",
            Self::SourceReview=>"발주원문 검토",
            Self::SourceReplace=>"발주원문 교체",
            Self::SourceEntry=>"발주원문 직접 입력",
            Self::WorkflowAction=>"주문 업무 변경",
            Self::DeleteReception=>"발주서 삭제",
            Self::NavigateReception=>"발주서 전환",
        }
    }
}

/// A single policy for both native jobs and source/workflow UI transitions.
/// In-flight staff writes are exclusive. Employee-approved changes/navigation
/// may supersede read-only/advisory results, which the owner must invalidate.
/// Remote lookups can replace a previous DIFFERENT snapshot without waiting
/// for obsolete IO. Autosave is a separate CAS-serialized persistence sidecar,
/// not a competing user operation.
pub fn admit(request:Kind, active:&[Kind], same_request:bool)->Admission{
    if active.contains(&request) && same_request {
        return Admission::Duplicate;
    }
    let blocker=active.iter().copied().find(|kind|{
        !matches!(
            (request,*kind),
            (Kind::ManualClient,
                Kind::AutomaticClient | Kind::PricingBasis | Kind::RemoteRecheck)
            | (Kind::SourceReplace,
                Kind::SourceReview | Kind::AutomaticClient | Kind::PricingBasis
                | Kind::Analysis | Kind::RemoteRecheck)
            | (Kind::NavigateReception,
                Kind::ManualClient | Kind::AutomaticClient | Kind::PricingBasis
                | Kind::Analysis | Kind::RemoteRecheck | Kind::WorkflowAction)
            | (Kind::RemoteRecheck,Kind::RemoteRecheck)
        )
    });
    match blocker {
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
    fn native_analysis_and_master_validation_cannot_overlap_a_write(){
        let writes=[
            K::ManualClient,K::InlineEdit,K::VatEdit,
            K::WorkingSync,K::ManualWorkspace,K::InlineEditor
        ];
        for writer in writes{
            assert_eq!(admit(K::Analysis,&[writer],false),A::Conflict(writer));
            assert_eq!(admit(writer,&[K::Analysis],false),
                A::Conflict(K::Analysis));
            assert_eq!(admit(K::PricingBasis,&[writer],false),
                A::Conflict(writer));
        }
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
    #[test]
    fn all_source_and_workflow_employee_writes_conflict_with_existing_writes(){
        let writers=[
            K::ManualClient,K::InlineEdit,K::VatEdit,K::WorkingSync,
            K::ManualWorkspace,K::InlineEditor,K::WorkflowAction,
            K::SourceIntake,K::SourceEntry,K::DeleteReception
        ];
        for first in writers {
            for second in writers {
                if first==second {
                    assert_eq!(admit(second,&[first],true),A::Duplicate);
                }else{
                    assert_eq!(admit(second,&[first],false),A::Conflict(first),
                        "unexpected overlap {first:?} -> {second:?}");
                }
            }
        }
    }

    #[test]
    fn source_approval_supersedes_advice_but_never_staff_writes(){
        for advisory in [
            K::SourceReview,K::AutomaticClient,K::PricingBasis,
            K::Analysis,K::RemoteRecheck
        ] {
            assert_eq!(admit(K::SourceReplace,&[advisory],false),A::Start);
        }
        for writer in [
            K::ManualClient,K::InlineEdit,K::VatEdit,K::WorkingSync,
            K::ManualWorkspace,K::InlineEditor,K::WorkflowAction,K::SourceIntake,
            K::SourceEntry
        ]{
            assert_eq!(admit(K::SourceReplace,&[writer],false),A::Conflict(writer));
        }
    }

    #[test]
    fn remote_recheck_deduplicates_or_supersedes_older_snapshot(){
        assert_eq!(admit(K::RemoteRecheck,&[K::RemoteRecheck],true),A::Duplicate);
        assert_eq!(admit(K::RemoteRecheck,&[K::RemoteRecheck],false),A::Start);
        assert_eq!(admit(K::RemoteRecheck,&[K::WorkingSync],false),
            A::Conflict(K::WorkingSync));
        assert_eq!(admit(K::ManualClient,&[K::RemoteRecheck],false),A::Start);
    }

    #[test]
    fn navigation_abandons_advice_but_never_discards_uncommitted_edits(){
        for cancellable in [
            K::ManualClient,K::AutomaticClient,K::PricingBasis,
            K::Analysis,K::RemoteRecheck,K::WorkflowAction
        ] {
            assert_eq!(admit(K::NavigateReception,&[cancellable],false),A::Start);
        }
        for blocking in [
            K::SourceIntake,K::SourceReview,K::SourceEntry,K::InlineEdit,
            K::VatEdit,K::WorkingSync,K::ManualWorkspace,K::InlineEditor
        ] {
            assert_eq!(admit(K::NavigateReception,&[blocking],false),
                A::Conflict(blocking));
            assert_eq!(admit(K::DeleteReception,&[blocking],false),
                A::Conflict(blocking));
        }
    }

}
