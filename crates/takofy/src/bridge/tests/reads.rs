use super::super::*;
use super::test_support::*;
use crate::session::ArtistPage;
use std::sync::atomic::Ordering;
use std::sync::mpsc::channel;
use takofy_model::{
    ArtistDetail, ArtistPortrait, CoverSet, LikedTrack, LyricLine, SavedAt, TrackEntry, TrackLyrics,
};

/// A scripted liked-tracks answer; the default is an empty set.
#[derive(Clone)]
pub(super) enum LikedScript {
    Ready(Vec<LikedTrack>),
    Fail,
}

impl Default for LikedScript {
    fn default() -> Self {
        Self::Ready(Vec::new())
    }
}

/// A scripted artist answer; the default is a bare one at [`ARTIST`].
#[derive(Clone)]
pub(super) enum ArtistScript {
    Ready(ArtistPage),
    Fail,
}

impl Default for ArtistScript {
    fn default() -> Self {
        Self::Ready(ArtistPage {
            artist: artist_detail(ARTIST),
            followed: Some(false),
        })
    }
}

/// A scripted lyrics answer; the default is no lines.
///
/// `Pending` never answers, so a stop can be tested while the load is still
/// in flight (the same shape [`ShowScript`] uses).
#[derive(Clone)]
pub(super) enum LyricsScript {
    Ready(TrackLyrics),
    Fail,
    Pending,
}

impl Default for LyricsScript {
    fn default() -> Self {
        Self::Ready(TrackLyrics::default())
    }
}

/// A scripted search answer; the default is no tracks.
///
/// `Pending` never answers, so a stop can be tested while the load is still
/// in flight (the same shape [`LyricsScript`] uses).
#[derive(Clone)]
pub(super) enum SearchScript {
    Ready(Vec<TrackEntry>),
    Fail,
    Pending,
}

impl Default for SearchScript {
    fn default() -> Self {
        Self::Ready(Vec::new())
    }
}

/// A scripted portrait read; the default is no portraits.
///
/// `Pending` never answers, so a stop can be tested while the read is still
/// in flight (the same shape [`SearchScript`] uses).
#[derive(Clone)]
pub(super) enum PortraitScript {
    Ready(Vec<ArtistPortrait>),
    Fail,
    Pending,
}

impl Default for PortraitScript {
    fn default() -> Self {
        Self::Ready(Vec::new())
    }
}

/// One artist page's data, with no top tracks and no releases, so a test
/// names only what it reads.
pub(super) fn artist_detail(uri: &str) -> ArtistDetail {
    ArtistDetail {
        uri: uri.to_owned(),
        name: "Vela Moreno".to_owned(),
        portrait: None,
        top_tracks: Vec::new(),
        releases: Vec::new(),
        related: Vec::new(),
    }
}

#[test]
fn loading_the_liked_tracks_announces_the_whole_set_with_each_date() {
    let liked = LikedTrack {
        uri: TRACK.to_owned(),
        saved_at: SavedAt::from_secs(1_700_000_000),
    };
    assert_eq!(
        run_liked(LikedScript::Ready(vec![liked.clone()])),
        Update::LikedTracks(vec![liked])
    );
}

#[test]
fn a_liked_tracks_failure_announces_the_failed_update() {
    assert_eq!(
        run_liked(LikedScript::Fail),
        Update::LikedTracksFailed(Failure::Load),
        "a liked read has no surface of its own, so its failure is the library's"
    );
}

/// Runs one like write through the live path, with the scripted answer.
fn run_like(script: LikeScript, liked: bool) -> Update {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.like_script.lock().expect("lock") = script;

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::SetLiked {
                tracks: vec![TRACK.to_owned()],
                liked,
            })
            .expect("like");
        ui_end.updates.recv().expect("like answer")
    })
}

#[test]
fn a_like_announces_the_requested_tracks_and_value_on_success() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::SetLiked {
                tracks: vec![TRACK.to_owned()],
                liked: true,
            })
            .expect("like");
        ui_end.updates.recv().expect("liked")
    });

    assert_eq!(
        update,
        Update::Liked {
            tracks: vec![TRACK.to_owned()],
            liked: true,
        }
    );
    assert_eq!(shared.like_writes.load(Ordering::SeqCst), 1);
    assert_eq!(
        shared.last_like.lock().expect("lock").clone(),
        Some((vec![TRACK.to_owned()], true)),
        "the write is sent with the pick the user made"
    );
}

#[test]
fn a_like_failure_announces_like_failed_with_save() {
    assert_eq!(
        run_like(LikeScript::Fail, false),
        Update::LikeFailed {
            tracks: vec![TRACK.to_owned()],
            failure: Failure::Save,
        }
    );
}

#[test]
fn a_stop_during_a_like_write_ends_the_task() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.like_script.lock().expect("lock") = LikeScript::Pending;
    let (started_tx, started_rx) = channel();
    *shared.like_started.lock().expect("lock") = Some(started_tx);

    let runtime = runtime();
    let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

    let window = move || {
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::SetLiked {
                tracks: vec![TRACK.to_owned()],
                liked: true,
            })
            .expect("like");
        started_rx.recv().expect("like started");
    };
    let teardown = move || async move {
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("finish must resolve when the stop interrupts the like write");
        assert_eq!(
            shared.shutdowns.load(Ordering::SeqCst),
            1,
            "the shutdown must run when the stop interrupts the like write"
        );
    };

    run_ordered(runtime, window, teardown);
}

#[test]
fn a_scripted_artist_announces_the_page_then_the_followed_state() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    let detail = artist_detail(ARTIST);
    *shared.artist_script.lock().expect("lock") = ArtistScript::Ready(ArtistPage {
        artist: detail.clone(),
        followed: Some(true),
    });

    let (first, second) = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LoadArtist {
                artist: ARTIST.to_owned(),
            })
            .expect("load");
        let first = ui_end.updates.recv().expect("artist");
        let second = ui_end.updates.recv().expect("followed");
        (first, second)
    });

    assert_eq!(first, Update::Artist(detail));
    assert_eq!(
        second,
        Update::Followed {
            artist: ARTIST.to_owned(),
            followed: true,
        },
        "the page is announced first, then what the server said about it"
    );
}

#[test]
fn an_artist_without_a_probe_answer_announces_no_followed_state() {
    let detail = artist_detail(ARTIST);
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.artist_script.lock().expect("lock") = ArtistScript::Ready(ArtistPage {
        artist: detail.clone(),
        followed: None,
    });

    let (first, second) = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LoadArtist {
                artist: ARTIST.to_owned(),
            })
            .expect("load");
        let first = ui_end.updates.recv().expect("artist");
        ui_end
            .commands
            .send(Command::LoadReleaseTracks {
                release: "spotify:album:r".to_owned(),
                tracks: Vec::new(),
            })
            .expect("labels");
        let second = ui_end.updates.recv().expect("labels");
        (first, second)
    });

    assert_eq!(first, Update::Artist(detail));
    assert_eq!(
        second,
        Update::ReleaseTracks {
            release: "spotify:album:r".to_owned(),
            tracks: Vec::new(),
        },
        "no Followed state was queued behind the artist"
    );
}

#[test]
fn a_failed_artist_load_is_announced() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.artist_script.lock().expect("lock") = ArtistScript::Fail;

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LoadArtist {
                artist: ARTIST.to_owned(),
            })
            .expect("load");
        ui_end.updates.recv().expect("failure")
    });

    assert_eq!(
        update,
        Update::ArtistFailed {
            artist: ARTIST.to_owned(),
            failure: Failure::Load,
        }
    );
    assert_eq!(shared.artists.load(Ordering::SeqCst), 1);
}

/// One track's lyrics with a single timed line, so a test names only what
/// it reads.
fn lyrics_of(words: &str) -> TrackLyrics {
    TrackLyrics {
        lines: vec![LyricLine {
            start_ms: Some(0),
            end_ms: Some(1_000),
            words: words.to_owned(),
        }],
        synced: true,
    }
}

#[test]
fn a_scripted_lyrics_load_reaches_the_window() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    let lyrics = lyrics_of("first line");
    *shared.lyrics_script.lock().expect("lock") = LyricsScript::Ready(lyrics.clone());

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LoadLyrics {
                track: TRACK.to_owned(),
            })
            .expect("load");
        ui_end.updates.recv().expect("lyrics")
    });

    assert_eq!(
        update,
        Update::Lyrics {
            track: TRACK.to_owned(),
            lyrics,
        }
    );
    assert_eq!(shared.lyrics_loads.load(Ordering::SeqCst), 1);
}

#[test]
fn a_failed_lyrics_load_is_announced() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.lyrics_script.lock().expect("lock") = LyricsScript::Fail;

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LoadLyrics {
                track: TRACK.to_owned(),
            })
            .expect("load");
        ui_end.updates.recv().expect("failure")
    });

    assert_eq!(
        update,
        Update::LyricsFailed {
            track: TRACK.to_owned(),
            failure: Failure::Lyrics,
        }
    );
    assert_eq!(shared.lyrics_loads.load(Ordering::SeqCst), 1);
}

#[test]
fn a_stop_during_a_lyrics_load_shuts_down() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.lyrics_script.lock().expect("lock") = LyricsScript::Pending;
    let (started_tx, started_rx) = channel();
    *shared.lyrics_started.lock().expect("lock") = Some(started_tx);

    let runtime = runtime();
    let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

    let window = move || {
        loop {
            if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                break;
            }
        }
        ui_end
            .commands
            .send(Command::LoadLyrics {
                track: TRACK.to_owned(),
            })
            .expect("load");
        started_rx.recv().expect("lyrics started");
    };
    let teardown = move || async move {
        // A hung stop must fail the test, not hang the suite.
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("finish must resolve when the stop interrupts the lyrics load");
        assert_eq!(
            shared.shutdowns.load(Ordering::SeqCst),
            1,
            "the shutdown must run when the stop interrupts the lyrics load"
        );
    };

    run_ordered(runtime, window, teardown);
}

fn portrait(artist: &str) -> ArtistPortrait {
    ArtistPortrait {
        artist: artist.to_owned(),
        portrait: Some(CoverSet::single(
            CoverUrl::from_wire(&format!("https://i.scdn.co/image/{}", "ab".repeat(20)))
                .expect("a cover"),
        )),
    }
}

#[test]
fn a_scripted_portrait_read_echoes_the_query_and_the_list() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    let portraits = vec![portrait("spotify:artist:a"), portrait("spotify:artist:b")];
    *shared.portrait_script.lock().expect("lock") = PortraitScript::Ready(portraits.clone());
    let query = "slow  tide";
    let artists = vec!["spotify:artist:a".to_owned(), "spotify:artist:b".to_owned()];

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LoadPortraits {
                query: query.to_owned(),
                artists: artists.clone(),
            })
            .expect("portraits");
        ui_end.updates.recv().expect("portraits")
    });

    assert_eq!(
        update,
        Update::Portraits {
            query: query.to_owned(),
            portraits,
        }
    );
    assert_eq!(shared.portrait_reads.load(Ordering::SeqCst), 1);
    assert_eq!(
        *shared.portraited.lock().expect("lock"),
        vec![artists],
        "the host must be asked for exactly the artists sent"
    );
}

#[test]
fn a_failed_portrait_read_is_announced_silently() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.portrait_script.lock().expect("lock") = PortraitScript::Fail;

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LoadPortraits {
                query: "slow tide".to_owned(),
                artists: vec!["spotify:artist:a".to_owned()],
            })
            .expect("portraits");
        ui_end.updates.recv().expect("failure")
    });

    assert_eq!(
        update,
        Update::PortraitsFailed {
            query: "slow tide".to_owned(),
        },
        "a failed portrait read carries no failure word"
    );
}

#[test]
fn a_portrait_read_before_the_session_connects_is_announced_as_silent() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Fail, RootlistScript::default());
    let shared = Arc::clone(&host.shared);

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LoadPortraits {
                query: "slow tide".to_owned(),
                artists: vec!["spotify:artist:a".to_owned()],
            })
            .expect("portraits");
        ui_end.updates.recv().expect("failure")
    });

    assert_eq!(
        update,
        Update::PortraitsFailed {
            query: "slow tide".to_owned(),
        }
    );
    assert_eq!(shared.portrait_reads.load(Ordering::SeqCst), 1);
}

#[test]
fn a_stop_during_a_portrait_read_shuts_down() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.portrait_script.lock().expect("lock") = PortraitScript::Pending;
    let (started_tx, started_rx) = channel();
    *shared.portrait_started.lock().expect("lock") = Some(started_tx);

    let runtime = runtime();
    let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

    let window = move || {
        loop {
            if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                break;
            }
        }
        ui_end
            .commands
            .send(Command::LoadPortraits {
                query: "slow tide".to_owned(),
                artists: vec!["spotify:artist:a".to_owned()],
            })
            .expect("portraits");
        started_rx.recv().expect("portraits started");
    };
    let teardown = move || async move {
        // A hung stop must fail the test, not hang the suite.
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("finish must resolve when the stop interrupts the portrait read");
        assert_eq!(
            shared.shutdowns.load(Ordering::SeqCst),
            1,
            "the shutdown must run when the stop interrupts the portrait read"
        );
    };

    run_ordered(runtime, window, teardown);
}

#[test]
fn two_portrait_reads_queued_together_echo_their_own_queries() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    let portraits = vec![portrait("spotify:artist:a")];
    *shared.portrait_script.lock().expect("lock") = PortraitScript::Ready(portraits.clone());

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        for query in ["first", "second"] {
            ui_end
                .commands
                .send(Command::LoadPortraits {
                    query: query.to_owned(),
                    artists: vec!["spotify:artist:a".to_owned()],
                })
                .expect("portraits");
        }
        let mut received = Vec::new();
        for _ in 0..2 {
            received.push(ui_end.updates.recv().expect("portraits"));
        }
        received
    });

    assert_eq!(
        updates,
        vec![
            Update::Portraits {
                query: "first".to_owned(),
                portraits: portraits.clone(),
            },
            Update::Portraits {
                query: "second".to_owned(),
                portraits,
            },
        ]
    );
    assert_eq!(
        *shared.portraited.lock().expect("lock"),
        vec![
            vec!["spotify:artist:a".to_owned()],
            vec!["spotify:artist:a".to_owned()],
        ],
        "each read asks for its own artists, in the order sent"
    );
}

#[test]
fn a_scripted_search_echoes_the_query_it_was_asked() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    let tracks = vec![track("spotify:track:a")];
    *shared.search_script.lock().expect("lock") = SearchScript::Ready(tracks.clone());
    // An interior double space: the echo must be the command's string
    // byte for byte, because the window clears its in-flight marker on a
    // match and would otherwise never ask again.
    let query = "never  gonna";

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::Search {
                query: query.to_owned(),
            })
            .expect("search");
        ui_end.updates.recv().expect("results")
    });

    assert_eq!(
        update,
        Update::Search {
            query: query.to_owned(),
            tracks,
        }
    );
    assert_eq!(shared.searches.load(Ordering::SeqCst), 1);
    assert_eq!(
        *shared.searched.lock().expect("lock"),
        vec![query.to_owned()],
        "the worker must ask for the query the window sent"
    );
}

#[test]
fn a_failed_search_is_announced() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.search_script.lock().expect("lock") = SearchScript::Fail;

    let update = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::Search {
                query: "never gonna".to_owned(),
            })
            .expect("search");
        ui_end.updates.recv().expect("failure")
    });

    assert_eq!(
        update,
        Update::SearchFailed {
            query: "never gonna".to_owned(),
            failure: Failure::Search,
        }
    );
    assert_eq!(shared.searches.load(Ordering::SeqCst), 1);
}

#[test]
fn a_stop_during_a_search_shuts_down() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.search_script.lock().expect("lock") = SearchScript::Pending;
    let (started_tx, started_rx) = channel();
    *shared.search_started.lock().expect("lock") = Some(started_tx);

    let runtime = runtime();
    let (ui_end, _slot, worker) = start(runtime.handle(), host, store, source);

    let window = move || {
        loop {
            if matches!(ui_end.updates.recv().expect("update"), Update::Playlists(_)) {
                break;
            }
        }
        ui_end
            .commands
            .send(Command::Search {
                query: "never gonna".to_owned(),
            })
            .expect("search");
        started_rx.recv().expect("search started");
    };
    let teardown = move || async move {
        // A hung stop must fail the test, not hang the suite.
        tokio::time::timeout(Duration::from_secs(1), worker.finish())
            .await
            .expect("finish must resolve when the stop interrupts the search");
        assert_eq!(
            shared.shutdowns.load(Ordering::SeqCst),
            1,
            "the shutdown must run when the stop interrupts the search"
        );
    };

    run_ordered(runtime, window, teardown);
}

#[test]
fn two_back_to_back_searches_announce_their_own_queries() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    let tracks = vec![track("spotify:track:a")];
    *shared.search_script.lock().expect("lock") = SearchScript::Ready(tracks.clone());

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        let mut received = Vec::new();
        for query in ["first", "second"] {
            ui_end
                .commands
                .send(Command::Search {
                    query: query.to_owned(),
                })
                .expect("search");
            received.push(ui_end.updates.recv().expect("results"));
        }
        received
    });

    assert_eq!(
        updates,
        vec![
            Update::Search {
                query: "first".to_owned(),
                tracks: tracks.clone(),
            },
            Update::Search {
                query: "second".to_owned(),
                tracks,
            },
        ]
    );
    assert_eq!(
        *shared.searched.lock().expect("lock"),
        vec!["first".to_owned(), "second".to_owned()],
        "the two reads must be asked for in the order they were sent"
    );
}

#[test]
fn two_searches_queued_before_either_reply_announce_their_own_queries() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    let tracks = vec![track("spotify:track:a")];
    *shared.search_script.lock().expect("lock") = SearchScript::Ready(tracks.clone());

    let updates = run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        // Both reads are queued before either answer is read: a window
        // that is typing does not wait for the first reply to ask again.
        for query in ["first", "second"] {
            ui_end
                .commands
                .send(Command::Search {
                    query: query.to_owned(),
                })
                .expect("search");
        }
        let mut received = Vec::new();
        for _ in 0..2 {
            received.push(ui_end.updates.recv().expect("results"));
        }
        received
    });

    assert_eq!(
        updates,
        vec![
            Update::Search {
                query: "first".to_owned(),
                tracks: tracks.clone(),
            },
            Update::Search {
                query: "second".to_owned(),
                tracks,
            },
        ]
    );
    assert_eq!(
        *shared.searched.lock().expect("lock"),
        vec!["first".to_owned(), "second".to_owned()],
        "the queued reads are asked for in the order they were sent"
    );
}

/// Runs one liked-tracks command through the live path and returns the
/// update the window receives.
fn run_liked(script: LikedScript) -> Update {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let list = vec![playlist("spotify:playlist:a", "A")];
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(list));
    let shared = Arc::clone(&host.shared);
    *shared.liked_script.lock().expect("lock") = script;

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_updates(&ui_end, 2);
        ui_end
            .commands
            .send(Command::LoadLikedTracks)
            .expect("load");
        ui_end.updates.recv().expect("liked tracks")
    })
}
