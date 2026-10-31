# Cutting a release

One page, in the order it happens. The workflow builds the artefacts and
publishes the release; these are the steps around it, and each exists because
skipping it has gone wrong once.

## Before the tag

1. **The changelog has an entry for this version.** `CHANGELOG.md`, under
   `[Unreleased]`, written for the people using Takofy: what changed for them,
   not how it was built. The release notes come from this section, and the
   publish job refuses a tag that has none.
2. **The gates pass on `main`:** `cargo fmt --all -- --check`,
   `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test`.
3. **The version is the one the changelog names.** `version` in the workspace
   `Cargo.toml` is what the release workflow compares the tag against.
4. **The Flatpak stamp matches the lockfile.** A version bump changes
   `Cargo.lock`, and the Flatpak build refuses a stale crate set:
   `bash packaging/flatpak/sources`.

## The tag

5. **Move the entry out of `[Unreleased]`.** Leave `## [Unreleased]` at the top,
   empty; the released entry becomes `## [X.Y.Z] — <date>`; update the two link
   references at the bottom (`[Unreleased]` compares against the new tag, and the
   new tag against the previous one).
6. **Bump the version and the lockfile.** `Cargo.toml` first, then `cargo check`
   to rewrite `Cargo.lock`.
7. **Commit `Release X.Y.Z`:** the version, the changelog and the Flatpak stamp.
   Nothing else — a release commit that also changes behaviour is a release you
   cannot reproduce.
8. **Tag and push.** A lightweight tag `vX.Y.Z` on that commit, then push `main`
   and the tag. The Linux job fails on purpose when the tag and `Cargo.toml`
   disagree.
9. **Watch the run.** Linux tarball, Windows zip, Flatpak bundle, then Publish.
   The publish job reads the changelog's section with `awk` and publishes it as
   the notes.

## After the tag

10. **Check the release page.** Three artefacts plus `sha256sums.txt`, and the
    notes read as the changelog entry (no link definitions, no stray heading).
11. **Point the AUR recipes at the tag:** `bash packaging/aur/bump vX.Y.Z`, then
    commit the two `PKGBUILD`s and their `.SRCINFO`s. Pushing them to the AUR is
    a manual step with its own credentials.
12. **CI on `main` must be green, including the Windows job.** A test that only
    passed on Linux reached `main` once because the local gates cannot see the
    Windows path rules.

## What version to use

- `0.1.0` to `0.1.3` were patch bumps with features riding along. `0.2.0` is the
  first minor, because radio and autoplay were a phase rather than a fix.
- When in doubt, the question is whether a listener would describe the change as
  "more of the same" (patch) or "the app does something new" (minor).
- The notes are the changelog. They are never written twice.
