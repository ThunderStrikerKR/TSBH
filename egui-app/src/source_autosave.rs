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
