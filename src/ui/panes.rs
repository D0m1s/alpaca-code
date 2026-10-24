//! Bottom notebook: Agent (raw claude TUI) / Terminal ($SHELL) / Output (run
//! cmd) + the run lifecycle. Decision helpers
//! are pure fns tested headlessly, the widget layer stays thin (relm4 inputs).

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use relm4::gtk::{self, gdk, gio, glib, pango, prelude::*};
use relm4::{Component, ComponentParts, ComponentSender};
use vte4::prelude::*;
use vte4::PtyFlags;

use crate::render::vector;

/// /proc/<pid> existence — zombies keep /proc (life is judged by exit
/// status elsewhere).
pub fn alive(pid: Option<i32>) -> bool {
    match pid {
        Some(p) if p > 0 => Path::new(&format!("/proc/{p}")).exists(),
        _ => false,
    }
}

/// The pane-exit decision table. `budget` is the
/// pane's current respawn counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// set_root's own kill event — swallow
    Swallow,
    /// no root / bookkeeping gone / <3s broken child — no loop
    Ignore,
    /// revive: increment the budget, respawn
    Respawn,
    /// ≥60s-lived child: re-arm (budget→0) and respawn
    RespawnRearm,
    /// budget spent; the tab drops out of `spawned` so returning re-arms
    Exhausted,
}

pub fn respawn_decision(kills_pending: bool, rooted: bool, in_spawned: bool, lived: f64, budget: i32) -> Decision {
    if kills_pending {
        return Decision::Swallow;
    }
    if !rooted || !in_spawned {
        return Decision::Ignore;
    }
    if lived < 3.0 {
        return Decision::Ignore;
    }
    if lived >= 60.0 {
        return Decision::RespawnRearm;
    }
    if budget >= 3 {
        return Decision::Exhausted;
    }
    Decision::Respawn
}

/// The run lifecycle state of Panes as a pure struct.
#[derive(Debug, Default)]
pub struct RunState {
    pub starting: bool,
    pub stop_pending: bool,
    pub pid: Option<i32>,
}

/// What the run's spawn callback landed into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Landed {
    /// pid stored — the caller checks alive() → "Running" / "Process ended (label)"
    Track,
    /// Stop pressed during the starting window: kill this fresh pid, no status
    KillFresh(i32),
    /// error or pid ≤ 0 — "Run failed to start: …", err
    FailedToStart,
}

/// stop_run's transitions; the widget composes "Ready".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    /// the pid was live → kill_tree(pid) (already cleared here)
    KilledAlive(i32),
    /// only the starting window: stop_pending set, kill nothing yet
    Pending,
    /// no run at all
    Nothing,
}

impl RunState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn has_running(&self) -> bool {
        self.starting || alive(self.pid)
    }

    /// launch_run's guard; false = a run is live.
    pub fn launch(&mut self) -> bool {
        if self.has_running() {
            return false;
        }
        self.starting = true;
        self.stop_pending = false;
        true
    }

    /// The spawn callback's on_landed transitions.
    pub fn on_landed(&mut self, pid: i32, error: Option<String>) -> Landed {
        self.starting = false;
        if error.is_some() || pid <= 0 {
            return Landed::FailedToStart;
        }
        self.pid = Some(pid);
        if self.stop_pending {
            self.stop_pending = false;
            self.pid = None;
            return Landed::KillFresh(pid);
        }
        Landed::Track
    }

    /// stop_run; the widget composes "Ready" for every outcome.
    pub fn on_stop(&mut self) -> Stopped {
        match self.pid {
            Some(p) if alive(Some(p)) => {
                self.pid = None;
                self.starting = false;
                Stopped::KilledAlive(p)
            }
            _ if self.starting => {
                self.stop_pending = true;
                Stopped::Pending
            }
            _ => Stopped::Nothing,
        }
    }

    /// on_child_exited: None = cleanup exit — keep status.
    /// Some = a real run exit (the widget re-emits the running mirror). The
    /// decode + "Exit N"/ok/err text died at S5: nothing reads it.
    pub fn on_child_exited(&mut self) -> Option<()> {
        if self.pid.is_none() && !self.starting {
            return None;
        }
        self.pid = None;
        Some(())
    }
}

// ---- kill tree ------------------------------------------------------------------

/// VTE children are session leaders; the pgid kill reaches node/npm
/// grandchildren. SIGHUP first, SIGKILL escalation after
/// 2s via the glib timer (millisecond Duration — CLAUDE.md invariant: never
/// timeout_add_seconds on this box).
pub fn kill_tree(pid: i32) {
    let pgid = unsafe { libc::getpgid(pid) };
    if pgid < 0 {
        return; // ESRCH / EPERM → nothing to escalate
    }
    unsafe {
        let res = libc::killpg(pgid, libc::SIGHUP);
        if res < 0 {
            match std::io::Error::last_os_error().raw_os_error() {
                Some(libc::ESRCH) | Some(libc::EPERM) => return,
                _ => {}
            }
        }
    }
    glib::timeout_add(std::time::Duration::from_millis(2000), move || {
        unsafe {
            let pgid = libc::getpgid(pid);
            if pgid >= 0 {
                libc::killpg(pgid, libc::SIGKILL);
            }
        }
        glib::ControlFlow::Break
    });
}

// ---- palette + spawn ----------------------------------------------------------

/// Mockup (Agentic App – Dark IDE) VTE colors: card-bg terminals, ANSI slots
/// mapped to the same hues as the editor scheme.
pub const VTE_FG: &str = "#e6e8ee";
pub const VTE_BG: &str = "#0d1017";
pub const ANSI: [&str; 16] = [
    "#3a4152", "#ef4444", "#22c55e", "#f2c94c", "#5aa9f8", "#c3a6f7", "#9cdcfe", "#e6e8ee",
    "#5a6375", "#f16a6a", "#4ee08a", "#f6d97e", "#6db5ff", "#cf97f4", "#aee0ff", "#f7f9fc",
];

/// A fresh terminal dressed like the mockup console: mono 13px absolute (px
/// matched to the editor; points would scale differently), height scale lifts
/// VTE's tight default cell, dark palette (unthemed default is light).
fn make_term() -> vte4::Terminal {
    let t = vte4::Terminal::new();
    t.set_scrollback_lines(10_000);
    let mut font = pango::FontDescription::from_string("Noto Sans Mono");
    font.set_absolute_size(13.0 * pango::SCALE as f64);
    t.set_font(Some(&font));
    t.set_cell_height_scale(1.08); // keep the call (binding carries it; brief pins it)
    let fg = gdk::RGBA::parse(VTE_FG).expect("VTE_FG parses");
    let bg = gdk::RGBA::parse(VTE_BG).expect("VTE_BG parses");
    let pal: Vec<gdk::RGBA> = ANSI.iter().filter_map(|h| gdk::RGBA::parse(*h).ok()).collect();
    let pal: Vec<&gdk::RGBA> = pal.iter().collect();
    t.set_colors(Some(&fg), Some(&bg), &pal);
    t
}

/// Pane tab: pill head Box — `alpaca-panetab` carries the 12px side
/// insets (NOT the label); icon valign CENTER (FILL drops it to the row top).
fn pane_tab(title: &str, ipy: Option<gtk::Image>) -> gtk::Box {
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    head.set_css_classes(&["alpaca-panetab"]);
    head.set_valign(gtk::Align::Center);
    if let Some(img) = ipy {
        img.set_valign(gtk::Align::Center);
        head.append(&img);
    }
    let lbl = gtk::Label::new(Some(title));
    lbl.set_css_classes(&["alpaca-panetabname"]);
    head.append(&lbl);
    head
}

/// Per-terminal slot: the widget plus the pid the spawn callback landed.
struct PaneSlot {
    term: vte4::Terminal,
    pid: Cell<Option<i32>>,
}

/// relm4 inputs: spawn/exit/signal events route as messages (closures can't
/// hold `&mut self`).
#[derive(Debug, Clone)]
pub enum PanesMsg {
    SetRoot(PathBuf),
    LaunchRun { argv: Vec<String> },
    StopRun,
    /// a spawn's callback landed; argv0/label ride for
    /// the landing callback's verdict (argv0/label dropped at S5 — they fed
    /// only the never-displayed status texts)
    SpawnLanded { key: &'static str, pid: i32, error: Option<String> },
    /// agent/term child exit → revive logic (the handler
    /// gets the wait status and never reads it — S5 deleted the dead payload).
    PaneExited { key: &'static str },
    /// output pane's child exit → run lifecycle (status dropped at S5:
    /// it fed only the never-displayed "Exit N" text)
    RunExited,
    /// Notebook current-page readback — the
    /// current-page read is stale mid-emission on this build
    EnsureCurrent(Option<i32>),
}

#[derive(Debug, Clone)]
pub enum PanesOutput {
    /// launch_run accepted (the starting window) — App flips Run/Stop
    RunStarting,
    /// The status + the running mirror App can't query synchronously.
    /// text/cls dropped at S5: the consumer receives and
    /// discards both — the running flag is the only live payload.
    RunStatus { running: bool },
}

pub struct Panes {
    nb: gtk::Notebook,
    agent: PaneSlot,
    term: PaneSlot,
    out: PaneSlot,
    root: Option<PathBuf>,
    spawned: HashSet<&'static str>,
    spawned_at: HashMap<&'static str, Instant>,
    kills_pending: HashMap<&'static str, i32>,
    respawns: HashMap<&'static str, i32>,
    run: RunState,
}

#[relm4::component(pub)]
impl Component for Panes {
    type CommandOutput = ();
    type Input = PanesMsg;
    type Output = PanesOutput;
    type Init = ();

    view! {
        #[root]
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_spacing: 0,
            set_css_classes: &["alpaca-card"],
            set_overflow: gtk::Overflow::Hidden,
        }
    }

    fn init(_: (), root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let agent = PaneSlot { term: make_term(), pid: Cell::new(None) };
        let term = PaneSlot { term: make_term(), pid: Cell::new(None) };
        let out = PaneSlot { term: make_term(), pid: Cell::new(None) };

        // child-exited routing: output → run lifecycle; agent/term → revive
        // close law: input() PANICS on a dropped runtime — the closing
        // workspace's widget tree destroys these terminals and the notebook's
        // dispose can still fire switch-page into the dead runtime. send()
        // reports instead; a dead send is a no-op (the listener dies with
        // the widget it hangs on).
        let out_send = sender.clone();
        out.term.connect_child_exited(move |_t, _status| {
            if out_send.input_sender().send(PanesMsg::RunExited).is_err() {
                return;
            }
        });
        let agent_send = sender.clone();
        agent.term.connect_child_exited(move |_t, _status| {
            if agent_send.input_sender().send(PanesMsg::PaneExited { key: "agent" }).is_err() {
                return;
            }
        });
        let term_send = sender.clone();
        term.term.connect_child_exited(move |_t, _status| {
            if term_send.input_sender().send(PanesMsg::PaneExited { key: "term" }).is_err() {
                return;
            }
        });

        // pane notebook: scrollable, load-bearing page order 0=agent 1=term 2=out
        let nb = gtk::Notebook::builder().vexpand(true).scrollable(true).build();
        nb.add_css_class("alpaca-panes");
        let pages: [(&vte4::Terminal, &str, &str); 3] = [
            (&agent.term, "Agent", "bot.svg"),
            (&term.term, "Terminal", "prompt.svg"),
            (&out.term, "Output", "terminal.svg"),
        ];
        for (t, title, icon) in pages {
            let sw = gtk::ScrolledWindow::new();
            sw.set_child(Some(t));
            nb.append_page(
                &sw,
                Some(&pane_tab(
                    title,
                    vector::icon(icon).map(|vec| gtk::Image::from_paintable(Some(&vec))),
                )),
            );
        }
        let switch_send = sender.clone();
        nb.connect_switch_page(move |_, _, index| {
            // close law: dispose reselects a page → input() would panic
            if switch_send
                .input_sender()
                .send(PanesMsg::EnsureCurrent(Some(index as i32)))
                .is_err()
            {
                return;
            }
        });
        root.append(&nb);

        let model = Panes {
            nb,
            agent,
            term,
            out,
            root: None,
            spawned: HashSet::new(),
            spawned_at: HashMap::new(),
            kills_pending: HashMap::new(),
            respawns: HashMap::new(),
            run: RunState::new(),
        };

        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            PanesMsg::SetRoot(p) => self.set_root(p, sender),
            PanesMsg::LaunchRun { argv } => self.launch_run(argv, sender),
            PanesMsg::StopRun => self.stop_run(sender),
            PanesMsg::SpawnLanded { key, pid, error } => {
                self.on_spawn_landed(key, pid, error, sender)
            }
            PanesMsg::PaneExited { key } => self.on_pane_exited(key, sender),
            PanesMsg::RunExited => self.on_run_exited(sender),
            PanesMsg::EnsureCurrent(ix) => self.ensure_current(ix, sender),
        }
    }
}

impl Panes {
    fn has_running_run(&self) -> bool {
        self.run.has_running()
    }

    /// The status + the running mirror (App re-enables buttons off it).
    fn status(&self, sender: ComponentSender<Self>) {
        let _ = sender.output(PanesOutput::RunStatus { running: self.has_running_run() });
    }

    /// spawn argv on a terminal's pty; the callback lands the
    /// pid back through the component sender. Flags must be
    /// SEARCH_PATH|SEARCH_PATH_FROM_ENVP — DEFAULT is plain execv and a bare
    /// "claude" dies ENOENT (CLAUDE.md invariant).
    fn spawn(
        send: ComponentSender<Panes>,
        key: &'static str,
        term: &vte4::Terminal,
        cwd: Option<&str>,
        argv: &[String],
    ) {
        let envv = crate::support::runctl::pane_environ();
        let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
        let envv: Vec<&str> = envv.iter().map(String::as_str).collect();
        // every spawn failure prints keyed by argv[0]
        let argv0 = argv[0].to_owned();
        let cb_send = send.clone();
        term.spawn_async(
            PtyFlags::DEFAULT,
            cwd,
            &argv,
            &envv,
            glib::SpawnFlags::SEARCH_PATH | glib::SpawnFlags::SEARCH_PATH_FROM_ENVP,
            || {},
            -1,
            None::<&gio::Cancellable>,
            move |res| {
                let (pid, error) = match res {
                    Ok(pid) => (pid.0, None),
                    Err(e) => (0, Some(format!("{e}"))),
                };
                if let Some(e) = &error {
                    eprintln!("spawn failed ({argv0}): {e}");
                }
                // close law: input() panics on a dropped runtime — a spawn in
                // flight at a close lands with the terminal already dying
                let _ = cb_send.input_sender().send(PanesMsg::SpawnLanded { key, pid, error });
            },
        );
    }

    // ---- workspace -----------------------------------------------------------

    /// Live pane pids for the workspace close (App kills them SYNCHRONOUSLY
    /// before dropping the component — a queued kill message races relm4's
    /// shutdown branch in `futures::select!` and can be dropped with it),
    /// run pid first (session leaders — the pgid kill reaches the tree).
    pub fn live_pids(&self) -> Vec<i32> {
        let mut pids = Vec::new();
        if let Some(pid) = self.run.pid {
            pids.push(pid);
        }
        for slot in [&self.agent, &self.term] {
            if let Some(pid) = slot.pid.get() {
                pids.push(pid);
            }
        }
        pids
    }

    /// set_root: kill live children, clear all bookkeeping, reset all
    /// three terminals, status "Ready", idle ensure_current.
    fn set_root(&mut self, path: PathBuf, sender: ComponentSender<Self>) {
        if let Some(pid) = self.run.pid
            && alive(Some(pid)) {
                // the out cleanup exit is ignored by on_child_exited itself
                kill_tree(pid);
            }
        self.run.pid = None;
        self.kills_pending.clear();
        for (slot, key) in [(&self.agent, "agent"), (&self.term, "term")] {
            if let Some(pid) = slot.pid.get()
                && alive(Some(pid)) {
                    kill_tree(pid);
                    self.kills_pending.insert(key, 1); // that exit event is ours — swallow it
                }
        }
        self.spawned.clear();
        self.spawned_at.clear();
        self.respawns.clear();
        self.run = RunState::new();
        for slot in [&self.agent, &self.term, &self.out] {
            slot.term.reset(true, true);
        }
        self.root = Some(path);
        self.status(sender.clone());
        let idle_send = sender;
        glib::idle_add_local(move || {
            // close law: input() panics on a dropped runtime — the idle can
            // outlive a close that lands between turns
            let _ = idle_send.input_sender().send(PanesMsg::EnsureCurrent(None));
            glib::ControlFlow::Break
        });
    }

    /// ensure_current: spawn the visible tab's child once.
    fn ensure_current(&mut self, page_ix: Option<i32>, sender: ComponentSender<Self>) {
        if self.root.is_none() {
            return;
        }
        // page_ix or current-page readback — GTK reads the OLD index
        // mid-emission, so a switch-page signal passes its own index
        let i = page_ix.or(self.nb.current_page().map(|p| p as i32)).unwrap_or(-1);
        // re-entry re-arms the exit budget (deliberate only)
        if i == 0 && !self.spawned.contains("agent") {
            self.respawns.remove("agent");
            self.spawn_pane("agent", true, sender);
        } else if i == 1 && !self.spawned.contains("term") {
            self.respawns.remove("term");
            self.spawn_pane("term", false, sender);
        }
    }

    /// the ensure-current body once gated (spawned bookkeeping + argv pick).
    fn spawn_pane(&mut self, key: &'static str, is_agent: bool, sender: ComponentSender<Self>) {
        self.spawned.insert(key);
        // respawns.pop lives in ensure_current (deliberate re-entry only);
        // clearing here would reset the exit budget each respawn — measured live:
        // 5 mid-band deaths never exhausted (claude resurrection forever)
        self.spawned_at.insert(key, Instant::now());
        let argv: Vec<String> = if is_agent {
            vec!["claude".to_owned()]
        } else {
            vec![std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_owned())]
        };
        let term = if is_agent { &self.agent.term } else { &self.term.term };
        let cwd = self.root.clone().map(|p| p.to_string_lossy().into_owned());
        Self::spawn(sender, key, term, cwd.as_deref(), &argv);
    }

    // ---- run lifecycle ---------------------------------------------------------

    /// launch_run; input only proceeds (RunStarting) when accepted.
    fn launch_run(&mut self, argv: Vec<String>, sender: ComponentSender<Self>) {
        if !self.run.launch() {
            return;
        }
        self.nb.set_current_page(Some(2u32)); // Output is page 2
        self.out.term.reset(true, true);
        let cwd = self.root.clone().map(|p| p.to_string_lossy().into_owned());
        Self::spawn(sender.clone(), "output", &self.out.term, cwd.as_deref(), &argv);
        let _ = sender.output(PanesOutput::RunStarting);
    }

    /// spawn callback landed (the run lifecycle for Output; a plain pid store for the rest).
    fn on_spawn_landed(
        &mut self,
        key: &'static str,
        pid: i32,
        error: Option<String>,
        sender: ComponentSender<Self>,
    ) {
        if key == "output" {
            match self.run.on_landed(pid, error.clone()) {
                Landed::FailedToStart => self.status(sender),
                Landed::KillFresh(p) => kill_tree(p),
                Landed::Track => {
                    if alive(Some(pid)) {
                        self.status(sender);
                    } else {
                        self.run.pid = None; // died instantly (e.g. script missing)
                        self.status(sender);
                    }
                }
            }
            return;
        }
        let slot = if key == "agent" { &self.agent } else { &self.term };
        if pid > 0 {
            slot.pid.set(Some(pid));
        } else {
            slot.pid.set(None);
            // spawn failed (missing binary): drop the once-gate so re-entering
            // the tab retries — re-entry-triggered only, no respawn loop.
            self.spawned.remove(key);
        }
    }

    /// stop_run: kill+clear the live run else arm stop-pending; "Ready".
    fn stop_run(&mut self, sender: ComponentSender<Self>) {
        if let Stopped::KilledAlive(p) = self.run.on_stop() {
            kill_tree(p);
        }
        self.status(sender);
    }

    /// the output pane's child exit: a run's real exit
    /// re-emits the status (buttons/pill flip); a stop/set_root cleanup exit
    /// is ignored — the pill already reads its own mirror.
    fn on_run_exited(&mut self, sender: ComponentSender<Self>) {
        if self.run.on_child_exited().is_some() {
            self.status(sender);
        }
    }

    /// pane child exit: revive logic with the capped budget.
    fn on_pane_exited(&mut self, key: &'static str, sender: ComponentSender<Self>) {
        let kills = *self.kills_pending.get(key).unwrap_or(&0);
        let rooted = self.root.is_some();
        let in_spawned = self.spawned.contains(key);
        // a missing stamp reads as an ancient spawn (epoch default);
        // unreachable in practice — every spawned.insert accompanies a stamp
        let lived = self
            .spawned_at
            .get(key)
            .map_or(f64::INFINITY, |t| t.elapsed().as_secs_f64());
        let budget = *self.respawns.get(key).unwrap_or(&0);
        // (status is unused for pane children)
        match respawn_decision(kills > 0, rooted, in_spawned, lived, budget) {
            Decision::Swallow => *self.kills_pending.get_mut(key).unwrap() -= 1,
            Decision::Ignore => {}
            Decision::Respawn => {
                *self.respawns.entry(key).or_insert(0) += 1;
                self.spawn_pane(key, key == "agent", sender);
            }
            Decision::RespawnRearm => {
                self.respawns.insert(key, 0);
                self.spawn_pane(key, key == "agent", sender);
            }
            Decision::Exhausted => {
                self.spawned.remove(key); // returning to the tab offers a fresh attempt
                self.status(sender);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alive_rules() {
        assert!(!alive(None), "None pid is never alive");
        assert!(!alive(Some(999_999_999)), "unborn pid is not alive"); // /proc absent
        // A real pid: our own process — proves /proc lookups work, nothing else.
        let me: i32 = std::process::id() as i32;
        assert!(alive(Some(me)));
    }

    #[test]
    fn respawns_policy() {
        // The pane-exit decision table. The brief's
        // literal arg list repeats equal prefixes with contradictory outcomes;
        // the respawn budget is a 5th arg.
        assert_eq!(respawn_decision(true, true, true, 100.0, 0), Decision::Swallow);
        assert_eq!(respawn_decision(false, false, true, 100.0, 0), Decision::Ignore);
        assert_eq!(respawn_decision(false, true, false, 100.0, 0), Decision::Ignore);
        assert_eq!(respawn_decision(false, true, true, 2.0, 0), Decision::Ignore); // <3s broken child
        assert_eq!(respawn_decision(false, true, true, 10.0, 0), Decision::Respawn);
        assert_eq!(respawn_decision(false, true, true, 10.0, 2), Decision::Respawn);
        assert_eq!(respawn_decision(false, true, true, 10.0, 3), Decision::Exhausted); // budget spent
        // ≥60s-lived child re-arms (budget read resets) and respawns at ANY budget
        assert_eq!(respawn_decision(false, true, true, 3_600.0, 1), Decision::RespawnRearm);
        assert_eq!(respawn_decision(false, true, true, 3_600.0, 3), Decision::RespawnRearm);
    }

    #[test]
    fn run_lifecycle_transitions() {
        // launch_run guard: the starting window blocks double launches
        let mut st = RunState::new();
        assert!(st.launch());
        assert!(!st.launch());
        assert!(st.has_running());

        // landed ok → tracked → alive; clean exit reads Some (not cleanup); relaunch allowed
        assert!(matches!(st.on_landed(4242, None), Landed::Track));
        assert!(st.on_child_exited().is_some());
        assert!(!st.has_running());
        assert!(st.launch(), "relaunch allowed after exit");

        // spawn error → failed-to-start
        let mut st = RunState::new();
        st.launch();
        assert!(matches!(st.on_landed(0, Some("boom".into())), Landed::FailedToStart));

        // Stop before the pid lands → pending → the fresh pid is killed on land
        let mut st = RunState::new();
        st.launch();
        assert!(matches!(st.on_stop(), Stopped::Pending));
        assert!(matches!(st.on_landed(77, None), Landed::KillFresh(77)));
        assert!(!st.has_running());

        // Stop with a live pid → kill it; state clears; the cleanup exit is ignored
        let mut st = RunState::new();
        st.launch();
        assert!(matches!(st.on_landed(77, None), Landed::Track));
        assert!(matches!(st.on_stop(), Stopped::KilledAlive(77)));
        assert!(!st.has_running());
        assert_eq!(st.on_child_exited(), None);

        // no run at all: stop is a state no-op (the widget still sets "Ready")
        assert!(matches!(st.on_stop(), Stopped::Nothing));
    }
}