use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, mpsc};

use crate::gpu::Gpu;
use crate::webgl::objects::Readback;

pub(crate) struct Ticket {
    result: Mutex<Option<Option<wgpu::SubmissionIndex>>>,
    settled: Condvar,
}

impl Ticket {
    fn new() -> Arc<Self> {
        Arc::new(Ticket { result: Mutex::new(None), settled: Condvar::new() })
    }

    fn settle(&self, index: Option<wgpu::SubmissionIndex>) {
        *self.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(index);
        self.settled.notify_all();
    }

    pub(crate) fn wait(&self) -> Option<wgpu::SubmissionIndex> {
        let mut result = self.result.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(index) = result.as_ref() {
                return index.clone();
            }
            result = self.settled.wait(result).unwrap_or_else(|e| e.into_inner());
        }
    }
}

type Then = Box<dyn FnOnce() + Send>;

enum Job {
    Submit { gpu: Arc<Gpu>, encoder: wgpu::CommandEncoder, readbacks: Vec<(wgpu::Buffer, Arc<AtomicU8>)>, ticket: Arc<Ticket> },
    Then(Then),
    Done { gpu: Arc<Gpu>, then: Then },
}

static SUBMITTER: OnceLock<Option<Mutex<mpsc::Sender<Job>>>> = OnceLock::new();

fn submitter() -> Option<&'static Mutex<mpsc::Sender<Job>>> {
    SUBMITTER
        .get_or_init(|| {
            let (tx, rx) = mpsc::channel::<Job>();
            match std::thread::Builder::new().name("acde-webgl-submit".into()).spawn(move || {
                for job in rx {
                    run(job);
                }
            }) {
                Ok(_) => Some(Mutex::new(tx)),
                Err(e) => {
                    log::warn!("webgl: no submit thread ({e}), submitting on the page thread");
                    None
                }
            }
        })
        .as_ref()
}

fn send(job: Job) {
    let refused = match submitter() {
        Some(tx) => tx.lock().unwrap_or_else(|e| e.into_inner()).send(job).err().map(|e| e.0),
        None => Some(job),
    };
    if let Some(job) = refused {
        run(job);
    }
}

pub(crate) fn submit(gpu: Arc<Gpu>, encoder: wgpu::CommandEncoder, readbacks: &[std::rc::Rc<Readback>]) -> Arc<Ticket> {
    let ticket = Ticket::new();
    let readbacks = readbacks.iter().map(|r| (r.buffer.clone(), r.state.clone())).collect();
    send(Job::Submit { gpu, encoder, readbacks, ticket: ticket.clone() });
    ticket
}

pub(crate) fn then(then: Then) {
    send(Job::Then(then));
}

pub(crate) fn when_done(gpu: Arc<Gpu>, then: Then) {
    send(Job::Done { gpu, then });
}

fn run(job: Job) {
    match job {
        Job::Submit { gpu, encoder, readbacks, ticket } => {
            let index = if gpu.gone() {
                None
            } else {
                match crate::webgl::helpers::scoped(&gpu.device, |_| gpu.submit([encoder.finish()])) {
                    Ok(index) => Some(index),
                    Err(e) => {
                        log::error!("webgl: submitting: {e}");
                        None
                    }
                }
            };
            for (buffer, state) in readbacks {
                buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                    state.store(if r.is_ok() { Readback::MAPPED } else { Readback::FAILED }, Ordering::Release);
                });
            }
            ticket.settle(index);
        }
        Job::Then(then) => then(),
        Job::Done { gpu, then } => {
            if gpu.gone() {
                then();
                return;
            }
            gpu.hold_poll();
            let held = gpu.clone();
            gpu.queue.on_submitted_work_done(move || {
                then();
                held.release_poll();
            });
        }
    }
}
