//! Serialized, nonblocking autosave of the active egui reception source.
//! This is a preview-only working source, not a confirmed business order.
//! The worker coalesces rapid edits and commits the last source on shutdown.

use crate::gateway::BandoGateway;
use eframe::egui;
use std::{
    sync::{mpsc::{self, Receiver, Sender, RecvTimeoutError}, Arc},
    thread::{self, JoinHandle},
    time::Duration,
};

enum Command {
    Save(String),
    Shutdown,
}

#[derive(Debug)]
pub enum SaveStatus {
    Saved(u64),
    Failed(String),
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
                let mut raw = match command {
                    Command::Save(raw) => raw,
                    Command::Shutdown => break,
                };
                let mut shutdown = false;
                // A small debounce only affects disk writes, never the input.
                // Shutdown drains the latest pending edit before exiting.
                loop {
                    match rx.recv_timeout(Duration::from_millis(180)) {
                        Ok(Command::Save(new_raw)) => raw = new_raw,
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
                let status = match gateway.save_working_source(&raw, revision) {
                    Ok(next) if next > revision => {
                        revision = next;
                        SaveStatus::Saved(next)
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

    pub fn queue(&self, raw: String) -> Result<(), String> {
        self.sender.send(Command::Save(raw))
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
