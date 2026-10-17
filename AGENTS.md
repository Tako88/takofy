# Takofy

A Spotify client for Windows and Linux. Desktop only — no phone, no web build,
no touch. Rust, with `librespot` speaking Spotify's own protocol and
`eframe`/`egui` drawing. One binary, no runtime, no Electron. A private
repository, not distributed. Not affiliated with Spotify.

The shape of the system is in `docs/architecture.md`.

<!--
A stack detail earns a place here only when a rule depends on it. The reasoning
behind the stack lives in that document, not here.
-->

## Workflow and tracking

All remaining work lives in **GitHub issues** — the roadmap, phase work, one-off
features, bugs, design judgements, human actions. Nothing is tracked by hand in
the repository.

- **One tracking issue per phase**, labelled `phase`, carrying the exit criterion,
  the open items and the blockers. Its state is the only place a phase's progress
  is recorded; nothing else reports it. A feature or bug that is not part of a
  phase gets its own issue and its own label.
- **The pinned roadmap issue maps the phases**, in order, one line each saying
  what the phase is for and linking to its tracking issue. It carries no status,
  so closing a phase never edits it. A phase is added there when it gets its
  issue, not before.
- **A phase closes when the commit that verifies its exit criterion lands on
  `main` carrying `Closes #N`.** Nothing is marked done by hand.
- **Branches**: one per phase (`<phase-slug>`), and one per milestone off it
  (`<phase-slug>-<m2>-<slice>`). A milestone merges into its phase branch when the
  milestone is done; the phase branch merges into `main` when the phase's exit
  criterion holds. Process docs — this file — are committed to `main` directly,
  because they belong to no phase.
- **Plans and scratch are local.** `.pi/plans/<slice>/` holds the audited
  contract, its adversarial audit, its gate evidence and the milestone extracts.
  `.pi/scratch/` holds spikes, throwaway probes and diagnostic scripts; what a
  spike finds belongs in an issue before the probe is discarded. All of `.pi/` is
  gitignored, because it churns and is not repo content.
- **An issue reads as plain language.** Written for someone who has not opened
  the code: what is wanted, why it matters, how you will know it is done. No
  shorthand that only means something inside this repository.
- **A commit message is short too.** A subject that says what changed, then only
  what the diff cannot show — why, and what surprised you. Not a narration of the
  diff, and not a summary of the session that produced it.

## Testing

TDD for logic with defined inputs and outputs. Write the test first. What counts
as logic depends on the stack; the shape does not: parsers, formatters, address
and date handling, ranking, thread or graph reconstruction, state transitions,
the storage and query layer, anything that touches credentials or user data.

Not TDD: appearance, layout, density, and typography. These are judged by looking
at them. Test UI only where there is a contract — keyboard navigation, stores,
reducers, state machines.

Every non-trivial module ships with its test in the same change.

`cargo test` runs the suite. `cargo test -p <crate> <name>` runs one file or case.

## Modules

Single responsibility. A module or component gets split when it:

- has two or more reasons to change, or
- needs a mock it should not need, or
- imports from two unrelated domains.

Line count is a smell, not a rule. Do not split on size alone.

Anti-rules, both of these are bugs:

- God files. One module doing sync, parsing, and storage.
- File soup. Fifteen 8-line files with re-exports and no readable flow.
  If following one operation means opening six files, it is over-split.

UI seam: data/state, formatting, presentation are three separate things.
Formatting lives in pure functions and is tested. Presentational components may
be long and still single-purpose.

The crate boundaries, the threading model and the invariants are in
`docs/architecture.md`. The rule that falls out of them: a view that needs data
is handed it, because `takofy-ui` cannot go and get it.

## DRY

DRY applies to duplicated knowledge, not duplicated-looking code. Before merging
two similar blocks, ask: would these ever need to change for different reasons?
If yes, keep them separate — an abstraction with a mode flag is worse than the
duplication. Otherwise prefer rule of three.

## Comments

Short, and only where the code does not already say it. A good name and
signature are the explanation; a comment that repeats them makes the reader read
twice. One line is the norm, two usually means the code should be clearer
instead. Where a comment earns its place it says why, or states a constraint the
type system cannot carry. It never narrates the lines below it.

- No plan identifiers: `D7`, `M4`, `G1`, `D-H`, "step 25", "obligation 3", "Gate A".
  They point at a plan that is local and churns, so they mean nothing to the next
  reader.
- No history: "since", "used to", "found in", "the old …". Provenance belongs in
  the commit message.
- Durable references only: an issue (`#50`).

## Design system

If the project has a UI, it has a token layer and nothing else. Every colour,
size, weight, spacing step, radius and duration is defined once as a token in the
theme file. Components consume tokens; they never invent values.

- No magic numbers or literal colours in a component. Not "only when it looks
  better" — never.
- Need a new value? Add it to the token layer first, then use it. A token used
  exactly once is a smell: extend an existing scale instead.
- Contrast and dark mode are properties of the token layer, not of components.
  A component never branches on theme.

Tokens live in `crates/takofy-ui/src/theme.rs`. Enforcement is mechanical:
`cargo clippy --workspace --all-targets -- -D warnings` fails on a literal
`Color32` in any other file, through `disallowed-methods` in `clippy.toml`, and
that file is the one place the allow is lifted. Needing a literal twice means a
missing token, not a reason to write the literal.

## Layout

Target is desktop only: Windows and Linux. Responsive means the window was
resized, never a phone, so there are no touch affordances.

Breakpoints live in `crates/takofy-ui/src/layout.rs` and nowhere else. Region
visibility is driven by `layout::regions_for`, so no view measures the window and
there is nothing to keep in sync. `layout::shell_for` is the only place a region
width appears, and it expresses that width in tokens.

## Secrets

Never commit credentials, tokens or keys. Never read or print them. Keep them
behind an abstraction — `takofy-core`'s `CredentialStore` — never a loose string
in the UI or the binary. Credentials go to the OS keychain, not to a file. Never
log credentials or the contents of user data. The operator enters secrets, not an
agent.

## Conventions

- Rust 2024, with no `unsafe`. `unsafe_code` is denied. No `unwrap` or `expect`
  outside tests and startup, where failing loudly is the point.
- rustc 1.99, pinned in `rust-toolchain.toml`. `librespot` 0.8.0 and
  `eframe`/`egui` 0.37.0. Versions live in `[workspace.dependencies]`; a crate
  never names one.
- egui immediate mode. There is no stylesheet and no CSS: `theme::style()` builds
  the egui `Style` from the tokens, and components read tokens directly.
- Pure functions in files that import no I/O, so they are testable alone.
- One job per file, named for the job, not for the layer.
