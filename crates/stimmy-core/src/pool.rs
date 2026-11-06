//! A small fixed pool for artwork work.
//!
//! Newest jobs run first: while someone scrolls or switches tabs, what they're
//! looking at now matters more than what they looked at a moment ago.

use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex},
    thread,
};

type Job = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    closed: bool,
}

#[derive(Clone)]
pub struct Pool {
    shared: Arc<(Mutex<Queue>, Condvar)>,
}

impl Pool {
    pub fn new(workers: usize) -> Self {
        let shared: Arc<(Mutex<Queue>, Condvar)> = Arc::default();
        for index in 0..workers {
            let shared = shared.clone();
            thread::Builder::new()
                .name(format!("artwork-{index}"))
                .spawn(move || {
                    loop {
                        let job = {
                            let (lock, ready) = &*shared;
                            let mut queue = lock.lock().unwrap_or_else(|e| e.into_inner());
                            loop {
                                if let Some(job) = queue.jobs.pop_back() {
                                    break job;
                                }
                                if queue.closed {
                                    return;
                                }
                                queue = ready.wait(queue).unwrap_or_else(|e| e.into_inner());
                            }
                        };
                        job();
                    }
                })
                .expect("spawn artwork worker");
        }
        Self { shared }
    }
    pub fn submit(&self, job: impl FnOnce() + Send + 'static) {
        let (lock, ready) = &*self.shared;
        lock.lock()
            .unwrap_or_else(|e| e.into_inner())
            .jobs
            .push_back(Box::new(job));
        ready.notify_one();
    }
    /// Lets workers exit once the queue drains.
    pub fn close(&self) {
        let (lock, ready) = &*self.shared;
        lock.lock().unwrap_or_else(|e| e.into_inner()).closed = true;
        ready.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn runs_every_job_and_prefers_the_newest() {
        let pool = Pool::new(1);
        let (gate_tx, gate_rx) = mpsc::channel::<()>();
        let (done_tx, done_rx) = mpsc::channel();
        pool.submit(move || gate_rx.recv().unwrap()); // Occupies the only worker.
        std::thread::sleep(std::time::Duration::from_millis(30));
        for index in 0..3 {
            let done = done_tx.clone();
            pool.submit(move || done.send(index).unwrap());
        }
        gate_tx.send(()).unwrap();
        let order: Vec<i32> = (0..3).map(|_| done_rx.recv().unwrap()).collect();
        assert_eq!(order, [2, 1, 0]);
        pool.close();
    }
}
