//! The rendezvous between the window and the worker: updates out, commands in.
//!
//! The window owns only what it draws. The worker owns the blocking calls, and
//! teardown finishes it before the runtime is dropped. Blocking work runs on
//! plain threads rather than the runtime: a call parked on a browser callback
//! must not hold up runtime shutdown, and a detached thread dies with the
//! process.
//!
//! The command loop and the per-command helpers stay here; the jobs below have
//! their own module:
//!
//! - [`wake`]: the waker slot, the notifier, and both ends of the loop.
//! - [`signin`]: starts the sign-in worker and turns an auth result into an
//!   update.
//! - [`playback`]: starts the player, loads a track, and drains its events.
//! - [`rebuild`]: settles a failed attempt and times the next one.
//! - [`orders`]: hands a transport order to the window and reads its answer.
//! - [`devices`]: keeps the device watch and re-arms it after a rebuild.
//! - [`covers`]: answers cover requests, and the drains a session feeds.
//! - [`errors`]: maps a session error onto the failure the window can name.

mod covers;
mod devices;
mod errors;
mod orders;
mod playback;
mod rebuild;
mod signin;
mod wake;

pub(crate) use orders::LANDING_LOG_TARGET;
pub(crate) use wake::{Notifier, UiEnd};

use covers::{Drains, handle_fetch, reap_finished_covers};
use devices::{Devices, adopt_watch, arm_watch};
use errors::{
    albums_failure, artists_failure, ends_playability, followed_failure, library_failure,
    liked_failure, lyrics_failure, playback_failure, related_failure, search_failure,
};
use orders::{deliver_answer, run_ordered, serve_order};
use playback::{ensure_drain, interrupts_attempt, play, start_load};
use rebuild::{
    Rebuilt, Refresh, Step, link_signal_applies, settle, silence_session, start_refresh,
};
use signin::{resume_update, spawn_authorize, start};
use wake::{WakeSlot, Worker};

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use takofy_core::Resolution;
use takofy_core::connect::{ClusterEvent, Steer as CoreSteer};
use takofy_core::credentials::{AccessToken, CredentialStore};
use takofy_core::instruction::Instruction;
use takofy_core::login::{TokenSource, resume};
use takofy_core::receive::Inbound;
use takofy_model::{CoverUrl, Move, Owner, PlaylistEntry, Removal, RequestId, Revision};
use takofy_ui::message::{
    Command, DeviceUpdate, Failure, RemoteUpdate, Steer, TransportUpdate, Update,
};
use tokio::runtime::{Handle, Runtime};
use tokio::sync::Semaphore;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::oneshot;
use tokio::sync::oneshot::Receiver as StopReceiver;
use tokio::task::JoinSet;

use crate::cover::{FETCH_TIMEOUT, MAX_CONCURRENT_COVERS};
use crate::devices::SentRoster;
use crate::playhead::ResumePoint;
use crate::recovery::{Attempt, Change, Pacing, Recovery};
use crate::session::{Retired, SessionError, SessionHost, stop_or};

/// What the task received from the select, once the borrow on `stop` is gone.
enum Event {
    /// An access token, or `None` once every sender has gone.
    Token(Option<AccessToken>),
    /// A command, or `None` once the window has gone.
    Command(Option<Command>),
    /// A request from the account, or `None` once that stream has ended.
    Inbound(Option<Inbound>),
    /// The once-a-second look at whether the session is still alive.
    LinkPoll,
    /// The rebuild schedule fell due.
    Retry,
    /// A playback drain judged a failure to be the link, naming its generation;
    /// `None` once the signal channel has closed.
    LinkSignal(Option<u64>),
}

/// Waits for the account's next request, forgetting a stream that has ended.
///
/// A stream that closed leaves no receiver behind, so this arm never resolves
/// again; leaving it armed on a dead channel would spin the loop. Cancelling the
/// await loses nothing, because the receiver is polled in place rather than
/// rebuilt.
async fn next_inbound(slot: &mut Option<UnboundedReceiver<Inbound>>) -> Option<Inbound> {
    let Some(receiver) = slot else {
        return std::future::pending().await;
    };
    let inbound = receiver.recv().await;
    if inbound.is_none() {
        *slot = None;
    }
    inbound
}

/// Starts the worker with an explicit outer withdraw bound and rebuild
/// schedule.
fn start_with<H: SessionHost, S: TokenSource + Send + Sync + 'static>(
    handle: &Handle,
    host: H,
    store: Arc<dyn CredentialStore>,
    source: Arc<S>,
    withdraw_timeout: Duration,
    pacing: Pacing,
) -> (UiEnd, WakeSlot, Worker) {
    let (updates_tx, updates_rx) = channel();
    let (commands_tx, mut commands_rx) = tokio::sync::mpsc::unbounded_channel();
    let (tokens_tx, mut tokens_rx) = tokio::sync::mpsc::unbounded_channel();
    let (stop_tx, mut stop_rx) = oneshot::channel();
    // A playback drain names the generation whose failure it judged, so a
    // signal from a retired drain cannot start a rebuild for the live one.
    let (signal_tx, mut signal_rx) = tokio::sync::mpsc::unbounded_channel();
    let slot = WakeSlot::default();
    let notifier = Notifier {
        tx: updates_tx,
        slot: slot.clone(),
    };

    {
        let notifier = notifier.clone();
        let tokens = tokens_tx.clone();
        let store = Arc::clone(&store);
        let source = Arc::clone(&source);
        std::thread::spawn(move || {
            let result = resume(store.as_ref(), source.as_ref());
            if let Ok(Some(token)) = &result
                && tokens.send(token.clone()).is_err()
            {
                return;
            }
            send_update(&notifier, resume_update(result));
        });
    }

    let task_notifier = notifier.clone();
    let task = handle.spawn(async move {
        let mut host = host;
        let mut connected = false;
        let mut devices = Devices::default();
        let mut token: Option<AccessToken> = None;
        let mut recovery = Recovery::new(pacing);
        let mut drains = Drains::new(signal_tx);
        // Whether an engine is up. A dead engine is still up until it is
        // retired, and a retired one is gone even though the account is signed
        // in.
        let mut engine_up = false;
        // Whether the window already carries the stuck words, so the schedule
        // says them once.
        let mut stalled = false;
        // Whether the window is still showing the loss, so it can be settled
        // when the rebuild finds nothing left to reload.
        let mut reconnecting = false;
        // Commands that arrived during an attempt and have no say in it, kept
        // in order until the attempt ends.
        let mut deferred: VecDeque<Command> = VecDeque::new();
        // The one refresh that may be in flight, kept so a cancelled attempt
        // does not start a second exchange with the rotating token.
        let mut refresh: Option<Refresh> = None;
        let mut poll = tokio::time::interval(pacing.poll);
        poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Built once: the timeout, the redirect ban, and the bounded pool live
        // on the client, and covers are the only thing it is ever used for.
        // Redirects are refused so an allowlisted host cannot hand the fetch to
        // a host the allowlist never approved; a 3xx becomes a fetch error.
        let cover_client = match reqwest::Client::builder()
            .timeout(FETCH_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .pool_max_idle_per_host(2)
            .build()
        {
            Ok(client) => Some(client),
            Err(error) => {
                // Without a client no fetch can be served; the request handler
                // answers each one instead of leaving it hanging.
                log::warn!("cover client could not be built: {error}");
                None
            }
        };
        let cover_permits = Arc::new(Semaphore::new(MAX_CONCURRENT_COVERS));
        let cover_in_flight: Arc<Mutex<HashSet<CoverUrl>>> = Arc::new(Mutex::new(HashSet::new()));
        let mut cover_tasks = JoinSet::new();
        loop {
            reap_finished_covers(&mut cover_tasks);
            // A command an attempt set aside, the interrupting one included,
            // is run here in the order it was sent.
            let event = match deferred
                .pop_front()
                .map(|command| Event::Command(Some(command)))
            {
                Some(event) => event,
                None => tokio::select! {
                    biased;
                    _ = &mut stop_rx => break,
                    received = tokens_rx.recv() => Event::Token(received),
                    command = commands_rx.recv() => Event::Command(command),
                    request = next_inbound(&mut devices.inbound) => Event::Inbound(request),
                    _ = poll.tick() => Event::LinkPoll,
                    _ = sleep_until(recovery.wake_at()) => Event::Retry,
                    generation = signal_rx.recv(), if !signal_rx.is_closed() => {
                        Event::LinkSignal(generation)
                    }
                },
            };
            match event {
                Event::Token(Some(received)) => {
                    if connected {
                        // The first token opened the session; a second cannot
                        // improve it.
                        continue;
                    }
                    token = Some(received);
                    let step = load_library(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        token.as_ref(),
                        &mut connected,
                    )
                    .await;
                    if connected {
                        engine_up = true;
                    }
                    if step.stopped() {
                        break;
                    }
                }
                Event::Token(None) => {}
                Event::LinkPoll => {
                    let down = host.is_down();
                    drains.playhead.set_down(down);
                    if down && !recovery.is_active() {
                        let step = lose_link(
                            &mut host,
                            &mut stop_rx,
                            &task_notifier,
                            &mut devices,
                            &drains,
                            &mut recovery,
                            &mut engine_up,
                            &mut reconnecting,
                        )
                        .await;
                        if step.stopped() {
                            break;
                        }
                    }
                }
                Event::LinkSignal(generation) => {
                    let live = generation.is_some_and(|generation| {
                        link_signal_applies(generation, &drains.playhead, &recovery)
                    });
                    if live {
                        let step = lose_link(
                            &mut host,
                            &mut stop_rx,
                            &task_notifier,
                            &mut devices,
                            &drains,
                            &mut recovery,
                            &mut engine_up,
                            &mut reconnecting,
                        )
                        .await;
                        if step.stopped() {
                            break;
                        }
                    }
                }
                Event::Retry => {
                    if !recovery.due(Instant::now()) {
                        continue;
                    }
                    let point = recovery.point().cloned();
                    let had_point = point.is_some();
                    let outcome = rebuild_attempt(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        &mut devices,
                        &mut drains,
                        &mut commands_rx,
                        &mut deferred,
                        &mut refresh,
                        &mut token,
                        &mut engine_up,
                        &store,
                        &source,
                        point,
                        pacing.attempt,
                    )
                    .await;
                    let stalled_now = settle(&mut recovery, &outcome, Instant::now());
                    if stalled_now == Some(Attempt::Stalled) && !stalled {
                        stalled = true;
                        task_notifier.send(Update::Transport(TransportUpdate::Failed(
                            Failure::Reconnect,
                        )));
                    }
                    match outcome {
                        Rebuilt::Done => {
                            // Nothing is coming back to settle the transport, so
                            // whatever the window was last told about the loss
                            // ends here.
                            if !had_point && (reconnecting || stalled) {
                                task_notifier.send(Update::Transport(TransportUpdate::Stopped));
                            }
                            stalled = false;
                            reconnecting = false;
                        }
                        Rebuilt::Failed => {}
                        // What ended it is already queued behind the commands
                        // the attempt set aside, so the loop runs it next.
                        Rebuilt::Interrupted => {}
                        Rebuilt::Stopped => break,
                    }
                }
                Event::Command(Some(Command::Authorize)) => {
                    spawn_authorize(
                        task_notifier.clone(),
                        tokens_tx.clone(),
                        Arc::clone(&store),
                        Arc::clone(&source),
                    );
                }
                Event::Command(Some(Command::ReloadLibrary)) => {
                    let signed_in = connected;
                    let step = load_library(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        token.as_ref(),
                        &mut connected,
                    )
                    .await;
                    if connected && !signed_in {
                        engine_up = true;
                    }
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadTracks { playlist })) => {
                    let step =
                        load_tracks(&mut host, &mut stop_rx, &task_notifier, &playlist).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadAlbum { album })) => {
                    let step = load_album(&mut host, &mut stop_rx, &task_notifier, &album).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadArtist { artist })) => {
                    let step = load_artist(&mut host, &mut stop_rx, &task_notifier, &artist).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadRelated { artist, uris })) => {
                    let step =
                        load_related(&mut host, &mut stop_rx, &task_notifier, &artist, uris).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadLyrics { track })) => {
                    let step = load_lyrics(&mut host, &mut stop_rx, &task_notifier, &track).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadShow { show })) => {
                    let step = load_show(&mut host, &mut stop_rx, &task_notifier, &show).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadFollowedShows)) => {
                    let step = load_followed_shows(&mut host, &mut stop_rx, &task_notifier).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadFollowedArtists)) => {
                    let step = load_followed_artists(&mut host, &mut stop_rx, &task_notifier).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadSavedAlbums)) => {
                    let step = load_saved_albums(&mut host, &mut stop_rx, &task_notifier).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadLikedTracks)) => {
                    let step = load_liked_tracks(&mut host, &mut stop_rx, &task_notifier).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::SetLiked { tracks, liked })) => {
                    let step =
                        set_liked(&mut host, &mut stop_rx, &task_notifier, &tracks, liked).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::Search { query })) => {
                    let step = load_search(&mut host, &mut stop_rx, &task_notifier, &query).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadPortraits { query, artists })) => {
                    let step =
                        load_portraits(&mut host, &mut stop_rx, &task_notifier, &query, &artists)
                            .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadReleaseTracks { release, tracks })) => {
                    let step = load_release_tracks(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        &release,
                        &tracks,
                    )
                    .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::SetFollow { artist, followed })) => {
                    let step =
                        set_follow(&mut host, &mut stop_rx, &task_notifier, &artist, followed)
                            .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::MoveTrack {
                    playlist,
                    revision,
                    moves,
                })) => {
                    let step = move_track(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        &playlist,
                        &revision,
                        &moves,
                    )
                    .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::RemoveTracks {
                    playlist,
                    revision,
                    removals,
                })) => {
                    let step = remove_tracks(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        &playlist,
                        &revision,
                        &removals,
                    )
                    .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::MovePlaylist { revision, from, to })) => {
                    let step =
                        move_playlist(&mut host, &mut stop_rx, &task_notifier, &revision, from, to)
                            .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::RemovePlaylist {
                    playlist,
                    revision,
                    index,
                })) => {
                    let step = remove_playlist(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        &playlist,
                        &revision,
                        index,
                    )
                    .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LinkPlaylist { playlist, revision })) => {
                    let step = link_playlist(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        &playlist,
                        &revision,
                    )
                    .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LoadSaved { uri })) => {
                    let step = load_saved(&mut host, &mut stop_rx, &task_notifier, &uri).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::SetSaved { uri, saved })) => {
                    let step =
                        set_saved(&mut host, &mut stop_rx, &task_notifier, &uri, saved).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::AddTracks {
                    playlist,
                    revision,
                    from_index,
                    tracks,
                })) => {
                    let step = add_tracks(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        &playlist,
                        &revision,
                        from_index,
                        &tracks,
                    )
                    .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::RenamePlaylist { playlist, name })) => {
                    let step =
                        rename_playlist(&mut host, &mut stop_rx, &task_notifier, &playlist, &name)
                            .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::CreatePlaylist {
                    name,
                    revision,
                    tracks,
                })) => {
                    let step = create_playlist(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        &name,
                        &revision,
                        &tracks,
                    )
                    .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::WatchDevices)) => {
                    let step = watch_devices(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        &mut devices,
                        &drains,
                        connected,
                    )
                    .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::RefreshDevices)) => {
                    let step = refresh_devices(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        &mut devices,
                        &drains,
                        connected,
                    )
                    .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::Steer { device, steer })) => {
                    let step = steer_device(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        &device,
                        core_steer(steer),
                        connected,
                    )
                    .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::Transfer { to })) => {
                    let step =
                        transfer_device(&mut host, &mut stop_rx, &task_notifier, &to, connected)
                            .await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LabelRemote { track })) => {
                    let step = label_remote(&mut host, &mut stop_rx, &task_notifier, &track).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LabelRows { uris })) => {
                    let step = label_rows(&mut host, &mut stop_rx, &task_notifier, &uris).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::LabelSaved { tracks })) => {
                    let step = label_saved(&mut host, &mut stop_rx, &task_notifier, &tracks).await;
                    if step.stopped() {
                        break;
                    }
                }
                Event::Command(Some(Command::Play { uri })) => {
                    let step = play_now(
                        &mut host,
                        &mut stop_rx,
                        &task_notifier,
                        &mut devices,
                        &drains,
                        &mut recovery,
                        &mut engine_up,
                        &mut reconnecting,
                        &uri,
                    )
                    .await;
                    if step.stopped() {
                        break;
                    }
                }
                // The window applied an order, or could not. An entry the budget
                // already answered is gone: the phone has been told `Failure`,
                // and the next report put corrects what it shows.
                Event::Command(Some(Command::Answer { id, ok })) => {
                    deliver_answer(host.answer_order(id, ok));
                }
                // A report only has a wire to reach while the device is on the
                // account; before that the engine's own slot is not announced.
                Event::Command(Some(Command::Report(report))) => {
                    if devices.watching {
                        host.report(report);
                    }
                }
                Event::Command(Some(Command::Pause)) => {
                    if recovery.is_active() {
                        recovery.adjust(Change::Play(false));
                    } else {
                        host.pause();
                    }
                }
                Event::Command(Some(Command::Resume)) => {
                    if recovery.is_active() {
                        recovery.adjust(Change::Play(true));
                    } else {
                        host.resume();
                    }
                }
                Event::Command(Some(Command::Stop)) => {
                    if recovery.is_active() {
                        recovery.adjust(Change::Clear);
                    } else {
                        host.stop();
                    }
                }
                Event::Command(Some(Command::Seek { position_ms })) => {
                    if recovery.is_active() {
                        recovery.adjust(Change::Seek(position_ms));
                    } else {
                        host.seek(position_ms);
                    }
                }
                Event::Command(Some(Command::Volume { percent })) => host.set_volume(percent),
                // The arm only spawns: the fetch must not park the command loop
                // that also awaits host calls. A signed-out window has no cover
                // to want, and every request is answered either way.
                Event::Command(Some(Command::Fetch { url })) => {
                    handle_fetch(
                        connected,
                        cover_client.as_ref(),
                        &mut cover_tasks,
                        Arc::clone(&cover_permits),
                        Arc::clone(&cover_in_flight),
                        url,
                        task_notifier.clone(),
                    );
                }
                Event::Command(None) => break,
                // Every request is answered. A hand-over is served in full, a
                // transport order is handed to the window that owns the sound,
                // and the rest are refused at once, so the phone is never left
                // waiting out its budget.
                Event::Inbound(Some(request)) => {
                    log::info!("connect inbound: {}", request.kind.word());
                    let step = match request.instruction {
                        Instruction::Order(order) => {
                            serve_order(&host, &task_notifier, request.id, request.kind, order)
                        }
                        instruction => {
                            serve_landing(
                                &mut host,
                                &mut stop_rx,
                                &task_notifier,
                                &drains,
                                request.id,
                                instruction,
                            )
                            .await
                        }
                    };
                    if step.stopped() {
                        break;
                    }
                }
                Event::Inbound(None) => {}
            }
        }
        // Cancels every in-flight fetch. A detached decode thread is left to
        // finish and its result is dropped with the oneshot receiver.
        cover_tasks.abort_all();
        // The device goes before the player does, so a forced quit never leaves
        // the account with an entry this client will not answer on.
        if devices.watching {
            withdraw(&mut host, withdraw_timeout).await;
        }
        let _ = host.shutdown().await;
    });

    (
        UiEnd {
            updates: updates_rx,
            commands: commands_tx,
        },
        slot,
        Worker {
            stop: stop_tx,
            task,
        },
    )
}

/// Connects if needed and lists the rootlist, announcing the outcome.
///
/// `Stopped` means the window closed while the call was in flight, so the
/// caller must leave the loop and run the shutdown.
async fn load_library<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    token: Option<&AccessToken>,
    connected: &mut bool,
) -> Step {
    if !*connected {
        let Some(token) = token else {
            // A reload with nothing to connect with; a retry can only work once
            // sign-in has delivered a token.
            notifier.send(Update::PlaylistFailed(library_failure(
                &SessionError::NotConnected,
            )));
            return Step::Done;
        };
        match stop_or(stop, host.connect(token)).await {
            None => return Step::Stopped,
            Some(Ok(())) => *connected = true,
            Some(Err(error)) => {
                notifier.send(Update::PlaylistFailed(library_failure(&error)));
                return Step::Done;
            }
        }
    }

    match stop_or(stop, host.rootlist()).await {
        None => Step::Stopped,
        Some(Ok(read)) => {
            notifier.send(Update::Playlists(read.entries));
            // Sent after the list, and only when the read named a revision: the
            // list is not reorderable until its own access arrives.
            if let Some(access) = read.access {
                notifier.send(Update::RootlistAccess(access));
            }
            Step::Done
        }
        Some(Err(error)) => {
            notifier.send(Update::PlaylistFailed(library_failure(&error)));
            Step::Done
        }
    }
}

/// Sends the ready update, or the failure update with its reason collapsed for
/// the window.
pub(crate) fn announce<T>(
    notifier: &Notifier,
    result: Result<T, SessionError>,
    ready: impl FnOnce(T) -> Update,
    failed: impl FnOnce(Failure) -> Update,
) {
    match result {
        Ok(value) => notifier.send(ready(value)),
        Err(error) => notifier.send(failed(library_failure(&error))),
    };
}

/// Loads the first page of a playlist's tracks and announces it.
async fn load_tracks<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    playlist: &str,
) -> Step {
    match stop_or(stop, host.tracks(playlist)).await {
        None => Step::Stopped,
        Some(result) => {
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
            Step::Done
        }
    }
}

/// Loads one album with its labelled tracks and announces it.
async fn load_album<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    album: &str,
) -> Step {
    match stop_or(stop, host.album(album)).await {
        None => Step::Stopped,
        Some(result) => {
            announce(notifier, result, Update::Album, |failure| {
                Update::AlbumFailed {
                    album: album.to_owned(),
                    failure,
                }
            });
            Step::Done
        }
    }
}

/// Loads one show with its episodes and announces it.
async fn load_show<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    show: &str,
) -> Step {
    match stop_or(stop, host.show(show)).await {
        None => Step::Stopped,
        Some(result) => {
            announce(notifier, result, Update::Show, |failure| {
                Update::ShowFailed {
                    show: show.to_owned(),
                    failure,
                }
            });
            Step::Done
        }
    }
}

/// Loads the shows the user follows and announces the whole list.
///
/// It has its own failure, because the playlists beside it draw fine and
/// "Could not load the library" would be false there.
async fn load_followed_shows<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
) -> Step {
    match stop_or(stop, host.followed_shows()).await {
        None => Step::Stopped,
        Some(Ok(shows)) => {
            notifier.send(Update::FollowedShows(shows));
            Step::Done
        }
        Some(Err(error)) => {
            notifier.send(Update::FollowedShowsFailed(followed_failure(&error)));
            Step::Done
        }
    }
}

/// Loads the artists the user follows and announces the whole list.
///
/// It has its own failure, because the playlists beside it draw fine and
/// "Could not load the library" would be false there.
async fn load_followed_artists<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
) -> Step {
    match stop_or(stop, host.followed_artists()).await {
        None => Step::Stopped,
        Some(Ok(artists)) => {
            notifier.send(Update::FollowedArtists(artists));
            Step::Done
        }
        Some(Err(error)) => {
            notifier.send(Update::FollowedArtistsFailed(artists_failure(&error)));
            Step::Done
        }
    }
}

/// Loads the albums the user saved and announces the whole list.
///
/// It has its own failure, because the playlists beside it draw fine and
/// "Could not load the library" would be false there.
async fn load_saved_albums<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
) -> Step {
    match stop_or(stop, host.saved_albums()).await {
        None => Step::Stopped,
        Some(Ok(albums)) => {
            notifier.send(Update::SavedAlbums(albums));
            Step::Done
        }
        Some(Err(error)) => {
            notifier.send(Update::SavedAlbumsFailed(albums_failure(&error)));
            Step::Done
        }
    }
}

/// Loads the tracks the user has liked and announces the whole set.
///
async fn load_liked_tracks<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
) -> Step {
    match stop_or(stop, host.liked_tracks()).await {
        None => Step::Stopped,
        Some(Ok(tracks)) => {
            notifier.send(Update::LikedTracks(tracks));
            Step::Done
        }
        Some(Err(error)) => {
            notifier.send(Update::LikedTracksFailed(liked_failure(&error)));
            Step::Done
        }
    }
}

/// Writes the liked set and announces what was requested.
///
async fn set_liked<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    tracks: &[String],
    liked: bool,
) -> Step {
    match stop_or(stop, host.set_liked(tracks.to_vec(), liked)).await {
        None => Step::Stopped,
        Some(Ok(())) => {
            notifier.send(Update::Liked {
                tracks: tracks.to_vec(),
                liked,
            });
            Step::Done
        }
        Some(Err(error)) => {
            log::warn!("like write failed: {}", error.kind());
            notifier.send(Update::LikeFailed {
                tracks: tracks.to_vec(),
                failure: Failure::Save,
            });
            Step::Done
        }
    }
}

/// Loads one artist and announces the page, then the followed state when the
/// probe answered.
///
/// The two updates are separate because they are separate facts: the page is
/// drawn from the first, and the Follow label moves only on the second. A probe
/// that did not answer sends nothing, so the label stays where it was.
/// Names the artists one artist is heard with, and announces the cards they make.
async fn load_related<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    artist: &str,
    uris: Vec<String>,
) -> Step {
    match stop_or(stop, host.related(uris)).await {
        None => Step::Stopped,
        Some(Ok(cards)) => {
            notifier.send(Update::Related {
                artist: artist.to_owned(),
                cards,
            });
            Step::Done
        }
        Some(Err(error)) => {
            notifier.send(Update::RelatedFailed {
                artist: artist.to_owned(),
                failure: related_failure(&error),
            });
            Step::Done
        }
    }
}

async fn load_artist<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    artist: &str,
) -> Step {
    match stop_or(stop, host.artist(artist)).await {
        None => Step::Stopped,
        Some(Ok(page)) => {
            notifier.send(Update::Artist(page.artist));
            if let Some(followed) = page.followed {
                notifier.send(Update::Followed {
                    artist: artist.to_owned(),
                    followed,
                });
            }
            Step::Done
        }
        Some(Err(error)) => {
            notifier.send(Update::ArtistFailed {
                artist: artist.to_owned(),
                failure: library_failure(&error),
            });
            Step::Done
        }
    }
}

/// Loads one track's lyrics and announces them.
///
/// Its failure is `Lyrics`, not `Load`: the page around the pane may draw fine,
/// and the pane's own message names only the lyrics.
async fn load_lyrics<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    track: &str,
) -> Step {
    match stop_or(stop, host.lyrics(track)).await {
        None => Step::Stopped,
        Some(Ok(lyrics)) => {
            notifier.send(Update::Lyrics {
                track: track.to_owned(),
                lyrics,
            });
            Step::Done
        }
        Some(Err(error)) => {
            notifier.send(Update::LyricsFailed {
                track: track.to_owned(),
                failure: lyrics_failure(&error),
            });
            Step::Done
        }
    }
}

/// Loads one search query's tracks and announces them.
///
/// The query is echoed verbatim. The window clears its in-flight marker on an
/// echo that matches the key it sent, so a reworded query here would strand
/// the read.
async fn load_search<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    query: &str,
) -> Step {
    match stop_or(stop, host.search(query)).await {
        None => Step::Stopped,
        Some(Ok(tracks)) => {
            notifier.send(Update::Search {
                query: query.to_owned(),
                tracks,
            });
            Step::Done
        }
        Some(Err(error)) => {
            notifier.send(Update::SearchFailed {
                query: query.to_owned(),
                failure: search_failure(&error),
            });
            Step::Done
        }
    }
}

/// Reads one answer's artist portraits and announces them.
///
/// The query is echoed verbatim, like a search's, because the window drops a
/// reply for a query it has typed past. A read that does not answer is silent:
/// the row keeps the look of a cover that has not arrived.
async fn load_portraits<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    query: &str,
    artists: &[String],
) -> Step {
    match stop_or(stop, host.portraits(artists.to_vec())).await {
        None => Step::Stopped,
        Some(Some(portraits)) => {
            notifier.send(Update::Portraits {
                query: query.to_owned(),
                portraits,
            });
            Step::Done
        }
        Some(None) => {
            notifier.send(Update::PortraitsFailed {
                query: query.to_owned(),
            });
            Step::Done
        }
    }
}

/// Labels one release's tracks and announces them.
async fn load_release_tracks<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    release: &str,
    tracks: &[String],
) -> Step {
    match stop_or(stop, host.release_tracks(tracks)).await {
        None => Step::Stopped,
        Some(Ok(tracks)) => {
            notifier.send(Update::ReleaseTracks {
                release: release.to_owned(),
                tracks,
            });
            Step::Done
        }
        Some(Err(error)) => {
            notifier.send(Update::ReleaseTracksFailed {
                release: release.to_owned(),
                failure: library_failure(&error),
            });
            Step::Done
        }
    }
}

/// Writes the followed state and announces what the server accepted.
///
/// The announced value is the one that was requested, because the write either
/// landed or it did not: there is no partial follow. A failure is `Save`, not
/// `Load`, because the user asked for this and it did not stick.
async fn set_follow<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    artist: &str,
    followed: bool,
) -> Step {
    match stop_or(stop, host.set_follow(artist, followed)).await {
        None => Step::Stopped,
        Some(Ok(())) => {
            notifier.send(Update::Followed {
                artist: artist.to_owned(),
                followed,
            });
            Step::Done
        }
        Some(Err(error)) => {
            log::warn!("follow write failed: {}", error.kind());
            notifier.send(Update::FollowFailed {
                artist: artist.to_owned(),
                failure: Failure::Save,
            });
            Step::Done
        }
    }
}

/// Writes one move and announces the revision its reply names.
///
/// The announced revision is the one the next move must be based on. A failure
/// is `Save`, not `Load`, because the user asked for this and it did not stick.
async fn move_track<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    playlist: &str,
    revision: &Revision,
    moves: &[Move],
) -> Step {
    match stop_or(stop, host.move_track(playlist, revision, moves)).await {
        None => Step::Stopped,
        Some(Ok(revision)) => {
            notifier.send(Update::TrackMoved {
                playlist: playlist.to_owned(),
                revision,
            });
            Step::Done
        }
        Some(Err(error)) => {
            log::warn!("move write failed: {}", error.kind());
            notifier.send(Update::MoveFailed {
                playlist: playlist.to_owned(),
                failure: Failure::Save,
            });
            Step::Done
        }
    }
}

/// Removes a playlist's rows and announces the revision its reply names.
///
/// A failure is `Save`, not `Load`, because the user asked for this and it did
/// not stick.
async fn remove_tracks<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    playlist: &str,
    revision: &Revision,
    removals: &[Removal],
) -> Step {
    match stop_or(stop, host.remove_tracks(playlist, revision, removals)).await {
        None => Step::Stopped,
        Some(Ok(revision)) => {
            notifier.send(Update::TracksRemoved {
                playlist: playlist.to_owned(),
                revision,
            });
            Step::Done
        }
        Some(Err(error)) => {
            log::warn!("remove write failed: {}", error.kind());
            notifier.send(Update::RemoveFailed {
                playlist: playlist.to_owned(),
                failure: Failure::Save,
            });
            Step::Done
        }
    }
}

/// Writes one move to the sidebar's playlists and announces the reply revision.
async fn move_playlist<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    revision: &Revision,
    from: usize,
    to: usize,
) -> Step {
    match stop_or(stop, host.move_playlist(revision, from, to)).await {
        None => Step::Stopped,
        Some(Ok(revision)) => {
            notifier.send(Update::PlaylistMoved { revision });
            Step::Done
        }
        Some(Err(error)) => {
            log::warn!("playlist move failed: {}", error.kind());
            notifier.send(Update::PlaylistMoveFailed);
            Step::Done
        }
    }
}

/// Unlinks one rootlist row and announces the revision its reply names.
async fn remove_playlist<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    playlist: &str,
    revision: &Revision,
    index: usize,
) -> Step {
    match stop_or(stop, host.remove_playlist(revision, index)).await {
        None => Step::Stopped,
        Some(Ok(revision)) => {
            notifier.send(Update::PlaylistRemoved {
                playlist: playlist.to_owned(),
                revision,
            });
            Step::Done
        }
        Some(Err(error)) => {
            log::warn!("playlist removal failed: {}", error.kind());
            notifier.send(Update::PlaylistRemoveFailed {
                playlist: playlist.to_owned(),
            });
            Step::Done
        }
    }
}

/// Puts one playlist at the front of the rootlist and announces the reply.
///
/// The link answers the revision everything after it is based on, so the window
/// keeps that rather than the one it sent.
async fn link_playlist<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    playlist: &str,
    revision: &Revision,
) -> Step {
    match stop_or(stop, host.link_playlist(revision, playlist)).await {
        None => Step::Stopped,
        Some(Ok(revision)) => {
            notifier.send(Update::PlaylistLinked {
                playlist: playlist.to_owned(),
                revision,
            });
            Step::Done
        }
        Some(Err(error)) => {
            log::warn!("playlist link failed: {}", error.kind());
            notifier.send(Update::PlaylistLinkFailed {
                playlist: playlist.to_owned(),
            });
            Step::Done
        }
    }
}

/// Asks whether one album or show is in the library, and announces the answer.
///
/// A probe that does not answer is announced as unknown rather than as "not
/// saved": the heart then stays inert instead of claiming the item is not there.
async fn load_saved<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    uri: &str,
) -> Step {
    let Some(answer) = stop_or(stop, host.saved_state(uri)).await else {
        return Step::Stopped;
    };
    notifier.send(Update::SavedState {
        uri: uri.to_owned(),
        saved: answer,
    });
    Step::Done
}

/// Writes one album or show in or out, and echoes the URI it was asked for.
///
/// The reply carries no body, so the echo is the whole confirmation the window
/// gets until its next read.
async fn set_saved<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    uri: &str,
    saved: bool,
) -> Step {
    match stop_or(stop, host.set_saved(uri, saved)).await {
        None => Step::Stopped,
        Some(Ok(())) => {
            notifier.send(Update::Saved {
                uri: uri.to_owned(),
                saved,
            });
            Step::Done
        }
        Some(Err(error)) => {
            log::warn!("saved write failed: {}", error.kind());
            notifier.send(Update::SavedFailed {
                uri: uri.to_owned(),
                saved,
            });
            Step::Done
        }
    }
}

/// Appends tracks to a playlist and announces the reply revision.
async fn add_tracks<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    playlist: &str,
    revision: &Revision,
    from_index: usize,
    tracks: &[String],
) -> Step {
    match stop_or(
        stop,
        host.add_tracks(playlist, revision, from_index, tracks),
    )
    .await
    {
        None => Step::Stopped,
        Some(Ok(revision)) => {
            notifier.send(Update::TrackAdded {
                playlist: playlist.to_owned(),
                revision,
            });
            Step::Done
        }
        Some(Err(error)) => {
            log::warn!("add write failed: {}", error.kind());
            notifier.send(Update::AddFailed {
                playlist: playlist.to_owned(),
            });
            Step::Done
        }
    }
}

/// Renames one playlist, reading its own state for the base revision.
///
/// The UI may hold several stale copies of a list, so the revision is never
/// taken from it: a read that names the list and says it is editable is the
/// only basis the write may name.
async fn rename_playlist<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    playlist: &str,
    name: &str,
) -> Step {
    let Some(state) = stop_or(stop, host.playlist_state(playlist)).await else {
        return Step::Stopped;
    };
    let Some(state) = state.filter(|state| state.editable) else {
        notifier.send(Update::PlaylistRenameFailed {
            playlist: playlist.to_owned(),
        });
        return Step::Done;
    };
    match stop_or(stop, host.rename_playlist(playlist, &state.revision, name)).await {
        None => Step::Stopped,
        Some(Ok(revision)) => {
            notifier.send(Update::PlaylistRenamed {
                playlist: playlist.to_owned(),
                name: name.to_owned(),
                revision,
            });
            Step::Done
        }
        Some(Err(error)) => {
            log::warn!("rename write failed: {}", error.kind());
            notifier.send(Update::PlaylistRenameFailed {
                playlist: playlist.to_owned(),
            });
            Step::Done
        }
    }
}

/// Creates a playlist, links it, and lands the pick it was made for.
///
/// One command, up to four host calls, and the first failure ends it: a create
/// that failed created nothing, so there is nothing to link, and a link that
/// failed leaves a real playlist the account holds and this app cannot see.
/// Neither is retried here, because no playlist write is idempotent.
///
/// The add is the one failure that is not fatal: the list exists either way, so
/// it is announced as a list with nothing in it plus a notice of its own, never
/// as an add failure, which the sidebar refuses while the create is pending.
async fn create_playlist<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    name: &str,
    revision: &Revision,
    tracks: &[String],
) -> Step {
    let Some(created) = stop_or(stop, host.make_playlist(name)).await else {
        return Step::Stopped;
    };
    let uri = match created {
        Ok(uri) => uri,
        Err(error) => {
            log::warn!("playlist create failed: {}", error.kind());
            notifier.send(Update::PlaylistCreateFailed);
            return Step::Done;
        }
    };
    let Some(linked) = stop_or(stop, host.link_playlist(revision, &uri)).await else {
        return Step::Stopped;
    };
    let revision = match linked {
        Ok(revision) => revision,
        Err(error) => {
            log::warn!("playlist link failed: {}", error.kind());
            notifier.send(Update::PlaylistUnlinked);
            return Step::Done;
        }
    };
    let Some(state) = stop_or(stop, host.playlist_state(&uri)).await else {
        return Step::Stopped;
    };
    let mut access = state;
    // The name is the one thing a create does not carry: the body's name field
    // is ignored, so a fresh list is nameless until it is renamed. The rename
    // answers the revision everything after it is based on.
    let mut named = false;
    if let Some(revision) = access.as_ref().map(|access| access.revision.clone()) {
        match stop_or(stop, host.rename_playlist(&uri, &revision, name)).await {
            None => return Step::Stopped,
            Some(Ok(reply)) => {
                named = true;
                if let Some(access) = &mut access {
                    access.revision = reply;
                }
            }
            Some(Err(error)) => log::warn!("naming a new playlist failed: {}", error.kind()),
        }
    }
    // The list's length is the index its next append names, so a count that did
    // not land would make the next add prepend.
    let mut landed = 0;
    if let Some(revision) = access.as_ref().map(|access| access.revision.clone())
        && !tracks.is_empty()
    {
        match stop_or(stop, host.add_tracks(&uri, &revision, 0, tracks)).await {
            None => return Step::Stopped,
            Some(Ok(reply)) => {
                landed = tracks.len();
                if let Some(access) = &mut access {
                    access.revision = reply;
                }
            }
            Some(Err(error)) => log::warn!("add to a new playlist failed: {}", error.kind()),
        }
    }
    // No reply on this chain carries a picture, so a new row would draw an empty
    // slot until the next rootlist reload. One read fills it, and a read that
    // does not answer costs the row its picture and nothing else.
    let (cover, composed, description) = match stop_or(stop, host.playlist_entry(&uri)).await {
        None => return Step::Stopped,
        Some(Some(entry)) => (entry.cover, entry.composed, entry.description),
        Some(None) => (None, Vec::new(), None),
    };
    notifier.send(Update::PlaylistCreated {
        entry: PlaylistEntry {
            saved_at: None,
            uri: uri.clone(),
            name: name.to_owned(),
            // A list the rename did not reach keeps the typed name for this
            // session only, and the notice below says so.
            named,
            cover,
            composed,
            description,
            tracks: Some(landed),
            owner: Some(Owner::You),
            access,
        },
        revision,
    });
    if !named {
        notifier.send(Update::PlaylistUnnamed {
            playlist: uri.clone(),
        });
    }
    if !tracks.is_empty() && landed == 0 {
        notifier.send(Update::PlaylistUnfilled { playlist: uri });
    }
    Step::Done
}

/// Plays `uri` now, or drops the link first when the session is dead or being
/// rebuilt.
///
/// A play pressed mid-rebuild is not handed to the player: the window already
/// loads the pressed track once the session is back.
#[allow(clippy::too_many_arguments)]
async fn play_now<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    devices: &mut Devices,
    drains: &Drains,
    recovery: &mut Recovery,
    engine_up: &mut bool,
    reconnecting: &mut bool,
    uri: &str,
) -> Step {
    if !host.is_down() && !recovery.is_active() {
        play(host, notifier, drains, uri);
        return Step::Done;
    }
    if !recovery.is_active() {
        let step = lose_link(
            host,
            stop,
            notifier,
            devices,
            drains,
            recovery,
            engine_up,
            reconnecting,
        )
        .await;
        if step.stopped() {
            return Step::Stopped;
        }
    }
    recovery.defer(
        ResumePoint {
            uri: uri.to_owned(),
            position_ms: 0,
            play: true,
        },
        Instant::now(),
    );
    Step::Done
}

/// Waits for a deadline that may not exist; an idle schedule never wakes.
async fn sleep_until(at: Option<Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await,
        None => std::future::pending().await,
    }
}

/// Begins recovering from a dead link: snapshot the track, silence the old
/// drains, retire the session, and tell the window when there is something to
/// come back to.
#[allow(clippy::too_many_arguments)]
async fn lose_link<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    devices: &mut Devices,
    drains: &Drains,
    recovery: &mut Recovery,
    engine_up: &mut bool,
    reconnecting: &mut bool,
) -> Step {
    if recovery.is_active() {
        return Step::Done;
    }
    let now = Instant::now();
    recovery.lost(now, drains.playhead.resume_point(now));
    silence_session(devices, drains, engine_up);
    let retired = match stop_or(stop, host.retire()).await {
        None => return Step::Stopped,
        Some(retired) => retired,
    };
    log::info!(
        "session retired: {}",
        match retired {
            Retired::Joined => "joined",
            Retired::Lingering => "lingering",
        }
    );
    if recovery.has_resume() {
        notifier.send(Update::Transport(TransportUpdate::Reconnecting));
        *reconnecting = true;
    }
    Step::Done
}

/// One bounded attempt to bring the session and the player back, raced against
/// the window closing and the next command that retargets it.
#[allow(clippy::too_many_arguments)]
async fn rebuild_attempt<H: SessionHost, S: TokenSource + Send + Sync + 'static>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    devices: &mut Devices,
    drains: &mut Drains,
    commands: &mut UnboundedReceiver<Command>,
    deferred: &mut VecDeque<Command>,
    refresh: &mut Option<Refresh>,
    token: &mut Option<AccessToken>,
    engine_up: &mut bool,
    store: &Arc<dyn CredentialStore>,
    source: &Arc<S>,
    point: Option<ResumePoint>,
    budget: Duration,
) -> Rebuilt {
    // Taken before the attempt borrows the host, so an order can be answered
    // while a step holds it.
    let orders = host.orders();
    // Set around the watch, whose start cannot be dropped, and set once a
    // command has ended the attempt while it waits there.
    let arming = AtomicBool::new(false);
    let ended = AtomicBool::new(false);
    let mut work = std::pin::pin!(async {
        // A live-but-dead engine is closed before anything replaces it:
        // `connect` assigns over the field, leaving the old session and its
        // player running.
        if host.is_down() {
            silence_session(devices, drains, engine_up);
            host.retire().await;
        }
        if !*engine_up {
            let fresh = refresh_token(refresh, store, source).await?;
            host.connect(&fresh).await?;
            *token = Some(fresh);
            *engine_up = true;
            // A new session opens a new generation, and the drains of the old
            // one stay retired.
            drains.playhead.next_generation();
            drains.retired = Arc::new(AtomicBool::new(false));
            // The account still names the track from before the drop. A fresh
            // state is honest until the new player reports what it is doing.
            host.report(None);
        }
        host.prepare_player()?;
        ensure_drain(host, notifier, drains);
        if devices.rewatch {
            arm_watch(host, devices);
            // Not raced against the commands: a start that is dropped spends
            // the session's one dealer builder, so this one is always
            // answered.
            arming.store(true, Ordering::SeqCst);
            let result = host.watch_devices().await;
            arming.store(false, Ordering::SeqCst);
            adopt_watch(host, notifier, devices, drains, result);
            if ended.load(Ordering::SeqCst) {
                // What ended the attempt waited for the watch, and the reload
                // is the next attempt's business.
                return Ok(());
            }
        }
        if let Some(point) = point {
            start_load(
                host,
                &drains.playhead,
                notifier,
                &point.uri,
                point.position_ms,
                point.play,
            )?;
        }
        Ok::<(), SessionError>(())
    });
    let raced = async {
        loop {
            tokio::select! {
                biased;
                _ = &mut *stop => return Rebuilt::Stopped,
                // Polled before the commands, so a stream of them cannot hold
                // the attempt still.
                result = &mut work => {
                    return match result {
                        Ok(()) if ended.load(Ordering::SeqCst) => Rebuilt::Interrupted,
                        Ok(()) => Rebuilt::Done,
                        Err(error) => {
                            log::info!("rebuild step failed: {}", error.kind());
                            Rebuilt::Failed
                        }
                    };
                }
                command = commands.recv() => match command {
                    // An answer is the phone's, not the attempt's: it is
                    // delivered at once and the attempt carries on.
                    Some(Command::Answer { id, ok }) => {
                        deliver_answer(orders.as_ref().is_some_and(|orders| orders.answer(id, ok)));
                    }
                    Some(command)
                        if interrupts_attempt(&command) && arming.load(Ordering::SeqCst) =>
                    {
                        // The watch cannot be dropped, but an attempt that
                        // did not finish its track has not rebuilt it.
                        ended.store(true, Ordering::SeqCst);
                        deferred.push_back(command);
                    }
                    Some(command) if interrupts_attempt(&command) => {
                        // It joins the commands already waiting, so nothing
                        // overtakes the order the window sent them in.
                        deferred.push_back(command);
                        return Rebuilt::Interrupted;
                    }
                    // Anything else is no reason to drop an attempt: it waits
                    // its turn, in order, once the attempt is over.
                    Some(command) => deferred.push_back(command),
                    None => return Rebuilt::Stopped,
                },
            }
        }
    };
    match tokio::time::timeout(budget, raced).await {
        Ok(outcome) => outcome,
        Err(_) => {
            log::info!("rebuild attempt timed out");
            Rebuilt::Failed
        }
    }
}

/// Awaits the refresh the task already has in flight, or starts the one it does
/// not.
///
/// The receiver stays in the task across a cancelled attempt, so the next one
/// waits on the same exchange instead of rotating the token a second time.
async fn refresh_token<S: TokenSource + Send + Sync + 'static>(
    pending: &mut Option<Refresh>,
    store: &Arc<dyn CredentialStore>,
    source: &Arc<S>,
) -> Result<AccessToken, SessionError> {
    if pending.is_none() {
        *pending = Some(start_refresh(store, source));
    }
    let answer = std::future::poll_fn(|cx| match pending.as_mut() {
        Some(receiver) => std::future::Future::poll(std::pin::Pin::new(receiver), cx).map(Some),
        None => std::task::Poll::Ready(None),
    })
    .await;
    *pending = None;
    match answer {
        Some(Ok(Some(token))) => Ok(token),
        Some(Ok(None)) | Some(Err(_)) | None => {
            log::info!("session refresh failed");
            Err(SessionError::NotConnected)
        }
    }
}

/// Serves one hand-over or one phone-side play: prepare, read what is missing,
/// take the sound, name the track and load it.
///
/// Every refusal is answered before anything is put, so a transfer or a play
/// this window cannot carry out leaves the phone's own playback untouched.
async fn serve_landing<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    drains: &Drains,
    id: RequestId,
    instruction: Instruction,
) -> Step {
    let plan = match instruction {
        // A phone-side play is the same load as a hand-over: one path, so the
        // two cannot prepare, put or land differently.
        Instruction::Transfer(plan) | Instruction::Play(plan) => plan,
        Instruction::Refused(refusal) => {
            log::info!(target: LANDING_LOG_TARGET, "landing refused: {}", refusal.word());
            host.refuse(id);
            return Step::Done;
        }
        // The order path dispatches before this call, so this arm only keeps a
        // misplaced order from being dropped unanswered.
        Instruction::Order(_) => {
            log::info!(target: LANDING_LOG_TARGET, "landing other: order");
            host.refuse(id);
            return Step::Done;
        }
        // A context the phone asked us to adopt says where its own playback
        // lives, not what to change here: the rows this window holds are the
        // queue. `Failure` would only make the phone retry, and the ack still
        // rides the next put like any other command's.
        Instruction::Ignored => {
            log::info!(target: LANDING_LOG_TARGET, "landing ignored: answered");
            host.answer_order(id, true);
            return Step::Done;
        }
        Instruction::Other(kind) => {
            log::info!(target: LANDING_LOG_TARGET, "landing other: {}", kind.word());
            host.refuse(id);
            return Step::Done;
        }
    };

    if let Err(error) = host.prepare_player() {
        if ends_playability(&error) {
            host.set_playable(false);
        }
        log::warn!("landing prepare failed: {}", error.kind());
        host.refuse(id);
        return Step::Done;
    }
    host.set_playable(true);
    ensure_drain(host, notifier, drains);
    log::info!(target: LANDING_LOG_TARGET, "landing player ready");

    // The phone's own pages cost no read; a context is read only when they are
    // absent, and a failed read falls back to the one track rather than
    // refusing the hand-over.
    let resolution = match plan.resolve_uri() {
        None => Resolution::Failed,
        Some(uri) => match stop_or(stop, host.resolve_context(uri)).await {
            None => return Step::Stopped,
            Some(Ok(uris)) => Resolution::Resolved(uris),
            Some(Err(error)) => {
                log::warn!("context resolve failed: {}", error.kind());
                Resolution::Failed
            }
        },
    };

    // A play can name its row by position alone, so the row may only be known
    // once the read has answered. A landing on no row would take the sound with
    // nothing to play, so it is refused before the put.
    if plan.anchor(&resolution).is_none() {
        log::info!(target: LANDING_LOG_TARGET, "landing refused: no row to anchor on");
        host.refuse(id);
        return Step::Done;
    }

    // Accept answers the phone itself, either way, so a failure here is the
    // whole answer and nothing is loaded.
    let landing = match stop_or(stop, host.accept(id, &plan, resolution)).await {
        None => return Step::Stopped,
        Some(Ok(landing)) => landing,
        Some(Err(error)) => {
            log::info!(
                target: LANDING_LOG_TARGET,
                "landing accept failed: {}",
                error.kind()
            );
            return Step::Done;
        }
    };

    // The row to load is the landing's, not the plan's: a play may have named
    // its row by position, so only the landing knows which uri that position
    // found. `accept` refuses a landing that names no track, so this only keeps
    // a row the landing left empty from being loaded.
    let Some(current) = landing
        .rows
        .get(landing.index)
        .filter(|row| !row.is_empty())
        .cloned()
    else {
        log::info!(target: LANDING_LOG_TARGET, "landing names no row to load");
        return Step::Done;
    };

    // The loss rule may have ended this activation while the accept ran, so the
    // landing is not announced and the player is left alone.
    if !host.holding() {
        log::info!(
            target: LANDING_LOG_TARGET,
            "landing lost the sound before it was announced"
        );
        return Step::Done;
    }

    // Before the load, so no playback update can overtake the rows it belongs
    // to.
    notifier.send(Update::Remote(RemoteUpdate::Landed(landing.clone())));

    // A loss between the announcement and the load leaves the player paused,
    // which is where the transport settles through its own event.
    if let Err(error) = start_load(
        host,
        &drains.playhead,
        notifier,
        &current,
        landing.position_ms,
        landing.playing && host.holding(),
    ) {
        log::warn!("landing load failed: {}", error.kind());
        notifier.send(Update::Transport(TransportUpdate::Failed(
            playback_failure(&error),
        )));
        // The sound was already taken, so a load that cannot happen gives it
        // back instead of leaving the account naming a silent device.
        if let Err(error) = host.release_sound().await {
            log::warn!("landing give-back failed: {}", error.kind());
            notifier.send(Update::Transport(TransportUpdate::Failed(
                playback_failure(&error),
            )));
        }
        // Nothing plays here, so the transport the landing left waiting stops
        // through the loss rule rather than sitting in `Loading`.
        notifier.send(Update::Remote(RemoteUpdate::Lost {
            epoch: landing.epoch,
        }));
        return Step::Done;
    }

    log::info!(
        target: LANDING_LOG_TARGET,
        "landing loaded at {} ms, playing={}",
        landing.position_ms,
        landing.playing
    );
    // The phone was answered before either, and a row that lands as a bare URI
    // is renamed when its names arrive.
    let labelled = match stop_or(stop, host.release_tracks(std::slice::from_ref(&current))).await {
        None => return Step::Stopped,
        Some(Ok(named)) => named,
        Some(Err(error)) => {
            log_device_error(&error);
            Vec::new()
        }
    };
    if !labelled.is_empty() {
        notifier.send(Update::Remote(RemoteUpdate::Labelled(labelled)));
    }
    Step::Done
}

/// Starts the device watch, once per session.
///
/// `devices.watching` is the only guard that keeps a duplicate or a late request
/// off the dealer's one start: the builder is consumed by the first, so a second
/// cannot be attempted and a failure here is not retryable by asking again.
async fn watch_devices<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    devices: &mut Devices,
    drains: &Drains,
    connected: bool,
) -> Step {
    if !connected {
        notifier.send(device_failure(Failure::Devices));
        return Step::Done;
    }
    if devices.watching {
        return Step::Done;
    }
    arm_watch(host, devices);
    match stop_or(stop, host.watch_devices()).await {
        None => Step::Stopped,
        Some(result) => {
            adopt_watch(host, notifier, devices, drains, result);
            Step::Done
        }
    }
}

/// Asks the cluster for its roster, which is the only way to learn about a
/// device that appeared after launch.
async fn refresh_devices<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    devices: &mut Devices,
    drains: &Drains,
    connected: bool,
) -> Step {
    if !devices.watching {
        return watch_devices(host, stop, notifier, devices, drains, connected).await;
    }
    if devices.dead.load(Ordering::SeqCst) {
        notifier.send(device_failure(Failure::DevicesOff));
        return Step::Done;
    }
    match stop_or(stop, host.refresh_devices()).await {
        None => Step::Stopped,
        Some(Ok(view)) => {
            // Compared against the one roster both paths share, so a later push
            // cannot restore a roster this ask has replaced.
            for update in devices.sent.updates(&view) {
                notifier.send(update);
            }
            Step::Done
        }
        Some(Err(error)) => {
            log_device_error(&error);
            notifier.send(device_failure(Failure::Devices));
            Step::Done
        }
    }
}

/// Sends one transport action to the device holding the sound.
///
/// A refused steer is reported once and never retried: the device ignores a
/// command aimed at a cluster that has moved on.
async fn steer_device<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    device: &str,
    steer: CoreSteer,
    connected: bool,
) -> Step {
    if !connected {
        notifier.send(Update::Devices(DeviceUpdate::SteerFailed));
        return Step::Done;
    }
    match stop_or(stop, host.steer(device, steer)).await {
        None => Step::Stopped,
        Some(Ok(())) => Step::Done,
        Some(Err(error)) => {
            log_device_error(&error);
            notifier.send(Update::Devices(DeviceUpdate::SteerFailed));
            Step::Done
        }
    }
}

/// Moves the account's sound to another device.
///
/// A hand-over that is acked reports nothing: the cluster push naming the new
/// holder is what tells the window, so a guess here would race the truth.
async fn transfer_device<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    to: &str,
    connected: bool,
) -> Step {
    if !connected {
        notifier.send(Update::Devices(DeviceUpdate::TransferFailed(
            Failure::Transfer,
        )));
        return Step::Done;
    }
    match stop_or(stop, host.hand_over(to)).await {
        None => Step::Stopped,
        Some(Ok(())) => Step::Done,
        Some(Err(error)) => {
            log_device_error(&error);
            // A window playing from no list has nothing to hand over, and no
            // retry will change that until it plays one.
            let failure = if error.no_context() {
                Failure::NoContext
            } else {
                Failure::Transfer
            };
            notifier.send(Update::Devices(DeviceUpdate::TransferFailed(failure)));
            Step::Done
        }
    }
}

/// Names the track another device is playing, when the label read answers.
///
/// A miss is silent: the title stays `None` and the caption still draws.
async fn label_remote<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    track: &str,
) -> Step {
    let tracks = [track.to_owned()];
    match stop_or(stop, host.release_tracks(&tracks)).await {
        None => Step::Stopped,
        Some(Ok(named)) => {
            if let Some(entry) = named.into_iter().next() {
                notifier.send(Update::Devices(DeviceUpdate::Labelled(entry)));
            }
            Step::Done
        }
        Some(Err(error)) => {
            log_device_error(&error);
            Step::Done
        }
    }
}

/// Names the rows a hand-over adopted, in one read.
///
/// A miss is silent: the rows keep their URIs and the bar still draws.
async fn label_rows<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    uris: &[String],
) -> Step {
    match stop_or(stop, host.release_tracks(uris)).await {
        None => Step::Stopped,
        Some(Ok(named)) => {
            if !named.is_empty() {
                notifier.send(Update::Remote(RemoteUpdate::Labelled(named)));
            }
            Step::Done
        }
        Some(Err(error)) => {
            log_device_error(&error);
            Step::Done
        }
    }
}

/// Names the liked songs Home's shelf is about to draw.
///
/// The reply is always sent, even when the read fails or names nothing: Home
/// waits for every answer before it draws. A miss is an answer the shelf cannot
/// draw from; a failure is told apart from it so the page can offer the retry.
async fn label_saved<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    tracks: &[String],
) -> Step {
    let asked = tracks.to_vec();
    match stop_or(stop, host.release_tracks(tracks)).await {
        None => Step::Stopped,
        Some(Ok(named)) => {
            notifier.send(Update::SavedLabelled { asked, named });
            Step::Done
        }
        Some(Err(error)) => {
            log_device_error(&error);
            // The labels have no surface of their own, so their failure is the
            // library's load, as a failed liked read is.
            notifier.send(Update::SavedLabelFailed {
                asked,
                failure: Failure::Load,
            });
            Step::Done
        }
    }
}

/// Takes the device off the account before the player goes, within a bound.
async fn withdraw<H: SessionHost>(host: &mut H, timeout: Duration) {
    if tokio::time::timeout(timeout, host.withdraw_devices())
        .await
        .is_err()
    {
        log::warn!("device withdraw did not finish before shutdown");
    }
}

/// One device-side update carrying a failure with no detail; the reason is for
/// the log.
fn device_failure(failure: Failure) -> Update {
    Update::Devices(DeviceUpdate::Failed(failure))
}

/// Logs the reason, which never reaches the window: it can embed a body.
fn log_device_error(error: &SessionError) {
    log::warn!("device call failed: {}", error.kind());
}

/// The window's steer in the words the wire uses. Total, so a new action is a
/// build failure here.
pub(crate) fn core_steer(steer: Steer) -> CoreSteer {
    match steer {
        Steer::Pause => CoreSteer::Pause,
        Steer::Resume => CoreSteer::Resume,
        Steer::SkipNext => CoreSteer::SkipNext,
        Steer::SkipPrev => CoreSteer::SkipPrev,
        Steer::SeekTo(position_ms) => CoreSteer::SeekTo(position_ms),
    }
}

/// Drains cluster views onto the update channel for as long as the pump lives.
///
/// A plain thread, like the playback drain: no runtime, so `blocking_recv`
/// cannot delay the runtime's drop.
fn spawn_device_drain(
    notifier: Notifier,
    mut events: UnboundedReceiver<ClusterEvent>,
    sent: SentRoster,
    dead: Arc<AtomicBool>,
    retired: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        while let Some(event) = events.blocking_recv() {
            if retired.load(Ordering::SeqCst) {
                return;
            }
            match event {
                ClusterEvent::View(view) => {
                    for update in sent.updates(&view) {
                        notifier.send(update);
                    }
                }
                ClusterEvent::Failed => {
                    notifier.send(device_failure(Failure::Devices));
                }
                // A loss is not a fault: its epoch is what orders it against a
                // landing still in flight, so it reaches the window as itself.
                ClusterEvent::Lost { epoch } => {
                    log::debug!("connect lost: epoch {epoch}");
                    notifier.send(Update::Remote(RemoteUpdate::Lost { epoch }));
                }
            }
        }
        // The pump is gone while the window is not: a stale remote would
        // otherwise keep advancing with nobody left to report it.
        dead.store(true, Ordering::SeqCst);
        if !retired.load(Ordering::SeqCst) {
            notifier.send(device_failure(Failure::DevicesOff));
        }
    });
}

/// Logs the failure variant, then hands the update to the window. The variant
/// is a fixed name; the underlying error can embed a response body and is
/// never formatted.
fn send_update(notifier: &Notifier, update: Update) {
    if let Update::SignInFailed(failure) = update {
        log::warn!("sign-in failed: {failure:?}");
    }
    notifier.send(update);
}

/// Builds the bridge, runs the window, and finishes the worker before the
/// runtime is dropped.
pub(crate) fn run_window<H: SessionHost, S, R>(
    runtime: Runtime,
    host: H,
    store: Arc<dyn CredentialStore>,
    source: Arc<S>,
    window: impl FnOnce(UiEnd, WakeSlot) -> R,
) -> R
where
    S: TokenSource + Send + Sync + 'static,
{
    let (ui_end, slot, worker) = start(runtime.handle(), host, store, source);
    run_ordered(
        runtime,
        move || window(ui_end, slot),
        move || worker.finish(),
    )
}

#[cfg(test)]
mod tests;
