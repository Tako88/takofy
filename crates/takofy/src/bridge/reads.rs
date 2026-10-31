//! The worker's reads, and the batch that runs several of them at once.
//!
//! A read only reads, so another read can run beside it. Everything that
//! writes, orders the transport or changes the session is a `Barrier`: it ends
//! admission, waits for the reads in flight, and then runs alone in the order
//! it was sent. A command that changes nothing a read sees is an `Instant` and
//! is served the moment it arrives.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use futures_util::stream::{FuturesUnordered, StreamExt};
use takofy_core::rootlist::Walked;
use takofy_model::home::{HomeAsk, StationSeed};
use takofy_model::{CoverUrl, Report, RequestId};
use takofy_ui::message::{Command, DeviceUpdate, Failure, RemoteUpdate, Update};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::oneshot::Receiver as StopReceiver;

use super::errors::{
    albums_failure, artists_failure, followed_failure, library_failure, liked_failure,
    lyrics_failure, related_failure, search_failure,
};
use super::home;
use super::log_device_error;
use super::rebuild::Step;
use super::startup::{Stage, Startup};
use super::wake::Notifier;
use crate::session::SessionHost;

/// Runs one read as the first of its own batch.
///
/// The instant parts stay the caller's, so a command served during the batch
/// changes the same loop state it would have changed on its own.
#[expect(
    clippy::too_many_arguments,
    reason = "the batch's own channels plus the instant parts"
)]
pub(super) async fn run_reads<H: SessionHost>(
    read: Read,
    host: &H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    startup: &Startup,
    parts: &mut super::InstantParts<'_>,
    commands: &mut UnboundedReceiver<Command>,
    deferred: &mut VecDeque<Command>,
) -> Step {
    run_batch(
        read,
        host,
        stop,
        notifier,
        startup,
        commands,
        deferred,
        &mut |immediate| super::serve_instant(host, notifier, parts, immediate),
    )
    .await
}

/// How many reads the batch lets run at once.
///
/// Six rather than four on the measured account: at four the places were all
/// taken while the page still had reads to send, and the cap, not the link, was
/// what the load waited on.
pub(super) const MAX_CONCURRENT_READS: usize = 6;

/// How long a batch with nothing in flight waits for the next ask before it
/// hands the loop back.
pub(super) const IDLE_GRACE: Duration = Duration::from_millis(50);

/// How one command is scheduled against the reads in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Class {
    /// Reads the session; runs beside other reads, up to the cap.
    Read,
    /// Touches nothing a read can see, so it is served at once.
    Instant,
    /// Writes, orders the transport, or changes the session: it runs alone.
    Barrier,
}

/// Names the class of one command. Total, with no wildcard, so a new command
/// fails the build until its class is decided.
pub(super) fn classify(command: &Command) -> Class {
    match command {
        Command::LoadTracks { .. }
        | Command::LoadAlbum { .. }
        | Command::LoadArtist { .. }
        | Command::LoadRelated { .. }
        | Command::LoadLyrics { .. }
        | Command::LoadShow { .. }
        | Command::LoadFollowedShows
        | Command::LoadFollowedArtists
        | Command::LoadSavedAlbums
        | Command::LoadLikedTracks
        | Command::LoadHome(_)
        | Command::StartRadio { .. }
        | Command::Search { .. }
        | Command::LoadPortraits { .. }
        | Command::LoadReleaseTracks { .. }
        | Command::LoadSaved { .. }
        | Command::LabelRemote { .. }
        | Command::LabelRows { .. }
        | Command::LabelSaved { .. } => Class::Read,
        Command::Fetch { .. }
        | Command::Report(_)
        | Command::Volume { .. }
        | Command::Answer { .. }
        | Command::Pause
        | Command::Resume
        | Command::Stop
        | Command::Seek { .. } => Class::Instant,
        Command::Play { .. }
        | Command::Steer { .. }
        | Command::Transfer { .. }
        | Command::SetFollow { .. }
        | Command::SetLiked { .. }
        | Command::SetSaved { .. }
        | Command::MoveTrack { .. }
        | Command::RemoveTracks { .. }
        | Command::AddTracks { .. }
        | Command::CreatePlaylist { .. }
        | Command::RenamePlaylist { .. }
        | Command::MovePlaylist { .. }
        | Command::RemovePlaylist { .. }
        | Command::LinkPlaylist { .. }
        | Command::WatchDevices
        | Command::RefreshDevices
        | Command::Authorize
        | Command::ReloadLibrary => Class::Barrier,
    }
}

/// Reports a command whose class and claim disagree. Two total matches that
/// name different commands is a bug here, and a command that vanishes leaves
/// the window's in-flight marker set. Only the class is named: a command can
/// carry what the user typed.
pub(super) fn log_unclaimed(class: Class) {
    log::error!("a command classified {class:?} was not claimed: dropped");
}

/// One command served the moment it arrives, carrying what it changes.
///
/// One variant per Instant command, so `serve_instant` names all eight with no
/// wildcard: a variant added here does not compile until it has a body.
pub(super) enum Immediate {
    Answer { id: RequestId, ok: bool },
    Report(Box<Option<Report>>),
    Pause,
    Resume,
    Stop,
    Seek { position_ms: u32 },
    Volume { percent: u16 },
    Fetch { url: CoverUrl },
}

impl Immediate {
    /// Claims a command that is an instant, by the payload it carries.
    pub(super) fn claim(command: Command) -> Option<Self> {
        Some(match command {
            Command::Answer { id, ok } => Self::Answer { id, ok },
            Command::Report(report) => Self::Report(Box::new(report)),
            Command::Pause => Self::Pause,
            Command::Resume => Self::Resume,
            Command::Stop => Self::Stop,
            Command::Seek { position_ms } => Self::Seek { position_ms },
            Command::Volume { percent } => Self::Volume { percent },
            Command::Fetch { url } => Self::Fetch { url },
            // `classify` names the instants, and it is the total match.
            _ => return None,
        })
    }
}

/// One read a command asked for, with the arguments it named.
pub(super) enum Read {
    Tracks {
        playlist: String,
    },
    Album {
        album: String,
    },
    Artist {
        artist: String,
    },
    Related {
        artist: String,
        uris: Vec<String>,
    },
    Lyrics {
        track: String,
    },
    Show {
        show: String,
    },
    FollowedShows,
    FollowedArtists,
    SavedAlbums,
    LikedTracks,
    Home(HomeAsk),
    Radio {
        seed: StationSeed,
    },
    Search {
        query: String,
    },
    Portraits {
        query: String,
        artists: Vec<String>,
    },
    ReleaseTracks {
        release: String,
        tracks: Vec<String>,
    },
    Saved {
        uri: String,
    },
    LabelRemote {
        track: String,
    },
    LabelRows {
        uris: Vec<String>,
    },
    LabelSaved {
        tracks: Vec<String>,
    },
    /// The playlists a walk kept, named. Not a command: the worker starts it
    /// itself once a walk has announced readiness.
    RootlistLabels {
        walked: Walked,
    },
}

impl Read {
    /// Claims a command that is a read, by the arguments it named.
    pub(super) fn claim(command: Command) -> Option<Self> {
        Some(match command {
            Command::LoadTracks { playlist } => Self::Tracks { playlist },
            Command::LoadAlbum { album } => Self::Album { album },
            Command::LoadArtist { artist } => Self::Artist { artist },
            Command::LoadRelated { artist, uris } => Self::Related { artist, uris },
            Command::LoadLyrics { track } => Self::Lyrics { track },
            Command::LoadShow { show } => Self::Show { show },
            Command::LoadFollowedShows => Self::FollowedShows,
            Command::LoadFollowedArtists => Self::FollowedArtists,
            Command::LoadSavedAlbums => Self::SavedAlbums,
            Command::LoadLikedTracks => Self::LikedTracks,
            Command::LoadHome(ask) => Self::Home(ask),
            Command::StartRadio { seed } => Self::Radio { seed },
            Command::Search { query } => Self::Search { query },
            Command::LoadPortraits { query, artists } => Self::Portraits { query, artists },
            Command::LoadReleaseTracks { release, tracks } => {
                Self::ReleaseTracks { release, tracks }
            }
            Command::LoadSaved { uri } => Self::Saved { uri },
            Command::LabelRemote { track } => Self::LabelRemote { track },
            Command::LabelRows { uris } => Self::LabelRows { uris },
            Command::LabelSaved { tracks } => Self::LabelSaved { tracks },
            // `classify` names the reads, and it is the total match: anything
            // else never reaches here.
            _ => return None,
        })
    }

    /// Performs the read and announces it. A read that does not answer sends
    /// nothing, and a batch cancelled by a stop drops it mid-call.
    pub(super) async fn run<H: SessionHost>(
        self,
        host: &H,
        notifier: &Notifier,
        startup: &Startup,
    ) {
        match self {
            Self::Tracks { playlist } => load_tracks(host, notifier, &playlist).await,
            Self::Album { album } => load_album(host, notifier, &album).await,
            Self::Artist { artist } => load_artist(host, notifier, &artist).await,
            Self::Related { artist, uris } => load_related(host, notifier, &artist, uris).await,
            Self::Lyrics { track } => load_lyrics(host, notifier, &track).await,
            Self::Show { show } => load_show(host, notifier, &show).await,
            Self::FollowedShows => {
                let began = Instant::now();
                if let Some(count) = load_followed_shows(host, notifier).await {
                    startup.record(Stage::Shows, began, count);
                }
            }
            Self::FollowedArtists => {
                let began = Instant::now();
                if let Some(count) = load_followed_artists(host, notifier).await {
                    startup.record(Stage::Artists, began, count);
                }
            }
            Self::SavedAlbums => {
                let began = Instant::now();
                if let Some(count) = load_saved_albums(host, notifier).await {
                    startup.record(Stage::Albums, began, count);
                }
            }
            Self::LikedTracks => {
                let began = Instant::now();
                if let Some(count) = load_liked_tracks(host, notifier).await {
                    startup.record(Stage::Likes, began, count);
                }
            }
            Self::Home(ask) => {
                let began = Instant::now();
                if home::load_home(host, notifier, ask).await {
                    startup.record(Stage::HomeFirst, began, 1);
                }
            }
            Self::Search { query } => load_search(host, notifier, &query).await,
            Self::Radio { seed } => load_radio(host, notifier, &seed).await,
            Self::Portraits { query, artists } => {
                load_portraits(host, notifier, &query, &artists).await;
            }
            Self::ReleaseTracks { release, tracks } => {
                load_release_tracks(host, notifier, &release, &tracks).await;
            }
            Self::Saved { uri } => load_saved(host, notifier, &uri).await,
            Self::LabelRemote { track } => label_remote(host, notifier, &track).await,
            Self::LabelRows { uris } => label_rows(host, notifier, &uris).await,
            Self::LabelSaved { tracks } => label_saved(host, notifier, &tracks).await,
            Self::RootlistLabels { walked } => {
                label_rootlist_read(host, notifier, startup, walked).await;
            }
        }
    }
}

/// Runs `first` and the reads that follow it, up to the cap, until a barrier or
/// the closed channel ends admission, or the batch sits idle long enough to
/// hand the loop back.
///
/// A read admitted while the cap is reached is not read from the channel at
/// all, so the order the window sent is kept. A barrier goes back into
/// `deferred` and the loop runs it once the reads in flight have finished.
#[expect(
    clippy::too_many_arguments,
    reason = "the batch's own channels plus the instant parts"
)]
pub(super) async fn run_batch<H: SessionHost>(
    first: Read,
    host: &H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    startup: &Startup,
    commands: &mut UnboundedReceiver<Command>,
    deferred: &mut VecDeque<Command>,
    // The closure is `Send` because the task that awaits the batch has to be.
    instant: &mut (dyn FnMut(Immediate) + Send),
) -> Step {
    let mut running = FuturesUnordered::new();
    running.push(first.run(host, notifier, startup));
    // Whether another command may be taken from the channel.
    let mut admitting = true;
    loop {
        if !admitting && running.is_empty() {
            return Step::Done;
        }
        // Decided before the select: a guard reading these would borrow what
        // `running.next()` holds.
        let has_running = !running.is_empty();
        let may_admit = admitting && running.len() < MAX_CONCURRENT_READS;
        // A window that asks one shelf per frame must have the next ask join
        // this batch, so an idle batch waits a short grace for it. A grace
        // that expires gives the loop its turn at the link poll, the account's
        // requests and the rebuild schedule.
        let idle = !has_running && admitting && deferred.is_empty();
        tokio::select! {
            biased;
            _ = &mut *stop => return Step::Stopped,
            _ = running.next(), if has_running => {}
            next = next_command(deferred, commands), if may_admit => match next {
                Some(command) => match classify(&command) {
                    Class::Read => match Read::claim(command) {
                        Some(read) => running.push(read.run(host, notifier, startup)),
                        None => log_unclaimed(Class::Read),
                    },
                    Class::Instant => match Immediate::claim(command) {
                        Some(immediate) => instant(immediate),
                        None => log_unclaimed(Class::Instant),
                    },
                    Class::Barrier => {
                        deferred.push_front(command);
                        admitting = false;
                    }
                },
                // The window is gone; the loop ends after the reads in flight.
                None => admitting = false,
            },
            _ = tokio::time::sleep(IDLE_GRACE), if idle => return Step::Done,
        }
    }
}

/// The next command to consider: what an attempt set aside first, then the
/// channel. Both are cancel-safe, so a losing select branch loses nothing.
async fn next_command(
    deferred: &mut VecDeque<Command>,
    commands: &mut UnboundedReceiver<Command>,
) -> Option<Command> {
    match deferred.pop_front() {
        Some(command) => Some(command),
        None => commands.recv().await,
    }
}

/// Sends the ready update, or the failure update with its reason collapsed for
/// the window.
pub(super) fn announce<T>(
    notifier: &Notifier,
    result: Result<T, crate::session::SessionError>,
    ready: impl FnOnce(T) -> Update,
    failed: impl FnOnce(Failure) -> Update,
) {
    match result {
        Ok(value) => notifier.send(ready(value)),
        Err(error) => notifier.send(failed(library_failure(&error))),
    };
}

/// Loads the first page of a playlist's tracks and announces it.
async fn load_tracks<H: SessionHost>(host: &H, notifier: &Notifier, playlist: &str) {
    let result = host.tracks(playlist).await;
    announce(
        notifier,
        result,
        |page| Update::Tracks {
            playlist: playlist.to_owned(),
            tracks: page.tracks,
            cover: page.cover,
            access: page.access,
        },
        |failure| Update::TracksFailed {
            playlist: playlist.to_owned(),
            failure,
        },
    );
}

/// Loads one album with its labelled tracks and announces it.
async fn load_album<H: SessionHost>(host: &H, notifier: &Notifier, album: &str) {
    let result = host.album(album).await;
    announce(notifier, result, Update::Album, |failure| {
        Update::AlbumFailed {
            album: album.to_owned(),
            failure,
        }
    });
}

/// Loads one show with its episodes and announces it.
async fn load_show<H: SessionHost>(host: &H, notifier: &Notifier, show: &str) {
    let result = host.show(show).await;
    announce(notifier, result, Update::Show, |failure| {
        Update::ShowFailed {
            show: show.to_owned(),
            failure,
        }
    });
}

/// Loads the shows the user follows and announces the whole list.
///
/// It has its own failure, because the playlists beside it draw fine and
/// "Could not load the library" would be false there. Answers the count it
/// announced, or `None` when the read failed.
async fn load_followed_shows<H: SessionHost>(host: &H, notifier: &Notifier) -> Option<usize> {
    match host.followed_shows().await {
        Ok(shows) => {
            let count = shows.len();
            notifier.send(Update::FollowedShows(shows));
            Some(count)
        }
        Err(error) => {
            notifier.send(Update::FollowedShowsFailed(followed_failure(&error)));
            None
        }
    }
}

/// Loads the artists the user follows and announces the whole list.
///
/// It has its own failure, because the playlists beside it draw fine and
/// "Could not load the library" would be false there. Answers the count it
/// announced, or `None` when the read failed.
async fn load_followed_artists<H: SessionHost>(host: &H, notifier: &Notifier) -> Option<usize> {
    match host.followed_artists().await {
        Ok(artists) => {
            let count = artists.len();
            notifier.send(Update::FollowedArtists(artists));
            Some(count)
        }
        Err(error) => {
            notifier.send(Update::FollowedArtistsFailed(artists_failure(&error)));
            None
        }
    }
}

/// Loads the albums the user saved and announces the whole list.
///
/// It has its own failure, because the playlists beside it draw fine and
/// "Could not load the library" would be false there. Answers the count it
/// announced, or `None` when the read failed.
async fn load_saved_albums<H: SessionHost>(host: &H, notifier: &Notifier) -> Option<usize> {
    match host.saved_albums().await {
        Ok(albums) => {
            let count = albums.len();
            notifier.send(Update::SavedAlbums(albums));
            Some(count)
        }
        Err(error) => {
            notifier.send(Update::SavedAlbumsFailed(albums_failure(&error)));
            None
        }
    }
}

/// Loads the tracks the user has liked and announces the whole set.
///
/// Answers the count it announced, or `None` when the read failed.
async fn load_liked_tracks<H: SessionHost>(host: &H, notifier: &Notifier) -> Option<usize> {
    match host.liked_tracks().await {
        Ok(tracks) => {
            let count = tracks.len();
            notifier.send(Update::LikedTracks(tracks));
            Some(count)
        }
        Err(error) => {
            notifier.send(Update::LikedTracksFailed(liked_failure(&error)));
            None
        }
    }
}

/// Names the walked playlists and announces the list the sidebar draws, then the
/// walk's access when it named one; a failed pass announces the failure instead.
async fn label_rootlist_read<H: SessionHost>(
    host: &H,
    notifier: &Notifier,
    startup: &Startup,
    walked: Walked,
) {
    let began = Instant::now();
    match host.label_rootlist(walked).await {
        Ok(read) => {
            startup.record(Stage::Labels, began, read.entries.len());
            notifier.send(Update::Playlists(read.entries));
            // Sent after the list, and only when the read named a revision: the
            // list is not reorderable until its own access arrives.
            if let Some(access) = read.access {
                notifier.send(Update::RootlistAccess(access));
            }
        }
        // The readiness stands: the lists it opened may still have answered.
        Err(error) => {
            notifier.send(Update::PlaylistFailed(library_failure(&error)));
        }
    }
}

/// Names the artists one artist is heard with, and announces the cards they
/// make.
async fn load_related<H: SessionHost>(
    host: &H,
    notifier: &Notifier,
    artist: &str,
    uris: Vec<String>,
) {
    match host.related(uris).await {
        Ok(cards) => notifier.send(Update::Related {
            artist: artist.to_owned(),
            cards,
        }),
        Err(error) => notifier.send(Update::RelatedFailed {
            artist: artist.to_owned(),
            failure: related_failure(&error),
        }),
    };
}

/// Loads one artist and announces the page, then the followed state when the
/// probe answered.
///
/// The two updates are separate because they are separate facts: the page is
/// drawn from the first, and the Follow label moves only on the second. A probe
/// that did not answer sends nothing, so the label stays where it was.
async fn load_artist<H: SessionHost>(host: &H, notifier: &Notifier, artist: &str) {
    match host.artist(artist).await {
        Ok(page) => {
            notifier.send(Update::Artist(page.artist));
            if let Some(followed) = page.followed {
                notifier.send(Update::Followed {
                    artist: artist.to_owned(),
                    followed,
                });
            }
        }
        Err(error) => {
            notifier.send(Update::ArtistFailed {
                artist: artist.to_owned(),
                failure: library_failure(&error),
            });
        }
    }
}

/// Loads one track's lyrics and announces them.
///
/// Its failure is `Lyrics`, not `Load`: the page around the pane may draw fine,
/// and the pane's own message names only the lyrics.
async fn load_lyrics<H: SessionHost>(host: &H, notifier: &Notifier, track: &str) {
    match host.lyrics(track).await {
        Ok(lyrics) => notifier.send(Update::Lyrics {
            track: track.to_owned(),
            lyrics,
        }),
        Err(error) => notifier.send(Update::LyricsFailed {
            track: track.to_owned(),
            failure: lyrics_failure(&error),
        }),
    };
}

/// Loads one search query's tracks and announces them.
///
/// The query is echoed verbatim. The window clears its in-flight marker on an
/// echo that matches the key it sent, so a reworded query here would strand
/// the read.
async fn load_search<H: SessionHost>(host: &H, notifier: &Notifier, query: &str) {
    match host.search(query).await {
        Ok(tracks) => notifier.send(Update::Search {
            query: query.to_owned(),
            tracks,
        }),
        Err(error) => notifier.send(Update::SearchFailed {
            query: query.to_owned(),
            failure: search_failure(&error),
        }),
    };
}

/// Reads one track's radio and announces the rows.
///
/// The seed is echoed verbatim, like a search's query, because the window drops
/// a reply for a seed it has stopped waiting on.
async fn load_radio<H: SessionHost>(host: &H, notifier: &Notifier, seed: &StationSeed) {
    match host.radio(seed).await {
        Ok(rows) => notifier.send(Update::Radio {
            seed: seed.uri.clone(),
            rows,
        }),
        Err(error) => {
            log_device_error(&error);
            notifier.send(Update::RadioFailed {
                seed: seed.uri.clone(),
            })
        }
    };
}

/// Reads one answer's artist portraits and announces them.
///
/// The query is echoed verbatim, like a search's, because the window drops a
/// reply for a query it has typed past. A read that does not answer is silent:
/// the row keeps the look of a cover that has not arrived.
async fn load_portraits<H: SessionHost>(
    host: &H,
    notifier: &Notifier,
    query: &str,
    artists: &[String],
) {
    match host.portraits(artists.to_vec()).await {
        Some(portraits) => notifier.send(Update::Portraits {
            query: query.to_owned(),
            portraits,
        }),
        None => notifier.send(Update::PortraitsFailed {
            query: query.to_owned(),
        }),
    };
}

/// Labels one release's tracks and announces them.
async fn load_release_tracks<H: SessionHost>(
    host: &H,
    notifier: &Notifier,
    release: &str,
    tracks: &[String],
) {
    match host.release_tracks(tracks).await {
        Ok(tracks) => notifier.send(Update::ReleaseTracks {
            release: release.to_owned(),
            tracks,
        }),
        Err(error) => notifier.send(Update::ReleaseTracksFailed {
            release: release.to_owned(),
            failure: library_failure(&error),
        }),
    };
}

/// Asks whether one album or show is in the library, and announces the answer.
///
/// A probe that does not answer is announced as unknown rather than as "not
/// saved": the heart then stays inert instead of claiming the item is not there.
async fn load_saved<H: SessionHost>(host: &H, notifier: &Notifier, uri: &str) {
    let saved = host.saved_state(uri).await;
    notifier.send(Update::SavedState {
        uri: uri.to_owned(),
        saved,
    });
}

/// Names the track another device is playing, when the label read answers.
///
/// A miss is silent: the title stays `None` and the caption still draws.
async fn label_remote<H: SessionHost>(host: &H, notifier: &Notifier, track: &str) {
    let tracks = [track.to_owned()];
    match host.release_tracks(&tracks).await {
        Ok(named) => {
            if let Some(entry) = named.into_iter().next() {
                notifier.send(Update::Devices(DeviceUpdate::Labelled(entry)));
            }
        }
        Err(error) => log_device_error(&error),
    }
}

/// Names the rows a hand-over adopted, in one read.
///
/// A miss is silent: the rows keep their URIs and the bar still draws.
async fn label_rows<H: SessionHost>(host: &H, notifier: &Notifier, uris: &[String]) {
    match host.release_tracks(uris).await {
        Ok(named) => {
            if !named.is_empty() {
                notifier.send(Update::Remote(RemoteUpdate::Labelled(named)));
            }
        }
        Err(error) => log_device_error(&error),
    }
}

/// Names the liked songs Home's shelf is about to draw.
///
/// The reply is always sent, even when the read fails or names nothing: Home
/// waits for every answer before it draws. A miss is an answer the shelf cannot
/// draw from; a failure is told apart from it so the page can offer the retry.
async fn label_saved<H: SessionHost>(host: &H, notifier: &Notifier, tracks: &[String]) {
    let asked = tracks.to_vec();
    match host.release_tracks(tracks).await {
        Ok(named) => notifier.send(Update::SavedLabelled { asked, named }),
        Err(error) => {
            log_device_error(&error);
            // The labels have no surface of their own, so their failure is the
            // library's load, as a failed liked read is.
            notifier.send(Update::SavedLabelFailed {
                asked,
                failure: Failure::Load,
            })
        }
    };
}
