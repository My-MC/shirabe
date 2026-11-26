//! A single filesystem worker with no queued follow-up jobs.

use std::io;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::time::Duration;

type Job<T> = Box<dyn FnOnce() -> T + Send>;

pub struct Worker<T> {
    jobs: SyncSender<Job<T>>,
    results: Receiver<T>,
    busy: bool,
}

impl<T: Send + 'static> Worker<T> {
    pub fn new(wake: impl Fn() + Send + 'static) -> io::Result<Self> {
        let (jobs, inbox) = mpsc::sync_channel::<Job<T>>(1);
        let (outbox, results) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("shirabe-file-io".to_owned())
            .spawn(move || {
                for job in inbox {
                    if outbox.send(job()).is_err() {
                        break;
                    }
                    wake();
                }
            })?;
        Ok(Self {
            jobs,
            results,
            busy: false,
        })
    }

    pub fn busy(&self) -> bool {
        self.busy
    }

    pub fn start(&mut self, job: impl FnOnce() -> T + Send + 'static) -> io::Result<()> {
        if self.busy {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "another file operation is in progress",
            ));
        }
        self.jobs
            .try_send(Box::new(job))
            .map_err(|_| Self::unavailable())?;
        self.busy = true;
        Ok(())
    }

    pub fn poll(&mut self) -> io::Result<Option<T>> {
        self.receive(Duration::ZERO)
    }

    pub fn receive(&mut self, timeout: Duration) -> io::Result<Option<T>> {
        if !self.busy {
            return Ok(None);
        }
        match self.results.recv_timeout(timeout) {
            Ok(result) => {
                self.busy = false;
                Ok(Some(result))
            }
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => {
                self.busy = false;
                Err(Self::unavailable())
            }
        }
    }

    fn unavailable() -> io::Error {
        io::Error::other("file worker is unavailable")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn a_blocked_job_neither_blocks_the_consumer_nor_accepts_another_job() {
        let limit = Duration::from_secs(5);
        let (started_tx, started) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let (wake_tx, wake) = mpsc::channel();
        let mut worker = Worker::new(move || {
            let _ = wake_tx.send(());
        })
        .unwrap();
        let caller = std::thread::current().id();
        worker
            .start(move || {
                started_tx.send(std::thread::current().id()).unwrap();
                release_rx.recv_timeout(limit).unwrap();
                42
            })
            .unwrap();
        assert_ne!(started.recv_timeout(limit).unwrap(), caller);
        assert!(worker.poll().unwrap().is_none());
        assert!(worker.start(|| 0).is_err());
        assert!(wake.try_recv().is_err());
        release.send(()).unwrap();
        wake.recv_timeout(limit).unwrap();
        assert_eq!(worker.poll().unwrap(), Some(42));
        worker.start(|| 7).unwrap();
        wake.recv_timeout(limit).unwrap();
        assert_eq!(worker.poll().unwrap(), Some(7));
    }
}
