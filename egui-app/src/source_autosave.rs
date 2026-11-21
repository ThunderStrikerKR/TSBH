//! Serialized, nonblocking autosave of the active egui reception source.
//! This is a preview-only working source, not a confirmed business order.
//! The worker coalesces rapid edits and commits the last source on shutdown.

use crate::gateway::BandoGateway;
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
                        if let Err(error)=PreviewStore::append_ocr_feedback(&feedback) {
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
