//! The session work the command task drives.
//!
//! The task reaches the engine through one trait, so its exit paths can be
//! exercised without a network or an audio device. The production adapter is
//! thin: a call-through or one `Option` check.

use std::future::Future;

use librespot::core::SessionConfig;
use takofy_core::credentials::AccessToken;
use takofy_core::engine::{Engine, EngineError};
use takofy_model::PlaylistEntry;
use tokio::sync::oneshot::Receiver;

/// Why a session-side operation failed, including the wiring's own state.
#[derive(Debug)]
pub(crate) enum SessionError {
    /// No engine exists yet, so the call had nothing to run on.
    NotConnected,
    /// The engine call itself failed.
    Engine(EngineError),
}

impl SessionError {
    /// A fixed name for the log. The source error is never formatted: it can
    /// embed a response body.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::NotConnected => "not connected",
            Self::Engine(EngineError::Session(_)) => "session",
            Self::Engine(EngineError::Rootlist(_)) => "rootlist",
            Self::Engine(EngineError::Context(_)) => "context",
        }
    }
}

/// The session work the command task drives.
///
/// Returns `impl Future + Send` rather than being an `async fn` because an
/// `async fn` in a trait does not declare its future `Send`, and the task has
/// to be spawnable. `&mut self` is only where the lazily started player is
/// written; `shutdown` consumes, because that is what joins the thread.
pub(crate) trait SessionHost: Send + 'static {
    fn connect(
        &mut self,
        token: &AccessToken,
    ) -> impl Future<Output = Result<(), SessionError>> + Send;

    fn rootlist(&self) -> impl Future<Output = Result<Vec<PlaylistEntry>, SessionError>> + Send;

    fn shutdown(self) -> impl Future<Output = Result<(), SessionError>> + Send;
}

/// Runs one host call, or gives up on it if the window closes first.
pub(crate) async fn stop_or<T>(
    stop: &mut Receiver<()>,
    work: impl Future<Output = T>,
) -> Option<T> {
    tokio::select! {
        biased;
        _ = stop => None,
        value = work => Some(value),
    }
}

/// The production adapter. Holds the config so `connect` can run once the
/// token arrives.
pub(crate) struct LiveSession {
    cfg: SessionConfig,
    engine: Option<Engine>,
}

impl LiveSession {
    pub(crate) fn new(cfg: SessionConfig) -> Self {
        Self { cfg, engine: None }
    }
}

impl SessionHost for LiveSession {
    async fn connect(&mut self, token: &AccessToken) -> Result<(), SessionError> {
        let engine = Engine::connect(self.cfg.clone(), token)
            .await
            .map_err(SessionError::Engine)?;
        self.engine = Some(engine);
        Ok(())
    }

    async fn rootlist(&self) -> Result<Vec<PlaylistEntry>, SessionError> {
        match &self.engine {
            Some(engine) => engine.rootlist().await.map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn shutdown(self) -> Result<(), SessionError> {
        // Consuming the adapter is what will join a started player; the engine
        // simply drops with it.
        drop(self);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::oneshot;

    use super::stop_or;

    #[tokio::test]
    async fn stop_or_returns_the_work_when_it_finishes_first() {
        let (_tx, mut stop) = oneshot::channel();
        assert_eq!(stop_or(&mut stop, async { 7 }).await, Some(7));
    }

    #[tokio::test]
    async fn stop_or_gives_up_when_the_stop_arrives_first() {
        let (tx, mut stop) = oneshot::channel();
        tx.send(()).expect("stop");
        assert_eq!(
            stop_or(&mut stop, std::future::pending::<u32>()).await,
            None
        );
    }
}
