# Multi-workspace Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Open up to 3 projects as always-alive tabs in one window — each with the full app (editor, VTE Agent/Terminal/Output, file browser + git), switched instantly from centered header pills.

**Architecture:** Every open project gets its own `Workspace` instance — one `Controller<Editor>` + `Controller<Panes>` + `Controller<GitPanel>` behind its own `GtkPaned` assembly, exactly the widgets that today are App's single instances. The window's content becomes a `GtkStack` holding one page per workspace (`StackTransitionType::None` → instant switch); hidden pages stay alive, so VTE children (claude/shell/runs), git probe flights, editor pages and scroll pairing keep running per project in the background — nothing is ever re-rooted on switch, which is what makes this "a separate app per tab". The headerbar's title widget becomes a centered `GtkBox` of project pills (idle = the exact current title look, hover/press = the branch pill's) plus a `+` picker button. All App→workspace routing is index-tagged; a `MAX_WORKSPACES = 3` cap + `add_decision()` govern every open path.

**Tech Stack:** Rust, relm4 0.11 + gtk4 0.11 (v4_22), stack pages over the existing relm4 components, state.json schema extension. No new dependencies.

**Spec:** The user's request (2026-10-14), requirements verbatim:

1. Remove the "alpaca-code — ", keep only the project name.
2. The project name becomes a button. Its look should NOT change — exactly like now; hover/click styled like the git branch button (unhovered = plain pill row, hovered = dark pill `#1c2431`).
3. To the right of the project name, a new button "+"; pressing it opens the file explorer to select a folder to open as a project in alpaca-code.
4. A new project appears to the right of the first, the next to the right of the previous, and so on.
5. Maximum project cap: 3.
6. All projects at the top are clickable buttons (like requirement #2).
7. However many are open, ALL are centered as one entity (tasteful gaps between).
8. Switching projects is instant, like switching browser tabs.

Project requirements: every project tab has its own File Browser (with git changes), its own Agent/Terminal/Output windows and its own code viewer — you must NOT see project B's code/agent output while in project A's tab. Essentially like a separate app.

(Not requested, deliberate non-goal: no close/remove-project button. When 3 are open the cap rejects further opens until state.json's `open_projects` is edited or this ships later. Flagged to the user for review.)

## Global Constraints

- Test command: `cargo test --manifest-path alpaca-code-rs/Cargo.toml` (displayless — THE test command). A test-only session does NOT refresh the runnable bin; `cargo build --manifest-path alpaca-code-rs/Cargo.toml` must run before any manual app launch.
- Timers: `glib::timeout_add` / `timeout_add_local` in ms — `timeout_add_seconds` never fires on this box. Timer closures that capture non-Send relm4 messages must be `_local`. Never `SourceId::remove` (rust panic on fired sources) — cancel by generation counter only.
- relm4 idioms (CLAUDE.md): `#[relm4::component(pub)]`, init epilogue `let widgets = view_output!();`, `T::builder().launch(x).connect_receiver(cb)` where connect_receiver RETURNS the Controller; component outputs carry state — App cannot query panes synchronously.
- VTE spawn flags must be `SEARCH_PATH | SEARCH_PATH_FROM_ENVP` (in `Panes::spawn`, untouched here).
- state.json load must NEVER raise on corrupt input — the `Value` + `parse()` read path stays (the `Deserialize` derive on `State` is unused; don't route reads through it).
- App CSS loads at USER priority; headerbar buttons carry a ~16px vertical additive that only an everything-0 sweep first + re-raise in the same block kills (the `.alpaca-runpill` pattern measured 2026-10-05).
- No `pkill -f` (it matches the invoking shell — exit 144); kill by resolved pid.
- Cap is `MAX_WORKSPACES = 3` (Requirement 5); dedupe-by-`PathBuf` equality everywhere (mirrors state.json's exact-string keys); add paths are never symlink-canonicalized (consistent with state keys).
- Probes are env-guarded (`ALPACA_PROBE_*`) and never run in production (env unset).

## Review Focus

The inputs the spec implies but no unit test exercises, in the order most likely to bite, each pinned to its owning task:

1. **Already-open project re-opened** (File▸Open Project/recents/`+`/CLI arg with an open path) — must switch tabs, never add, never re-root/re-restore (a Restore on an open workspace would duplicate its tabs). Pinned: T2 unit test `add_decision_switch_add_full`; T4 probe step 4 (`re-open-wsA` assert).
2. **Cap edge** — 4th open attempt must reject cleanly and add nothing to the strip or `open_projects`. Pinned: T2 unit test (Full arm); T3 (+ disabled + tooltip at 3); T4 probe step 3 (`three-open`, `add-btn-disabled`).
3. **Foreign/corrupt state.json** — no `open_projects` key (old file), wrong types inside, >3 entries → parse tolerant, caps at 3, no startup brick. Pinned: T1 tests `open_projects_parse_toleration_and_cap` + roundtrip.
4. **Hidden workspaces keep living** — switch away and back: open tabs, run/agent state, git rows, the paned's 1161px split must all survive (GTK hides stack pages; the position property must hold through first-allocation-after-mapping). Pinned: T4 probe step 2 asserts (`stack-visible-is-wsA`, `editor-tabs-preserved`, `hpane-pos-1161`, `.on` classes, run tooltip).
5. **Duplicate basenames** (two dirs named `web`) — identical pill labels are ambiguous only visually; tooltip carries the full path (decision: don't disambiguate). Pinned: T3 manual screenshot check (tooltips).

---

### Task 1: state.json — persist the open-workspace set

**Files:**
- Modify: `alpaca-code-rs/src/state.rs`
- Modify: `alpaca-code-rs/src/app.rs` (tests-module `State{}` literal only — keeps the build green)
- Modify: CLAUDE.md (persistence schema block)

**Interfaces:**
- Consumes: existing `State`/`parse()`/`load()`/`save()`.
- Produces: `State::open_projects: Vec<String>` (field), `pub fn open_projects(s: &State) -> Vec<String>`, `pub fn add_open(s: State, project_path: &str) -> State`. T2 consumes exactly these.

- [ ] **Step 1: Write the failing tests** — add to state.rs `mod tests`:

```rust
    #[test]
    fn open_projects_toleration_and_cap() {
        // old files lack the key entirely → empty open_projects, nothing
        // else downgraded (must NOT hit the wholesale default)
        let old = parse(&json!({"last_project": "/a", "recents": ["/a"], "projects": {}}));
        assert_eq!(old.open_projects, Vec::<String>::new());
        assert_eq!(old.recents, vec!["/a".to_string()]);
        // non-array → empty; non-string/non-empty filtered; cap 3
        let s = parse(&json!({"recents": [], "projects": {}, "open_projects":
                               ["one", 5, null, "", "two", "three", "four"]}));
        assert_eq!(s.open_projects, vec!["one", "two", "three"]);
        // add_open dedupes, grows
        assert_eq!(add_open(s, "one").open_projects, vec!["one", "two", "three"]);
        assert_eq!(
            add_open(parse(&json!({"open_projects": ["a"]})), "b").open_projects,
            vec!["a", "b"]
        );
    }

    #[test]
    fn open_projects_roundtrip() {
        let p = tmp("op.json");
        let mut s = State::default();
        s = add_open(s, "/w1");
        s = add_open(s, "/w2");
        s = add_open(s, "/w1"); // dedupe keeps order
        assert_eq!(s.open_projects, vec!["/w1", "/w2"]);
        save_to(&s, &p);
        assert_eq!(load_from(&p), s);
        std::fs::remove_file(&p).ok();
    }
```

- [ ] **Step 2: Run to verify it fails** — `cargo test --manifest-path alpaca-code-rs/Cargo.toml` → FAIL (E0425: `open_projects`/`add_open` undefined — compile error is the failing state).

- [ ] **Step 3: Implement** — in `state.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct State {
    pub last_project: Option<String>,
    pub recents: Vec<String>,
    pub projects: HashMap<String, ProjectTabs>,
    /// multi-workspace (2026-10-14): the open tabs at last save, add order =
    /// strip order. Read path is parse() — the derive's Deserialize is never
    /// exercised (load_from goes through serde_json::Value), absent keys on
    /// old files are fine.
    pub open_projects: Vec<String>,
}
```

In `parse()`'s object arm, extend the final `State { … }` construction with a lenient per-key read (NOT part of the wholesale check — old files must keep loading):

```rust
            // lenient per-key: absent on pre-multi-workspace files → empty;
            // non-array → empty; strings only, capped at the strip's 3
            let open_projects = get("open_projects")
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|p| p.as_str().filter(|s| !s.is_empty()))
                        .map(str::to_owned)
                        .take(3)
                        .collect()
                })
                .unwrap_or_default();
```

Helpers next to `remember`:

```rust
/// The open-workspace set as saved (boot decides its own fallback:
/// empty set + existing last_project → open just that one).
pub fn open_projects(s: &State) -> Vec<String> {
    s.open_projects.clone()
}

/// Add to the open set, deduped, first-add order preserved. No cap here —
/// the cap is a UI rule (app.rs MAX_WORKSPACES) enforced at the only path
/// that grows this list.
pub fn add_open(mut s: State, project_path: &str) -> State {
    if !s.open_projects.iter().any(|p| p == project_path) {
        s.open_projects.push(project_path.to_owned());
    }
    s
}
```

Also fix the tests-module fixture in `app.rs` (line ~1162) so the build stays green:

```rust
        State {
            last_project: None,
            recents: recents.iter().map(|s| s.to_string()).collect(),
            projects: Default::default(),
            open_projects: Vec::new(),
        }
```

And in CLAUDE.md's `## Persistence schema` block, extend the example line:

```json
{"last_project": "/path|null", "recents": ["/path", …cap 10, newest first],
 "open_projects": ["/path", …cap 3, opened-order], "projects": {"/path": {"open_tabs": ["rel/path", …], "active_tab": 0}}}
```

- [ ] **Step 4: Run to verify it passes** — `cargo test --manifest-path alpaca-code-rs/Cargo.toml` → PASS (all, including pre-existing coercion/roundtrip tests).

- [ ] **Step 5: Commit** — `git add -A && git commit -m "state: persist the multi-workspace open set"`

---

### Task 2: Workspaces — per-project UI instances behind a GtkStack

The whole feature's core atom (can't compile half-extracted: AppMsg, routing and construction are one shape). Everything below lands in `app.rs` + `editor.rs`.

**Files:**
- Modify: `alpaca-code-rs/src/app.rs` (bulk of the task)
- Modify: `alpaca-code-rs/src/editor.rs` (2-line dead-field removal)
- Modify: `alpaca-code-rs/assets/app.css` (wintitle→projbtn)
- Test: app.rs `mod tests`

**Interfaces:**
- Consumes: T1's `state::open_projects`, `state::add_open`.
- Produces (later tasks / probes rely on these):
  - `pub enum EditKind { Undo, Redo, Cut, Copy, Paste, SelectAll }`
  - `AppMsg::{ OpenWorkspace(PathBuf), SwitchWorkspace(usize), BootActivate(PathBuf), OpenFile(usize, PathBuf), EditorState(usize, EditorSnapshot), RunStarting(usize), RunStatus(usize, bool), OpenDiff { ws: usize, rel, letter }, Commit { ws, root, paths, msg }, Flushed(usize), GitChanged(usize, Rows), EditOp(EditKind) }` (`RunPressed`/`StopPressed`/`FlushTabs`/`RecentMissing`/`ProjectCreated`/`WindowActive`/probe msgs retain their shape; `SetWorkspace`/`DiscardWorkspace` are deleted)
  - `pub const MAX_WORKSPACES: usize = 3`
  - `pub enum AddDecision { Switch(usize), Add, Full }` + `pub fn add_decision(open: &[PathBuf], path: &Path) -> AddDecision`
  - `pub struct Workspace` (fields below) + `App::ws: Vec<Workspace>`, `App::active: usize`
  - the probe chains (ALPACA_PROBE_S2/S3) still run — routed through the now-active workspace.

- [ ] **Step 1: Write the failing unit test** (app.rs `mod tests`):

```rust
    fn pb(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn add_decision_switch_add_full() {
        // requirement 5 + the dedupe rule: already open → switch (never add),
        // under cap → add, at cap → rejected
        let open = [pb("/a"), pb("/b")];
        assert_eq!(add_decision(&open, Path::new("/b")), AddDecision::Switch(1));
        assert_eq!(add_decision(&open, Path::new("/c")), AddDecision::Add);
        let three = [pb("/a"), pb("/b"), pb("/c")];
        assert_eq!(add_decision(&three, Path::new("/d")), AddDecision::Full);
        // empty boot: the first open always adds
        assert_eq!(add_decision(&[], Path::new("/a")), AddDecision::Add);
    }
```

- [ ] **Step 2: Run to verify it fails** — `cargo test --manifest-path alpaca-code-rs/Cargo.toml` → FAIL (`add_decision`/`AddDecision` not defined).

- [ ] **Step 3: Add the new types + Workspace** — in app.rs, above `struct App`:

```rust
/// Requirement 5 — maximum simultaneously open workspaces.
pub const MAX_WORKSPACES: usize = 3;

/// open_workspace's routing decision (pure; the unit test pins it).
#[derive(Debug, PartialEq, Eq)]
pub enum AddDecision {
    /// already open → switch to this strip index (no re-root, no restore)
    Switch(usize),
    /// not open, under the cap → add to the right
    Add,
    /// cap reached (requirement 5) — the caller reports, nothing opens
    Full,
}

pub fn add_decision(open: &[PathBuf], path: &Path) -> AddDecision {
    if let Some(i) = open.iter().position(|p| p.as_path() == path) {
        return AddDecision::Switch(i);
    }
    if open.len() >= MAX_WORKSPACES {
        return AddDecision::Full;
    }
    AddDecision::Add
}

/// Edit-menu ops route by active workspace (T8's actions now fan out to N
/// editors; the shared actions can't hold a Sender per editor).
#[derive(Debug, Clone, Copy)]
pub enum EditKind {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
}

/// One open project: its own editor card, VTE pane card, git panel card and
/// header-strip pill. Switching = a GtkStack visibility flip over these
/// assemblies — everything (VTE children, git probe timers, editor pages,
/// scroll pairing) stays ALIVE while hidden, which is the feature: a
/// project's processes keep running in the background. No re-rooting ever.
pub struct Workspace {
    root: PathBuf,
    /// the card assembly (hpaned → [editor | panes-vpaned, gitpanel]) — one GtkStack page
    hpane: gtk::Paned,
    /// the header-strip pill (App places it left of nothing; T3 adds "+" after)
    btn: gtk::Button,
    editor: relm4::Controller<Editor>,
    panes: relm4::Controller<Panes>,
    gitpanel: relm4::Controller<GitPanel>,
    /// the running mirror for the shared header Run/Stop pills
    running: bool,
    /// latest editor snapshot — FlushTabs persists EVERY workspace's
    snapshot: EditorSnapshot,
    /// gitview Commit → App → SaveOpen → Flushed(ack) → gitpanel StartCommit,
    /// per workspace so a switch mid-flight can't strand the chain
    pending_commit: Option<(PathBuf, Vec<String>, String)>,
}

impl Workspace {
    /// Build the whole per-project UI. `ws` = the index this instance will
    /// occupy in App's vec; component outputs are tagged with it so App
    /// routes back into THIS workspace regardless of the active one.
    fn new(path: PathBuf, ws: usize, app_out: ComponentSender<App>) -> Self {
        let path_str = path.to_string_lossy().into_owned();

        // Task 6 (editor card) — the component carries its own alpaca-card
        let editor_slot = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let app_in = app_out.clone();
        let editor = Editor::builder()
            .launch(())
            .connect_receiver(move |_, out| match out {
                EditorOutput::StateChanged(s) => app_in.input(AppMsg::EditorState(ws, s)),
                EditorOutput::Flushed => app_in.input(AppMsg::Flushed(ws)), // the flush chain's ack
            });
        let tabs = state::project_tabs(&state::load(), &path_str);
        editor_slot.append(editor.widget());
        editor.emit(EditorMsg::Restore(
            path.clone(),
            tabs.open_tabs.clone(),
            tabs.active_tab,
        ));

        // Task 8 (the real panes card)
        let app_in = app_out.clone();
        let panes = Panes::builder()
            .launch(())
            .connect_receiver(move |_, out| match out {
                PanesOutput::RunStarting => app_in.input(AppMsg::RunStarting(ws)),
                PanesOutput::RunStatus { running } => app_in.input(AppMsg::RunStatus(ws, running)),
            });
        let panes_slot = gtk::Box::new(gtk::Orientation::Vertical, 0);
        panes_slot.append(panes.widget());
        let vpane = gtk::Paned::new(gtk::Orientation::Vertical);
        vpane.set_wide_handle(true);
        vpane.set_start_child(Some(&editor_slot));
        vpane.set_end_child(Some(&panes_slot));
        // design: editor card 56..649 (593) − pane handle overlap
        vpane.set_position(592);

        // git panel card (the hpane end child)
        let app_in = app_out.clone();
        let gitpanel = GitPanel::builder()
            .launch(())
            .connect_receiver(move |_, out| match out {
                GitPanelOutput::OpenFile(p) => app_in.input(AppMsg::OpenFile(ws, p)),
                GitPanelOutput::OpenDiff { rel, letter } => {
                    app_in.input(AppMsg::OpenDiff { ws, rel, letter });
                }
                GitPanelOutput::Commit { root, paths, msg } => {
                    app_in.input(AppMsg::Commit { ws, root, paths, msg });
                }
                GitPanelOutput::GitChanged(rows) => app_in.input(AppMsg::GitChanged(ws, rows)),
            });

        // hpane DIRECTLY as the stack page child — a wrapping Box under-allocates
        // (measured 43px tall)
        let hpane = gtk::Paned::new(gtk::Orientation::Horizontal);
        hpane.set_wide_handle(true);
        hpane.set_start_child(Some(&vpane));
        hpane.set_end_child(Some(gitpanel.widget()));
        // design: cards gap columns 1172..1180 → pane pos = card end − 10.
        // NOT re-applied at switch (the position property must survive the
        // hidden page's first allocation — the S4 probe asserts exactly that).
        hpane.set_position(1161);

        // Requirement 2: the pill's idle look IS the old title look
        // (.alpaca-projbtn mirrors .alpaca-wintitle); hover = the branch pill's.
        let btn = gtk::Button::builder()
            .has_frame(false)
            .css_classes(["alpaca-projbtn"])
            .build();
        let lbl = gtk::Label::builder().label(py_basename(&path_str)).build();
        lbl.set_ellipsize(pango::EllipsizeMode::End);
        lbl.set_max_width_chars(24); // 3 long names must not blow the strip out
        btn.set_child(Some(&lbl));
        btn.set_tooltip_text(Some(&path_str));

        Self {
            root: path,
            hpane,
            btn,
            editor,
            panes,
            gitpanel,
            running: false,
            snapshot: EditorSnapshot {
                open_tabs: tabs.open_tabs,
                active_tab: tabs.active_tab,
            },
            pending_commit: None,
        }
    }
}
```

Replace `AppMsg` wholesale (SetWorkspace → OpenWorkspace+SwitchWorkspace+BootActivate; everything workspace-bound gains the index tag; AddProjectDialog arrives in Task 3 — NOT yet):

```rust
#[derive(Debug)]
pub enum AppMsg {
    /// every open path (File▸Open Project…, recents, "+", New Project…,
    /// probe chains): switch if open / add under the cap / report when full
    OpenWorkspace(PathBuf),
    /// header-pill click → instant switch (requirement 8)
    SwitchWorkspace(usize),
    /// startup: swing to the remembered project after ALL queued opens ran
    BootActivate(PathBuf),
    /// tree → editor routing within the tagged workspace
    OpenFile(usize, PathBuf),
    /// editor state flip → snapshot storage + persist debounce
    EditorState(usize, EditorSnapshot),
    /// panes → header Run/Stop sync (launch accepted — the starting window)
    RunStarting(usize),
    RunStatus(usize, bool),
    /// the run/stop pills (shared chrome → the ACTIVE workspace's panes)
    RunPressed,
    StopPressed,
    FlushTabs,
    /// open-recent target no longer exists → info dialog
    RecentMissing(String),
    /// CHANGES row → editor diff page (tagged)
    OpenDiff {
        ws: usize,
        rel: String,
        letter: String,
    },
    /// gitview commit button → that workspace's flush chain
    Commit {
        ws: usize,
        root: PathBuf,
        paths: Vec<String>,
        msg: String,
    },
    /// editor SaveOpen's ack — the ONLY thing that releases pending_commit
    Flushed(usize),
    /// git rows → that workspace's diff-tab renewal
    GitChanged(usize, crate::gitstatus::Rows),
    /// window notify::is-active → the ACTIVE workspace's git re-sync
    WindowActive,
    /// Edit menu → the active workspace's editor
    EditOp(EditKind),
    /// New Project dialog result (makedirs + git init ran OFF the main thread)
    ProjectCreated { target: PathBuf, res: Result<(), String> },
    /// PROBE-ONLY (ALPACA_PROBE_S2) — never runs in production (env unset)
    ProbeS2(PathBuf),
    /// PROBE-ONLY (ALPACA_PROBE_S2) — the step-chain driver
    ProbeS2Step,
    /// PROBE-ONLY (ALPACA_PROBE_S3) — never runs in production (env unset)
    ProbeS3(PathBuf),
    /// PROBE-ONLY (ALPACA_PROBE_S3) — the step-chain driver
    ProbeS3Step,
    /// PROBE-ONLY (ALPACA_PROBE_S3) — 60ms damage keeper
    ProbeDraw,
}
```

- [ ] **Step 4: Rewrite App** — new fields (drop `root`, `title`, `dirty`, `editor`, `panes`, `gitpanel`, `pending_commit`, `editor_state`, `running`):

```rust
struct App {
    stack: gtk::Stack,
    proj_box: gtk::Box,
    run_btn: gtk::Button,
    stop_btn: gtk::Button,
    run_row: gtk::Box,
    menu_btns: HeaderButtons,
    ws: Vec<Workspace>,
    /// valid iff !ws.is_empty() — never read on empty
    active: usize,
    /// PROBE-ONLY (ALPACA_PROBE_S2) — step chain (workspace, step index)
    probe_ws: Option<(PathBuf, usize)>,
    /// PROBE-ONLY (ALPACA_PROBE_S3) — step chain (workspace, step index)
    probe_s3: Option<(PathBuf, usize)>,
}
```

In `init()`:

- First line of the body: `let arg = model.clone();` — the CLI arg (App's
  `root: Option<String>` field is deleted below, so the startup block must
  read the init param before `let model = App { … }` shadows it).

- Delete the `title` label construction (3 lines: `let title = gtk::Label…` through its ellipsize).
- Replace the header assembly's title line with the strip box:

```rust
        // Requirements 1/6/7: the title label is GONE — the header center is a
        // strip of project pills, centered as ONE entity by the headerbar's own
        // title-widget centering. 6px tasteful gaps (requirement 7).
        let proj_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        // header.set_title_widget(Some(&title));  ← deleted
        header.set_title_widget(Some(&proj_box));
```

- Replace the editor/panes/gitpanel/hpane assembly block (old lines 231–287: `editor_slot` through `hpane.set_position(1161)` and `root.set_child(Some(&hpane))`) with `Workspace::new` (it moved verbatim) and the stack:

```rust
        // The window's content = one stack page per open workspace
        // (requirement 8: transition None = instant, like browser tabs).
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::None)
            .hexpand(true)
            .vexpand(true)
            .build();
        root.set_child(Some(&stack));
```

- The model literal becomes:

```rust
        let model = App {
            stack,
            proj_box,
            run_btn,
            stop_btn,
            run_row,
            menu_btns,
            ws: Vec::new(),
            active: 0,
            probe_ws: None,
            probe_s3: None,
        };
```

- Delete `self.dirty: false`-era fields and `refresh_menus()` stays; the OLD startup block (`if let Some(p) = model.root…SetWorkspace`) becomes the boot sequence:

```rust
        // startup: reopen last session's strip (T1's open_projects; old files
        // coerce to [last_project] via the fallback), then swing active to the
        // remembered one. BootActivate must ride the SAME queue as the opens.
        let st = state::load();
        let mut opens: Vec<PathBuf> = state::open_projects(&st)
            .iter()
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .collect();
        if opens.is_empty()
            && let Some(l) = st.last_project.clone()
            && Path::new(&l).is_dir()
        {
            opens.push(PathBuf::from(l));
        }
        for p in opens {
            sender.input(AppMsg::OpenWorkspace(p));
        }
        if let Some(l) = st.last_project {
            sender.input(AppMsg::BootActivate(PathBuf::from(l)));
        }
        // the CLI arg opens LAST — it ends active (the old launch's whole point)
        if let Some(p) = arg {
            sender.input(AppMsg::OpenWorkspace(p));
        }
```

(The S2/S3 probe-arm blocks right below stay as-is.)

- `register_actions(&relm4::main_application(), &root, sender.clone(), ed_in);` → drop the `ed_in` arg (`register_actions(&relm4::main_application(), &root, sender.clone());`).

- [ ] **Step 5: Rewrite the update() arms + helpers** — the new match body (unchanged probe plumbing appears once more, routed through the active workspace; `AppMsg::AddProjectDialog` lands in Task 3):

```rust
    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match msg {
            AppMsg::OpenWorkspace(p) => self.open_workspace(&p, root, sender),
            AppMsg::SwitchWorkspace(i) => self.switch_to(i, root),
            AppMsg::BootActivate(p) => {
                if let Some(i) = self.ws.iter().position(|w| w.root.as_path() == p.as_path()) {
                    self.switch_to(i, root);
                }
            }
            AppMsg::OpenFile(ws, p) => {
                if let Some(w) = self.ws.get_mut(ws) {
                    w.editor.emit(EditorMsg::OpenFile(p));
                }
            }
            AppMsg::EditorState(ws, s) => {
                if let Some(w) = self.ws.get_mut(ws) {
                    w.snapshot = s;
                }
                if !self.ws.is_empty() {
                    // debounced ≤1s; each event re-arms (stacked timers flush
                    // the LATEST snapshots of EVERY workspace; last event
                    // before quit may be lost — acceptable, as before)
                    let sender = sender.clone();
                    glib::timeout_add(std::time::Duration::from_millis(1000), move || {
                        sender.input(AppMsg::FlushTabs);
                        glib::ControlFlow::Break
                    });
                }
            }
            AppMsg::FlushTabs => {
                let mut st = state::load();
                for w in &self.ws {
                    st = state::set_tabs(
                        st,
                        w.root.to_string_lossy().as_ref(),
                        w.snapshot.open_tabs.clone(),
                        w.snapshot.active_tab,
                    );
                }
                state::save(&st);
            }
            AppMsg::RunStatus(ws, running) => {
                if let Some(w) = self.ws.get_mut(ws) {
                    w.running = running;
                }
                if ws == self.active {
                    self.run_btn.set_sensitive(!running);
                    self.stop_btn.set_sensitive(running);
                    self.refresh_run_style();
                }
            }
            AppMsg::RunStarting(ws) => {
                if let Some(w) = self.ws.get_mut(ws) {
                    w.running = true;
                }
                if ws == self.active {
                    self.run_btn.set_sensitive(false); // Stop-only until run ends
                    self.refresh_run_style();
                }
            }
            AppMsg::RunPressed => {
                let running = self.ws.get(self.active).map(|w| w.running).unwrap_or(true);
                if running {
                    return; // no workspace yet / live run → noop
                }
                let cmd = self
                    .ws
                    .get(self.active)
                    .and_then(|w| runctl::detect(w.root.as_path()));
                let Some(cmd) = cmd else {
                    self.run_btn.set_sensitive(false);
                    self.run_btn
                        .set_tooltip_text(Some("No dev/start script or .csproj found"));
                    return;
                };
                self.stop_btn.set_sensitive(true);
                if let Some(w) = self.ws.get(self.active) {
                    w.panes.emit(PanesMsg::LaunchRun { argv: cmd.argv });
                }
            }
            AppMsg::StopPressed => {
                if let Some(w) = self.ws.get(self.active) {
                    w.panes.emit(PanesMsg::StopRun);
                }
                self.stop_btn.set_sensitive(false);
                self.run_btn.set_sensitive(true);
                self.refresh_run_style();
            }
            AppMsg::RecentMissing(p) => {
                error_dialog(root, &format!("Project not found: {p}"), "");
            }
            AppMsg::ProjectCreated { target, res } => match res {
                Err(e) => error_dialog(root, &format!("Couldn't create {}", target.display()), &e),
                Ok(()) => sender.input(AppMsg::OpenWorkspace(target)),
            },
            AppMsg::OpenDiff { ws, rel, letter } => {
                if let Some(w) = self.ws.get(ws) {
                    w.editor.emit(EditorMsg::OpenDiff { root: w.root.clone(), rel, letter });
                }
            }
            AppMsg::Commit { ws, root, paths, msg } => {
                // the commit's dirty-file flush must run SYNCHRONOUSLY on the
                // commit's mainloop turn — the per-workspace pending store +
                // the Flushed ack chain (relm4 inputs are QUEUE-processed)
                if let Some(w) = self.ws.get_mut(ws) {
                    // SaveOpen FIRST (paths by ref), the store move second —
                    // pending_commit takes ownership of the payload tail
                    w.editor.emit(EditorMsg::SaveOpen(
                        paths.iter().map(|rel| root.join(rel)).collect(),
                    ));
                    w.pending_commit = Some((root, paths, msg));
                }
            }
            AppMsg::Flushed(ws) => {
                // the ack strictly follows its SaveOpen in the editor's FIFO
                // queue → a sound chain (save_open ALWAYS emits Flushed,
                // zero-flushed included — the chain cannot strand)
                if let Some(w) = self.ws.get_mut(ws)
                    && let Some((root, paths, msg)) = w.pending_commit.take() {
                        w.gitpanel.emit(GitPanelMsg::StartCommit { root, paths, msg });
                    }
            }
            AppMsg::GitChanged(ws, rows) => {
                if let Some(w) = self.ws.get(ws) {
                    let letters = rows
                        .iter()
                        .map(|(rel, letter)| (rel.clone(), letter.clone()))
                        .collect();
                    w.editor.emit(EditorMsg::RefreshDiffs { root: w.root.clone(), letters });
                }
            }
            AppMsg::WindowActive => {
                if let Some(w) = self.ws.get(self.active) {
                    w.gitpanel.emit(GitPanelMsg::RefreshGit);
                }
            }
            AppMsg::EditOp(kind) => {
                if let Some(w) = self.ws.get_mut(self.active) {
                    w.editor.emit(match kind {
                        EditKind::Undo => EditorMsg::Undo,
                        EditKind::Redo => EditorMsg::Redo,
                        EditKind::Cut => EditorMsg::Cut,
                        EditKind::Copy => EditorMsg::Copy,
                        EditKind::Paste => EditorMsg::Paste,
                        EditKind::SelectAll => EditorMsg::SelectAll,
                    });
                }
            }
            AppMsg::ProbeS2(dir) => {
                // PROBE-ONLY (ALPACA_PROBE_S2) — unchanged arm + schedule
                self.probe_ws = Some((dir, 0));
                let snd = sender.clone();
                glib::timeout_add(std::time::Duration::from_millis(PROBE_STEP_MS[0]), move || {
                    snd.input(AppMsg::ProbeS2Step);
                    glib::ControlFlow::Break
                });
            }
            AppMsg::ProbeS2Step => self.probe_step(root, &sender),
            AppMsg::ProbeS3(dir) => {
                self.probe_s3 = Some((dir, 0));
                root.present();
                let snd = sender.clone();
                glib::timeout_add(std::time::Duration::from_millis(60), move || {
                    snd.input(AppMsg::ProbeDraw);
                    glib::ControlFlow::Continue
                });
                let snd = sender.clone();
                glib::timeout_add(std::time::Duration::from_millis(PROBE_S3_STEP_MS[0]), move || {
                    snd.input(AppMsg::ProbeS3Step);
                    glib::ControlFlow::Break
                });
            }
            AppMsg::ProbeS3Step => {
                // PROBE-ONLY (ALPACA_PROBE_S3) — one chain; emissions target
                // the active workspace (the chain's own opens)
                let Some((ws_dir, step)) = self.probe_s3.as_ref().map(|(w, s)| (w.clone(), *s)) else {
                    return;
                };
                println!("PROBE step {}: ts={}", step, epoch_ms());
                let hpane = {
                    let mut found: Option<gtk::Paned> = None;
                    let mut c = root.first_child();
                    while let Some(w) = c {
                        let next = w.next_sibling();
                        if let Ok(p) = w.downcast::<gtk::Paned>() {
                            found = Some(p);
                        }
                        c = next;
                    }
                    found
                };
                match step {
                    0 => {
                        self.open_workspace(ws_dir.as_path(), root, sender.clone());
                        let i = self.active;
                        for f in ["package-lock.json", "next.config.ts", "components.json",
                                  "related-products.tsx", "trude.json",
                                  "deep/path/segments/segment-file-with-long-name.tsx"] {
                            sender.input(AppMsg::OpenFile(i, ws_dir.join(f)));
                        }
                    }
                    1 => {
                        let Some(w) = self.ws.get(self.active) else { return };
                        let checks = [
                            ("window", root.measure(gtk::Orientation::Horizontal, -1).0, 560),
                            ("editor", w.editor.widget().measure(gtk::Orientation::Horizontal, -1).0, 300),
                            ("panes", w.panes.widget().measure(gtk::Orientation::Horizontal, -1).0, 300),
                        ];
                        for (name, got, cap) in checks {
                            let ok = got <= cap;
                            println!("PROBE assert: s3-min-{name} {got}<={cap}: {}",
                                     if ok { "PASS" } else { "FAIL" });
                        }
                        w.gitpanel.emit(GitPanelMsg::TreeMin);
                    }
                    2 => root.set_size_request(1586, 992),
                    3 => if let Some(h) = &hpane { h.set_position(900); },
                    4 => {
                        let Some(w) = self.ws.get(self.active) else { return };
                        if let Some(h) = &hpane {
                            println!("PROBE hpane pos={} w={} at wide-trace",
                                     h.position(), h.allocated_width());
                        }
                        w.editor.emit(EditorMsg::ProbeTrace("wide".into()));
                    }
                    5 => if let Some(h) = &hpane { h.set_position(850); },
                    6 => {
                        if let Some(w) = self.ws.get(self.active) {
                            w.editor.emit(EditorMsg::ProbeTrace("narrow".into()));
                        }
                        println!("PROBE S3 DONE");
                        self.probe_s3 = None;
                        return;
                    }
                    _ => {} // unreachable — the chain drives 0..=6 only
                }
                if let Some(next) = PROBE_S3_STEP_MS.get(step + 1) {
                    self.probe_s3.as_mut().unwrap().1 = step + 1;
                    let snd = sender.clone();
                    glib::timeout_add(std::time::Duration::from_millis(*next), move || {
                        snd.input(AppMsg::ProbeS3Step);
                        glib::ControlFlow::Break
                    });
                }
            }
            AppMsg::ProbeDraw => {
                if self.probe_s3.is_some() {
                    root.queue_draw();
                    PROBE_S3_TICKS.with(|t| {
                        let n = t.get() + 1;
                        t.set(n);
                        if n % 8 == 0 {
                            println!("PROBE tick {n}: root w={}", root.allocated_width());
                        }
                    });
                }
            }
        }
    }
```

- [ ] **Step 6: Probe chains + helpers** — `probe_step` gains a `root` arg and routes through the active workspace (S2 chain):

```rust
    fn probe_step(&mut self, root: &gtk::ApplicationWindow, sender: &ComponentSender<Self>) {
        const PROBE_MSG: &str = "s2 probe commit";
        let Some((ws, step)) = self.probe_ws.as_ref().map(|(w, s)| (w.clone(), *s)) else {
            return;
        };
        let ws_b = ws.to_string_lossy().replace("wsA", "wsB");
        println!("PROBE step {}: ts={}", step, epoch_ms());
        match step {
            // the REAL workspace route — panel SetRoot + editor Restore + the
            // open set; the OpenDiff arm needs this workspace active
            0 => {
                self.open_workspace(ws.as_path(), root, sender.clone());
                let i = self.active;
                for f in ["package-lock.json", "next.config.ts", "components.json",
                          "related-products.tsx", "trude.json",
                          "deep/path/segments/segment-file-with-long-name.tsx"] {
                    sender.input(AppMsg::OpenFile(i, ws.join(f)));
                }
            }
            1 => {
                if let Some(w) = self.ws.get(self.active) {
                    w.gitpanel.emit(GitPanelMsg::ModeChanges);
                }
            }
            2 => {
                if let Some(w) = self.ws.get(self.active) {
                    w.gitpanel.emit(GitPanelMsg::ProbeChild(ProbeChild::Changes(
                        GitViewMsg::ProbeMsg(PROBE_MSG.into()),
                    )));
                }
            }
            3 => sender.input(AppMsg::OpenDiff {
                ws: self.active,
                rel: "a.txt".into(),
                letter: "M".into(),
            }),
            4 => {
                if let Some(w) = self.ws.get(self.active) {
                    w.editor.emit(EditorMsg::ProbeClip);
                }
            }
            5 => {
                if let Some(w) = self.ws.get(self.active) {
                    w.gitpanel.emit(GitPanelMsg::ProbeEntry("alpha".into()));
                }
            }
            6 => {
                if let Some(w) = self.ws.get(self.active) {
                    w.gitpanel.emit(GitPanelMsg::ProbeEntry("alphabet".into()));
                }
            }
            7 => {
                if let Some(w) = self.ws.get(self.active) {
                    w.gitpanel.emit(GitPanelMsg::ProbeEntry("alphabetZ".into()));
                }
            }
            8 => {
                if let Some(w) = self.ws.get(self.active) {
                    w.gitpanel.emit(GitPanelMsg::ProbeEntry("alphabetZx".into()));
                }
            }
            9 => {
                if let Some(w) = self.ws.get(self.active) {
                    w.gitpanel.emit(GitPanelMsg::ProbeEntry("".into()));
                    w.editor.emit(EditorMsg::ProbeDiffScroll);
                }
            }
            10 => {
                // select-all via the REAL row-toggle law — rows arrive
                // PRE-CHECKED, so toggle twice (off→on)
                if let Some(w) = self.ws.get(self.active) {
                    let mast = || GitPanelMsg::ProbeChild(ProbeChild::Changes(
                        GitViewMsg::RowToggled(gtk::TreePath::from_string("0").unwrap()),
                    ));
                    w.gitpanel.emit(mast()); // off
                    w.gitpanel.emit(mast()); // on — ends all-checked
                }
            }
            11 => {
                if let Some(w) = self.ws.get(self.active) {
                    w.gitpanel.emit(GitPanelMsg::ProbeChild(ProbeChild::Changes(
                        GitViewMsg::CommitClicked,
                    )));
                }
            }
            // MID-FLIGHT injection: wsB is its own clean repo; under the new
            // semantics this ADDS wsB as workspace 2 and switches (wsA stays
            // alive — its flight's root was SNAPSHOT into StartCommit, the
            // commit still lands on wsA, asserted next)
            12 => self.open_workspace(Path::new(&ws_b), root, sender.clone()),
            // assertions — unchanged (state.json + git fixtures)
            13 => {
                let ws_a = ws.clone();
                let wsb: PathBuf = (&ws_b).into();
                sender.spawn_command(move |_| {
                    let git = |dir: &Path, args: &[&str]| {
                        let o = std::process::Command::new("git")
                            .args(args)
                            .current_dir(dir)
                            .output();
                        o.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                            .unwrap_or_default()
                    };
                    let say = |name: &str, ok| {
                        println!("PROBE assert: {}: {}", name, if ok { "PASS" } else { "FAIL" })
                    };
                    say("wsA-commit-count", git(&ws_a, &["rev-list", "--count", "HEAD"]) == "2");
                    say("wsA-commit-msg", git(&ws_a, &["log", "-1", "--format=%s"]) == PROBE_MSG);
                    say("wsB-commit-count", git(&wsb, &["rev-list", "--count", "--all"]) == "0");
                    say(
                        "wsA-pushed",
                        git(&ws_a, &["rev-parse", "origin/main"]) == git(&ws_a, &["rev-parse", "HEAD"]),
                    );
                    let st = std::fs::read_to_string(state::state_path()).unwrap_or_default();
                    say(
                        "state-last-project",
                        st.contains(&format!("\"last_project\": \"{}\"", ws_b)),
                    );
                });
            }
            14 => {
                println!("PROBE S2 DONE");
                self.probe_ws = None;
                return;
            }
            _ => return,
        }
        if let Some(n) = std::env::var("ALPACA_PROBE_S2_HOLD")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            && step + 1 >= n
        {
            println!("PROBE S2 HOLD: parked after step {step} — window open for capture");
            self.probe_ws = None;
            return;
        }
        self.probe_ws.as_mut().unwrap().1 = step + 1;
        let ms = step_ms(step + 1);
        let snd = sender.clone();
        glib::timeout_add(std::time::Duration::from_millis(ms), move || {
            snd.input(AppMsg::ProbeS2Step);
            glib::ControlFlow::Break
        });
    }
```

- [ ] **Step 7: Replace the workspace methods + delete dead code** — replace `set_workspace`/`set_workspace_now`/`show_discard_dialog` wholesale with:

```rust
    /// Every workspace-open path lands here (header "+", File ▸ Open Project…,
    /// recents, New Project…, probe chains): switch if open, add to the right
    /// under the cap, else report (requirements 3/4/5). Nothing is ever
    /// destroyed on open, so the old discard dialog is GONE — unsaved buffers
    /// survive inside their own always-alive workspace.
    fn open_workspace(
        &mut self,
        path: &Path,
        root_win: &gtk::ApplicationWindow,
        sender: ComponentSender<Self>,
    ) {
        let open: Vec<PathBuf> = self.ws.iter().map(|w| w.root.clone()).collect();
        match add_decision(&open, path) {
            AddDecision::Full => {
                error_dialog(root_win, "Maximum of 3 projects open", "");
            }
            AddDecision::Switch(i) => self.switch_to(i, root_win),
            AddDecision::Add => {
                let i = self.ws.len();
                let ws = Workspace::new(path.to_path_buf(), i, sender.clone());
                // stack page name: unique — open paths are deduped
                self.stack
                    .add_named(ws.hpane.as_ref(), Some(&ws.root.to_string_lossy()));
                self.ws.push(ws);
                // requirement 4: appended to the right of the previous one;
                // title_widget centers the strip as one entity (requirement 7)
                self.proj_box.append(&self.ws[i].btn);
                let path_str = path.to_string_lossy();
                state::save(&state::add_open(
                    state::remember(state::load(), path_str.as_ref()),
                    path_str.as_ref(),
                ));
                self.switch_to(i, root_win);
            }
        }
    }

    /// Header-pill click / an open's activation: flip the stack page — state
    /// carries over because NOTHING is re-rooted (the feature's core).
    fn switch_to(&mut self, i: usize, root: &gtk::ApplicationWindow) {
        if i >= self.ws.len() {
            return;
        }
        self.stack.set_visible_child(self.ws[i].hpane.as_ref());
        self.active = i;
        let path_str = self.ws[i].root.to_string_lossy().into_owned();
        // requirement 1: only the project name, in the WM title as well
        root.set_title(Some(&py_basename(&path_str)));
        state::save(&state::remember(state::load(), &path_str));
        for (j, w) in self.ws.iter().enumerate() {
            if j == i {
                w.btn.add_css_class("alpaca-on");
            } else {
                w.btn.remove_css_class("alpaca-on");
            }
        }
        // the shared header pills track the NEW workspace's run state
        self.sync_run_buttons();
        // a switch re-syncs git once (the set-root refresh's law, reused)
        self.ws[i].gitpanel.emit(GitPanelMsg::RefreshGit);
    }

    /// Run/Stop pills + tooltip follow the active workspace (its own run
    /// mirror + the per-root detect). Called on switch (and from RunStatus).
    fn sync_run_buttons(&mut self) {
        let Some(w) = self.ws.get(self.active) else {
            self.run_btn.set_sensitive(false);
            self.stop_btn.set_sensitive(false);
            self.refresh_run_style();
            return;
        };
        let cmd = runctl::detect(w.root.as_path());
        self.run_btn.set_tooltip_text(Some(
            cmd.as_ref()
                .map(|c| c.label.as_str())
                .unwrap_or("No dev/start script or .csproj found"),
        ));
        self.run_btn.set_sensitive(!w.running);
        self.stop_btn.set_sensitive(w.running);
        self.refresh_run_style();
    }

    /// the running class rides the pill row — the ACTIVE workspace's mirror
    fn refresh_run_style(&self) {
        let running = self.ws.get(self.active).map(|w| w.running).unwrap_or(false);
        if running {
            self.run_row.set_css_classes(&["alpaca-runpill", "running"]);
        } else {
            self.run_row.set_css_classes(&["alpaca-runpill"]);
        }
    }
```

Also in `init()`/`run()`: `open_project`'s response arm becomes `sender.input(AppMsg::OpenWorkspace(p))`, `new_project`'s `ProjectCreated` Ok arm already routes `OpenWorkspace` (step 5), recents action: `sender.input(AppMsg::OpenWorkspace(p.into()))`, and the edit-actions loop becomes:

```rust
    for name in ["undo", "redo", "cut", "copy", "paste", "select-all"] {
        let a = gio::SimpleAction::new(name, None);
        let sender = sender.clone();
        a.connect_activate(move |_a, _p| {
            let kind = match name {
                "undo" => EditKind::Undo,
                "redo" => EditKind::Redo,
                "cut" => EditKind::Cut,
                "copy" => EditKind::Copy,
                "paste" => EditKind::Paste,
                _ => EditKind::SelectAll,
            };
            sender.input(AppMsg::EditOp(kind));
        });
        app.add_action(&a);
    }
```

with `register_actions(app, root_win, sender)` (ed_in param gone).

DELETE: `AppMsg::DiscardWorkspace` arm, `show_discard_dialog`, the `dirty` field + `self.dirty = s.has_dirty` line, `AppMsg::Flushed`'s old un-tagged body, the `root: Option<String>` field's consumers (`self.root.is_some()` guards in arms become the `self.ws.get(ws)` guards shown above).

- [ ] **Step 8: editor.rs dead field** — the dirty-gate was the only consumer. In `editor.rs`: delete `pub has_dirty: bool` from `EditorSnapshot` (line ~188), and in emit_state (line ~1689) delete `let has_dirty = …;` + use `EditorSnapshot { open_tabs, active_tab }`.

- [ ] **Step 9: CSS** — in `assets/app.css`, delete the `.alpaca-wintitle` rule (line 42, dead) and add:

```css
/* --- workspace pills (multi-project header, 2026-10-14) ---
   Idle = the EXACT old wintitle look (user requirement 2); hover = the branch
   pill's #1c2431 fill (Image #7). Inactive rides dimmer so the active still
   reads as the current title. has_frame(false) buttons; text color rides the
   button (branchbtn law) + explicit label:backdrop states (leak law). The
   runpill sweep first (headerbar buttons' ~16px vertical additive dies ONLY
   with the everything-0 sweep), then the wanted shape re-raised same block. */
.alpaca-projbtn, .alpaca-projbtn > label,
.alpaca-projadd, .alpaca-projadd > label {
    background: transparent; border: none; border-radius: 6px;
    min-height: 0px; min-width: 0px; padding: 0px; margin: 0px; border-spacing: 0px;
}
.alpaca-projbtn { color: #4d5666; font-size: 13px; font-weight: 500; padding: 2px 10px; }
.alpaca-projbtn.alpaca-on { color: #6b7487; }
.alpaca-projbtn:hover, .alpaca-projbtn.alpaca-on:hover { background: #1c2431; color: #e6e8ee; }
.alpaca-projbtn label:backdrop { color: #4d5666; }
.alpaca-projbtn.alpaca-on label:backdrop { color: #6b7487; }
.alpaca-projbtn:hover label:backdrop, .alpaca-projbtn.alpaca-on:hover label:backdrop { color: #e6e8ee; }
```

(`.alpaca-projadd` styles arrive with Task 3 — the sweep already names it so the selector list doesn't grow later.)

- [ ] **Step 10: Verify** — `cargo test --manifest-path alpaca-code-rs/Cargo.toml` → PASS (`add_decision_switch_add_full` now defines+passes, all pre-existing green). Then `cargo build --manifest-path alpaca-code-rs/Cargo.toml` → clean compile, no warnings.

- [ ] **Step 11: Commit** — `git add -A && git commit -m "feat: multi-workspace core — per-project instances in stack pages"`

---

### Task 3: Header "+" button + the cap

**Files:**
- Modify: `alpaca-code-rs/src/app.rs` (init strip assembly + `AddProjectDialog` msg) 
- Modify: `alpaca-code-rs/assets/app.css` (the `.alpaca-projadd` shape rules)

**Interfaces:**
- Consumes: T2's `AppMsg::OpenWorkspace` + `open_workspace` (Full arm), `AddDecision::Full`.
- Produces: `AppMsg::AddProjectDialog` + the requirement-3 `+`; T4's probe asserts `add-btn-disabled`.

- [ ] **Step 1: Add the msg + button.** In `AppMsg` add:

```rust
    /// requirement 3: header "+" → the folder picker (Shared open path)
    AddProjectDialog,
```

In `init()`, after `proj_box` is built:

```rust
        // requirement 3: "+" to the right of the project pills; the strip is
        // [pill … pill | +] — the same flat pill family (+ CSS Task 3)
        let add_btn = gtk::Button::builder()
            .has_frame(false)
            .css_classes(["alpaca-projadd"])
            .label("+")
            .build();
        add_btn.set_tooltip_text(Some("Open project…"));
        proj_box.append(&add_btn);
        let send = sender.clone();
        add_btn.connect_clicked(move |_b| send.input(AppMsg::AddProjectDialog));
```

In `open_workspace`'s `Add` arm, replace the bare append with right-of-pills ordering + the cap kill (requirement 5):

```rust
                // the + rides LAST (strip order [pill … pill | +]): re-append
                // moves it back behind the fresh pill
                self.proj_box.remove(&self.add_btn);
                self.proj_box.append(&self.ws[i].btn);
                self.proj_box.append(&self.add_btn);
                self.add_btn.set_sensitive(self.ws.len() < MAX_WORKSPACES);
```

In the `Full` arm nothing changes (the dialog IS the requirement-5 report for menu-driven open paths).

`App` model gains `add_btn: gtk::Button` (init literal too). The `AddProjectDialog` arm:

```rust
            AppMsg::AddProjectDialog => open_project(root, sender),
```

- [ ] **Step 2: CSS** — append to the task-2 block in `app.css`:

```css
.alpaca-projadd { color: #8a93a6; font-size: 13px; font-weight: 500; padding: 2px 8px; }
.alpaca-projadd:disabled { color: #3d4454; }
```

- [ ] **Step 3: Verify** — `cargo test --manifest-path alpaca-code-rs/Cargo.toml` → PASS; `cargo build --manifest-path alpaca-code-rs/Cargo.toml` → clean; manual smoke (per CLAUDE.md: exact-pid kill of a previous instance first — a second spawn forwards to the running primary):

```
cargo run --manifest-path alpaca-code-rs/Cargo.toml <existing-dir>
```

Expected by requirement: header center = [pill(basename)] 6px [+] at the exact old title position with the exact old text look; click opens a folder picker; a chosen folder appends as pill #2 and switches instantly; pill hover shows the branch-pill's dark fill; open 3 → "+" dims; a 4th via File▸Open Project… → "Maximum of 3 projects open"; Editor/Agent/Terminal/Output/File Browser of project 2 invisible in project 1's tab; switching back instantly shows project 1's tabs + agent still running; switching re-syncs Run/Stop to the switched-to project's state; the headerbar strip stays one centered entity; window title bar text = project basename only.

- [ ] **Step 4: Commit** — `git add -A && git commit -m "feat: header workspace pills + picker and the 3-project cap"`

---

### Task 4: S4 live probe (the widget-layer gate) + CLAUDE.md notes

CLAUDE.md discipline: widget-layer bookkeeping shipped real bugs past 21 green unit tests (the respawn-band). The stack/btn-strip/hidden-allocation behavior gets the same treatment: an env-guarded S4 chain, run headless on the broadway rig, asserting what only live checks can.

**Files:**
- Modify: `alpaca-code-rs/src/app.rs` (ProbeS4 arms + table)
- Create: `/tmp/s4probe/` rig via one script step (out-of-repo, like the seti/s2 rigs; the script itself lives in this plan, never in the repo)
- Modify: CLAUDE.md (probe env list + workspace status lines)

**Interfaces:**
- Consumes: T2's `open_workspace`/`switch_to`, T3's `add_btn`.
- Produces: `ALPACA_PROBE_S4=<dir>` env + `PROBE_S4`/`PROBE_S4Step` msg pair + step table; asserts `three-open`, `add-btn-disabled`, `stack-visible-is-wsA`, `editor-tabs-preserved`, `hpane-pos-1161`, `on-class-wsA`, `re-open-wsA-switches`.

- [ ] **Step 1: Write the probe chain** — in AppMsg add `ProbeS4(PathBuf)` / `ProbeS4Step`; `App` gains `probe_s4: Option<(PathBuf, usize)>`; in `init()` after the S2/S3 arms:

```rust
        // PROBE-ONLY (ALPACA_PROBE_S4) — never runs in production (env unset)
        if let Ok(dir) = std::env::var("ALPACA_PROBE_S4") {
            sender.input(AppMsg::ProbeS4(PathBuf::from(dir)));
        }
```

The step table (same shape discipline as S2's):

```rust
// PROBE-ONLY (ALPACA_PROBE_S4) multi-workspace chain — wsB/wsC are the sibling
// dirs of the passed wsA (name-replace, like S2's convention):
// 0 open wsA (+ open a.txt) · 1 open wsB (+ open b.txt) ·
// 2 switch BACK to wsA + the live asserts (stack identity, tabs, paned pos) ·
// 3 open wsC → cap-full asserts (3 open, "+" disabled) ·
// 4 re-open wsA → must SWITCH not add, then DONE
const PROBE_S4_STEP_MS: [u64; 6] = [500, 700, 700, 700, 700, 500];
```

Handler in `update`:

```rust
            AppMsg::ProbeS4(dir) => {
                // PROBE-ONLY (ALPACA_PROBE_S4) — never runs in production
                self.probe_s4 = Some((dir, 0));
                root.present();
                let snd = sender.clone();
                glib::timeout_add(std::time::Duration::from_millis(PROBE_S4_STEP_MS[0]), move || {
                    snd.input(AppMsg::ProbeS4Step);
                    glib::ControlFlow::Break
                });
            }
            AppMsg::ProbeS4Step => {
                let Some((ws_dir, step)) = self.probe_s4.as_ref().map(|(w, s)| (w.clone(), *s)) else {
                    return;
                };
                let say = |name: &str, ok: bool| {
                    println!("PROBE assert: {name}: {}", if ok { "PASS" } else { "FAIL" })
                };
                match step {
                    0 => {
                        self.open_workspace(ws_dir.as_path(), root, sender.clone());
                        let i = self.active;
                        sender.input(AppMsg::OpenFile(i, ws_dir.join("a.txt")));
                    }
                    1 => {
                        let ws_b = ws_dir.to_string_lossy().replace("wsA", "wsB");
                        self.open_workspace(std::path::Path::new(&ws_b), root, sender.clone());
                        sender.input(AppMsg::OpenFile(self.active, std::path::Path::new(&ws_b).join("b.txt")));
                    }
                    2 => {
                        println!("PROBE step 2: ts={}", epoch_ms());
                        self.switch_to(0, root);
                        say("two-open", self.ws.len() == 2);
                        // THE feature asserts, live: state survives the flip.
                        // GObject PartialEq compares memory addresses
                        // (verified in glib-0.22 object.rs) — Widget vs Paned
                        // is the same instance's pointer, no upcast dance.
                        say(
                            "stack-visible-is-wsA",
                            self.stack
                                .visible_child()
                                .is_some_and(|v| v == self.ws[0].hpane),
                        );
                        say(
                            "editor-tabs-preserved",
                            // snapshot open_tabs are RELPATHS (editor emit_state)
                            self.ws[0].snapshot.open_tabs == vec!["a.txt".to_string()],
                        );
                        say("hpane-pos-1161", self.ws[0].hpane.position() == 1161);
                        say(
                            "on-class-wsA",
                            self.ws[0].btn.has_css_class("alpaca-on")
                                && !self.ws[1].btn.has_css_class("alpaca-on"),
                        );
                        // requirement 1: the WM title carries the bare name
                        say(
                            "title-is-basename",
                            root.title().as_deref()
                                == Some(self.ws[0].root.file_name().unwrap().to_str().unwrap()),
                        );
                    }
                    3 => {
                        let ws_c = ws_dir.to_string_lossy().replace("wsA", "wsC");
                        self.open_workspace(std::path::Path::new(&ws_c), root, sender.clone());
                        say("three-open", self.ws.len() == 3);
                        say("add-btn-disabled", !self.add_btn.is_sensitive());
                        // active is wsC now; swing back to wsA via the real
                        // switch path
                        self.switch_to(0, root);
                    }
                    // requirement: an already-open path must SWITCH (T2's
                    // add_decision), never duplicate the strip pill
                    4 => {
                        self.open_workspace(ws_dir.as_path(), root, sender.clone());
                        say("re-open-wsA-switches", self.ws.len() == 3 && self.active == 0);
                        println!("PROBE S4 DONE");
                        self.probe_s4 = None;
                        return;
                    }
                    _ => {}
                }
                if let Some(next) = PROBE_S4_STEP_MS.get(step + 1) {
                    self.probe_s4.as_mut().unwrap().1 = step + 1;
                    let snd = sender.clone();
                    glib::timeout_add(std::time::Duration::from_millis(*next), move || {
                        snd.input(AppMsg::ProbeS4Step);
                        glib::ControlFlow::Break
                    });
                }
            }
```

- [ ] **Step 2: Write the rig script** (out-of-repo `/tmp/s4run.sh`, mirroring the CLAUDE.md broadway rig; probe dirs reset every run; state.json is isolated behind XDG_CONFIG_HOME because a probe run must NOT leak open_projects into the user's session):

```bash
#!/bin/bash
set -e
ROOT=/tmp/s4probe
rm -rf "$ROOT"
mkdir -p "$ROOT/config"
for n in A B C; do
  mkdir -p "$ROOT/ws$n"
  echo "probe" > "$ROOT/ws$n/$(echo $n | tr 'A-Z' 'a-z').txt"
done
export GDK_BACKEND=broadway
N=${1:-2}                              # fresh number per run (retry with :3… on a stale socket)
gtk4-broadwayd :$N &
trap 'pkill -x broadwayd; rm -f /run/user/1000/broadway*.socket' EXIT
sleep 0.5
dbus-run-session -- \
  env GDK_BACKEND=broadway BROADWAY_DISPLAY=:$N \
      XDG_CONFIG_HOME="$ROOT/config" \
      ALPACA_PROBE_S4="$ROOT/wsA" \
  "$PWD/target/debug/alpaca-code-rs" 2>&1 | tee /tmp/s4probe/log
grep -q 'PROBE S4 DONE' /tmp/s4probe/log
while IFS= read -r line; do
  echo "$line" | grep -q ': PASS' || { echo "$line"; FAILED=1; }
done < <(grep 'PROBE assert' /tmp/s4probe/log)
exit ${FAILED:-0}
```

Run exactly as CLAUDE.md's rig law: fresh broadway number per run, socket + daemon cleanup between runs ("pkill -x broadwayd" — never `pkill -f`).

- [ ] **Step 3: Run it** — `cargo build --manifest-path alpaca-code-rs/Cargo.toml && bash /tmp/s4run.sh` → every `PROBE assert` line reads `PASS`, `PROBE S4 DONE` in the log. If `hpane-pos-1161` FAILS: first READ the `PROBE hpane-pos reads` line. `pos==966` with `hpane-w==972` is the CORRECT clamp law (max position = allocation − 6px separator; a narrow broadway vfb clamps — assert amended to `pos == 1161.min(hw-6)`), while a tiny/collapsed pos under a WIDE hpane would be the real hidden-page recompute — that contingency fix is re-applying `set_position(1161)/592` at switch time in `switch_to` (before `set_visible_child`).

- [ ] **Step 4: CLAUDE.md** — under the Rust-probe-env paragraph, add: `ALPACA_PROBE_S4=<dir>` (multi-workspace chain: 2 opens + switch-survival + cap asserts; wsB/wsC are the passed dir's name-replaced siblings; the rig isolates state.json via XDG_CONFIG_HOME — probe runs must not leak open_projects into the user's session). Under `## Status`, note the S4 stage + the multi-workspace design lines (stack pages, Workspace in app.rs, no close-pill — cap rejects via dialog). Also re-running S2/S3 rigs now requires the same XDG_CONFIG_HOME isolation line in their wrappers (their probes open projects; without isolation a probe run leaves wsA/wsB in the user's strip).

- [ ] **Step 5: Verify + commit** — `cargo test --manifest-path alpaca-code-rs/Cargo.toml` green → `git add -A && git commit -m "test: S4 live probe — workspace switch survival + cap"`

---

## Self-review notes (done at write time)

- Spec coverage: R1 (title text → basename, WM title too — T2 switch_to), R2 (`.alpaca-projbtn` mirrors wintitle idle + branchbtn hover — T2 step 9), R3 (`+` → FileChooser → OpenWorkspace — T3), R4 (append order — T2 open_workspace), R5 (MAX_WORKSPACES=3 + Full dialog + `+` dim — T2/T3), R6 (all pills clickable — T2 buttons), R7 (single title-widget box, 6px spacing, headerbar-centered — T2 init), R8 (StackTransitionType::None — T2 init), isolation (per-workspace Editor/Panes/GitPanel, index-tagged routing — T2).
- Known corner flagged as a question for the user, not silently decided otherwise: no close-project control (not in spec; cap needs one eventually — hover-✕ on pills is the natural follow-up).
- Type consistency: `AddDecision`/`add_decision`/`MAX_WORKSPACES`/`EditKind`/`AppMsg` variants/`Workspace` fields appear with the same names in the consuming tasks; probe tables reference only fields defined above.
- No placeholders: every step carries concrete code or exact replacement instructions anchored on unique strings; the S2/S3 chain rewrites are full bodies.