# Arch packages

Two recipes, both built from the release the tag names:

- `takofy/` compiles the tagged source tree with the committed lockfile. Slow
  (a Rust release build with fat LTO), and the one to install if you want the
  binary to match your machine's CPU.
- `takofy-bin/` installs the binary from the release's Linux tarball. Seconds.

They conflict with each other and both provide `takofy`.

Neither is on the AUR yet. When they are, this directory is the source of truth
and the AUR gets a copy.

## Bumping for a release

Three numbers change, and they must agree:

1. `pkgver` in both `PKGBUILD`s. AUR's `pkgver` cannot contain a hyphen (it
   separates the release), so a release candidate is `0.1.0rc1` while its tag is
   `v0.1.0-rc1`. A plain release has both: `pkgver=0.1.0`, `_tag=v0.1.0`.
2. `_tag`, the tag itself, in both.
3. `sha256sums`: the tag archive for the source package, the Linux tarball for
   `-bin`. `makepkg -g` prints them, or take them from the release's
   `sha256sums.txt`.

Then regenerate the metadata AUR actually reads:

```
cd packaging/aur/takofy && makepkg --printsrcinfo > .SRCINFO
cd ../takofy-bin && makepkg --printsrcinfo > .SRCINFO
```

## Why the source recipe turns makepkg's LTO off

`options=('!lto')` is load-bearing. makepkg adds `-flto` to `CFLAGS` by default,
that reaches the C and assembly that a build script compiles (ring, behind
rustls), and the link then fails on undefined `ring_core_*` symbols. The Rust
profile asks for its own LTO, so nothing is lost.

## Why there is no `check()`

The suite runs on every push in CI, on both platforms, against the committed
lockfile. Running 4,420 tests inside every install adds a minute and a half, and
turns any flake into a build a user cannot fix. #58 is the one flake that has
been seen.

## What the packages install

```
/usr/bin/takofy
/usr/share/applications/io.github.Tako88.Takofy.desktop
/usr/share/icons/hicolor/{scalable,16x16,...,512x512}/apps/io.github.Tako88.Takofy.{svg,png}
/usr/share/licenses/takofy/{LICENSE,OFL-*.txt}
```

The id in the desktop entry's `Icon=` and `StartupWMClass=` is the one the window
sets (`app::APP_ID`), and a test fails if the two drift apart.

## Before the first submission

- A release that carries `share/`. The `v0.1.0-rc1` tarball predates the desktop
  entry and the icons, so `takofy-bin` needs a newer tag — the release workflow
  packs `share/` from now on.
- AppStream metainfo (`io.github.Tako88.Takofy.metainfo.xml`) for Flathub. The
  AUR does not need it; a store does.
