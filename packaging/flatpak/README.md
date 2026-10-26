# Flatpak

A local Flatpak, built from this working tree, for seeing the app in the sandbox
every distribution that is not Arch will run it in. There is no Flathub
submission yet and nothing here is what a submission would look like: the source
is `type: dir`, not a tag, because locally you want to build what is on disk.

Three scripts, run with `bash` (they are not executable in the repository, since
nothing here changes file modes):

```
bash packaging/flatpak/sources   # regenerate cargo-sources.json from Cargo.lock
bash packaging/flatpak/build     # build into .build/, leave a .flatpak bundle
bash packaging/flatpak/install   # install the bundle, then launch it
```

`sources` needs a generator that is not part of the build: a Python package in a
throwaway virtualenv, at `.pi/scratch/flatpak/venv` unless `FLATPAK_CARGO_VENV`
says otherwise. `build` refuses to run when the crate set no longer matches
`Cargo.lock` — the alternative is a `no matching package` error ten minutes into
a sandboxed build.

A clean build takes about four minutes and produces a 75 MB bundle; the codecs,
the fonts and the whole crate set are inside it. `.build/` is ignored.

## What the sandbox is given, and why

Every flag is one thing the app actually uses, and the app fails silently
without it:

| flag | for |
| --- | --- |
| `--share=network` | Spotify's own protocol, and the cover CDN over HTTPS |
| `--socket=pulseaudio` | sound: librespot plays through rodio, which is cpal, which is ALSA, and the runtime's ALSA is configured to reach the pulse socket |
| `--socket=wayland`, `--socket=fallback-x11`, `--share=ipc` | the window; eframe is built with both backends and X11 needs the ipc socket |
| `--device=dri` | eframe draws through glow, which is OpenGL |
| `--talk-name=org.freedesktop.secrets` | the refresh token in the user's keyring; without it the app starts at the sign-in card on every launch |

## Known gaps in the sandbox

- **The clipboard does not work.** `arboard` reaches it through
  `ext-data-control-v1` or `zwlr_data_control_v1`, and Flatpak's Wayland proxy
  does not forward those — the host advertises both, the sandbox sees neither,
  so it falls back to X11, which is not there. Copy-link and pasting into the
  search field are therefore dead inside the Flatpak and fine outside it. No
  flag fixes it; it is a clipboard backend question.
- **No AppStream metainfo**, so no store listing and no app-data export. That is
  Flathub's requirement, not a build one, and it belongs to #57.

## When it goes to Flathub

The submission is its own repository, and it needs three things this one does
not have: a `type: git` source pinned to a tag and commit, the metainfo file, and
a generated `cargo-sources.json` kept beside the manifest rather than rebuilt.
That work is item 4 of #57.
