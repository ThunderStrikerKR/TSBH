//! One running read job and one replaceable pending job per view.
//! Painting only polls results; superseded jobs can never publish into the view.
use eframe::egui::Context;
use std::sync::{
    mpsc::{self, SyncSender},
    Arc, Mutex,
};

// Swapping ownership is the only operation allowed under these locks.
// Superseded closures/results may run arbitrary Drop implementations.
fn replace_slot<T>(slot:&Mutex<Option<T>>,value:Option<T>){
    let retired={
        let mut guard=slot.lock().unwrap();
        std::mem::replace(&mut *guard,value)
    };
    drop(retired);
}

type Answer<T> = (u64, Result<T, String>);
struct Job<T> {
    generation: u64,
    context: Context,
    work: Box<dyn FnOnce() -> Result<T, String> + Send>,
}
struct Worker<T> {
    next: Arc<Mutex<Option<Job<T>>>>,
    answer: Arc<Mutex<Option<Answer<T>>>>,
    wake: SyncSender<()>,
}
pub struct LatestJob<T> {
    worker: Option<Worker<T>>,
    #[cfg(test)]
    staged_transport: bool,
    generation: u64,
    pending: bool,
}
impl<T> Default for LatestJob<T> {
    fn default() -> Self {
        Self {
            worker: None,
            #[cfg(test)]
            staged_transport: false,
            generation: 0,
            pending: false,
        }
    }
}
impl<T: Send + 'static> LatestJob<T> {
    pub fn pending(&self) -> bool {
        self.pending
    }
    pub fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.pending = false;
        if let Some(worker) = &self.worker {
            replace_slot(&worker.next,None);
            replace_slot(&worker.answer,None);
        }
    }
    pub fn submit(
        &mut self,
        context: Context,
        work: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) {
        self.invalidate();
        #[cfg(test)]
        if self.staged_transport {
            self.worker=None;
            self.staged_transport=false;
        }
        if self.worker.is_none() {
            let next: Arc<Mutex<Option<Job<T>>>> = Arc::new(Mutex::new(None));
            let answer = Arc::new(Mutex::new(None));
            let (wake, signals) = mpsc::sync_channel(1);
            let pending = Arc::clone(&next);
            let completed = Arc::clone(&answer);
            std::thread::spawn(move || {
                while signals.recv().is_ok() {
                    // Never hold a mutex while querying a catalog or database.
                    let job = pending.lock().unwrap().take();
                    if let Some(job) = job {
                        let result =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(job.work))
                                .unwrap_or_else(|_| Err("조회 작업이 중단되었습니다.".into()));
                        replace_slot(&completed,Some((job.generation,result)));
                        job.context.request_repaint();
                    }
                }
            });
            self.worker = Some(Worker { next, answer, wake });
        }
        let worker = self.worker.as_ref().unwrap();
        replace_slot(&worker.next,Some(Job {
            generation: self.generation,
            context,
            work: Box::new(work),
        }));
        self.pending = true;
        // A full wake channel already guarantees the latest pending job runs.
        let _ = worker.wake.try_send(());
    }
    // Deterministic test transport: stage an answer for the current
    // generation. The real poll() path still consumes it.
    #[cfg(test)]
    pub(crate) fn geometry_begin_read(&mut self)->Box<dyn FnOnce(Result<T,String>)>{
        self.invalidate();
        let answer:Arc<Mutex<Option<Answer<T>>>>=Arc::new(Mutex::new(None));
        let (wake,_signals)=mpsc::sync_channel(1);
        self.worker=Some(Worker{next:Arc::new(Mutex::new(None)),
            answer:Arc::clone(&answer),wake});
        self.pending=true;
        self.staged_transport=true;
        // Preserve the former channel fixture's sender-drop semantics.
        // A dropped completion represents an interrupted read, not a job
        // that silently stays pending forever.
        let staged=StagedAnswer{answer,generation:self.generation,completed:false};
        Box::new(move |result|staged.complete(result))
    }
    pub fn poll(&mut self) -> Option<Result<T, String>> {
        let answer = self.worker.as_ref()?.answer.lock().unwrap().take()?;
        if answer.0 != self.generation {
            return None;
        }
        self.pending = false;
        Some(answer.1)
    }
}

#[cfg(test)]
struct StagedAnswer<T>{
    answer:Arc<Mutex<Option<Answer<T>>>>,
    generation:u64,
    completed:bool,
}
#[cfg(test)]
impl<T> StagedAnswer<T>{
    fn complete(mut self,result:Result<T,String>){
        replace_slot(&self.answer,Some((self.generation,result)));
        self.completed=true;
    }
}
#[cfg(test)]
impl<T> Drop for StagedAnswer<T>{
    fn drop(&mut self){
        if !self.completed{
            replace_slot(&self.answer,Some((self.generation,Err("조회 작업이 중단되었습니다.".into()))));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    fn completed<T: Send + 'static>(job: &mut LatestJob<T>) -> Result<T, String> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(value) = job.poll() {
                return value;
            }
            assert!(Instant::now() < deadline, "worker did not finish");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn superseded_payload_is_dropped_after_slot_unlock(){
        struct CheckDrop(Arc<Mutex<Option<CheckDrop>>>);
        impl Drop for CheckDrop{
            fn drop(&mut self){
                assert!(self.0.try_lock().is_ok(),"payload destructor ran under its own slot lock");
            }
        }
        let slot=Arc::new(Mutex::new(None));
        replace_slot(&slot,Some(CheckDrop(Arc::clone(&slot))));
        replace_slot(&slot,Some(CheckDrop(Arc::clone(&slot))));
        replace_slot(&slot,None);
    }
    #[test]
    fn staged_disconnect_is_consumed_and_real_worker_can_restart(){
        let mut job=LatestJob::<u64>::default();
        drop(job.geometry_begin_read());
        assert!(job.poll().unwrap().is_err());
        assert!(!job.pending());
        job.submit(Context::default(),||Ok(42));
        assert_eq!(completed(&mut job).unwrap(),42);
    }
    #[test]
    fn burst_replaces_pending_work_and_rejects_old_results() {
        let mut job = LatestJob::default();
        let (started, running) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        job.submit(Context::default(), move || {
            started.send(()).unwrap();
            gate.recv().unwrap();
            Ok(1)
        });
        running.recv_timeout(Duration::from_secs(5)).unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        for value in 2..=100 {
            let calls = Arc::clone(&calls);
            job.submit(Context::default(), move || {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(value)
            });
        }
        assert!(job.pending());
        release.send(()).unwrap();
        assert_eq!(completed(&mut job).unwrap(), 100);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(!job.pending());
    }
    #[test]
    fn invalidation_discards_in_flight_work_and_does_not_wait() {
        let mut job = LatestJob::default();
        let (started, running) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        job.submit(Context::default(), move || {
            started.send(()).unwrap();
            gate.recv().unwrap();
            Ok(1)
        });
        running.recv_timeout(Duration::from_secs(5)).unwrap();
        job.invalidate();
        assert!(!job.pending());
        release.send(()).unwrap();
        job.submit(Context::default(), || Ok(2));
        assert_eq!(completed(&mut job).unwrap(), 2);
    }
}
