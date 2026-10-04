use std::future::Future;
use std::sync::mpsc;
use std::thread::JoinHandle;

use crate::Error;

type Job = Box<dyn FnOnce(&tokio::runtime::Runtime) + Send>;

pub struct DeterministicVm {
    jobs: Option<mpsc::Sender<Job>>,
    thread: Option<JoinHandle<()>>,
}

impl DeterministicVm {
    pub fn start() -> Result<Self, Error> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| Error::Runtime(error.to_string()))?;
        let (jobs, queue) = mpsc::channel::<Job>();
        let thread = std::thread::Builder::new()
            .name("cordial-vm".into())
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                rspace_plus_plus::rspace::deterministic::enable_stable_matching_for_current_thread();
                while let Ok(job) = queue.recv() {
                    job(&runtime);
                }
            })
            .map_err(|error| Error::Runtime(error.to_string()))?;
        Ok(Self {
            jobs: Some(jobs),
            thread: Some(thread),
        })
    }

    pub async fn run<F>(&self, work: F) -> Result<F::Output, Error>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let (reply, result) = tokio::sync::oneshot::channel();
        self.jobs
            .as_ref()
            .ok_or_else(|| Error::Runtime("Cordial VM is stopped".into()))?
            .send(Box::new(move |runtime| {
                let _ = reply.send(runtime.block_on(work));
            }))
            .map_err(|_| Error::Runtime("Cordial VM thread stopped".into()))?;
        result
            .await
            .map_err(|_| Error::Runtime("Cordial VM thread stopped".into()))
    }
}

impl Drop for DeterministicVm {
    fn drop(&mut self) {
        self.jobs.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
