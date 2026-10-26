# Arch packages

Two recipes, both built from the release the tag names:

- `takofy/` compiles the tagged source tree with the committed lockfile. Slow
  (a Rust release build with fat LTO), and the one to install if you want the
  binary to match your machine's CPU.
- `takofy-bin/` installs the binary from the release's Linux tarball. Seconds.

They conflict with each other and both provide `takofy`.

Neither is on the AUR yet. When they are, this directory is the source of truth
and the AUR gets a copy.

## Why the AUR repositories are not inside this one

They cannot be. An AUR package is a repository hosted at
`ssh://aur@aur.archlinux.org/<name>.git`, created by pushing to a name that does
not exist yet; the files it holds are a copy, not the original. Nested clones,
submodules or a `remote` pointing there would all mean this repository's history
depends on two servers it cannot write to without a key, and a contributor's
`git clone` would fail on them.

The recipes here are the original. `push` copies them over:

```
git clone ssh://aur@aur.archlinux.org/takofy.git ~/dev/aur/takofy
git clone ssh://aur@aur.archlinux.org/takofy-bin.git ~/dev/aur/takofy-bin
./packaging/aur/push ~/dev/aur
```

It copies `PKGBUILD` and `.SRCINFO` into each clone, commits `Update to
<version>` when something changed, and pushes. The first push creates the AUR
package.

## Bumping for a release

One command, after the tag exists locally and its release has been published:

```
./packaging/aur/bump v0.1.0
```

It hashes the tag archive and the release tarball, rewrites `pkgver`, `_tag` and
both `sha256sums`, and regenerates each `.SRCINFO`. AUR's `pkgver` cannot carry
a hyphen (it separates the release), so a candidate is `0.1.0rc2` while its tag
is `v0.1.0-rc2`; a plain release has both the same. Running it twice changes
nothing, which is the way to check that the recipes really name the artefacts
that were published.

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

- Nothing blocking: `v0.1.0-rc2` carries `share/`, and both recipes install the
  binary, the desktop entry, the icons and the licences from it.
- AppStream metainfo (`io.github.Tako88.Takofy.metainfo.xml`) for Flathub. The
  AUR does not need it; a store does.
