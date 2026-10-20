//! The session work the command task drives.
//!
//! The task reaches the engine through one trait, so its exit paths can be
//! exercised without a network or an audio device. The production adapter is
//! thin: a call-through or one `Option` check.

use std::future::Future;

use librespot::core::SessionConfig;
use takofy_core::credentials::AccessToken;
use takofy_core::engine::{Engine, EngineError};
use takofy_core::playback::{Playback, PlaybackError, PlaybackEvent};
use takofy_model::{
    AlbumDetail, ArtistDetail, CoverUrl, FollowedArtist, FollowedShow, PlaylistEntry, SavedAlbum,
    ShowDetail, TrackEntry,
};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::oneshot::Receiver;

/// Why a session-side operation failed, including the wiring's own state.
#[derive(Debug)]
pub(crate) enum SessionError {
    /// No engine exists yet, so the call had nothing to run on.
    NotConnected,
    /// The engine call itself failed.
    Engine(EngineError),
    /// The player could not be started, driven or shut down.
    Playback(PlaybackError),
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
            Self::Engine(EngineError::Album(_)) => "album",
            Self::Engine(EngineError::Artist(_)) => "artist",
            Self::Engine(EngineError::Show(_)) => "show",
            Self::Engine(EngineError::Followed(_)) => "followed",
            Self::Playback(error) => match error {
                PlaybackError::NoAudioBackend => "no audio backend",
                PlaybackError::NoAudioDevice => "no audio device",
                PlaybackError::PlayerShared => "player shared",
                PlaybackError::PlayerClosed => "player closed",
                PlaybackError::NotPlayable(_) => "not playable",
            },
        }
    }
}

/// One page of a playlist's tracks, plus the context's own image.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TrackPage {
    pub tracks: Vec<TrackEntry>,
    pub cover: Option<CoverUrl>,
}

/// One artist's page data, plus whether the user follows it.
///
/// The followed state is `None` when the collection probe did not answer: the
/// label then reads `Follow` and stays pressable, because the write is the only
/// thing that can confirm it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ArtistPage {
    pub artist: ArtistDetail,
    pub followed: Option<bool>,
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

    fn tracks(
        &mut self,
        playlist: &str,
    ) -> impl Future<Output = Result<TrackPage, SessionError>> + Send;

    fn album(&self, album: &str) -> impl Future<Output = Result<AlbumDetail, SessionError>> + Send;

    fn show(&self, show: &str) -> impl Future<Output = Result<ShowDetail, SessionError>> + Send;

    /// The shows the user follows, newest follow first, each as its card draws
    /// it.
    fn followed_shows(
        &self,
    ) -> impl Future<Output = Result<Vec<FollowedShow>, SessionError>> + Send;

    /// The artists the user follows, newest follow first, each as its card
    /// draws it.
    fn followed_artists(
        &self,
    ) -> impl Future<Output = Result<Vec<FollowedArtist>, SessionError>> + Send;

    /// The albums the user saved, newest save first, each as its card draws it.
    fn saved_albums(&self) -> impl Future<Output = Result<Vec<SavedAlbum>, SessionError>> + Send;

    fn artist(&self, artist: &str)
    -> impl Future<Output = Result<ArtistPage, SessionError>> + Send;

    /// One release's track names, in the release's own order.
    fn release_tracks(
        &self,
        tracks: &[String],
    ) -> impl Future<Output = Result<Vec<TrackEntry>, SessionError>> + Send;

    /// Writes the artist into the user's followed set, or out of it.
    fn set_follow(
        &self,
        artist: &str,
        follow: bool,
    ) -> impl Future<Output = Result<(), SessionError>> + Send;

    fn play(&mut self, uri: &str) -> Result<(), SessionError>;

    /// Jumps within the loaded track. A call with no player is a no-op.
    fn seek(&self, position_ms: u32);

    /// The event stream of a player started since the last call, so the one
    /// drain thread can be handed it exactly once.
    fn take_events(&mut self) -> Option<UnboundedReceiver<PlaybackEvent>>;

    fn pause(&self);

    fn resume(&self);

    fn stop(&self);

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
/// token arrives, and the player so it can be started lazily.
pub(crate) struct LiveSession {
    cfg: SessionConfig,
    engine: Option<Engine>,
    playback: Option<Playback>,
    events: Option<UnboundedReceiver<PlaybackEvent>>,
}

impl LiveSession {
    pub(crate) fn new(cfg: SessionConfig) -> Self {
        Self {
            cfg,
            engine: None,
            playback: None,
            events: None,
        }
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

    async fn tracks(&mut self, playlist: &str) -> Result<TrackPage, SessionError> {
        let Some(engine) = &self.engine else {
            return Err(SessionError::NotConnected);
        };
        let mut pages = engine.tracks(playlist);
        let tracks = pages
            .next()
            .await
            .map_err(SessionError::Engine)?
            .unwrap_or_default();
        Ok(TrackPage {
            tracks,
            cover: pages.cover(),
        })
    }

    async fn album(&self, album: &str) -> Result<AlbumDetail, SessionError> {
        match &self.engine {
            Some(engine) => engine.album(album).await.map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn show(&self, show: &str) -> Result<ShowDetail, SessionError> {
        match &self.engine {
            Some(engine) => engine.show(show).await.map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn followed_shows(&self) -> Result<Vec<FollowedShow>, SessionError> {
        match &self.engine {
            Some(engine) => engine.followed_shows().await.map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn followed_artists(&self) -> Result<Vec<FollowedArtist>, SessionError> {
        match &self.engine {
            Some(engine) => engine
                .followed_artists()
                .await
                .map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn saved_albums(&self) -> Result<Vec<SavedAlbum>, SessionError> {
        match &self.engine {
            Some(engine) => engine.saved_albums().await.map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn artist(&self, artist: &str) -> Result<ArtistPage, SessionError> {
        match &self.engine {
            Some(engine) => {
                let (artist, followed) =
                    engine.artist(artist).await.map_err(SessionError::Engine)?;
                Ok(ArtistPage { artist, followed })
            }
            None => Err(SessionError::NotConnected),
        }
    }

    async fn release_tracks(&self, tracks: &[String]) -> Result<Vec<TrackEntry>, SessionError> {
        match &self.engine {
            Some(engine) => Ok(engine.release_tracks(tracks).await),
            None => Err(SessionError::NotConnected),
        }
    }

    async fn set_follow(&self, artist: &str, follow: bool) -> Result<(), SessionError> {
        match &self.engine {
            Some(engine) => engine
                .set_follow(artist, follow)
                .await
                .map_err(SessionError::Engine),
            None => Err(SessionError::NotConnected),
        }
    }

    fn play(&mut self, uri: &str) -> Result<(), SessionError> {
        if self.playback.is_none() {
            let engine = self.engine.as_ref().ok_or(SessionError::NotConnected)?;
            let (playback, events) = engine.playback().map_err(SessionError::Playback)?;
            self.playback = Some(playback);
            self.events = Some(events);
        }
        match self.playback.as_ref() {
            Some(playback) => playback.play(uri).map_err(SessionError::Playback),
            None => Err(SessionError::NotConnected),
        }
    }

    fn take_events(&mut self) -> Option<UnboundedReceiver<PlaybackEvent>> {
        self.events.take()
    }

    fn pause(&self) {
        if let Some(playback) = &self.playback {
            playback.pause();
        }
    }

    fn resume(&self) {
        if let Some(playback) = &self.playback {
            playback.resume();
        }
    }

    fn seek(&self, position_ms: u32) {
        if let Some(playback) = &self.playback {
            playback.seek(position_ms);
        }
    }

    fn stop(&self) {
        if let Some(playback) = &self.playback {
            playback.stop();
        }
    }

    async fn shutdown(self) -> Result<(), SessionError> {
        // Consuming the player is what joins its thread; the engine drops with
        // the adapter.
        match self.playback {
            Some(playback) => playback.shutdown().await.map_err(SessionError::Playback),
            None => Ok(()),
        }
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
