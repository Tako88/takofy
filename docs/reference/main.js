/* ==========================================================================
   Music client — application behaviour.
   Progressive and dependency-free: everything below enhances markup that is
   already meaningful without it (links keep working, artwork still renders).
   ========================================================================== */

(function () {
  "use strict";

  /* ----------------------------------------------------------------------
     Icon sprite — one definition for the whole app, injected so no page has
     to repeat 30 <symbol> blocks.
     ---------------------------------------------------------------------- */

  var SPRITE = [
    // transport
    '<symbol id="i-play" viewBox="0 0 24 24"><path d="M7.5 4.6v14.8L20 12z" fill="currentColor" stroke="none"/></symbol>',
    '<symbol id="i-pause" viewBox="0 0 24 24"><path d="M7.4 4h3.4v16H7.4zM13.2 4h3.4v16h-3.4z" fill="currentColor" stroke="none"/></symbol>',
    '<symbol id="i-prev" viewBox="0 0 24 24"><path d="M6.4 5v14" fill="none" stroke="currentColor"/><path d="M19 5.7v12.6L9.7 12z" fill="currentColor" stroke="none"/></symbol>',
    '<symbol id="i-next" viewBox="0 0 24 24"><path d="M17.6 5v14" fill="none" stroke="currentColor"/><path d="M5 5.7v12.6L14.3 12z" fill="currentColor" stroke="none"/></symbol>',
    '<symbol id="i-shuffle" viewBox="0 0 24 24"><path d="M3.4 7h4.4l8.5 10h4.3" fill="none" stroke="currentColor"/><path d="M3.4 17h4.4l8.5-10h4.3" fill="none" stroke="currentColor"/><path d="M17.7 4.3 20.8 7l-3.1 2.7" fill="none" stroke="currentColor"/><path d="M17.7 14.3 20.8 17l-3.1 2.7" fill="none" stroke="currentColor"/></symbol>',
    '<symbol id="i-repeat" viewBox="0 0 24 24"><path d="M6.6 17.5h10.8A2.6 2.6 0 0 0 20 14.9v-1.4" fill="none" stroke="currentColor"/><path d="M9.2 20.2 6.4 17.5l2.8-2.7" fill="none" stroke="currentColor"/><path d="M17.4 6.5H6.6A2.6 2.6 0 0 0 4 9.1v1.4" fill="none" stroke="currentColor"/><path d="M14.8 3.8l2.8 2.7-2.8 2.7" fill="none" stroke="currentColor"/></symbol>',
    // library + navigation
    '<symbol id="i-home" viewBox="0 0 24 24"><path d="M12 2.9 2.6 10.4V20a1.4 1.4 0 0 0 1.4 1.4h4.6v-6.2h6.8v6.2H20a1.4 1.4 0 0 0 1.4-1.4v-9.6z" fill="currentColor" stroke="none"/></symbol>',
    '<symbol id="i-search" viewBox="0 0 24 24"><circle cx="10.6" cy="10.6" r="6.6" fill="none" stroke="currentColor" stroke-width="2"/><path d="M15.6 15.6 20.6 20.6" fill="none" stroke="currentColor" stroke-width="2"/></symbol>',
    '<symbol id="i-library" viewBox="0 0 24 24"><path d="M4 4.6h2.7v14.8H4zM9.1 4.6h2.7v14.8H9.1zM14.7 5.4l2.6-.7 3.9 14.3-2.6.7z" fill="currentColor" stroke="none"/></symbol>',
    '<symbol id="i-panel" viewBox="0 0 24 24"><rect x="3" y="4.5" width="18" height="15" rx="2" fill="none" stroke="currentColor"/><path d="M9.5 4.5v15" fill="none" stroke="currentColor"/></symbol>',
    '<symbol id="i-mic" viewBox="0 0 24 24"><rect x="8.8" y="2.8" width="6.4" height="11.4" rx="3.2" fill="none" stroke="currentColor"/><path d="M5.4 11.4a6.6 6.6 0 0 0 13.2 0M12 18.2v3M8.6 21.2h6.8" fill="none" stroke="currentColor"/></symbol>',
    // arrows
    '<symbol id="i-arrow-left" viewBox="0 0 24 24"><path d="M20 12H4.6M11 5.4 4.4 12 11 18.6" fill="none" stroke="currentColor"/></symbol>',
    '<symbol id="i-arrow-right" viewBox="0 0 24 24"><path d="M4 12h15.4M13 5.4 19.6 12 13 18.6" fill="none" stroke="currentColor"/></symbol>',
    '<symbol id="i-chevron-down" viewBox="0 0 24 24"><path d="M6 9.4 12 15.4 18 9.4" fill="none" stroke="currentColor"/></symbol>',
    '<symbol id="i-chevron-right" viewBox="0 0 24 24"><path d="M9.4 6 15.4 12 9.4 18" fill="none" stroke="currentColor"/></symbol>',
    // actions
    '<symbol id="i-heart" viewBox="0 0 24 24"><path d="M20.4 5.9a5.05 5.05 0 0 0-7.1 0L12 7.2l-1.3-1.3a5.05 5.05 0 1 0-7.1 7.2l8.4 8.4 8.4-8.4a5.05 5.05 0 0 0 0-7.2z" fill="none" stroke="currentColor"/></symbol>',
    '<symbol id="i-heart-filled" viewBox="0 0 24 24"><path d="M20.4 5.9a5.05 5.05 0 0 0-7.1 0L12 7.2l-1.3-1.3a5.05 5.05 0 1 0-7.1 7.2l8.4 8.4 8.4-8.4a5.05 5.05 0 0 0 0-7.2z" fill="currentColor" stroke="none"/></symbol>',
    '<symbol id="i-more" viewBox="0 0 24 24"><circle cx="5" cy="12" r="1.7" fill="currentColor" stroke="none"/><circle cx="12" cy="12" r="1.7" fill="currentColor" stroke="none"/><circle cx="19" cy="12" r="1.7" fill="currentColor" stroke="none"/></symbol>',
    '<symbol id="i-plus" viewBox="0 0 24 24"><path d="M12 5.4v13.2M5.4 12h13.2" fill="none" stroke="currentColor"/></symbol>',
    '<symbol id="i-download" viewBox="0 0 24 24"><path d="M12 3.6v11.6M7.6 10.6 12 15l4.4-4.4M4.6 19.6h14.8" fill="none" stroke="currentColor"/></symbol>',
    '<symbol id="i-close" viewBox="0 0 24 24"><path d="M6 6 18 18M18 6 6 18" fill="none" stroke="currentColor"/></symbol>',
    '<symbol id="i-check" viewBox="0 0 24 24"><path d="M4.8 12.6 9.6 17.4 19.2 6.8" fill="none" stroke="currentColor" stroke-width="2"/></symbol>',
    '<symbol id="i-verified" viewBox="0 0 24 24"><circle cx="12" cy="12" r="9.6" fill="currentColor" stroke="none"/><path d="M7.6 12.3 10.7 15.3 16.5 9.2" fill="none" stroke="#fff" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round"/></symbol>',
    // output + volume
    '<symbol id="i-volume" viewBox="0 0 24 24"><path d="M4 9.4h3.6L12 5.4v13.2L7.6 14.6H4z" fill="currentColor" stroke="none"/><path d="M15.4 9.2a4.2 4.2 0 0 1 0 5.6M18.1 6.6a8 8 0 0 1 0 10.8" fill="none" stroke="currentColor"/></symbol>',
    '<symbol id="i-volume-low" viewBox="0 0 24 24"><path d="M4 9.4h3.6L12 5.4v13.2L7.6 14.6H4z" fill="currentColor" stroke="none"/><path d="M15.4 9.2a4.2 4.2 0 0 1 0 5.6" fill="none" stroke="currentColor"/></symbol>',
    '<symbol id="i-volume-mute" viewBox="0 0 24 24"><path d="M4 9.4h3.6L12 5.4v13.2L7.6 14.6H4z" fill="currentColor" stroke="none"/><path d="M15.6 9.6 20.4 14.4M20.4 9.6 15.6 14.4" fill="none" stroke="currentColor"/></symbol>',
    '<symbol id="i-devices" viewBox="0 0 24 24"><rect x="2.8" y="4.4" width="12.4" height="9.4" rx="1.5" fill="none" stroke="currentColor"/><path d="M6.6 18.2h6M9 13.8v4.4" fill="none" stroke="currentColor"/><rect x="17.4" y="8.8" width="4" height="10.4" rx="1.4" fill="none" stroke="currentColor"/></symbol>',
    '<symbol id="i-queue" viewBox="0 0 24 24"><path d="M3.4 6.4h11M3.4 12h11M3.4 17.6h6.6" fill="none" stroke="currentColor"/><path d="M17 17.2V7.4l4.2-1.1v9.8" fill="none" stroke="currentColor"/><ellipse cx="15.4" cy="17.2" rx="1.8" ry="1.5" fill="currentColor" stroke="none"/><ellipse cx="19.6" cy="16.1" rx="1.8" ry="1.5" fill="currentColor" stroke="none"/></symbol>',
    '<symbol id="i-expand" viewBox="0 0 24 24"><path d="M4 9.4V4h5.4M20 9.4V4h-5.4M4 14.6V20h5.4M20 14.6V20h-5.4" fill="none" stroke="currentColor"/></symbol>',
    '<symbol id="i-clock" viewBox="0 0 24 24"><circle cx="12" cy="12" r="8.2" fill="none" stroke="currentColor"/><path d="M12 7.4V12l3.2 2" fill="none" stroke="currentColor"/></symbol>',
    '<symbol id="i-grid" viewBox="0 0 24 24"><rect x="3.6" y="3.6" width="7.2" height="7.2" rx="1.2" fill="currentColor" stroke="none"/><rect x="13.2" y="3.6" width="7.2" height="7.2" rx="1.2" fill="currentColor" stroke="none"/><rect x="3.6" y="13.2" width="7.2" height="7.2" rx="1.2" fill="currentColor" stroke="none"/><rect x="13.2" y="13.2" width="7.2" height="7.2" rx="1.2" fill="currentColor" stroke="none"/></symbol>',
    '<symbol id="i-list" viewBox="0 0 24 24"><path d="M4 6.6h2.4M9.4 6.6H20M4 12h2.4M9.4 12H20M4 17.4h2.4M9.4 17.4H20" fill="none" stroke="currentColor" stroke-width="2"/></symbol>',
    // Identity mark: a solid geometric T monogram. The geometric vocabulary of
    // a music UI is crowded (play, add, queue, close), so the mark claims no
    // icon meaning at all — it just signs the app.
    '<symbol id="i-logo" viewBox="0 0 24 24"><path d="M2.5 3h19v4.5H14.5V21h-5V7.5H2.5z" fill="currentColor" stroke="none"/></symbol>',
  ].join("");

  // Injected the moment the script runs (the tag sits at the end of <body>) so
  // icons are on screen before first paint — no icon-less flash.
  if (document.body) {
    document.body.insertAdjacentHTML("afterbegin", '<svg aria-hidden="true" focusable="false" style="position:absolute;width:0;height:0;overflow:hidden">' + SPRITE + "</svg>");
  }

  /* ----------------------------------------------------------------------
     Data — the user's library and the play queue.
     ---------------------------------------------------------------------- */

  var LIBRARY = [
    { kind: "playlist", name: "Late Night Drive", sub: "Playlist · Vela Moreno", art: "img/covers/signal-decay.jpg", href: "playlist.html" },
    { kind: "playlist", name: "Discover Weekly", sub: "Playlist · Made for you", art: "img/covers/discover-weekly.jpg", href: "playlist.html" },
    { kind: "playlist", name: "Release Radar", sub: "Playlist · Made for you", art: "img/covers/release-radar.jpg", href: "playlist.html" },
    { kind: "album", name: "Slow Tide", sub: "Album · Vela Moreno", art: "img/covers/slow-tide.jpg", href: "album.html" },
    { kind: "playlist", name: "Windows Down", sub: "Playlist · Made for you", art: "img/covers/long-way-down.jpg", href: "playlist.html" },
    { kind: "artist", name: "Vela Moreno", sub: "Artist", art: "img/artists/vela-moreno.jpg", href: "artist.html" },
    { kind: "playlist", name: "Deep Focus", sub: "Playlist · Made for you", art: "img/covers/cold-storage.jpg", href: "playlist.html" },
    { kind: "podcast", name: "Field Notes", sub: "Podcast · Northern Ridge Radio", art: "img/covers/pod-field-notes.jpg", href: "podcasts.html" },
    { kind: "playlist", name: "Neon Rain", sub: "Playlist · Made for you", art: "img/covers/harbour-lights.jpg", href: "playlist.html" },
    { kind: "album", name: "Blue Hour", sub: "Album · Hana Sō", art: "img/covers/blue-hour.jpg", href: "album.html" },
    { kind: "playlist", name: "Tape Hiss", sub: "Playlist · Made for you", art: "img/covers/paper-rooms.jpg", href: "playlist.html" },
    { kind: "artist", name: "Nils Aker", sub: "Artist", art: "img/artists/nils-aker.jpg", href: "artist.html" },
    { kind: "playlist", name: "Night Bus", sub: "Playlist · Vela Moreno", art: "img/covers/nocturne-field.jpg", href: "playlist.html" },
    { kind: "podcast", name: "The Long Way Round", sub: "Podcast · Wanderline", art: "img/covers/pod-long-way-round.jpg", href: "podcasts.html" },
    { kind: "album", name: "Cold Storage", sub: "Album · Nils Aker", art: "img/covers/cold-storage.jpg", href: "album.html" },
    { kind: "playlist", name: "First Light", sub: "Playlist · Made for you", art: "img/covers/northlight.jpg", href: "playlist.html" },
    { kind: "playlist", name: "Sunday Morning Coffee", sub: "Playlist · Made for you", art: "img/covers/fieldwork.jpg", href: "playlist.html" },
    { kind: "artist", name: "Coastal Divide", sub: "Artist", art: "img/artists/coastal-divide.jpg", href: "artist.html" },
    { kind: "playlist", name: "Salt & Static", sub: "Playlist · Made for you", art: "img/covers/saltglass.jpg", href: "playlist.html" },
    { kind: "album", name: "Halcyon Street", sub: "Album · The Rookery", art: "img/covers/halcyon-street.jpg", href: "album.html" },
    { kind: "playlist", name: "Frostbitten", sub: "Playlist · Made for you", art: "img/covers/wintering.jpg", href: "playlist.html" },
    { kind: "podcast", name: "Rooftop Sessions", sub: "Podcast · Halcyon Audio", art: "img/covers/pod-rooftop-sessions.jpg", href: "podcasts.html" },
    { kind: "podcast", name: "Two Cars, One Road", sub: "Podcast · Grease & Chrome Media", art: "img/covers/pod-two-cars.jpg", href: "podcasts.html" },
    { kind: "playlist", name: "Long Way Home", sub: "Playlist · Made for you", art: "img/covers/dune-song.jpg", href: "playlist.html" },
    { kind: "album", name: "Terminal", sub: "Album · The Rookery", art: "img/covers/terminal.jpg", href: "album.html" },
    { kind: "playlist", name: "Iron & Rain", sub: "Playlist · The Rookery", art: "img/covers/chrome-gospel.jpg", href: "playlist.html" },
    { kind: "artist", name: "Marta Ísleif", sub: "Artist", art: "img/artists/marta-isleif.jpg", href: "artist.html" },
    { kind: "playlist", name: "After Hours", sub: "Playlist · Made for you", art: "img/covers/undertow.jpg", href: "playlist.html" }
  ];

  var QUEUE = [
    { name: "Cotton Light", artist: "Vela Moreno", art: "img/covers/slow-tide.jpg", duration: 198 },
    { name: "Kestrel", artist: "Vela Moreno", art: "img/covers/slow-tide.jpg", duration: 236 },
    { name: "Low Country", artist: "Fen & Farrow", art: "img/covers/low-country.jpg", duration: 271 },
    { name: "Wintering", artist: "Coastal Divide", art: "img/covers/wintering.jpg", duration: 324 },
    { name: "Row of Pines", artist: "Nils Aker", art: "img/covers/nocturne-field.jpg", duration: 289 },
    { name: "Glovebox", artist: "The Rookery", art: "img/covers/halcyon-street.jpg", duration: 214 },
    { name: "Amber Room", artist: "Vela Moreno", art: "img/covers/saltglass.jpg", duration: 247 }
  ];

  /* ----------------------------------------------------------------------
     Helpers
     ---------------------------------------------------------------------- */

  function $(sel, root) {
    return (root || document).querySelector(sel);
  }

  function $$(sel, root) {
    return Array.prototype.slice.call((root || document).querySelectorAll(sel));
  }

  function fmtTime(total) {
    total = Math.max(0, Math.round(total));
    var m = Math.floor(total / 60);
    var s = total % 60;
    return m + ":" + (s < 10 ? "0" : "") + s;
  }

  function fmtLong(total) {
    var h = Math.floor(total / 3600);
    var m = Math.round((total % 3600) / 60);
    return h ? h + " hr " + m + " min" : m + " min";
  }

  /* One phrasing for the library's size readout, so the unfiltered and
     filtered states can never drift apart. */
  function countLabel(shown, total) {
    return shown === total ? total + " saved" : shown + " of " + total;
  }

  /* A tracklist's readout, which only exists while a query is narrowing the list —
     at rest the list is countable by looking at it, and a page that already says
     "62 songs" in its hero must not be contradicted by a second total. The unit
     comes from the markup because a discography is counted in releases. */
  function trackLabel(shown, total, unit) {
    unit = unit || "tracks";
    return shown + " of " + total + " " + (total === 1 ? unit.replace(/s$/, "") : unit);
  }

  function icon(id, cls) {
    return '<svg class="icon ' + (cls || "") + '" aria-hidden="true"><use href="#' + id + '"></use></svg>';
  }

  /* ----------------------------------------------------------------------
     Player state
     ---------------------------------------------------------------------- */

  var state = {
    title: "Slow Tide",
    artist: "Vela Moreno",
    art: "img/covers/slow-tide.jpg",
    duration: 222,
    time: 74,
    playing: true,
    volume: 0.72,
    shuffled: false,
    repeated: false
  };

  function currentKey() {
    return state.title + "|" + state.artist;
  }

  /* ----------------------------------------------------------------------
     Now-playing surfaces (playbar + side panel)
     ---------------------------------------------------------------------- */

  function renderNowPlaying() {
    $$("[data-now-art]").forEach(function (el) {
      if (el.getAttribute("src") !== state.art) el.setAttribute("src", state.art);
      el.alt = state.title + " cover";
    });
    $$("[data-now-title]").forEach(function (el) {
      el.textContent = state.title;
    });
    $$("[data-now-artist]").forEach(function (el) {
      el.textContent = state.artist;
    });

    var pct = (state.time / state.duration) * 100;
    $$("[data-seek-fill], [data-mini-fill]").forEach(function (el) {
      el.style.width = pct.toFixed(2) + "%";
    });
    $$("[data-seek]").forEach(function (el) {
      el.setAttribute("aria-valuenow", Math.round(state.time));
      el.setAttribute("aria-valuemax", state.duration);
      el.setAttribute("aria-valuetext", fmtTime(state.time) + " of " + fmtTime(state.duration));
      var thumb = $(".slider__thumb", el);
      if (thumb) thumb.style.left = pct.toFixed(2) + "%";
    });

    var elapsed = $("[data-elapsed]");
    if (elapsed) elapsed.textContent = fmtTime(state.time);
    var remaining = $("[data-remaining]");
    if (remaining) remaining.textContent = "-" + fmtTime(state.duration - state.time);

    var big = $("[data-transport-icon]");
    if (big) big.setAttribute("href", state.playing ? "#i-pause" : "#i-play");

    var volumeIcon = $("[data-volume-icon]");
    if (volumeIcon) {
      volumeIcon.setAttribute("href", state.volume === 0 ? "#i-volume-mute" : state.volume < 0.4 ? "#i-volume-low" : "#i-volume");
    }
  }

  function renderPlayIcons() {
    $$("[data-play-icon]").forEach(function (use) {
      var trigger = use.closest("[data-track]");
      var active = trigger && trigger.getAttribute("data-track") + "|" + trigger.getAttribute("data-artist") === currentKey();
      use.setAttribute("href", active && state.playing ? "#i-pause" : "#i-play");
    });
    var transport = $("[data-transport-icon]");
    if (transport) transport.setAttribute("href", state.playing ? "#i-pause" : "#i-play");
  }

  function renderPlayingRows() {
    $$("[data-playing-scope] > *").forEach(function (row) {
      var trigger = $("[data-track]", row) || (row.matches("[data-track]") ? row : null);
      var active = trigger && trigger.getAttribute("data-track") + "|" + trigger.getAttribute("data-artist") === currentKey();
      row.classList.toggle("is-playing", !!active);
      if (row.classList.contains("track-row")) row.classList.toggle("is-paused", !state.playing);
    });
  }

  function renderLikes() {
    var liked = !!$('[data-like][aria-pressed="true"]');
    $$("[data-like]").forEach(function (btn) {
      var on = btn.getAttribute("aria-pressed") === "true";
      var use = $("use", btn);
      if (use) use.setAttribute("href", on ? "#i-heart-filled" : "#i-heart");
      btn.classList.toggle("is-liked", on);
    });
    void liked;
  }

  function render() {
    renderNowPlaying();
    renderPlayIcons();
    renderPlayingRows();
    renderLikes();
  }

  /* ----------------------------------------------------------------------
     Playback
     ---------------------------------------------------------------------- */

  function play(meta) {
    var same = meta.track + "|" + meta.artist === currentKey();
    if (same) {
      state.playing = !state.playing;
    } else {
      state.title = meta.track;
      state.artist = meta.artist;
      if (meta.art) state.art = meta.art;
      state.duration = meta.duration || 222;
      state.time = meta.time || 0;
      state.playing = true;
    }
    render();
  }

  function transportToggle() {
    state.playing = !state.playing;
    render();
  }

  function nudge(seconds) {
    state.time = Math.min(state.duration, Math.max(0, state.time + seconds));
    render();
  }

  function next(inQueue) {
    if (!QUEUE.length) return;
    var idx = 0;
    for (var i = 0; i < QUEUE.length; i++) {
      if (QUEUE[i].name === state.title && QUEUE[i].artist === state.artist) {
        idx = (i + 1) % QUEUE.length;
        break;
      }
    }
    var t = QUEUE[idx];
    state.title = t.name;
    state.artist = t.artist;
    state.art = t.art;
    state.duration = t.duration;
    state.time = 0;
    state.playing = true;
    void inQueue;
    render();
  }

  function previous() {
    if (state.time > 4) {
      state.time = 0;
      render();
      return;
    }
    nudge(-state.time);
  }

  var lastTick = 0;
  function tick(now) {
    if (!lastTick) lastTick = now;
    var delta = (now - lastTick) / 1000;
    lastTick = now;
    if (state.playing) {
      state.time += delta;
      if (state.time >= state.duration) {
        next(true);
      } else {
        renderNowPlaying();
      }
    }
    window.requestAnimationFrame(tick);
  }

  /* ----------------------------------------------------------------------
     Sliders (seek + volume): click, drag and keyboard
     ---------------------------------------------------------------------- */

  function bindSlider(el, opts) {
    if (!el) return;

    function ratioFromEvent(event) {
      var box = el.getBoundingClientRect();
      var x = (event.touches ? event.touches[0].clientX : event.clientX) - box.left;
      return Math.min(1, Math.max(0, x / box.width));
    }

    function apply(event) {
      opts.onChange(ratioFromEvent(event));
    }

    var dragging = false;
    el.addEventListener("pointerdown", function (event) {
      dragging = true;
      el.classList.add("is-active");
      el.setPointerCapture(event.pointerId);
      apply(event);
    });
    el.addEventListener("pointermove", function (event) {
      if (dragging) apply(event);
    });
    el.addEventListener("pointerup", function (event) {
      dragging = false;
      el.classList.remove("is-active");
      if (el.hasPointerCapture(event.pointerId)) el.releasePointerCapture(event.pointerId);
    });
    el.addEventListener("pointercancel", function () {
      dragging = false;
      el.classList.remove("is-active");
    });

    el.addEventListener("keydown", function (event) {
      var step = (opts.step || 5) * (event.shiftKey ? 4 : 1);
      var value = opts.value();
      if (event.key === "ArrowRight" || event.key === "ArrowUp") {
        opts.onChange(Math.min(1, (value + step) / opts.max()));
      } else if (event.key === "ArrowLeft" || event.key === "ArrowDown") {
        opts.onChange(Math.max(0, (value - step) / opts.max()));
      } else if (event.key === "Home") {
        opts.onChange(0);
      } else if (event.key === "End") {
        opts.onChange(1);
      } else {
        return;
      }
      event.preventDefault();
    });
  }

  /* ----------------------------------------------------------------------
     Hero tint — the header colour is genuinely sampled from the artwork.
     ---------------------------------------------------------------------- */

  function tintFromArtwork(hero) {
    var img = $("img[data-tint]", hero);
    if (!img || !window.HTMLCanvasElement) return;

    function sample() {
      try {
        var canvas = document.createElement("canvas");
        canvas.width = 20;
        canvas.height = 20;
        var ctx = canvas.getContext("2d", { willReadFrequently: true });
        ctx.drawImage(img, 0, 0, 20, 20);
        var data = ctx.getImageData(0, 0, 20, 20).data;
        var r = 0;
        var g = 0;
        var b = 0;
        var n = 0;
        for (var i = 0; i < data.length; i += 4) {
          if (data[i + 3] < 8) continue;
          r += data[i];
          g += data[i + 1];
          b += data[i + 2];
          n++;
        }
        if (!n) return;
        r /= n;
        g /= n;
        b /= n;

        // rgb -> hsl, then normalise to a tone that reads as a tinted header.
        r /= 255;
        g /= 255;
        b /= 255;
        var max = Math.max(r, g, b);
        var min = Math.min(r, g, b);
        var l = (max + min) / 2;
        var s = max === min ? 0 : l > 0.5 ? (max - min) / (2 - max - min) : (max - min) / (max + min);
        var h = 0;
        if (max !== min) {
          if (max === r) h = ((g - b) / (max - min) + (g < b ? 6 : 0)) / 6;
          else if (max === g) h = ((b - r) / (max - min) + 2) / 6;
          else h = ((r - g) / (max - min) + 4) / 6;
        }
        // Same hue, mirrored tone: 16–34% lightness is a tinted band under bone
        // ink on black, and a slate band under charcoal ink on paper. The light
        // theme takes the artwork's colour near the top of the scale instead,
        // with the saturation pulled in harder — over paper a 40% wash of a
        // mid-saturation tint reads as a coloured header block, not as a tint.
        var isLight = document.documentElement.getAttribute("data-theme") === "light";
        var tone = Math.min(0.34, Math.max(0.16, l));
        var band = isLight ? 0.94 - tone * 0.2 : tone;
        var saturation = Math.min(isLight ? 0.32 : 0.55, s * 1.5);
        var tint = "hsl(" + Math.round(h * 360) + ", " + Math.round(saturation * 100) + "%, " +
          Math.round(band * 100) + "%)";
        hero.style.setProperty("--hero-tint", tint);
      } catch (err) {
        /* Cross-origin or canvas-less: the CSS fallback tint stands. */
      }
    }

    if (img.complete && img.naturalWidth) sample();
    else img.addEventListener("load", sample, { once: true });
  }

  /* ----------------------------------------------------------------------
     Sidebar library + filters
     ---------------------------------------------------------------------- */

  function renderLibrary() {
    var list = $("[data-library-list]");
    if (!list) return;
    list.innerHTML = LIBRARY.map(function (item) {
      var cls = item.kind === "artist" ? " library-item--artist" : item.kind === "podcast" ? " library-item--podcast" : "";
      return (
        '<li data-kind="' + item.kind + '"><a class="library-item' + cls + '" href="' + item.href + '">' +
        '<img class="library-item__art" src="' + item.art + '" alt="" loading="lazy">' +
        '<span class="library-item__text">' +
        '<span class="library-item__name truncate">' + item.name + "</span>" +
        '<span class="library-item__kind truncate">' + item.sub + "</span></span></a></li>"
      );
    }).join("") +
      // The filter can empty this list, and an empty column with no sentence in
      // it reads as a bug rather than as a result.
      '<li class="library__empty" id="library-list-empty" hidden></li>';

    setLibraryCount(countLabel(LIBRARY.length, LIBRARY.length));
  }

  /* The size readout is the field's resting label (its placeholder) and, at the
     same time, a status node a screen reader hears while the query is typed —
     the visible half of it is covered by the caret the moment you start typing. */
  function setLibraryCount(text) {
    var countEl = $("[data-library-count]");
    if (countEl) countEl.textContent = text;
    var input = $("[data-library-filter]");
    if (input) input.setAttribute("placeholder", text);
  }

  function libraryQuery() {
    var input = $("[data-library-filter]");
    return input ? input.value.trim() : "";
  }

  function activeKind(group) {
    var chip = $('[data-chip][aria-pressed="true"]', group);
    return chip ? chip.getAttribute("data-chip") : "";
  }

  function renderQueue() {
    var list = $("[data-queue-list]");
    if (!list) return;
    list.innerHTML = QUEUE.map(function (t) {
      return (
        '<li class="queue-item truncate" role="button" tabindex="0" data-track="' + t.name + '"' +
        ' data-artist="' + t.artist + '" data-art="' + t.art + '" data-duration="' + t.duration + '"' +
        ' aria-label="Play ' + t.name + ' by ' + t.artist + '">' +
        '<img class="queue-item__art" src="' + t.art + '" alt="" loading="lazy">' +
        '<span><span class="queue-item__name truncate">' + t.name + '</span>' +
        '<span class="queue-item__artist truncate">' + t.artist + '</span></span>' +
        '<span class="queue-item__dur">' + fmtTime(t.duration) + "</span></li>"
      );
    }).join("");
  }

  function bindChips(root) {
    $$("[data-chip]", root).forEach(function (chip) {
      chip.addEventListener("click", function () {
        var group = chip.closest("[data-chip-group]") || document;
        var wasPressed = chip.getAttribute("aria-pressed") === "true";

        // Rows that carry an explicit "All" pill are exclusive: one is always
        // active, and "All" is the documented way back out.
        if (wasPressed && group.hasAttribute("data-chip-exclusive")) return;

        $$("[data-chip]", group).forEach(function (other) {
          other.setAttribute("aria-pressed", "false");
        });
        chip.setAttribute("aria-pressed", wasPressed ? "false" : "true");
        var active = !wasPressed ? chip.getAttribute("data-chip") : "";
        applyFilter(group, active);
      });
    });
  }

  function applyFilter(group, kind) {
    var target = group.getAttribute("data-chip-target");
    if (!target) return;
    var scope = document.querySelector(target);
    if (!scope) return;
    if (kind == null) kind = activeKind(group);

    // Two axes, one list: the chips narrow by kind, the field narrows by name,
    // and they are cumulative rather than competing. "Artists" plus "har" is one
    // row, which is what a person typing into a filtered column expects.
    var query = libraryQuery().toLowerCase();
    var items = $$("[data-kind]", scope);
    var shown = 0;
    items.forEach(function (item) {
      var byKind = !kind || item.getAttribute("data-kind") === kind;
      var byName = !query || item.textContent.toLowerCase().indexOf(query) > -1;
      var match = byKind && byName;
      item.hidden = !match;
      if (match) shown++;
    });

    var empty = document.querySelector(target + "-empty");
    if (empty) {
      empty.textContent = query ? "Nothing matches “" + libraryQuery() + "”" : "Nothing here";
      empty.hidden = shown > 0;
    }

    // Groups that own a size readout keep it honest as either axis changes.
    var text = countLabel(shown, items.length);
    var countSel = group.getAttribute("data-count-target");
    if (countSel) {
      var countEl = document.querySelector(countSel);
      if (countEl) countEl.textContent = text;
    }
    setLibraryCount(text);
  }

  /* One field, bound per page: typing re-runs the chip filter with whatever kind
     is already pressed, and Escape or the clear control puts the library back. */
  function bindLibraryFilter() {
    var input = $("[data-library-filter]");
    if (!input) return;
    var group = input.closest(".sidebar") && $("[data-chip-group]", input.closest(".sidebar"));
    if (!group) return;
    var clear = $("[data-library-filter-clear]");

    function run() {
      if (clear) clear.hidden = input.value === "";
      applyFilter(group);
    }

    input.addEventListener("input", run);

    input.addEventListener("keydown", function (event) {
      if (event.key !== "Escape") return;
      event.preventDefault();
      input.value = "";
      run();
    });

    if (clear) {
      clear.addEventListener("click", function () {
        input.value = "";
        run();
        input.focus();
      });
    }

    // Browsers restore search fields on back-navigation; the list has to agree
    // with the box the moment the page comes back.
    run();
  }

  /* A page-level list filter: the same field as the sidebar's, pointed at one
     list instead of the library. Numbers are left alone on purpose — in a
     playlist a row's number is the track's position, which is worth keeping.
     Nothing here renumbers, and nothing moves: rows are hidden, not removed. */
  function bindListFilters() {
    $$("[data-list-filter]").forEach(function (form) {
      var input = $("input", form);
      var selector = form.getAttribute("data-list-target");
      var scope = selector ? document.querySelector(selector) : null;
      if (!input || !scope) return;

      var clear = $("[data-list-filter-clear]", form);
      var countEl = $("[data-list-count]", form.parentElement) || $("[data-list-count]");
      var root = form.closest(".tracks") || form.closest(".section") || document;
      var emptyEl = $("[data-list-empty]", root);
      var headEl = scope.parentElement ? scope.parentElement.querySelector(":scope > .tracklist__head") : null;
      var footEl = $("[data-list-foot]");
      // The chips own a richer empty state of their own (a heading and a
      // sentence); the text filter speaks through a plain note, and only one of
      // the two is ever on screen.
      var chipEmpty = document.querySelector(selector + "-empty");
      var unit = form.getAttribute("data-list-unit") || "tracks";
      var rows = Array.prototype.slice.call(scope.children);

      // A field that shares its row with kind chips has to respect them: the two
      // axes are cumulative, exactly as they are in the sidebar.
      var chipGroup = form.closest("[data-chip-group]");
      var chips = chipGroup ? $$("[data-chip]", chipGroup) : [];

      function chipKind() {
        var pressed = chipGroup ? $('[data-chip][aria-pressed="true"]', chipGroup) : null;
        return pressed ? pressed.getAttribute("data-chip") : "";
      }

      function run() {
        var query = input.value.trim();
        var needle = query.toLowerCase();
        var kind = chipKind();
        var shown = 0;

        rows.forEach(function (row) {
          var byKind = !kind || row.getAttribute("data-kind") === kind;
          var byName = !needle || row.textContent.toLowerCase().indexOf(needle) > -1;
          var match = byKind && byName;
          row.hidden = !match;
          if (match) shown++;
        });

        // A column header over nothing is a table of no data; it goes with the
        // empty sentence, not instead of it.
        if (headEl) headEl.hidden = query !== "" && shown === 0;
        if (countEl) countEl.textContent = query ? trackLabel(shown, rows.length, unit) : "";
        if (emptyEl) {
          if (query) emptyEl.textContent = "Nothing in this list matches “" + query + "”";
          emptyEl.hidden = query === "" || shown > 0;
        }
        if (chipEmpty) chipEmpty.hidden = query !== "" || shown > 0;
        // The list's own footer counts the whole list; while a query narrows it,
        // the count in the tool row is the truthful one.
        if (footEl) footEl.hidden = query !== "";
        if (clear) clear.hidden = query === "";
      }

      input.addEventListener("input", run);
      input.addEventListener("keydown", function (event) {
        if (event.key !== "Escape") return;
        event.preventDefault();
        input.value = "";
        run();
      });

      // Bound after the chip handler, so the pressed state has already changed
      // by the time this re-applies both axes.
      chips.forEach(function (chip) {
        chip.addEventListener("click", run);
      });

      if (clear) {
        clear.addEventListener("click", function () {
          input.value = "";
          run();
          input.focus();
        });
      }

      run();
    });
  }

  /* ----------------------------------------------------------------------
     View toggle (grid / list) on the library page
     ---------------------------------------------------------------------- */

  function bindViewToggle() {
    var grid = $("[data-library-grid]");
    if (!grid) return;
    $$("[data-view]").forEach(function (btn) {
      btn.addEventListener("click", function () {
        var mode = btn.getAttribute("data-view");
        $$("[data-view]").forEach(function (other) {
          other.setAttribute("aria-pressed", other === btn ? "true" : "false");
        });
        grid.classList.toggle("library-grid--list", mode === "list");
      });
    });
  }

  /* ----------------------------------------------------------------------
     Loading demo — concept page only. Replays a staged load so the skeleton
     geometry can be compared with the grid that replaces it.
     ---------------------------------------------------------------------- */

  function bindLoadDemo() {
    var demo = $("[data-load-demo]");
    if (!demo) return;
    var out = $("[data-load-out]", demo);
    var skeleton = $("[data-load-skeleton]", demo);
    var content = $("[data-load-content]", demo);
    var status = $("[data-load-status]");
    var replay = $("[data-load-replay]");
    var timer = 0;

    function land() {
      if (out) out.setAttribute("aria-busy", "false");
      skeleton.hidden = true;
      content.hidden = false;
      if (status) status.textContent = "8 of 8 items loaded";
      if (replay) {
        replay.disabled = false;
        replay.textContent = "Replay load";
      }
    }

    function run() {
      window.clearTimeout(timer);
      if (out) out.setAttribute("aria-busy", "true");
      skeleton.hidden = false;
      content.hidden = true;
      if (status) status.textContent = "0 of 8 items loaded";
      if (replay) {
        replay.disabled = true;
        replay.textContent = "Loading…";
      }
      timer = window.setTimeout(land, 2400);
    }

    if (replay) replay.addEventListener("click", run);
  }

  /* ----------------------------------------------------------------------
     Panel tabs
     ---------------------------------------------------------------------- */

  function bindTabs() {
    $$("[data-panel-tab]").forEach(function (tab) {
      tab.addEventListener("click", function () {
        var panel = tab.closest(".now-playing") || document;
        $$("[data-panel-tab]", panel).forEach(function (other) {
          other.setAttribute("aria-selected", other === tab ? "true" : "false");
        });
        $$("[data-panel-pane]", panel).forEach(function (pane) {
          pane.classList.toggle("is-active", pane.id === tab.getAttribute("aria-controls"));
        });
      });
    });
  }

  /* ----------------------------------------------------------------------
     Scroll reactions: sticky headers
     ---------------------------------------------------------------------- */

  function bindScroll() {
    var view = $(".view");
    if (!view) return;
    var topbar = $(".topbar");
    var actionbar = $(".actionbar");
    var sentinelTop = $("[data-scroll-top]");

    /* Rows that hold their place under the actionbar. Their sticky offset is read
       once — it is a token, not something that moves — and compared against where
       the row actually sits on every scroll. */
    var stickyRows = $$("[data-sticky-row]").map(function (row) {
      return { el: row, top: parseFloat(getComputedStyle(row).top) || 0 };
    });

    function update() {
      var scrolled = view.scrollTop;
      if (topbar) topbar.classList.toggle("is-stuck", scrolled > 120);
      if (actionbar && sentinelTop) {
        actionbar.classList.toggle("is-stuck", sentinelTop.getBoundingClientRect().bottom <= 64);
      }
      stickyRows.forEach(function (row) {
        var holding = row.el.getBoundingClientRect().top <= row.top + 1;
        row.el.classList.toggle("is-stuck", holding);
      });
    }

    view.addEventListener("scroll", update, { passive: true });
    update();

    var back = $("[data-back]");
    if (back) back.addEventListener("click", function () { history.back(); });
    var forward = $("[data-forward]");
    if (forward) forward.addEventListener("click", function () { history.forward(); });
  }

  /* ----------------------------------------------------------------------
     Navigation state + sidebar drawer
     ---------------------------------------------------------------------- */

  function markCurrentPage() {
    var page = document.body.getAttribute("data-page");
    if (!page) return;
    $$("[data-nav]").forEach(function (link) {
      if (link.getAttribute("data-nav") === page) link.setAttribute("aria-current", "page");
    });
  }

  function bindDrawer() {
    var toggle = $("[data-drawer-toggle]");
    var sidebar = $(".sidebar");
    if (!toggle || !sidebar) return;
    toggle.addEventListener("click", function () {
      var open = sidebar.classList.toggle("is-open");
      toggle.setAttribute("aria-expanded", open ? "true" : "false");
    });
    document.addEventListener("click", function (event) {
      if (!sidebar.classList.contains("is-open")) return;
      if (sidebar.contains(event.target) || toggle.contains(event.target)) return;
      sidebar.classList.remove("is-open");
      toggle.setAttribute("aria-expanded", "false");
    });
    document.addEventListener("keydown", function (event) {
      if (event.key === "Escape" && sidebar.classList.contains("is-open")) {
        sidebar.classList.remove("is-open");
        toggle.setAttribute("aria-expanded", "false");
        toggle.focus();
      }
    });
  }

  /* The sidebar no longer offers Search, so the topbar field is the only route
     to that view — submitting it goes there. */
  function bindSearch() {
    var form = $(".searchbar");
    if (!form) return;
    form.addEventListener("submit", function (event) {
      event.preventDefault();
      if (window.location.pathname.indexOf("search.html") === -1) {
        window.location.href = "search.html";
      }
    });
  }

  /* ----------------------------------------------------------------------
     Keyboard shortcuts — a client, so: space, shift+arrows, m, and Ctrl/Cmd+F
     for the page's own filter instead of the browser's find
     ---------------------------------------------------------------------- */

  function bindKeys() {
    /* Ctrl/Cmd+F is the app's find, not the browser's. The field it wants is
       already on the page and already knows what it filters, so the shortcut
       means "find in what I am looking at": the list on screen first, the
       library second. It is bound here rather than inside a field because it has
       to work from wherever the focus happens to be. */
    document.addEventListener("keydown", function (event) {
      if (!event.key || event.key.toLowerCase() !== "f") return;
      if (!(event.ctrlKey || event.metaKey) || event.altKey) return;

      var field = $("[data-list-filter] input") || $("[data-library-filter]");
      if (!field) return;

      event.preventDefault();

      // Below 900px the library's field lives in the drawer, and focusing
      // something the eye cannot see helps nobody. The toggle owns that state,
      // so the toggle is what gets pressed.
      var sidebar = field.closest(".sidebar");
      var toggle = $("[data-drawer-toggle]");
      if (sidebar && toggle && getComputedStyle(toggle).display !== "none" && !sidebar.classList.contains("is-open")) {
        toggle.click();
      }

      field.focus();
      // Selecting what is already there makes the second Ctrl+F a reset: type
      // over the old query instead of appending to it.
      field.select();
    });

    document.addEventListener("keydown", function (event) {
      var tag = (event.target.tagName || "").toLowerCase();
      if (tag === "input" || tag === "textarea" || event.target.isContentEditable) return;
      if (event.target.closest('button, a, [role="button"], input, .slider')) return;
      if (event.metaKey || event.ctrlKey || event.altKey) return;

      if (event.key === " ") {
        event.preventDefault();
        transportToggle();
      } else if (event.key === "ArrowRight" && event.shiftKey) {
        next(true);
      } else if (event.key === "ArrowLeft" && event.shiftKey) {
        previous();
      } else if (event.key === "m") {
        state.volume = state.volume > 0 ? 0 : 0.72;
        $$("[data-volume]").forEach(function (el) {
          var fill = $(".slider__fill", el);
          if (fill) fill.style.width = state.volume * 100 + "%";
        });
        render();
      }
    });
  }

  /* ----------------------------------------------------------------------
     Wire everything up
     ---------------------------------------------------------------------- */

  /* ----------------------------------------------------------------------
     Theme — dark is the app; light is the documented inversion. The stored
     value is applied by theme.js in <head>; this only owns the switch.
     ---------------------------------------------------------------------- */

  function applyTheme(theme) {
    var root = document.documentElement;
    if (theme === "light") root.setAttribute("data-theme", "light");
    else root.removeAttribute("data-theme");

    try {
      window.localStorage.setItem("takofy-theme", theme);
    } catch (err) {
      /* No storage: the choice holds for this page and no longer. */
    }

    $$("[data-theme-set]").forEach(function (button) {
      button.setAttribute("aria-pressed", String(button.getAttribute("data-theme-set") === theme));
    });

    // The header band is sampled from the artwork, so it has to be sampled
    // again: the tone that reads under bone ink is a slate band under charcoal.
    $$("[data-tint-scope]").forEach(tintFromArtwork);
  }

  function bindTheme() {
    var buttons = $$("[data-theme-set]");
    if (!buttons.length) return;
    var current = document.documentElement.getAttribute("data-theme") === "light" ? "light" : "dark";
    buttons.forEach(function (button) {
      button.setAttribute("aria-pressed", String(button.getAttribute("data-theme-set") === current));
      button.addEventListener("click", function () {
        var next = button.getAttribute("data-theme-set");
        if (next === "light" || next === "dark") applyTheme(next);
      });
    });
  }

  /* ----------------------------------------------------------------------
     Boot
     ---------------------------------------------------------------------- */

  function init() {
    markCurrentPage();
    renderLibrary();
    renderQueue();
    bindChips(document);
    bindLibraryFilter();
    bindListFilters();
    bindViewToggle();
    bindTabs();
    bindLoadDemo();
    bindDrawer();
    bindSearch();
    bindKeys();

    // Rows that carry their own track data but are not <button>s (the queue)
    // still need Enter and Space.
    document.addEventListener("keydown", function (event) {
      var row = event.target.closest('[data-track][role="button"]');
      if (!row || (event.key !== "Enter" && event.key !== " ")) return;
      event.preventDefault();
      row.click();
    });

    // Order matters: a like or a shuffle sits inside rows that are themselves
    // play triggers, so the more specific intent is tested first.
    document.addEventListener("click", function (event) {
      var like = event.target.closest("[data-like]");
      if (like) {
        var pressed = like.getAttribute("aria-pressed") === "true";
        $$('[data-like][data-like-group="' + (like.getAttribute("data-like-group") || "now") + '"]').forEach(function (other) {
          other.setAttribute("aria-pressed", pressed ? "false" : "true");
        });
        render();
        return;
      }

      var toggleProp = event.target.closest("[data-toggle]");
      if (toggleProp) {
        var on = toggleProp.getAttribute("aria-pressed") === "true";
        toggleProp.setAttribute("aria-pressed", on ? "false" : "true");
        toggleProp.classList.toggle("is-on", !on);
        return;
      }

      // Real links keep behaving like real links.
      if (event.target.closest("a[href]")) return;

      var trigger = event.target.closest("[data-track]");
      if (!trigger) return;
      play({
        track: trigger.getAttribute("data-track"),
        artist: trigger.getAttribute("data-artist") || "",
        art: trigger.getAttribute("data-art"),
        duration: parseInt(trigger.getAttribute("data-duration"), 10) || 222,
        time: parseInt(trigger.getAttribute("data-time"), 10) || 0
      });
    });

    var transport = $("[data-transport]");
    if (transport) transport.addEventListener("click", transportToggle);
    var nextBtn = $("[data-next]");
    if (nextBtn) nextBtn.addEventListener("click", function () { next(true); });
    var prevBtn = $("[data-prev]");
    if (prevBtn) prevBtn.addEventListener("click", previous);

    bindSlider($("[data-seek]"), {
      max: function () { return state.duration; },
      step: 5,
      value: function () { return state.time; },
      onChange: function (ratio) {
        state.time = ratio * state.duration;
        renderNowPlaying();
      }
    });

    $$("[data-volume]").forEach(function (el) {
      bindSlider(el, {
        max: function () { return 1; },
        step: 0.05,
        value: function () { return state.volume; },
        onChange: function (ratio) {
          state.volume = ratio;
          var fill = $(".slider__fill", el);
          if (fill) fill.style.width = ratio * 100 + "%";
          var thumb = $(".slider__thumb", el);
          if (thumb) thumb.style.left = ratio * 100 + "%";
          render();
        }
      });
      var fill = $(".slider__fill", el);
      if (fill) fill.style.width = state.volume * 100 + "%";
      var thumb = $(".slider__thumb", el);
      if (thumb) thumb.style.left = state.volume * 100 + "%";
    });

    $$("[data-tint-scope]").forEach(tintFromArtwork);
    bindTheme();
    bindScroll();
    render();
    window.requestAnimationFrame(tick);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
