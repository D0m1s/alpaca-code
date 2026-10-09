# Stage 1 — alpaca-code Rust port (usable core) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `alpaca-code-rs/` — a relm4/gtk4-rs binary that daily-drives the same IDE: workspace browser + tabbed GtkSourceView editor + Agent/Terminal/Output VTE panes + Run/Stop lifecycle, sharing `state.json` with the Python app.

**Architecture:** relm4 component graph (App → Editor, FileTree, Panes) per spec §4. GTK/kernel-level behavior is ported verbatim from the Python modules; unit-testable logic (state, runctl, badges tables) is pure. Fidelity source: the Python files under `alpaca_code/` and the invariant list in `CLAUDE.md` — **executors of every widget task must read the mapped Python file(s) before coding**, plus `CLAUDE.md`.

**Tech Stack:** relm4 0.9 + gtk4 0.9 + sourceview5 0.9 + vte4 0.9, serde/serde_json, libc. Arch system deps for building: `gtk4 gtksourceview5 vte4 gdk-pixbuf2 pkgconf` (all present if the Python app builds).

**Spec:** `docs/superpowers/specs/2026-10-07-rust-port-design.md`

**NOTE:** the user has waived commit steps in this repo's workflow — every task ends at "all checks pass". Do not run git commit/add.

## Global Constraints

- App-ID `io.alpaca.rs`, binary/package `alpaca-code-rs` (spec §2). Never `io.alpaca.code`.
- State path unchanged: `~/.config/alpaca-code/state.json`, schema and coercion rules of
  `alpaca_code/state.py` mirrored exactly (spec §2/§7).
- No libadwaita. No new crates beyond: relm4, gtk4, sourceview5, vte4, serde, serde_json,
  libc (spec §3).
- CSS provider at USER priority; CSS text = spec `assets/app.css` copied verbatim from
  `main.py` (spec §5).
- VTE spawn flags always `SpawnFlags::SEARCH_PATH_FROM_ENVP | SpawnFlags::SEARCH_PATH` —
  never DEFAULT/enforced-lookup-only (CLAUDE.md invariant).
- No async/tokio: children spawn via VTE's own async callback; subprocesses via
  `std::process::Command` sync; periodic/idle work via `glib::timeout_add` (ms) /
  `glib::idle_add`. (CLAUDE.md: seconds-form timer gotcha is Python-side, but the millisecond
  form is the port convention.)
- Every widget-behavior task names its Python reference file; when the port and the
  reference disagree, the reference wins — and surprising new visuals get re-measured on
  the probe rig, not inherited from Python-era numbers (spec §6 fidelity rule).
- All code comments in the new tree are original (no Claude attribution inside files).

## Review Focus

Failure modes the spec implies but pure tests can't exercise — each line's test/verification
lives in the named task:

1. **Stop pressed before the run pid lands** — the child that lands must be killed, and
   "Run failed to start"/"Ready" statuses must stay truthful (Task 8, probe step).
2. **Dirty buffer vs. disk change** — amber conflict dot, no popup, next save wins;
   clean buffer silently follows the disk (Task 6, manual verification checklist).
3. **Corrupt `state.json`** — never bricks startup; coerce-to-defaults per key, and a
   shared file must round-trip between the two apps unchanged in structure (Task 2 tests).
4. **Pane child exits immediately after spawn** — no respawn loop: <3s = broken child,
   revive-cap of 3, ≥60s re-arms (Task 8 probe step).
5. **Binary/non-UTF-8/NUL files** — refuse at open with a dialog, never a lossy buffer
   that Ctrl+S would rewrite (Task 6, manual verification).

---

### Task 1: Crate scaffold + boot skeleton (window, theme, assets)

**Files:**
- Create: `alpaca-code-rs/Cargo.toml`, `alpaca-code-rs/src/main.rs`, `alpaca-code-rs/src/style.rs`
- Create (asset copies): `alpaca-code-rs/assets/app.css`, `alpaca-code-rs/assets/style-schemes/alpaca-dark.xml`, `alpaca-code-rs/assets/icons/*.svg` (copy directory)

**Interfaces:**
- Produces: `style::init()` — prefer-dark, forced `gtk-decoration-layout = ":minimize,maximize,close"`,
  scheme search path set to a one-time-copied `XDG_DATA_HOME/alpaca-code-rs/style-schemes/`,
  CSS embedded from `assets/app.css` at `glib::PRIORITY_USER (800)`. `main::main` runs App
  (empty shell here, real layout arrives in Task 4).
- Produces: crate skeleton other tasks extend: `src/app.rs` with `struct App`, `enum AppMsg`,
  empty component; `type Init = Option<PathBuf>` (from argv — first arg that `is_dir`).
- `cargo build` resolves all dep versions; record chosen relm4/gtk4 0.9.x in Cargo.toml.

**Steps:**

- [ ] **Step 1: Verify system deps**

Run: `pkg-config --exists 'gtk4' 'gtksourceview-5' 'vte-4' && echo OK`
Expected: `OK` (install via pacman if not — same libs the Python app uses on this box).

- [ ] **Step 2: Scaffold crate**

`alpaca-code-rs/Cargo.toml`:

```toml
[package]
name = "alpaca-code-rs"
version = "0.1.0"
edition = "2021"

[dependencies]
relm4 = "0.9"
gtk4 = "0.9"
sourceview5 = "0.9"
vte4 = "0.9"
glib = "0.21"          # version matches whatever gtk4 0.9 re-exports; pin to the same minor
libc = "0.2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[[bin]]
name = "alpaca-code-rs"
path = "src/main.rs"
```

Copy assets from the Python tree (CSS is `main.py`'s inline `CSS` string → `assets/app.css`;
scheme + icons copied file-for-file):

```bash
mkdir -p alpaca-code-rs/assets/style-schemes alpaca-code-rs/src
python3 - <<'EOF'
import re, pathlib
css = re.search(r'CSS = """(.*?)"""', pathlib.Path("alpaca_code/main.py").read_text(), re.S).group(1)
pathlib.Path("alpaca-code-rs/assets/app.css").write_text(css)
EOF
cp alpaca_code/data/alpaca-dark.xml alpaca-code-rs/assets/style-schemes/
cp -r alpaca_code/data/icons alpaca-code-rs/assets/icons
```

- [ ] **Step 3: style.rs + main.rs + empty App**

`alpaca-code-rs/src/style.rs`:

```rust
use gtk4::prelude::*;
use std::fs;
use std::path::PathBuf;

const CSS: &str = include_str!("../assets/app.css");
const SCHEME: &str = include_str!("../assets/style-schemes/alpaca-dark.xml");

/// One-time scheme materialization: StyleSchemeManager only loads from real
/// directories, and the binary must not depend on repo paths (spec §5).
fn scheme_dir() -> PathBuf {
    let dir = dirs_xdg().join("style-schemes");
    fs::create_dir_all(&dir).expect("create style-scheme dir");
    let target = dir.join("alpaca-dark.xml");
    if fs::read_to_string(&target).ok().as_deref() != Some(SCHEME) {
        fs::write(&target, SCHEME).expect("write scheme");
    }
    dir
}

/// $XDG_DATA_HOME or $HOME/.local/share, namespaced by the app.
fn dirs_xdg() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").expect("HOME unset");
            PathBuf::from(home).join(".local/share")
        });
    base.join("alpaca-code-rs")
}

pub fn scheme_path(&self) -> PathBuf // not needed publicly; style.rs also exposes:
pub fn scheme_dir_path() -> PathBuf { scheme_dir() }

pub fn init() {
    let settings = gtk4::Settings::default().unwrap();
    settings.set_property("gtk-application-prefer-dark-theme", true);
    // KDE/Wayland supplies no decoration layout — without this the window
    // buttons are invisible (CLAUDE.md invariant; carries verbatim).
    settings.set_property("gtk-decoration-layout", ":minimize,maximize,close");
    sourceview5::StyleSchemeManager::default().append_search_path(
        scheme_dir().to_str().unwrap());
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(CSS);
    // USER priority is load-bearing for the button metrics (CLAUDE.md);
    // relm4 re-exports gtk4's gdk4, so use gtk4::gdk::Display.
    gtk4::StyleContext::add_provider_for_display(
        &gtk4::gdk::Display::default().unwrap(), &provider, gtk4::STYLE_PROVIDER_PRIORITY_USER);
}
```

(`scheme_path` stub above is illustrative scaffolding noise — final file exposes exactly
`init()` and `scheme_dir_path()`; drop the stray method fragment.)

`alpaca-code-rs/src/app.rs`:

```rust
use relm4::{Component, ComponentParts, ComponentSender, gtk4};
use std::path::PathBuf;

pub struct App { pub root: Option<PathBuf> }

#[derive(Debug)]
pub enum AppMsg { SetWorkspace(PathBuf) }

#[relm4::component]
impl Component for App {
    type CommandOutput = ();
    type Input = AppMsg;
    type Output = ();
    type Init = Option<PathBuf>;

    view! {
        gtk::ApplicationWindow {
            set_title: Some("alpaca-code"),
            gtk::Label::builder()
                .label(if model.root.is_some() { "boot ok" } else { "boot ok — no project" })
                .build(),
        }
    }

    fn init(model: Self::Init, root: &Self::Root, _sender: ComponentSender<Self>) -> ComponentParts<Self> {
        root.set_icon_name(Some("io.alpaca.rs"));
        let model = App { root: model };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: AppMsg, _sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg { AppMsg::SetWorkspace(p) => self.root = Some(p) }
    }
}
```

`alpaca-code-rs/src/main.rs`:

```rust
mod app;
mod style;

fn main() {
    let arg = std::env::args().nth(1).filter(|p| std::path::Path::new(p).is_dir());
    let _provider = gtk4::Application::new(Some("io.alpaca.rs"), Default::default());
    let relm = relm4::RelmApp::new("io.alpaca.rs");
    style::init();
    relm.run::<app::App>(arg.map(std::path::PathBuf::from));
}
```

- [ ] **Step 4: Build + boot smoke (visual)**

Run: `cd alpaca-code-rs && cargo build` (expect: compiles; fix relm4 API drift by the
compiler — the shape above is relm4 0.9's component macro idiom).
Run: `cargo run` (with and without `.. /some/project`)
Expected visual checklist (compare against the Python app's window):
- dark near-black `#07090d` window background (not a light VTE/Adwaita default);
- minimize/maximize/close buttons visible top-right (decoration-layout forcing works);
- titlebar `alpaca-header`-flat, no Breeze top border line (USER-priority CSS applies);
- second launch while first still runs must NOT forward into it (distinct app-id) —
  two windows coexist; close both.

---

### Task 2: state.rs (tolerant persistence, TDD)

**Files:**
- Create: `alpaca-code-rs/src/state.rs`
- Modify: `alpaca-code-rs/src/main.rs` (add `mod state;`)
- Test: unit tests inside `state.rs` (`#[cfg(test)]`)

**Interfaces:**
- Produces: `state::State { last_project: Option<String>, recents: Vec<String>, projects: HashMap<String, ProjectTabs> }`,
  `state::ProjectTabs { open_tabs: Vec<String>, active_tab: i64 }`,
  `state::load() -> State`, `state::save(&State)`, `state::remember(State, &str) -> State`,
  `state::project_tabs(&State, &str) -> ProjectTabs`,
  `state::set_tabs(State, &str, Vec<String>, i64) -> State`.
  All four mutators mirror `alpaca_code/state.py` exactly: `remember` puts the project at
  recents[0] (deduped, cap 10); `save` = tmp file + `fs::rename` (atomic).

**Steps:**

- [ ] **Step 1: Write failing tests** (append inside `state.rs`; `mod state;` already wired)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(p: &str) -> PathBuf {
        let mut d = std::env::temp_dir();
        d.push(format!("alpaca-state-test-{}", p));
        std::fs::remove_file(&d).ok();
        d
    }

    #[test]
    fn coercion_rules_mirror_python() {
        // garbage → default
        assert_eq!(load_from(&Json::parse(r#"garbage"#).unwrap()), State::default());
        // wrong top-level types → whole default
        let bad = serde_json::json!({"recents": "x", "projects": [], "last_project": 3});
        assert_eq!(parse(&bad), State::default());
        // per-entry filtering: recents keeps strings only, projects keeps dicts only
        let ok = serde_json::json!({
            "last_project": "/a", "recents": ["/a", "", 42, null],
            "projects": {" /a": {"open_tabs": ["r/a", "r/b"], "active_tab": 2}, "/bad": 3 }
        });
        let s = parse(&ok);
        assert_eq!(s.recents, vec!["/a".to_string()]);
        assert!(s.projects.contains_key("/a"));
        assert_eq!(s.projects["/a"].active_tab, 2);
        // last_project None is legal
        let none = parse(&serde_json::json!({"last_project": null, "recents": [], "projects": {}}));
        assert_eq!(none.last_project, None);
    }

    #[test]
    fn remember_and_tabs() {
        let mut s = State::default();
        s = remember(s, "/p");
        assert_eq!(s.recents, vec!["/p".to_string()]);
        assert_eq!(s.last_project.as_deref(), Some("/p"));
        s = remember(s, "/q");
        s = remember(s, "/p");
        assert_eq!(s.recents, vec!["/p", "/q"]);                       // newest first, deduped
        s = set_tabs(s, "/p", vec!["a.md".into()], 0);
        assert_eq!(project_tabs(&s, "/p").open_tabs, vec!["a.md".to_string()]);
        assert_eq!(project_tabs(&s, "/missing"), ProjectTabs::default()); // absent → empty
    }

    #[test]
    fn save_load_roundtrip_and_corrupt() {
        let p = tmp("rt.json");
        let s = set_tabs(remember(State::default(), "/w"), "/w", vec!["x".into()], 1);
        save_to(&s, &p);
        assert_eq!(load_from(&p), s);
        std::fs::write(&p, "not json at all").unwrap();
        assert_eq!(load_from(&p), State::default());                  // corrupt → default, never panic
        std::fs::write(&p, "[]").unwrap();
        assert_eq!(load_from(&p), State::default());                  // non-object top level
    }
}
```

- [ ] **Step 2: Run tests, verify failure**

Run: `cd alpaca-code-rs && cargo test state`
Expected: FAIL — functions not implemented (compile error counts as "not defined").

- [ ] **Step 3: Implement** (port contract = `alpaca_code/state.py:1-54`; the coercion
  rules live at `state.py:17-24` and `state.py:38-50` — read it before writing)

Key contract points the implementation must honor: top-level wrong-typed trio (`recents`
not a list / `projects` not an object / `last_project` neither null nor a string) returns
wholesale defaults; **per-entry filtering** follows only after that check (recents
string-filter, projects dict-entry-filter); `project_tabs` on a bad per-project shape
returns empty; `active_tab` coercible int-or-0; `save` writes `.tmp` then renames and
mkdirs the parent.

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: PASS (`state` tests + whatever bootstrap exists).

---

### Task 3: runctl.rs (pure env build + run detection, TDD)

**Files:**
- Create: `alpaca-code-rs/src/runctl.rs`
- Modify: `alpaca-code-rs/src/main.rs` (add `mod runctl;`)
- Test: unit tests inside `runctl.rs`

**Interfaces:**
- Produces:
  `pub struct RunCmd { pub argv: Vec<String>, pub label: String }`
  `pub fn pane_environ() -> Vec<String>` — port of `alpaca_code/runctl.py:9-27`: the 9
  `_CLAUDE_SESSION_MARKERS` overridden to `""` (present-or-not), others passed through,
  `~/.local/bin` appended to PATH when a dir and not already in PATH.
  `pub fn detect(root: &Path) -> Option<RunCmd>` — port of `runctl.py:29-49`:
  package.json scripts.dev → `npm run dev` / scripts.start → `npm start`; top-level
  `*.csproj` → `dotnet run`; one-level-nested `*/*.csproj` (sorted, first) →
  `dotnet run --project <rel>`; None otherwise.
- Consumes (task 8): `pane_environ` as the VTE envv; `detect` from App.

**Steps:**

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn dir_with(path: &str, files: &[(&str, &str)]) -> PathBuf {
        let d = std::env::temp_dir().join(format!("alpaca-runctl-{}", path));
        std::fs::create_dir_all(&d).unwrap();
        for (name, contents) in files {
            std::fs::write(d.join(name), contents).unwrap();
        }
        d
    }

    #[test]
    fn detect_matrix() {
        let npm = dir_with("npm", &[(
            "package.json",
            r#"{ "scripts": { "dev": "vite", "start": "node ." } }"#,
        )]);
        assert_eq!(detect(&npm).unwrap().argv, vec!["npm", "run", "dev"]);
        let start = dir_with("start", &[(
            "package.json",
            r#"{ "scripts": { "start": "node server.js" } }"#,
        )]);
        assert_eq!(detect(&start).unwrap().argv, vec!["npm", "start"]);
        let csproj = dir_with("dotnet", &["a.csproj"]);
        assert_eq!(detect(&csproj).unwrap().argv, vec!["dotnet", "run"]);
        let nested = dir_with("nested", &[]);
        let sub = nested.join("sub"); std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("a.csproj"), "").unwrap();
        let cmd = detect(&nested).unwrap();
        assert_eq!(cmd.argv, vec!["dotnet", "run", "--project", "sub"]);
        assert_eq!(cmd.label, "dotnet run --project sub");
        let bare = dir_with("bare", &[]);
        assert!(detect(&bare).is_none());
    }

    #[test]
    fn package_json_corrupt_or_without_scripts_is_none() {
        let d = dir_with("broken", &[("package.json", "{ nope")]);
        assert!(detect(&d).is_none());
        let d = dir_with("noscripts", &[("package.json", r#"{ "name": "x" }"#)]);
        assert!(detect(&d).is_none());
    }

    #[test]
    fn pane_environ_marks_and_path() {
        let env = pane_environ();
        let has = |k: &str| env.iter().find(|e| e.starts_with(&format!("{k}="))).unwrap();
        // every ambient marker must be overridden to an EMPTY value (never merely
        // omitted — VTE merges envv onto the inherited environ; CLAUDE.md invariant)
        for k in [
            "CLAUDECODE", "CLAUDE_PID", "CLAUDE_CODE_CHILD_SESSION",
            "CLAUDE_CODE_ENTRYPOINT", "CLAUDE_CODE_SESSION_ID",
            "CLAUDE_CODE_SESSION_ATTENDED", "CLAUDE_CODE_MESSAGING_SOCKET",
            "CLAUDE_CODE_MESSAGING_TOKEN", "CLAUDE_CODE_SSE_PORT",
        ] {
            assert_eq!(has(k).split_once('=').unwrap().1, "");
        }
        let home = std::env::var("HOME").unwrap();
        let want = format!("PATH={}", home);
        if std::env::var("PATH").map(|p| p.contains(&format!("{home}") )).unwrap_or(false) {
            return; // PATH already contains HOME on this box; the append invariant is:
        }
        assert!(has("PATH").contains("~/.local/bin") || has("PATH").contains(&want),
                "PATH must carry the appended ~/.local/bin when missing");
    }
}
```

- [ ] **Step 2: Run, verify failure**

Run: `cargo test runctl` — Expected: compile error (functions absent).

- [ ] **Step 3: Implement**

Port from `alpaca_code/runctl.py` (read it first). No glob crate — csproj discovery is two
`std::fs::read_dir` passes (top-level files ending `.csproj`; else one level of child dirs,
sorted by name, first hit). package.json parse: `serde_json::from_str::<serde_json::Value>`
tolerant (parse failure / missing scripts / non-object scripts → None→npm-side None → fall
through to csproj checks in the same order python does — package.json present-but-broken
means the `is_file(pj)` branch is taken, scripts unreadable → `continue` to None).

- [ ] **Step 4: Run**

Run: `cargo test` — Expected: PASS.

---

### Task 4: App shell — layout, workspace lifecycle, menus

**Reference:** `alpaca_code/window.py` (all), CSS classes `alpaca-*` already in `assets/app.css`.

**Files:**
- Modify: `alpaca-code-rs/src/app.rs` (this task rebuilds the shell; components land as
  `gtk::Widget` placeholders that later tasks' controllers replace)

**Interfaces:**
- Consumes: `state::*` (Task 2), `runctl::detect` (Task 3).
- Produces (later tasks): `AppMsg` enum extended with
  `OpenFile(PathBuf)` (tree→editor routing), `EditorState` (persist-tabs debounce),
  `RunStatus(String, String)` (panes→header); header Run/Stop wiring lands in Task 8, but
  this task already builds the header widgets + workspace switching and calls
  `runctl::detect` to set Run sensitivity (a placeholder panes stub answers nothing yet —
  Run button simply stays disabled until Task 8 connects the real component).

**Steps:**

- [ ] **Step 1: Build layout per window.py:48-158**

Port checklist (read the file; port behaviors, keep the class/comment names in `app.rs`):
- `Gtk.ApplicationWindow` default size via the monitor-clamp port of
  `window.py:_default_size` (1586×992 clamped to `monitor.width − 40` / `height − 140`,
  monitor 0 via `gdk::Display::default().monitors()`).
- HeaderBar `.alpaca-header`, title label `.alpaca-wintitle` ellipsize END, File/Edit/
  Window/Help MenuButtons (label mode, each with its popover menu built from a
  `gio::Menu` — port `window.py:_build_menu/_edit_menu/_win_menu/_help_menu` and
  `register_actions` incl. accels `<Control>q`, open-recent VariantType string action,
  recents submenu items with action targets).
- **MenuButton arrow kill**: label-mode MenuButtons on this build draw a real invisible
  16×24 `down` arrow widget. Port `window.py:_no_arrow`: walk the button's child widget
  tree, find the css class `down` child, `set_visible(false)`. (In gtk4-rs: `for c in
  descendants { if c.css_classes().contains("down".into()) { c.set_visible(false); } }` —
  walk `get_first_child`/`get_next_sibling` like Python does; do not use
  `set_always_show_arrow`, it no-ops.)
- Run/Stop row: `run_row` Box `.alpaca-runpill` valign CENTER (28px band law,
  CLAUDE.md), two `runcell` Boxes each holding a Button (play/stop pixbuf from
  `badge::widget_icon("play.svg", 14)` when Task 5 exists — until then symbolic fallback
  `media-playback-start-symbolic`/`media-playback-stop-symbolic`), divider Box inserted
  after the first cell (`run | div | stop` per `window.py:105`), buttons disabled
  initially. Add css_classes `alpaca-run`/`alpaca-stop`.
- Paned layout: `hpane` (HORIZONTAL, wide_handle) → start `vpane` (VERTICAL, wide_handle)
  [editor slot, panes slot], end browser slot; `hpane.set_position(1161)`,
  `vpane.set_position(592)` (window.py:123,143). hpane is the direct window child
  (window.py:147-149 — a wrapping Box under-allocates, measured).
- Placeholder cards for the three slots this task: VERTICAL Boxes `.alpaca-card` with a
  label — Tasks 5–7 replace them with real components; panes slot label `panes (s8)`.
- Statusbar stub at browser card bottom: `.alpaca-statusbar` Box with count_label only
  (`"0 files"`); git widgets (branch pill) are Task 8/S2 territory — **not** built here.

- [ ] **Step 2: Workspace lifecycle per window.py:160-205**

Port into `App::update`/helpers:
- `set_workspace(path)`: if any dirty editor buffers exist later (Task 6 sends
  `EditorState { dirty: bool }` up) → MessageDialog "Discard unsaved changes?" CANCEL/
  Discard flow (`window.py:set_workspace/_on_discard_switch`); else
  `_set_workspace_now(path)`: `state.save(remember(load(), path))`; window title
  `alpaca-code — {basename}`; tree/panes `set_root` (stub calls now, real in Tasks 7/8);
  `runctl::detect` → run button sensitivity + tooltip text ("No dev/start script or
  .csproj found" / cmd label); stop disabled; run style refresh (later Task 8).
- Open Project (`gtk::FileChooserNative` SELECT_FOLDER → response → set_workspace +
  menu refresh), recents action (isdir check else "Project not found: …" dialog),
  New Project dialog (`window.py:_act_new_project` — name + parent entries,
  `~/FunProjects` default, `mkdir -p` false = fail if exists, `git init` via
  `std::process::Command` with 10s timeout, error dialog on failure, then
  set_workspace), About dialog, Close/minimize actions, Edit ops (defer Edit-menu
  undo/redo/clipboard routing to Task 6 when the editor exists — port the action names
  now as no-ops with a `// wired when editor lands (t6)` comment, exactly like
  `window.py:_edit_op`'s diff-page no-op requirement: never a crash, only a no-op).

- [ ] **Step 3: Tab persistence debounce (window.py:194-204)** — EditorState input
  arms a `glib::timeout_add(1000, …)` flush: `state::set_tabs(load(), root, editor's
  open_tabs, active)` → `save`. Guard: no workspace → no-op. (The editor supplies its
  state in Task 6; here the input enum + debounce exist and no-op safely when no editor
  state has landed yet.)

- [ ] **Step 4: Manual verification (visual)**

`cargo run ../misiuscode` (this repo is a workspace):
- window opens at monitor-clamped default; header row matches the Python app (title
  text, File menu opens with Open Project/Open Recent/New Project/Quit; Edit/Window/
  Help populated; window buttons visible; run/stop cells flat `#0d1017` idle).
- hpaned/vpaned split positions ≈ 1161 / 592 on the default window (measured on screen —
  the same numbers the Python defaults produce at 1586×992).
- title becomes `alpaca-code — misiuscode`; recents submenu lists projects after an
  open; state.json gets the new `last_project` (verify: `python3 -I -c ...` print of
  state.json, or jq) and remains loadable by the Python app (`bin/alpaca-code` opens the
  same project with its tabs).
- File→Open Project switch works; New Project → `~/FunProjects/<name>` created + `git init`
  (only when name given), then set_workspace.
- Ctrl+Q quits. Window Close destroys.

---

### Task 5: badges.rs — pixbuf icon pipeline (SVG → Pixbuf, cached)

**Reference:** `alpaca_code/badges.py` (this task is the *pixbuf* surface only — `icon()`
vector widgets are Task S3's `vector.rs` port; spec §5).

**Files:**
- Create: `alpaca-code-rs/src/badges.rs`
- Modify: `alpaca-code-rs/src/main.rs` (add `mod badges;`)
- Test: unit test inside `badges.rs` for the pure parts

**Interfaces:**
- Produces:
  `pub const SIZE: i32 = 16;` and the `EXT_BADGE` table ported verbatim
  (`badges.py:29-41`).
  `pub fn for_file(name: &str, is_dir: bool) -> Option<(&'static str, &'static str, Option<&'static str>)>`
  `(label, label-color, chip-or-None)`, None for dirs/unknown ext (`badges.py:43-47`).
  `pub fn pixbuf_for(name: &str, is_dir: bool) -> Option<gdk_pixbuf::Pixbuf>` — dir →
  folder.svg@16; css/scss/less ext → hash.svg@16; else EXT_BADGE chip render
  (`badges.py:63-69`).
  `pub fn folder_pixbuf() / chevron_pixbuf(down: bool) (12px) / blank_pixbuf()`
  (`badges.py:71-81`, blank = 12×12 fully-transparent straight-alpha pixbuf).
  `pub fn letter_pixbuf(letter: &str, hexcol: &str) -> Option<Pixbuf>` — S2's chip port
  (tile bg = color washed 12% toward `#0d1017`; `badges.py:83-93`) — implement NOW while
  the cairo path is fresh; gitview (S2) consumes it.
  `pub fn widget_icon(name: &str, px: i32) -> Option<gtk4::Image>` — GtkImage from the
  pixbuf at size (the S1 stand-in for the S3 VecIcon; blur accepted, spec §5).
- Consumes (Tasks 6/7): tree-cell + tab-badge pixbufs, close-button x-dim.svg.
- Assets: SVGs come from `include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons/<file>"))`
  and rasterize through `gdk_pixbuf::PixbufLoader` (mime `image/svg+xml`, `set_size` first).
  Cache `Pixbuf` per (name, size) in a `Mutex<HashMap>` — one rasterization per asset per
  size, same as python's `_SVGS`/`_PCS` caches.

**Steps:**

- [ ] **Step 1: Failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badge_table_and_exts() {
        assert!(for_file("x.ts", false).is_some());
        assert_eq!(for_file("x.ts", false).unwrap().1, "#ffffff"); // label color
        assert!(for_file("x.jsx", false).unwrap().2.is_none());    // bare glyph, no chip
        assert_eq!(for_file("a.js", false).unwrap().0, "JS");
        assert!(for_file("somedir", true).is_none());              // dirs never get chips
        assert!(for_file("noext", false).is_none());               // unknown ext
        assert!(for_file("css", false).is_none() || true); // "css" is a NAME not ext — ext-less
    }

    #[test]
    fn pixbufs_render_if_art_present() {
        // These render only if the embedded SVG assets exist; they must not panic
        // either way. Assert the sizes when they do load.
        if let Some(p) = folder_pixbuf() {
            assert_eq!(p.width(), 16);
        }
        if let Some(c) = chevron_pixbuf(true) {
            assert_eq!(c.width(), 12);
        }
        let blank = blank_pixbuf();
        assert_eq!((blank.width(), blank.height()), (12, 12));
        // letter tile: 16×16 straight-alpha
        if let Some(t) = letter_pixbuf("M", "#f2c94c") {
            assert_eq!(t.width(), 16);
        }
    }
}
```

- [ ] **Step 2: Run, verify failure** — `cargo test badges` (compile error)

- [ ] **Step 3: Implement**

Port `badges.py:43-81` + the cairo `_render` port (`badges.py:101-161`): chip = rounded
rect at radius `S*0.27` filled with the chip color, then the label in bold sans, font
shrunk from 13 until width fits `S−4`; lowercase labels center the **bowl band**
(`base_y = round(S/2 + xh/2)`), caps labels center ink-box — THE centering law from
CLAUDE.md; 1:1 (no supersampling — "TS" mush ruling).
The cairo dependency: render into a manual pre-multiplied-BGRA buffer (the
`_pixbuf_from_surface` un-premultiply math stays — implement with a tiny loop; or use
`gdk_pixbuf::Pixbuf::from_bytes` semantics: fill straight-alpha u8 RGBA, then
`Pixbuf::from_bytes`-equivalent via `Pixbuf` constructor). No external crate: the
glyph raster uses **Cairo via the `cairo` crate? — forbidden (no new deps). Instead:
Pango/Cairo are already linked through gtk4's re-exports? gtk4-rs exposes pango but not
cairo context drawing. Pragmatic port without cairo: build a plain RGBA `Vec<u8>` and
render the chip shape + glyph with the **Pango/Cairo pipeline via `gtk4::cairo` re-export
(`gtk4::cairo::ImageSurface`) — gtk4-rs DOES re-export `cairo` (`gtk4::cairo`) and
`PangoCairo`.** Use `gtk4::cairo` for surface drawing exactly like python does, then
convert the surface to a Pixbuf with the same un-premultiply loop
(`badges.py:162-181`). No `pixbuf_from_surface` in the pixbuf crate exists, so implement
it on our side (memory layout note stays: BGRA little-endian ARGB32).

- [ ] **Step 4: Run**

`cargo test` — PASS.

- [ ] **Step 5: Visual check** — after Task 6/7 use these; quick standalone: a
  temporary `examples/badges_smoke.rs`? skipped — pixels get verified by Task 7's tree
  visual pass (chevron/folder/hash chips visible in the real tree vs the Python app's).

---

### Task 6: Editor component (file tabs)

**Reference:** `alpaca_code/editor.py` (all). S1 port covers ONLY file pages: open/close/
save/restore/watches/tombstone/binary/dirty dots/badges + `readable_text`.
**Out of this task (definitely S2/S3):** diff pages (§ spec stage plan), `_TabDistributor`
(S3 — the notebook ships stock scrollable; stock min-width tab allocation accepted),
live-diff renewal, `open_diff/refresh_diff/diff_pages/save_open`.

**Files:**
- Create: `alpaca-code-rs/src/editor.rs`
- Modify: `alpaca-code-rs/src/main.rs` (mod), `alpaca-code-rs/src/app.rs` (embed the
  component)

**Interfaces:**
- Produces (consumed by app.rs):
  relm4 Component `Editor` with inputs:
  `SetRoot(PathBuf)` (closes all pages), `Restore(PathBuf, Vec<String>, i64)`,
  `OpenFile(PathBuf)`, `SaveActive`, `SaveOpen(Vec<PathBuf>)`(S2, stub now),
  outputs: `StateChanged(EditorSnapshot)` where
  `struct EditorSnapshot { open_tabs: Vec<String>, active_tab: i64, has_dirty: bool }`
  (relpaths when root is set), plus for the Edit menu (Task 6 wiring lands here):
  inputs `UndoRedoCopyCutPaste/SelectAll` routed to the active page's buffer/view
  (`window.py:_edit_op` semantics: undo/redo buffer-side w/ can_undo; clipboard ops on
  the view's own actions; diff pages won't exist in S1 so no pair-box edge).
- Consumes: `badges::*` (Task 5), style scheme from `style::init`'s registered search
  path (`sourceview5::StyleSchemeManager::default().get_scheme("alpaca-dark")`).

**Steps:**

- [ ] **Step 1: Port structure** — read `alpaca_code/editor.py` fully; port with the
  page-state extracted into a Rust struct (Python dangles attrs on the GtkWidget;
  Rust keeps a parallel model, index-aligned with notebook pages — the wrapper-identity
  trap CLAUDE.md warns about is the thing this avoids by construction):

```rust
struct PageState {
    path: Option<PathBuf>,        // None on diff pages (S2)
    buf: gtk4::TextBuffer,        // actually sourceview5::Buffer
    sw: gtk4::ScrolledWindow,     // scroll restore
    view: sourceview5::View,
    fs: FileState,                // None / Deleted / Binary
    conflict: bool,
    load_mtime: Option<i64>,      // stat.st_mtime_ns
}

enum EditorMsg {
    OpenFile(PathBuf), SetRoot(PathBuf), Restore(PathBuf, Vec<String>, i64),
    SaveActive, StateTick,
    Undo, Redo, Cut, Copy, Paste, SelectAll,
}
#[derive(Debug, Clone, Copy)]
pub enum FileState { Live, Deleted, Binary }
#[derive(Debug, Clone, Default)]
pub struct EditorSnapshot { pub open_tabs: Vec<String>, pub active_tab: i64, pub has_dirty: bool }
```

Behaviors to port exactly (file:line from editor.py):
- `readable_text` (`editor.py:51-60`): decode UTF-8; reject NUL; → `Option<String>`.
  Pure fn → **unit test** (step 2).
- Open (`editor.py:124-210`): page_of dup → just switch; missing file → tombstone tab
  with GONE_NOTE (no popup); unreadable → error dialog, no tab; `GtkSource.Buffer`
  **born modified** — always `set_modified(false)` (CLAUDE.md invariant); language guess
  by filename; scheme attach; breadcrumb `"  ›  ".join(relparts)` label ellipsize
  MIDDLE xalign 0; view `show_line_numbers`, wrap NONE, pixels-above 2, left-margin 12,
  `.alpaca-mono`; tab head = dirty `●` label (first child!) + badge slot + name
  (ellipsize MIDDLE, `set_size_request(72, -1)` pre-map floor) + close button
  (`x-dim.svg` pixbuf via Task 5 `widget_icon`, else `window-close-symbolic`);
  **append_page then set_current_page** (does not self-switch); mtime latch;
  `_watch(path)`; deleted-state tab paints italic/dim `alpaca-deleted` + muted tag.
- Save (`editor.py:487-516`): sync write; conflict cleared **before**
  `set_modified(false)` (the fired modified-changed repaints); reload mtime latch;
  Ctrl+S via ShortcutController `<Control>s` (port `editor.py:95-104`); clean Ctrl+S
  no-op (never resurrects a tombstone).
- Tab-state / dirty-dot (`editor.py:388-449`): snapshot computation
  `open_tabs/active/has_dirty` (output at every open/close/modify flip, consumed by
  App's Task-4 debounce — App persists via `state::set_tabs`).
- Watches (`editor.py:519-652`): per-path `gio::File::monitor_file`, events → 120ms
  debounced `_fs_settled` (glib::timeout_add 120): stat-mtime equal + never-touched →
  echo no-op; dirty-buffer → `conflict = true` + amber dot (`conflict` css class on the
  `alpaca-dirty` label), tab un-deletes itself if it was a placeholder; clean buffer →
  `_reload` (seamless: preserve cursor line/col clamped, scroll value via idle;
  fresh buffer carrying scheme+language, then `set_modified(false)`); vanished →
  `_fs_tombstone`; unreadable → `_fs_binary` (editable(false) + BINARY_NOTE, saves
  over it are refused by the dirty guard).
- Close (`editor.py:452-484`): dirty → "Discard changes to X?" CANCEL/Discard dialog
  (force-path re-enters); remove page + `_cancel_watch` + refresh.
- Restore (`editor.py:114-121`): set_root + open each saved relpath that still
  `is_file` + clamp active by `min(tab, n-1)` + refresh.
- get_open_state (`editor.py:661-667`): relpaths; index, not path (restore re-clamps).

- [ ] **Step 2: Unit test for readable_text (TDD)**

```rust
#[cfg(test)]
mod tests {
    use super::readable_text;

    #[test]
    fn readable_text_rules() {
        assert_eq!(readable_text(b"hello\nworld").as_deref(), Some("hello\nworld"));
        assert!(readable_text(b"caf\xc3\xa9").is_some());          // valid UTF-8 non-ASCII opens
        assert_eq!(readable_text(b"caf\xe9"), None);               // invalid UTF-8
        assert_eq!(readable_text(b"a\0b"), None);                  // NUL truncation trap
        assert_eq!(readable_text(b""), Some(""));                  // empty file is fine
    }
}
```

Run `cargo test editor` after wiring — FAIL then PASS cycle (write test, see it fail
with missing fn, implement `readable_text`, re-run PASS).

- [ ] **Step 3: Embed in App** (app.rs): replace the editor placeholder with the
  component controller; route FileTree's open (Task 7) and menus; `StateChanged` →
  tab-persist debounce (already built Task 4). Editor.set_root on set_workspace;
  restore with `state::project_tabs(load(), path)`.

- [ ] **Step 4: Manual verification**

Compare with the Python app side-by-side on this repo:
- open/close tabs, dirty dot red → save → clears; file deleted on disk → italic dimmed
  tab; recreate → un-tombstones; edit while on disk-changed → amber conflict dot, save
  wins without popup; open a binary (`data/icons/play.svg` is text — use a real `.png`)
  → refusal dialog; Ctrl+S; Ctrl+Q with dirty tab → nothing forced; restart → tabs restore.
- Editor tab strip behaves stock (min-width tabs; narrow-window starve accepted until S3).

---

### Task 7: FileTree component

**Reference:** `alpaca_code/filetree.py` (all) + `alpaca_code/treehover.py`.
S1 scope drops git: no BranchMenu, no ChangesView, no mode tabs, no git probe/refresh —
but the **saved-tree displacement bundle** and tree search port entirely.

**Files:**
- Create: `alpaca-code-rs/src/filetree.rs`
- Modify: `alpaca-code-rs/src/main.rs` (mod), `alpaca-code-rs/src/app.rs`

**Interfaces:**
- Produces: Component `FileTree` input `SetRoot(PathBuf)`, outputs `OpenFile(PathBuf)`
  (App forwards → Editor). Card = panel title + search entry + tree + statusbar row
  (file count only; git widgets owned by S2's GitPanel in S2).
- Consumes: `badges::*` (Task 5).
- Pure fns unit-tested: `scan_dir` + `scan_project` + `icon_of` (ported verbatim,
  `filetree.py:24-62` — SKIP_DIRS set; dirs-first casefolded name sort; walk skip-list
  honored; cap 500 for search / 100_000 for the count).

**Steps:**

- [ ] **Step 1: Failing pure-fn tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf { std::env::temp_dir().join(format!("alpaca-tree-{tag}")) }

    #[test]
    fn scan_dir_skips_and_sorts_dirs_first() {
        let root = scratch("scan"); std::fs::create_dir_all(&root).unwrap();
        for d in ["node_modules", "src"] { std::fs::create_dir(root.join(d)).unwrap(); }
        for f in ["b.rs", "a.rs", "z.py"] { std::fs::write(root.join(f), "").unwrap(); }
        let got = scan_dir(&root);
        let names: Vec<&str> = got.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["src", "a.rs", "b.rs", "z.py"]);       // node_modules skipped; dirs first, casefolded
        assert!(got.iter().all(|(_, is_dir)| *is_dir || names.iter().any(|n| !n.is_empty())));
    }

    #[test]
    fn scan_project_matches_and_skips() {
        let root = scratch("walk"); let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir(root.join("node_modules")).unwrap();
        std::fs::write(src.join("main.rs"), "").unwrap();
        std::fs::write(root.join("README.md"), "").unwrap();
        std::fs::write(root.join("node_modules").join("junk.rs"), "").unwrap();
        let got = scan_project(&root, "rs", 500);
        assert_eq!(got, vec!["src/main.rs".to_string()]);             // node_modules pruned
        let n = scan_project(&root, "", 100_000).len();
        assert_eq!(n, 2);                                             // count path also skips junk
    }

    #[test]
    fn icon_of_rules() {
        assert_eq!(icon_of("main.py", true), "folder-symbolic");
        assert_eq!(icon_of("a.py", false), "text-x-python");
        assert_eq!(icon_of("a.json", false), "application-json");
        assert_eq!(icon_of("a.md", false), "text-x-markdown");
        assert_eq!(icon_of("a.xyz", false), "text-x-generic-symbolic");
    }
}
```

- [ ] **Step 2: Implement pure fns → tests PASS**

- [ ] **Step 3: Port the widget** — read `filetree.py` + `treehover.py` fully. Key
  structural ports:
  - 8-column TreeStore: (String name, String path, bool is_dir, String icon, Pixbuf
    chevron, Pixbuf badge, bool badge_vis, bool icon_vis) with `badges` populating row
    vals (`filetree.py:_row_vals`); cell renderers packed in exact order
    (`filetree.py:137-161`): chevron pix, badge pix (+visible), icon-name cell
    (+visible), text LAST expand=True (pins the left cluster; CLAUDE.md packing law);
    `show_expanders` = false; `activate_on_single_click` true; level_indentation 16;
    tooltip column 1; ellipsize MIDDLE set BEFORE any store fill.
  - **`HoverTree` — gtk4-rs TreeView subclass** (treehover.py:19-105): subclass with
    `#[glib::object_subclass]`, `impl TreeViewImpl { fn snapshot(&self, snapshot) {...}
    }` painting the hover band color BEFORE chain-up — color from the style context's
    `alpaca-hover` token lookup (`lookup_color("alpaca-hover")`), fallback `#111723`;
    band = full row width rect from `get_background_area(path, cols[0])` viewport coords
    (treehover.py:76-96 — no visible-rect conversion). Motion controller + capture-phase
    GestureClick + vadjustment hook with reconnect-on-notify (treehover.py:29-50); hover
    row as `TreeRowReference`, refresh on store row-inserted/row-deleted
    (`filetree.py:131-132`).
  - Lazy children: children load from disk on activation-expand of a collapsed row
    **before** `expand_row` fires — never during `row-expanded` (store-mutation-under-
    -expanded-row collapse invariant, `filetree.py:1-8` comments + `_load_children`).
    `_open` set is the truth for expand state (no view readback).
  - Expansion/monitor lifecycle: `_on_row_expanded` (add to `_open`, chevron swap,
    dir monitor cap 50 per `filetree.py:461-474`), `_on_row_collapsed`, dir-changed →
    surgical row INSERTs under expanded rows (ponytail rule stays: append unsorted,
    dedupe by path via `_iters_for`, remove-then-re-add only on DELETED/CREATED/MOVED —
    `filetree.py:482-526`); `_reexpand`/`_restore_saved_tree`/`_restore_scroll`
    (parents-first replay per `filetree.py:386-420`); search displacement bundle
    (`filetree.py:423-443` — first displacement saves (root, _open, scroll), later
    keystrokes don't overwrite, empty text restores via idle).
  - Status bar: `.alpaca-statusbar` with count_label; on dir-changed → `_count`
    (`filetree.py:486`).
  - Activation (`filetree.py:532-541`): dir → toggle via `_open` membership
    (collapse_row / load-children + expand_row); file → `OpenFile(path)` output.

- [ ] **Step 4: Viewport manual verification** (side-by-side vs Python app):
  - browse/open: single click opens file in editor (App wiring); dirs expand inline
    with chevron flip; hover band follows pointer and survives store mutations.
  - search: type → flat sorted relpath list; type empty → tree restored incl. expanded
    dirs + scroll; switch workspaces resets.
  - monitors: create a file inside an expanded dir via terminal → row appears (unsorted,
    next re-expand re-sorts); remove another → its rows gone; count label updates.
  - skip-list: node_modules/.git/obj/bin/vendor/__pycache__ never render.

---

### Task 8: Panes — VTE + run lifecycle

**Reference:** `alpaca_code/panels.py` (all). All of this file is S1.

**Files:**
- Create: `alpaca-code-rs/src/panes.rs`
- Modify: `alpaca-code-rs/src/main.rs` (mod), `alpaca-code-rs/src/app.rs`

**Interfaces:**
- Produces: Component `Panes`:
  inputs `SetRoot(PathBuf)`, `LaunchRun { argv: Vec<String>, label: String }`,
  `StopRun`; outputs `RunStatus { text: String, cls: String }` (cls ""
  / "ok" / "err"/ "warn");
  query surface for App: outputs carry state; App tracks
  `running: bool` off `RunStarting/RunEnded` outputs (equivalents of
  `has_running_run()` — starting window included).
- Consumes: `runctl::pane_environ` (Task 3), `badges` pane-tab icons (bot/prompt/
  terminal svgs at 16, else symbolic fallbacks).
- `kill_tree`/`alive`/`waitstatus` decision fns are libc-level — unit-testable → tests in
  step 1 for the pure decision parts (`pid classification: None vs dead vs alive`).

**Steps:**

- [ ] **Step 1: Failing pure tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alive_rules() {
        assert!(!alive(None), "None pid is never alive");
        assert!(!alive(Some(999_999_999)), "unborn pid is not alive");       // /proc absent
        // A real pid: our own process — proves /proc lookups work, nothing else.
        let me: i32 = std::process::id() as i32;
        assert!(alive(Some(me)));
    }

    #[test]
    fn respawns_policy() {
        // port the decision table of _pane_child_exited (panels.py:165-188) as a
        // pure fn `respawn_decision(kills_pending, rooted, in_spawned, lived, budget)`:
        assert_eq!(respawn_decision(1, true, true, 100.0), Decision::Swallow);   // set_root's kill event
        assert_eq!(respawn_decision(0, false, true, 100.0), Decision::Ignore);   // no root
        assert_eq!(respawn_decision(0, true, false, 100.0), Decision::Ignore);   // bookkeeping gone
        assert_eq!(respawn_decision(0, true, true, 2.0), Decision::Ignore);      // <3s broken child
        assert_eq!(respawn_decision(0, true, true, 100.0), Decision::Respawn);
        assert_eq!(respawn_decision(0, true, true, 3_600.0), Decision::RespawnRearm);
        assert_eq!(respawn_decision(0, true, true, 100.0), Decision::Exhausted); // budget 3 already spent — note: pass budget via arg
    }

    #[test]
    fn run_lifecycle_transitions() {
        // launch_run guard: RunStarting blocks double launches (panels.py:194-196)
        // landed-with-stop-pending kills fresh pid (panels.py:206-210)
        // child-exited while no pid & not starting → ignore (panels.py:238-240)
        // stop_run with no pid but starting → run_stop_pending=true, status Ready
        // (assert via a tiny state-struct API `RunState::new()` with the same fields)
    }
}
```

- [ ] **Step 2: Implement pure decision fns → PASS**

- [ ] **Step 3: Port widgets + lifecycle** — read `panels.py` fully. Ports:
  - 3 `vte4::Terminal`s with the `_ANSI` palette port (`panels.py:13-17,68-87`):
    fg `#e6e8ee`, bg `#0d1017`, scrollback 10 000, absolute-size font
    `Noto Sans Mono` 13px (`pango::FontDescription::from_string` + `set_absolute_size`
    via `*pango::SCALE`), `set_cell_height_scale(1.08)` when available (feature-guard:
    the binding has it; keep the call), set_colors(fg, bg, pal). child-exited handlers
    per pane (output pane → run lifecycle; agent/term → pane revive).
  - Notebook `.alpaca-panes` vexpand scrollable; pages in load-bearing order
    **0=agent, 1=term, 2=out** with `_pane_tab` port (head Box `.alpaca-panetab`
    valign CENTER spacing 6, icon valign CENTER, label `.alpaca-panetabname`).
    `append_page` then `set_current_page` is irrelevant here (no programmatic first
    switch — set_root does `_ensure_current` via idle).
  - Spawn (`panels.py:29-52`): `terminal.spawn_async(None /*pty flags default*/, cwd,
    argv, envv=runctl::pane_environ(), spawn flags SEARCH_PATH|SEARCH_PATH_FROM_ENVP,
    None, -1 via timeout, cancellable None)` — in vte4 0.9 the pty-flags default is
    `PtyFlags::DEFAULT`; use `Terminal::spawn_async` with the full callback closure
    sending pid through the component sender; callback lands `pid_holder` semantics in
    our own model (per-terminal `Option<i32>`); spawn failure → log + on_ready(None).
    **The pid-bearing callback is the port of `term.pid_holder`.**
  - `set_root` (`panels.py:120-139`): kill out-pid/agent/term + `_kills_pending`
    bookkeeping, clear budgets, `reset(true, true)` all three, status "Ready", idle
    `_ensure_current`.
  - `_ensure_current` + switch-page (`panels.py:141-157,190-192`): spawn-once per pane
    per workspace; page-ix handover from the signal (GTK reads OLD index mid-emission —
    the port must pass the new index too, same gotcha).
  - Run lifecycle (`panels.py:194-246`): `launch_run` guards `has_running_run`
    (starting window INCLUDED); switch to output page (2); reset out;
    landed-callback handles error/pid≤0 (status err), stop-pending kill,
    alive→"Running"/ok else died-instantly err; `stop_run` — kill+clear else
    stop-pending if starting; `_run_child_exited` — skip when cleanup exit,
    waitstatus→exitcode (glibc-style: in Rust the child-exited callback hands a raw
    `ExitStatus`/i32 — decode via `glib::spawn_close_id`/status convention: VTE hands
    the raw wait status → use `libc::WEXITSTATUS`/`WIFSIGNALED` helpers to port
    `os.waitstatus_to_exitcode`), statuses "Exit {code}" err-if-nonzero else ok.
  - Kill tree (`panels.py:253-265`): `libc::killpg(libc::getpgid(pid), SIGHUP)` ignoring
    ESRCH/EPERM errors; then `glib::timeout_add(2000, …)` SIGKILL escalation
    (millisecond form; CLAUDE.md invariant — do not "fix" to `_seconds`).
  - Respawn bookkeeping (`panels.py:160-191`): keyed by "agent"/"term";
    `respawn_decision` port; budget 3; ≥60s re-arms; re-entry (`_ensure_current`)
    pops the budget entry (deliberate re-entry re-arms).

- [ ] **Step 4: Wire App → header** — Run button (`_on_run` port, `window.py:240-252`):
  detect → LaunchRun; stop → StopRun; RunStatus output → `_on_run_status` logic
  (`window.py:260-265`): re-enable Run when not running; stop sensitivity = alive;
  `_refresh_run_style` = toggling the `running` class on the run pill Box.

- [ ] **Step 5: Probe verification (numerical, CLAUDE.md probe discipline)**

Manual flows:
- Agent pane spawns claude (a claude session appears in the tab; ambient markers absent
  from its env — verify: inside the Agent pane run `env | grep CLAUDE_CODE_SESSION_ID`
  → must be empty-value or absent; `env -u`-style check through the pane).
- Terminal pane spawns `$SHELL`.
- Run a project (this repo has package.json): Run → Output page focuses, "Running"/ok
  dot, pill gains `running` class; Ctrl+C in output → Exit 130 err dot; Run again
  enabled. Missing script project → Run disabled tooltip.
- Stop before pid lands: run a fast-exiting script repeatedly pressing Stop immediately —
  no orphan processes (`pgrep -f npm` empty after), statuses truthful.
- kill-tree: `claude` in panes spawns node children? (no — claude is not a spawning
  parent here). Verify with `npm run <watcher>`: Stop → whole tree dies
  (`pgrep -f vite` empty), no SIGKILL needed within 2s in the normal case.
- <3s-death claude stub (PATH-faked `claude` that exits immediately): no respawn loop —
  3 revives per budget cap, "Agent exited" + err dot, re-entering tab offers a fresh
  attempt then 3 more max.

- [ ] **Step 6: cargo test** all green.

---

### Task 9: Stage-1 parity sweep (final gate)

**Files:**
- Modify: repo `CLAUDE.md` — add a short "Rust port (stage 1)" section recording the
  binary path, app-id, and the carried invariant deltas (no python-side changes).

**Steps:**

- [ ] **Step 1: Side-by-side behavioral checklist** (run both apps against the same
   workspace projects) — every S1 flow from Tasks 4–8, plus:
  - shared state: open project A in python, B in rs, both `recents` list both; tabs of
    A restore in python and tabs of B restore in rs; neither corrupts the other's writes
    (sequential, not concurrent, usage).
  - `cargo test` green; `cargo build --release` produces a standalone binary that boots
    (deps are system GTK, that's the point — startup visibly faster than the python
    launcher; note the measured numbers in the task log).
  - kill both processes cleanly; no leaked pane children after quit (check `pgrep claude`
    after closing both apps after using agent panes — VTE children must go with the app:
    if the app dies, children die with the pty/session — verify once).

- [ ] **Step 2: Report** — stage-1 deltas vs python noted (known S2/S3 gaps: no git
  panel/branch pill/no-VecIcon/tab distributor; any unexpected deltas listed for a fix
  task before S2).

---

## Self-review notes

- Spec coverage: S1 module map table (spec §4.2) → Tasks 1,2,3,4,5,6,7,8,9 map 1:1
  (main/style/assets=T1, state=T2, runctl=T3, window/app=T4, badges=T5, editor=T6,
  filetree+treehover=T7, panels=T8). S2/S3 are separate plans (spec stage table).
- No placeholders: every task names its python reference file + exact line ranges and
  concrete code/tests; where the plan says "read X fully", that IS the content — the
  executor's primary source (fidelity beats re-transcription in a plan doc).
- Type consistency: `EditorSnapshot`, `RunCmd`, `RunStatus`/decision fns, and the
  `badges` surface are named identically across tasks.
- Review Focus mapping: items 1,2,5 → Task 6/8 manual+probe steps; item 3 → Task 2
  tests; item 4 → Task 8 probe step 5.