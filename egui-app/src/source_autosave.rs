//! Serialized, nonblocking autosave of the active egui reception source.
//! This is a preview-only working source, not a confirmed business order.
//! The worker coalesces rapid edits and commits the last source on shutdown.

use crate::gateway::{BandoGateway,ReadOperation};
use serde_json::{json,Value};
use crate::preview_store::{OcrFeedback, PreviewStore, WorkingSource};
use eframe::egui;
use std::{
    sync::{mpsc::{self, Receiver, Sender, RecvTimeoutError}, Arc},
    thread::{self, JoinHandle},
    time::Duration,
};

enum Command {
    Save { source: WorkingSource, feedback: Vec<OcrFeedback> },
    Shutdown,
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
        initial_revision: u64,
        ctx: egui::Context,
    ) -> Self {
        let (sender, rx) = mpsc::channel::<Command>();
        let (updates, results) = mpsc::channel::<SaveStatus>();
        let worker = thread::spawn(move || {
            let mut revision = initial_revision;
            loop {
                let Ok(command) = rx.recv() else { break; };
                let (mut source, mut feedback) = match command {
                    Command::Save { source, feedback } => (source, feedback),
                    Command::Shutdown => break,
                };
                let mut shutdown = false;
                // A small debounce only affects disk writes, never the input.
                // Shutdown drains the latest pending edit before exiting.
                loop {
                    match rx.recv_timeout(Duration::from_millis(180)) {
                        Ok(Command::Save { source: latest, feedback: additions }) => {
                            source = latest;
                            // Keep every approved OCR pair even when rapid
                            // edits coalesce intermediate source snapshots.
                            feedback.extend(additions);
                        },
                        Ok(Command::Shutdown) => {
                            shutdown = true;
                            break;
                        }
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => {
                            shutdown = true;
                            break;
                        }
                    }
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
        // Order-preserving: every queued Save precedes Shutdown. The worker
        // persists the newest source before it terminates, even on app close.
        let _ = self.sender.send(Command::Shutdown);
        if let Some(worker) = self.thread.take() {
            let _ = worker.join();
        }
    }
}
