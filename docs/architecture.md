# Architecture

A GTK4 desktop editor wrapping the raw `claude` CLI: tabbed GtkSourceView editor + file browser + three VTE panes (Agent = raw claude TUI, Terminal = `$SHELL`, Output = npm/dotnet run), Run/Stop buttons, workbench launcher. Stack: relm4 0.11 + gtk4 0.11 (`v4_22`), sourceview5 0.11, vte4 0.10, glib/gio 0.22, gdk-pixbuf 0.22, serde/serde_json. App-id `io.alpaca.rs`. Single binary crate, no lib.rs, no build.rs.

## Module map

Hubs (`src/main.rs` → group hub files, named-file style):

- **`ui/`** — the window shell and top-level relm4 Components:
  - `app.rs` — App component: header (menu chevron, project pill strip, run/stop), the main GtkStack (`launcher` page + one page per Workspace), message routing, state save/quit paths, and all `ALPACA_PROBE_*` probe chains. Routes everything; can't be bypassed.
  - `editor.rs` — per-project GtkNotebook: source tabs, diff pages (side-by-side tinted mono, cross-linked scroll, number panes), dirty dots, save flush chain, TabDistributor tab-width law, clip ops, live file monitors.
  - `launcher.rs` — workbench hub GtkStack page: card per `workbenches/*.json` (most-recent-mtime first), rename/delete popovers, `+` naming prompt, boot adopt flow.
  - `panes.rs` — the three VTE panes + run state machine, respawn budget, kill_tree.
- **`git/`** — git features: `gitstatus.rs` (pure porcelain/diff parser + subprocess runners), `gitview.rs` (CHANGES view: checkbox tree, commit bar, flight; selection lenses to VISIBLE rows only — invisible files never drive a commit; toggles follow the visible-descendant law), `gitpanel.rs` (WORKSPACE/CHANGES mode card: search entry, status row, 2s live probe, hosts the others), `branchmenu.rs` (branch pill + Switch/Pull/New-Branch popover with hysteresis timers).
- **`fs/`** — file browsing: `filetree.rs` (lazy TreeStore, monitors, search open-set memory), `treehover.rs` (HoverTree + press capture mixin, shared by filetree + gitview).
- **`render/`** — pixels and styles: `vector.rs` (SVG → VecIcon paintable via gsk::Path), `badges.rs` (file-type pixbuf cache, from seti), `seti.rs` (baked seti mapping table), `style.rs` (include_str of `assets/app.css` + style scheme, provider setup).
- **`support/`** — non-GTK: `state.rs` (state.json + workbench files: tolerant parse — per-element and wholesale-default gates, atomic write, sanitize), `runctl.rs` (pane spawn env, run-command detection: dotnet runnable = class libraries sort before the Web host and the runnable must win regardless of name order, with Directory.Build.props / lexical-with-csproj fallbacks; new-project dirs must not pre-exist).

Key cross-references: app → everything; editor/gitview/launcher/branchmenu/panes → render; gitpanel → gitview/filetree/branchmenu/gitstatus; filetree → treehover. All internal refs are `crate::`-qualified.

## Message routing model

- AppMsg routing is index-tagged over workspace slots (`ws: Vec<Option<Workspace>>` tombstone — slots never shift, tags never stale).
- Components report by OUTPUT (outputs carry state; App cannot query a component synchronously). Sub-component outputs bridge via guarded sends (`input_sender().send(...)` — see [invariants.md](invariants.md), relm4 section).
- Everything open = one `Workspace` (own Editor/Panes/GitPanel + hpaned assembly); the window child is a GtkStack with transition None; hidden pages stay ALIVE (VTE children, probes, monitors keep running; nothing is ever re-rooted — no dirty-gate dialogs).
- Workspace lifecycle: open (adopt-or-cap 3 dialog) → active swing → close reads `live_pids()` and kills synchronously before drop → tombstone → zero-live shows the launcher.
- Commit flow: Commit → `pending_commit` → editor SaveOpen (flushes tabs) → Flushed → gitstatus commit (see invariants for the index-removed law) → Phase/Landed.
- Save/quit: every close path (Ctrl+Shift+S, menu, X button, Exit/Ctrl+Q) funnels into `App::save_all` (tabs flush, window geom + last_workbench into state.json, full session per project into workbench files).
- Boot: workbench launcher page (CLI `[dir]` argument boots straight into a project instead); `last_workbench` file is adopted only if it loads.

## Persistence

`~/.config/alpaca-code/state.json` (atomic temp+rename):

```json
{"last_project": "/path|null", "recents": ["/path", …cap 10, newest first],
 "open_projects": ["/path", …cap 3, opened-order],
 "projects": {"/path": {"open_tabs": ["rel/path", …], "active_tab": 0}},
 "last_workbench": "<uuid>|null", "window": {"width": 1586, "height": 992, "maximized": false}}
```

`~/.config/alpaca-code/workbenches/<uuid>.json` (one per workbench; stem = uuid, display = user name):

```json
{"name": "<uuid>", "display": "<user name>|null",
 "projects": {"/abs/project": {
  "language": "rust|null",
  "vpane": 592, "hpane": 1161,
  "scroll": {"rel": "src/main.rs", "frac": 0.37},
  "expanded": ["/abs/project/sub"],
  "changes_closed": ["src"]}}}
```

Load never raises (coerce to defaults). 0/neg paned values = "not saved". Save reads live state only (vadj, tree open sets, gitview closed set) — nothing mirrors in App.

## Assets pipeline

- `assets/app.css` — the whole custom stylesheet, loaded by `style.rs` via include_str; `assets/style-schemes/alpaca-dark.xml` — GtkSourceView scheme (`def:*` namespace law).
- `assets/icons/seti-*.svg` (~197) — frozen one-shot bake of seti-ui mapping.less (see invariants, Icons and assets). `assets/icons/*.svg` (8 app icons) — authored 1:1 art consumed by `vector::icon` via `badges::svg_data` (include_bytes). `assets/alpaca-80.png` — llama masthead, built once via PixbufLoader → gdk::Texture.
- Tree cells / CHANGES rows / tab badges consume pixbufs (badges.rs); everything else uses VecIcon paintables.

## Tests

All inline `#[cfg(test)]` (70 fns; heaviest: gitstatus 21, state 12), run displayless with `cargo test` — no `tests/` dir. Display tests are the probe chains (see [probes.md](probes.md)). Unit tests must never initialize GTK (`LanguageManager::default()` aborts displayless: "Gtk has to be initialized").

Known widget-layer gap: bookkeeping bugs can pass all unit tests because they live in widget code (the respawn-band bug shipped past 21 green tests) — widget-trust requires a live probe, never tests alone.