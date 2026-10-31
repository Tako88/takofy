//! A thread that saves the latest value of a burst, on the last value alone.
//!
//! The window changes a value every frame and the file should see one write, not
//! sixty. Sending is non-blocking and the sink runs on its own named thread, so
//! the drawing thread never waits on the filesystem. Closing waits a bounded
//! time for the thread and then detaches it.

use std::io;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use log::warn;

/// Signals that the writer thread has ended, whether the sink finished, panicked
/// or the channel closed.
struct Done(mpsc::Sender<()>);

impl Drop for Done {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

/// The thread that writes settled values, coalescing a burst into the last one.
pub struct LatestWriter<T: Send + 'static> {
    values: Option<mpsc::Sender<T>>,
    done: mpsc::Receiver<()>,
    handle: Option<JoinHandle<()>>,
    wait: Duration,
    name: &'static str,
}

impl<T: Send + 'static> LatestWriter<T> {
    /// Starts the writer named `name` whose sink saves a value, or `None` when
    /// the thread cannot start.
    pub fn start(
        name: &'static str,
        sink: impl Fn(&T) -> io::Result<()> + Send + 'static,
        wait: Duration,
    ) -> Option<Self> {
        let (values, received) = mpsc::channel();
        let (done, finished) = mpsc::channel();
        let body = move || {
            let _done = Done(done);
            while let Ok(mut latest) = received.recv() {
                while let Ok(next) = received.try_recv() {
                    latest = next;
                }
                if let Err(error) = sink(&latest) {
                    warn!("{name} write failed: {:?}", error.kind());
                }
            }
        };
        match thread::Builder::new().name(name.to_owned()).spawn(body) {
            Ok(handle) => Some(Self {
                values: Some(values),
                done: finished,
                handle: Some(handle),
                wait,
                name,
            }),
            Err(error) => {
                warn!("{name} writer could not start: {:?}", error.kind());
                None
            }
        }
    }

    /// Hands a value to the thread, replacing any value not yet picked up.
    pub fn send(&self, value: T) {
        if let Some(values) = &self.values {
            let _ = values.send(value);
        }
    }
}

impl<T: Send + 'static> Drop for LatestWriter<T> {
    fn drop(&mut self) {
        drop(self.values.take());
        match self.done.recv_timeout(self.wait) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                if let Some(handle) = self.handle.take()
                    && let Err(error) = handle.join()
                {
                    warn!("{} writer ended unexpectedly: {error:?}", self.name);
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                warn!("{} writer still running after {:?}", self.name, self.wait);
                drop(self.handle.take());
            }
        }
    }
}

#[cfg(test)]
mod tests;
