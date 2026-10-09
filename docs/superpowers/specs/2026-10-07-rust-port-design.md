# alpaca-code Rust port — design spec

Date: 2026-10-07
Status: approved design; implementation plan to follow (superpowers writing-plans)

## 1. Purpose

Rewrite the alpaca-code GTK4 desktop app (currently PyGObject, ~3.7k lines, 13 modules) in
Rust. Motivations, from the user:

- **Distribution** — single compiled binary, no Python runtime; faster startup.
- **Codebase quality / learning** — typed codebase, no PyGObject dynamic-trap class of bugs.
- **Runtime performance** — GUI code not interpreted.

Goal is **full behavioral parity**: same look, same measured pixel polish, same behavior,
same state file. The Python app stays in the repo and the launcher keeps working until the
Rust port reaches parity ("parallel port", 3 stages, usable early).

## 2. Coexistence rules (during the port)

| Thing | Rust port | Why |
|---|---|---|
| Package / binary | `alpaca-code-rs/`, binary `alpaca-code-rs` | never clobbers `bin/alpaca-code` |
| GtkApplication id | `io.alpaca.rs` | single-instance forward is per app-id for the session: sharing
`io.alpaca.code` would make a Rust launch join the running Python instance (and vice versa) |
| State file | **shared**: `~/.config/alpaca-code/state.json` | recents/tabs shared across both apps; schema unchanged |
| Python code | untouched by the port | parallel port = no churn in the reference implementation |

Final branding (whether the Rust app eventually inherits `io.alpaca.code` / the
`alpaca-code` name) is an open cutover decision, decided when Python retires — not during
stages 1–3.

## 3. Stack

- `relm4` 0.9 (with macros) over `gtk4` 0.9 — component/model/update/view structure the
  user chose (idiomatic restructure, not a line-by-line port).
- `sourceview5` 0.9, `vte4` 0.9 — the VTE-GTK4 build and GtkSourceView 5 this distro ships
  (the same libs the Python app binds to).
- `serde` + `serde_json` (state.json), `libc` (killpg/environ).
- **No libadwaita** — the app is a plain GTK app with forced dark + its own CSS; Adw would
  fight the theme we currently defeat (Breeze).
- No speculative dependencies. Stage 3 adds an SVG-path-parsing crate only if the VecIcon
  port turns out to need one.

Subcommand for runctl/git porcelain: `std::process::Command`, synchronous, as in Python.

## 4. Architecture

### 4.1 relm4 component graph

```
App (root Component; owns window, state, workspace path)
├── HeaderBar        — in App component: Run/Stop cells, File/Project menus, (git?) panel
├── Editor           : Component   GtkSourceView notebook, tabs, save, dirty/delete-watches
├── FileTree         : Component   workspace browser TreeView (the workspace mode's view)
├── Panes            : Component   Agent/Terminal/Output VTE notebook + run lifecycle
└── GitPanel         : Component   owns the browser-card mode strip (WORKSPACE/CHANGES
│                                   tabs), the changes view + commit bar, live editing,
│                                   and the branch pill/popover menu        (Stage 2)
```

In Python `gitview.py` hosts both browser-card modes; in Rust the mode strip is owned by
the GitPanel component, which swaps the card's child between the FileTree view and the
CHANGES view.
```

Components talk through relm4 senders (typed inputs/outputs). Mutable run state stays
inside the Panes component; the App receives run-state outputs (status pill, buttons).
No shared-state macros unless a measured need appears.

### 4.2 Module map (Python → Rust → stage)

| Python | Rust | Stage | Notes |
|---|---|---|---|
| `bin/alpaca-code` | bin target `alpaca-code-rs` | S1 | CLI arg = project dir; no arg → `state.last_project` |
| `main.py` CSS/theme/app | `main.rs` + `style.rs` + `assets/app.css` | S1 | CSS string embeds via `include_str!` |
| `window.py` | `app.rs` (App component) | S1 | layout, workspace dirty gate, File menu, Run/Stop, New Project |
| `editor.py` | `editor.rs` | S1 | |
| `filetree.py` + `treehover.py` | `filetree.rs` | S1 | TreeView+TreeStore kept; **no** ListView/factory rewrite; workspace mode only |
| `panels.py` + VTE plumbing | `panes.rs` | S1 | run state machine, kill-tree, respawn budget |
| `runctl.py` | `runctl.rs` (pure) | S1 | `pane_environ`, npm/dotnet detect |
| `state.py` | `state.rs` | S1 | serde, tolerant load |
| `badges.py` (pixbuf path) | `badges.rs` | S1 | pixbufs from SVG assets at exact display sizes |
| `gitstatus.py` (pure parser) | `gitstatus.rs` (pure) | S2 | |
| `gitview.py` | `gitview.rs` | S2 | Workspace/CHANGES views, commit bar, live editing features |
| `branchmenu.py` | `branchmenu.rs` | S2 | status-bar branch pill + popover |
| `vector.py` (Gsk paintable) | `vector.rs` | S3 | VecIcon port; replaces widget-side pixbufs |

### 4.3 The one structural improvement the port buys

Python's cross-module reach-ins (`window._refresh_tab_state`, `panels` pid plumbing,
`gitview` reaching into the store) become typed messages between components. Nothing else
gets restructured: the widget tree, the state machine, the CSS, and the data files are
ported as they are. The rewrite's point is language and structure around the same design,
not a redesign of a design that already works.

## 5. Assets

- `assets/app.css` — the CSS from `main.py` copied verbatim (Python keeps its inline copy;
  manual propagation if either side changes during the port).
- `assets/style-schemes/alpaca-dark.xml`, `assets/icons/*.svg` — copied, embedded via
  `include_str!`/`include_bytes!`; nothing reads repo paths at runtime.
- StyleSchemeManager needs a real directory: on startup, if
  `$XDG_DATA_HOME/alpaca-code-rs/style-schemes/` lacks the scheme (or its content differs),
  write it there, then `append_search_path` that dir. Works identically in dev runs and for
  an installed binary.
- Icon rendering in S1/S2: pixbufs rendered at the exact cell sizes from `assets/icons/`
  (accepts the old pixbuf-resample blur — Stage 3 replaces widget icons with the VecIcon
  paintable port; TreeView cells stay pixbuf regardless, per the measured ruling).

## 6. Behavior fidelity — carried invariants

GTK-level and kernel-level rulings carry **verbatim** (they belong to GTK/libc, not
PyGObject). Source of truth is the invariants/gotchas list in the repo `CLAUDE.md`; the
ones the Rust port must reproduce explicitly:

- Theme setup: `prefer-dark-theme`, forced `gtk-decoration-layout` (KDE/Wayland supplies
  none → invisible window buttons), CSS provider at **USER** priority, dark scheme path.
- VTE spawn flags `SEARCH_PATH | SEARCH_PATH_FROM_ENVP`, never DEFAULT; envv merge
  semantics (merged onto inherited environ — override-to-"" to scrub, empty values
  satisfy claude's truthiness checks); `pane_environ` marker list + `~/.local/bin` PATH
  append carried as the same pure function.
- Process kill-tree: `killpg(getpgid(pid), SIGHUP)` + ~2s SIGKILL escalation; liveness by
  exit status, not `/proc` existence (zombies).
- Run state machine: `_run_starting` async window covered by "running", Stop in that
  window arms stop-pending and kills the landed pid, cleanup exits not confused with user
  exits, pane respawn budget (3, ≥60s re-arm, re-entry re-arm, <3s = broken child).
- Editor: `set_modified(false)` after open (same trap in Rust), `readable_text`
  non-UTF-8/NUL rejection, delete-watches (GIO FileMonitor) + dirty conflict rules.
- Notebook: `set_current_page()` after `append_page` (does not self-switch).
- Cell/renderer properties set **before** store fills (cached at insert).
- CSS specifics stay untouched: box-shadow kills on notebook nodes, `margin: 0`
  load-bearing on tabs, padding-not-margin on rule-carrying widgets, popovers left bare,
  `label:backdrop` restatements, button metric sweep + re-raise at USER priority.

PyGObject-only traps do **not** carry (absent in Rust): wrapper identity instability,
property-by-attribute no-ops, stale `get_current_page` inside switch-page, `grab_focus`
select-all/re-arm quirks, `timeout_add_seconds` dead-on-this-build (both timeouts use
millisecond form in Rust anyway).

**Fidelity rule:** Rust widgets differ from Python wrappers in measured-corner cases, so
any surprising new visual gets re-measured on the existing probe rig (spectacle ×2 +
KWin Scripting geometry dump), never inherited from a Python-era number.

## 7. Error handling

- `state.rs`: corrupt/wrong-typed/absent state never bricks startup — full
  serde-structural, coerce-or-default mirror of `state.py` (`recents` string-filtered,
  per-project tabs coerced, `active_tab` int-or-0).
- Missing git / non-repo workspace: app opens fully, git panel shows its empty state
  (same as Python).
- Run detection failure: no Run offer (status reflects it), as today.
- Editor still refuses non-text files at open (lossy-buffer-rewrite guard).

## 8. Testing

- `cargo test` — displayless unit tests for the pure surface, mirroring
  `tests/test_selfcheck.py` scope: state coercion cases, `runctl` detect + `pane_environ`
  marker/PATH behavior, gitstatus porcelain parsing, kill-tree classification logic.
- Widget behavior keeps the probe discipline: throwaway probe programs against the live
  display, drive handlers directly, assert numerically; pixel work continues on the
  spectacle+KWin rig (Stage 3 owns the re-derivations).
- Manual smoke per stage against the same real project dirs used for the Python app.

## 9. Stages and acceptance criteria

| Stage | Ships | Acceptance |
|---|---|---|
| **S1 — usable core** | binary, App shell + headerbar + File menu + New Project + workspace dirty gate, Editor (tabs/save/dirty/watches/binary reject), FileTree (browse/search/monitors/hover), Panes (Agent `$SHELL`, Terminal, Output run lifecycle, Run/Stop, statuses, kill-tree, respawns), state.json shared, assets embedded | cargo run → daily-usable editor+agent workspace; run/stop a npm project; state round-trips shared with the Python app without corrupting each other |
| **S2 — git features** | gitstatus parser, Workspace status view, CHANGES view + commit bar + live-edit behavior, branch pill + Switch/New Branch popover, status refresh triggers | same flows as today's app on this repo; parser unit-tested |
| **S3 — pixel parity + polish** | `_TabDistributor` port (equal-share tab alloc), ink-centering laws (padding-bottom 3px/2px, tree-cell yalign), VecIcon paintable port for widgets, parity sweep | probe-rig comparisons vs the Python app show matching metrics; narrow-window tab behavior passes the responsive cases |

Each stage ends committed and reviewed before the next begins. Stages 2–3 reuse the
module map above; no new stages unless hidden complexity upgrades mid-stage (ratchet is
one-way — stop and re-classify).

## 10. Explicit non-goals

- Changing application look, behavior, or the mockup design "while we're at it" — parity
  only; improvements go into separate tasks on either codebase.
- ListView/factory rewrite of the file tree, libadwaita, theming away from Breeze.
- Packaging (PKGBUILD/Flatpak) — post-cutover work once a single app exists to package.
- Keeping Python's test suite working through the port — it keeps working by virtue of
  Python being untouched; new tests are Rust-side.