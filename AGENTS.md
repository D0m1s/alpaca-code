# AGENTS.md

alpaca-code — a GTK4 desktop editor (Rust, relm4) wrapping the raw `claude` CLI: tabbed editor, file browser, VTE panes (Agent/Terminal/Output), workbench launcher. App-id `io.alpaca.rs`, config under `~/.config/alpaca-code/`.

## Commands

```sh
cargo test          # THE gate — displayless, all 70 unit tests
cargo build         # cargo test does NOT refresh the runnable bin
./target/debug/alpaca-code-rs [project-dir]   # no arg → boot launcher hub
```

System deps (Arch): `sudo pacman -S --needed gtksourceview5 vte4`. Release install: `cargo build --release`, then copy `target/release/alpaca-code-rs` to `~/.local/bin/` yourself.

CI: GitHub Actions (`.github/workflows/ci.yml`) runs three hard gates — `test` (`cargo test --locked`, under `xvfb-run`: the VecIcon golden test calls `gtk4::init`, which needs a display and only passes locally via the session's display env), `clippy` (`--all-targets -- -D warnings`), `fmt` — on `runs-on: ubuntu-26.04`, because that runner is the first Ubuntu with GTK 4.22 (the crate's `v4_22` feature floor; the 24.04 image stops at 4.14). Commit hook: `git config core.hooksPath .githooks` once per clone runs the same triple pre-commit (`--no-verify` overrides). Two more layers: `audit.yml` (RustSec vs Cargo.lock, weekly) and dependabot (bumps `cargo` + `github-actions` weekly).

## Repo map

- `src/ui/` — app (window shell + routing + probe chains), editor (GtkNotebook, diff pages), launcher (workbench hub), panes (VTE).
- `src/git/` — gitstatus (pure parser), gitview, gitpanel, branchmenu.
- `src/fs/` — filetree, treehover.
- `src/render/` — vector (SVG paintables), badges, seti (baked icon table), style (css loading).
- `src/support/` — state (persistence), runctl (pane env/process).
- `assets/` — app.css, style scheme, seti/app SVGs (frozen artifacts — never hand-edit the seti bake).
- `docs/` — the three topic files this file routes to.

## Before you touch anything

- **Read [docs/invariants.md](docs/invariants.md)** — every GTK/VTE/relm4/state law below is measured, and this codebase leans on them (e.g. GestureClick claim-on-press, ScrolledWindow adjustment clobber, relm4 `input()` shutdown panic). Never "fix" code that follows one of these laws without re-measuring.
- Read **[docs/architecture.md](docs/architecture.md)** for module map, message routing, persistence schemas, assets pipeline.
- Read **[docs/probes.md](docs/probes.md)** before running any `ALPACA_PROBE_*` chain or any display capture.

## Rules of engagement

- `cargo test` stays green — no behavior change lands otherwise (restructure/move tasks included).
- Probes are env-gated and never run with env unset (single-instance forward loses them); headless runs always use the broadway rig + isolated `XDG_CONFIG_HOME`.
- `state.json` load never raises — corrupt files coerce to defaults, never brick startup.
- Widget behavior is live-verified only: unit tests can pass while widget-layer bugs ship. Probe what you can't test.
- CSS requires a real app restart to observe (single-instance forward shows you the old process).