//! Serialized, nonblocking autosave of the active egui reception source.
//! This is a preview-only working source, not a confirmed business order.
//! The worker coalesces rapid edits and commits the last source on shutdown.

use crate::gateway::{BandoGateway,ReadOperation};
use serde_json::{json,Value};
use crate::preview_store::{
    OcrFeedback, PreviewStore, WorkingReception, WorkingSource
};
use eframe::egui;
use std::{
    sync::{mpsc::{self, Receiver, Sender, RecvTimeoutError}, Arc},
    thread::{self, JoinHandle},
    time::Duration,
};

/// Only one draft's source fields cross the egui -> autosave channel for
/// keystrokes. The worker retains the last complete WorkingSource and applies
/// these deltas in FIFO order before the existing 180ms disk debounce.
#[derive(Clone, Debug)]
pub struct SourceTextEdit {
    pub order_id:String,
    pub raw:String,
    pub order_type:String,
    pub source_input_method:Option<String>,
    pub last_input_method:Option<String>,
}

/// Keep the source-dependent invalidation identical for full snapshots and
/// incremental edits; manual dealer selections and edit audit survive.
pub fn update_draft_source(
    row:&mut WorkingReception,raw:&str,order_type:&str
)->bool{
    let changed=row.raw!=raw || row.order_type!=order_type;
    if changed{
        row.analysis_raw=None;
        row.analysis_evidence=None;
        row.analysis_view=None;
        row.client_match=None;
        row.pricing_preview=None;
        row.manual_model=None;
        row.working_synced=false;
        if row.client_selection.as_deref()==Some("AUTO"){
            row.client_id=None;
            row.client_selection=None;
        }
    }
    row.raw=raw.to_owned();
    row.order_type=order_type.to_owned();
    changed
}

impl SourceTextEdit{
    fn apply(self,source:&mut WorkingSource)->Result<(),String>{
        if source.active_id.as_deref()!=Some(self.order_id.as_str()){
            return Err(
                "직접 입력 저장 대상이 현재 발주서와 일치하지 않습니다.".into()
            );
        }
        let Some(row)=source.drafts.iter_mut().find(|row|
            row.id==self.order_id
        )else{
            return Err("직접 입력 저장 대상 발주서가 없습니다.".into());
        };
        update_draft_source(row,&self.raw,&self.order_type);
        row.source_input_method=self.source_input_method;
        row.last_input_method=self.last_input_method;
        source.raw=self.raw;
        source.order_type=self.order_type;
        Ok(())
    }
}

enum Command {
    Save { source: WorkingSource, feedback: Vec<OcrFeedback> },
    Edit(SourceTextEdit),
    Shutdown,
}

fn merge_command(
    snapshot:&mut WorkingSource,feedback:&mut Vec<OcrFeedback>,
    command:Command
)->Result<bool,String>{
    match command{
        Command::Save{source,feedback:additional}=>{
            *snapshot=source;
            feedback.extend(additional);
            Ok(true)
        }
        Command::Edit(edit)=>{edit.apply(snapshot)?;Ok(true)}
        Command::Shutdown=>Ok(false),
    }
}

#[derive(Debug)]
pub enum SaveStatus {
    Saved { revision:u64, active_id:Option<String>, raw:String, order_type:String },
    Failed(String),
    FeedbackFailed(String),
}

pub struct SourceAutosave {
    sender: Sender<Command>,
    results: Receiver<SaveStatus>,
    thread: Option<JoinHandle<()>>,
}

impl SourceAutosave {
    pub fn new(
        gateway: Arc<dyn BandoGateway>,
        initial_source: WorkingSource,
        ctx: egui::Context,
    ) -> Self {
        let (sender, rx) = mpsc::channel::<Command>();
        let (updates, results) = mpsc::channel::<SaveStatus>();
        let worker = thread::spawn(move || {
            let mut revision = initial_source.revision;
            let mut source=initial_source;
            loop {
                let Ok(command)=rx.recv() else{break};
                let mut feedback=Vec::new();
                let mut dirty=false;
                let mut shutdown=false;
                match merge_command(&mut source,&mut feedback,command){
                    Ok(true)=>dirty=true,
                    Ok(false)=>shutdown=true,
                    Err(error)=>{let _=updates.send(SaveStatus::Failed(error));}
                }
                // Debounce disk IO without re-cloning the full drafts vector.
                // Full saves and source deltas retain FIFO order, including
                // OCR feedback and Shutdown's final flush.
                while !shutdown{
                    match rx.recv_timeout(Duration::from_millis(180)){
                        Ok(command)=>match merge_command(
                            &mut source,&mut feedback,command
                        ){
                            Ok(true)=>dirty=true,
                            Ok(false)=>shutdown=true,
                            Err(error)=>{
                                let _=updates.send(SaveStatus::Failed(error));
                            }
                        },
                        Err(RecvTimeoutError::Timeout)=>break,
                        Err(RecvTimeoutError::Disconnected)=>{
                            shutdown=true;
                            break;
                        }
                    }
                }
                if !dirty{
                    if shutdown{break;}
                    continue;
                }
                source.revision = revision;
                let status = match gateway.save_working_source(&source) {
                    Ok(next) if next > revision => {
                        revision = next;
                        // Never collect corrections for an unpersisted
                        // reception. This local-only write is separate from
                        // production orders and has no network fallback.
                        match PreviewStore::append_ocr_feedback(&feedback) {
                            Err(error)=>{
                                let _=updates.send(SaveStatus::FeedbackFailed(error));
                            }
                            Ok(()) if !feedback.is_empty()=>{
                                // Analysis must not delay the serialized
                                // working-source save queue. Compare BOTH
                                // texts with the existing Rust order engine
                                // in an independent background worker.
                                let engine=Arc::clone(&gateway);
                                let pairs=feedback;
                                std::thread::spawn(move||{
                                    let catalog=engine.load_catalog();
                                    for pair in pairs {
                                        let result=(||->Result<(Value,Value,Option<String>),String>{
                                            let state=catalog.as_ref().map_err(Clone::clone)?;
                                            let revision=state.get("revision").and_then(|v|
                                                v.as_str().map(str::to_owned)
                                                .or_else(||v.as_u64().map(|n|n.to_string()))
                                            );
                                            let parsed_initial=engine.execute(
                                                ReadOperation::AnalyzeOrder,
                                                json!({
                                                    "raw":pair.original_text,
                                                    "state":state,
                                                    "client":Value::Null
                                                })
                                            )?;
                                            let parsed_corrected=engine.execute(
                                                ReadOperation::AnalyzeOrder,
                                                json!({
                                                    "raw":pair.corrected_text,
                                                    "state":state,
                                                    "client":Value::Null
                                                })
                                            )?;
                                            Ok((parsed_initial,parsed_corrected,revision))
                                        })();
                                        match result {
                                            Ok((initial,corrected,revision))=>{
                                                if let Err(error)=PreviewStore::store_ocr_analysis(
                                                    &pair.id,&initial,&corrected,revision
                                                ){
                                                    let _=PreviewStore::fail_ocr_analysis(
                                                        &pair.id,&error
                                                    );
                                                }
                                            }
                                            Err(error)=>{
                                                let _=PreviewStore::fail_ocr_analysis(
                                                    &pair.id,&error
                                                );
                                            }
                                        }
                                    }
                                });
                            }
                            Ok(())=>{}
                        }
                        // This runs AFTER the append, so even an OCR event
                        // whose dealer was selected before capture is tagged.
                        // Future manual client selection updates earlier
                        // samples by the same unconfirmed receipt ID.
                        if let Err(error)=PreviewStore::bind_verified_ocr_clients(&source){
                            let _=updates.send(SaveStatus::FeedbackFailed(error));
                        }
                        SaveStatus::Saved {
                            revision:next,
                            active_id:source.active_id.clone(),
                            raw:source.raw.clone(),
                            order_type:source.order_type.clone(),
                        }
                    }
                    Ok(_) => SaveStatus::Failed(
                        "원문 저장 revision이 증가하지 않았습니다.".to_owned()
                    ),
                    Err(error) => SaveStatus::Failed(error),
                };
                let _ = updates.send(status);
                ctx.request_repaint();
                if shutdown { break; }
            }
        });
        Self { sender, results, thread: Some(worker) }
    }

    pub fn queue(&self, source: WorkingSource) -> Result<(), String> {
        self.queue_with_feedback(source, Vec::new())
    }

    /// Lightweight direct-source edit, never a full draft collection clone.
    pub fn queue_source_edit(&self,edit:SourceTextEdit)->Result<(),String>{
        self.sender.send(Command::Edit(edit))
            .map_err(|_| "egui 원문 자동저장 작업이 중단되었습니다.".to_owned())
    }

    pub fn queue_with_feedback(
        &self,source: WorkingSource,feedback:Vec<OcrFeedback>
    ) -> Result<(),String> {
        self.sender.send(Command::Save { source, feedback })
            .map_err(|_| "egui 원문 자동저장 작업이 중단되었습니다.".to_owned())
    }

    pub fn try_status(&self) -> Option<SaveStatus> {
        self.results.try_recv().ok()
    }
}

impl Drop for SourceAutosave {
    fn drop(&mut self) {
        // Order-preserving: every queued Save/Edit precedes Shutdown. The worker
        // persists the newest source before it terminates, even on app close.
        let _ = self.sender.send(Command::Shutdown);
        if let Some(worker) = self.thread.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod source_delta_tests{
    use super::*;
    use serde_json::json;

    fn draft(id:&str)->WorkingReception{
        WorkingReception{
            id:id.into(),raw:"old".into(),order_type:"AGENCY".into(),
            source_input_method:None,last_input_method:None,
            analysis_raw:Some("old".into()),
            analysis_evidence:Some(json!({"recipients":[{"id":"r1"}]})),
            analysis_view:Some(json!({"cached":true})),
            client_match:Some(json!({"clientId":"auto"})),
            chat_handle:None,chat_title:None,
            client_id:Some("auto".into()),client_selection:Some("AUTO".into()),
            pricing_preview:Some(json!({"total":42})),
            working_version:None,working_synced:true,
            manual_model:Some(json!({"model":"old"})),
            edit_history:vec![json!({"id":"immutable-audit"})],
        }
    }
    fn snapshot()->WorkingSource{
        WorkingSource{
            raw:"old".into(),order_type:"AGENCY".into(),revision:12,
            active_id:Some("first".into()),
            drafts:vec![draft("first"),draft("second")]
        }
    }
    fn edit(text:&str)->SourceTextEdit{
        SourceTextEdit{
            order_id:"first".into(),raw:text.into(),order_type:"AGENCY".into(),
            source_input_method:Some("DIRECT".into()),
            last_input_method:Some("DIRECT".into())
        }
    }

    #[test]
    fn repeated_keystrokes_only_mutate_active_draft_and_keep_immutable_audit(){
        let mut state=snapshot();
        let untouched=state.drafts[1].clone();
        let mut feedback=Vec::new();
        for raw in ["o","or","ord","order"]{
            assert!(merge_command(
                &mut state,&mut feedback,Command::Edit(edit(raw))
            ).unwrap());
        }
        assert_eq!(state.raw,"order");
        assert_eq!(state.revision,12);
        assert_eq!(state.drafts[0].raw,"order");
        assert_eq!(state.drafts[0].source_input_method.as_deref(),Some("DIRECT"));
        assert!(state.drafts[0].analysis_evidence.is_none());
        assert!(state.drafts[0].pricing_preview.is_none());
        assert!(state.drafts[0].manual_model.is_none());
        assert!(state.drafts[0].client_id.is_none());
        assert!(state.drafts[0].client_selection.is_none());
        assert_eq!(state.drafts[0].edit_history[0]["id"],"immutable-audit");
        assert_eq!(state.drafts[1].raw,untouched.raw);
        assert_eq!(state.drafts[1].analysis_evidence,untouched.analysis_evidence);
        assert_eq!(state.drafts[1].pricing_preview,untouched.pricing_preview);
    }

    #[test]
    fn full_snapshot_and_delta_keep_fifo_order_and_ocr_feedback(){
        let mut state=snapshot();
        let mut feedback=Vec::new();
        let correction=OcrFeedback{
            id:"fb".into(),order_id:"first".into(),client_id:None,
            original_text:"O".into(),corrected_text:"o".into(),
            learning_used:false,model_suggestion:None
        };
        let mut full=snapshot();
        full.drafts.push(draft("third"));
        full.drafts[0].client_selection=Some("MANUAL".into());
        full.drafts[0].client_id=Some("staff".into());
        assert!(merge_command(
            &mut state,&mut feedback,Command::Save{
                source:full,feedback:vec![correction]
            }
        ).unwrap());
        assert!(merge_command(
            &mut state,&mut feedback,Command::Edit(edit("after save"))
        ).unwrap());
        assert_eq!(feedback.len(),1);
        assert_eq!(feedback[0].id,"fb");
        assert_eq!(state.drafts.len(),3);
        assert_eq!(state.drafts[0].raw,"after save");
        assert_eq!(state.drafts[0].client_id.as_deref(),Some("staff"));
        assert_eq!(state.drafts[0].client_selection.as_deref(),Some("MANUAL"));
        assert!(merge_command(
            &mut state,&mut feedback,Command::Save{
                source:snapshot(),feedback:Vec::new()
            }
        ).unwrap());
        assert_eq!(state.raw,"old","later full snapshot supersedes edits");
        assert_eq!(feedback.len(),1,"OCR feedback survives coalescing");
    }

    #[test]
    fn stale_or_missing_draft_patch_refuses_without_mutating_state(){
        let mut state=snapshot();
        let mut feedback=Vec::new();
        let original=state.drafts[0].raw.clone();
        let mut wrong=edit("new");
        wrong.order_id="missing".into();
        assert!(merge_command(&mut state,&mut feedback,Command::Edit(wrong))
            .is_err());
        assert_eq!(state.raw,"old");
        assert_eq!(state.drafts[0].raw,original);
        state.active_id=Some("missing".into());
        assert!(merge_command(
            &mut state,&mut feedback,Command::Edit(SourceTextEdit{
                order_id:"missing".into(),..edit("new")
            })
        ).is_err());
        assert_eq!(state.raw,"old");
    }

    #[test]
    fn shutdown_signal_preserves_last_pending_delta_for_worker_flush(){
        let mut state=snapshot();
        let mut feedback=Vec::new();
        assert!(merge_command(
            &mut state,&mut feedback,Command::Edit(edit("final keystroke"))
        ).unwrap());
        assert!(!merge_command(
            &mut state,&mut feedback,Command::Shutdown
        ).unwrap());
        assert_eq!(state.raw,"final keystroke");
    }
}
