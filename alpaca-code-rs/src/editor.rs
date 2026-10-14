//! Editor component — file tabs: open/close/save/restore/watches/tombstone/
//! binary/dirty dots/badges (file-page S1; diff pages + `_TabDistributor` is
//! S2/S3).
//!
//! Rust keeps a parallel page model (`PageState`, index-aligned with the
//! notebook) instead of dangling attributes on the page GtkWidget — the
//! wrapper-identity trap CLAUDE.md warns about is avoided by construction, and
//! the dirty dot / tabname labels are held directly instead of re-found by
//! walking `get_first_child()`.

// 4.10-deprecated dialog API (the VTE open-with dialog) — S3 polish modernizes.
#![allow(deprecated)]

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use relm4::gtk::{self, gio, glib, pango, prelude::*};
use relm4::gtk::subclass::prelude::*;
use relm4::{Component, ComponentParts, ComponentSender};
use sourceview5::prelude::*; // BufferExt: set_style_scheme/set_language

use crate::vector;
use crate::badges;
use crate::gitstatus::{self, Sides};

pub const GONE_NOTE: &str = "‹file has been deleted›";
pub const BINARY_NOTE: &str = "‹file changed on disk — not UTF-8 text›";

/// chrome subtracted before sharing a tab's width: badge slot + ✕ + label/
/// head paddings (measured @13px; 62 is the starting hypothesis —
/// re-measure live if `max(wide) > share` ever trips)
const TAB_CHROME: i32 = 62;

/// walk a widget tree hunting a descendant carrying the tab-name class
/// (used by both the distributor's tab walk and ProbeTrace)
fn find_tabname(w: &gtk4::Widget) -> Option<gtk4::Widget> {
    if w.has_css_class("alpaca-tabname") {
        return Some(w.clone());
    }
    let mut kids = Vec::new();
    let mut c = w.first_child();
    while let Some(ch) = c {
        kids.push(ch.clone());
        c = ch.next_sibling();
    }
    for k in &kids {
        if let Some(f) = find_tabname(k) {
            return Some(f);
        }
    }
    None
}

/// `_TabDistributor`: the layout manager of the
/// vertical wrapper Box around the notebook strip. GTK 4.22 pins scrollable
/// notebook tabs at their minimum width even with room to spare, and scroll
/// mode's visible tail grows WIDER as the window narrows — this layout
/// manager instead floors every tab-name label at
/// min(share, natural): req = (wrap_width − end_action_w) // n − CHROME.
/// Recomputes only on (width, n) change.
#[derive(Default)]
pub struct TabDistributorImp {
    last: Cell<Option<(i32, i32)>>, // (wrap width, nb page count) — last-seen memo
}

glib::wrapper! {
    pub struct TabDistributor(ObjectSubclass<TabDistributorImp>) @extends gtk::BoxLayout, gtk::LayoutManager;
}

/// TabDistributor's C instance struct. glib's basic InstanceStruct embeds
/// `<TabDistributor as ObjectType>::GlibType` = ffi::GtkBoxLayout, which gir
/// emitted OPAQUE (0 bytes — GtkBoxLayout exposes no public fields), so
/// registration computed instance size 0 and GObject rejected the type
/// ("instance size smaller than GTypeInstance size"). Pad the C layout to the
/// real GtkBoxLayout instance footprint (GObject + LayoutManager + BoxLayout
/// privates — ~84B on this build, well under 128): the imp's PrivateStruct is
/// appended AFTER the instance struct by g_type_add_instance_private.
#[repr(C)]
pub struct TabDistributorInstance {
    parent: gtk::ffi::GtkBoxLayout,
    _size: [u8; 128],
}

unsafe impl glib::subclass::types::InstanceStruct for TabDistributorInstance {
    type Type = TabDistributorImp;
}

// gtk4 0.11.5 registers IsSubclassable for LayoutManager but generates none
// for BoxLayout (it has no overridable vfuncs, so gir skipped the link) —
// the subclass chain between ParentType=BoxLayout and LayoutManager needs it.
// Concrete T (not generic): the orphan rule only allows implementing the foreign
// trait for a foreign type when a LOCAL type appears in the impl.
unsafe impl IsSubclassable<TabDistributorImp> for gtk::BoxLayout {}

#[glib::object_subclass]
impl ObjectSubclass for TabDistributorImp {
    const NAME: &'static str = "AlpacaTabDistributor";
    type Type = TabDistributor;
    type ParentType = gtk::BoxLayout;
    type Instance = TabDistributorInstance;
}

impl ObjectImpl for TabDistributorImp {}

impl LayoutManagerImpl for TabDistributorImp {
    fn allocate(&self, widget: &gtk::Widget, width: i32, height: i32, baseline: i32) {
        self.parent_allocate(widget, width, height, baseline);
        let Some(nb) = widget.first_child().and_downcast::<gtk::Notebook>() else { return };
        let n = nb.n_pages();
        if n == 0 || self.last.get() == Some((width, n as i32)) {
            return;
        }
        // act_w = end-action widget's allocated width; never set in
        // this app → no widget → 0 (kept None-safe)
        let act = nb
            .action_widget(gtk::PackType::End)
            .map_or(0, |w| w.allocated_width());
        let req = i32::max((width.saturating_sub(act)) / n as i32 - TAB_CHROME, 0);
        for i in 0..n {
            let Some(pg) = nb.nth_page(Some(i)) else { continue };
            let Some(label) = nb.tab_label(&pg) else { continue };
            let Some(name) = find_tabname(&label) else { continue };
            let nat = name.measure(gtk::Orientation::Horizontal, -1).1;
            name.set_size_request(i32::min(req, nat), -1);
        }
        self.last.set(Some((width, n as i32)));
    }
}

/// decode UTF-8; reject NUL bytes (a lossy buffer
/// plus Ctrl+S would rewrite the file with truncated/replacement garbage).
pub fn readable_text(raw: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(raw).ok()?;
    if text.contains('\0') {
        return None;
    }
    Some(text.to_owned())
}

/// os.stat().st_mtime_ns via std (negative for pre-epoch mtimes).
fn mtime_ns(path: &Path) -> Option<i64> {
    let t = std::fs::metadata(path).ok()?.modified().ok()?;
    match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => Some(d.as_nanos() as i64),
        Err(e) => Some(-(e.duration().as_nanos() as i64)),
    }
}

// os.path.basename — the last component (empty for paths ending in /).
pub fn basename(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// os.path.relpath(path, root). Tabs always live under the open root (opened
/// from the tree, restored by joining), so this covers only the under-root
/// shape; an outside-root path keeps its absolute spelling.
fn relpath(path: &Path, root: &Path) -> String {
    let pa: Vec<_> = path.components().collect();
    let rb: Vec<_> = root.components().collect();
    if pa.len() >= rb.len() && pa[..rb.len()] == rb[..] {
        pa[rb.len()..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")
    } else {
        path.to_string_lossy().into_owned()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FileState {
    Live,
    /// file gone on disk: clean buffer → tombstone note, dirty keeps user text
    Deleted,
    /// disk content unreadable: view frozen (edit-off), note placed
    Binary,
}

#[derive(Debug, Clone, Default)]
pub struct EditorSnapshot {
    pub open_tabs: Vec<String>,
    pub active_tab: i64,
    pub has_dirty: bool,
}

#[derive(Debug)]
pub enum EditorMsg {
    Restore(PathBuf, Vec<String>, i64),   // set_root + open still-existing tabs (the only set path)
    OpenFile(PathBuf),
    SaveActive,
    /// PROBE-ONLY (ALPACA_PROBE_S3) — tab allocation trace (taballoc line);
    /// production never sends it (no S3 chain)
    ProbeTrace(String),
    /// commit flow flushes dirty buffers among the named abs paths
    SaveOpen(Vec<PathBuf>),
    /// CHANGES row click → side-by-side diff page
    OpenDiff {
        root: PathBuf,
        rel: String,
        letter: String,
    },
    /// fresh git data → refetch sides for every open diff page still present
    /// in the change set; rel→fresh-letter rows
    RefreshDiffs {
        root: PathBuf,
        letters: Vec<(String, String)>,
    },
    /// repaint every dirty dot + emit the snapshot (modified-changed, open…)
    StateTick,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    /// probe-only: exercise the Edit-menu op target on the active diff page
    /// and report which side it lands on (the Edit-menu focus route);
    /// production never sends it
    ProbeClip,
    /// PROBE-ONLY (ALPACA_PROBE_S2) — diff-page scrollbar-policy + dual-axis
    /// scroll-pairing asserts; production never sends it (no chain)
    ProbeDiffScroll,
    // internal
    /// post-emission re-stamp: fires after switch-page lands, when current_page()
    /// is already the new index (the staleness window is inside the emission only)
    Switched,
    /// a file monitor fired
    FsEvent(PathBuf),
    /// the 120ms settle elapsed
    FsSettled(PathBuf),
    ClosePage(usize, bool),
}

#[derive(Debug)]
pub enum EditorOutput {
    StateChanged(EditorSnapshot),
    /// ack of the SaveOpen flush — ALWAYS fires, even with zero flushed pages
    /// (drives T7's pending-commit chain; a missing ack would strand the
    /// eager-busy buttons)
    Flushed,
}

#[derive(Debug)]
pub enum EditorCommand {
    DiffData {
        rel: String,
        letter: String,
        text: String,
        binary: bool,
    },
    FreshDiffs(Vec<(String, String, Sides, String)>),
}

const DIFF_DEL_BG: &str = "#25181c";
const DIFF_ADD_BG: &str = "#15261d";
const DIFF_HDR_FG: &str = "#5a6375";

/// per-direction, per-axis scroll-pairing dedupe: the adjustment each axis
/// last linked to. Belt-only now — hooking is single-shot at open (the
/// pairing-law comment at the machinery); the identity guard just makes a
/// stray second diff_link on the same pair a no-op.
#[derive(Default)]
struct LinkedAxes {
    v: Option<gtk::Adjustment>,
    h: Option<gtk::Adjustment>,
}

/// both diff sides' scroll pairing state: the widgets the live renewal
/// refills, plus each view's linked-adjustment dedupe (one per direction).
/// `v_r` stays so the refill can retarget the right side's buffer in place
/// (the measured churn law says keep the widget tree: swapping the views
/// tears the page down and the visible page jumps — see refresh_diff).
/// `diff_text` is the raw text the sides came
/// from: the renewal flight dedupes against it (a byte-identical diff must
/// not tear the page down every tick).
struct DiffWidgets {
    sw_l: gtk::ScrolledWindow,
    sw_r: gtk::ScrolledWindow,
    v_r: sourceview5::View,
    diff_text: Option<String>,
    linked_l: Rc<RefCell<LinkedAxes>>,
    linked_r: Rc<RefCell<LinkedAxes>>,
    /// the resizable b/a divider (the paned widget is retained across
    /// renewals — only buffers swap)
    paned: gtk::Paned,
    /// the single full-width h bar (Task 3) — bound to sw_r's hadj
    bar: gtk::Scrollbar,
    /// number panes (Task 4): one per side, buffers = `diff_num_text` of the
    /// sides' number rows; each view SHARES its main's vadjustment object
    /// (lockstep without push races — see probe asserts)
    num_l: sourceview5::View,
    num_r: sourceview5::View,
    /// the panes' own sws: refresh re-stamps their width (a renewed side
    /// can gain a number digit, 99→100 — the fixed sw would clip mid-glyph)
    sw_num_l: gtk::ScrolledWindow,
    sw_num_r: gtk::ScrolledWindow,
}

struct PageState {
    path: Option<PathBuf>,
    buf: sourceview5::Buffer,
    sw: gtk::ScrolledWindow,
    view: sourceview5::View,
    page: gtk::Box,
    dot: gtk::Label,
    tabname: gtk::Label,
    badge_slot: gtk::Box,
    fs: FileState,
    conflict: bool,
    load_mtime: Option<i64>, // st_mtime_ns
    // diff page fields (S2) — path stays None so save/reload/fs paths guard out
    diff_of: Option<String>,   // "diff:<rel>" key
    diff_letter: String,
    diff: Option<DiffWidgets>,
}

pub struct Editor {
    nb: gtk::Notebook,
    pages: Vec<PageState>,
    /// workspace root for relpaths (crumb + snapshot); None until SetRoot
    root: Option<PathBuf>,
    lm: sourceview5::LanguageManager,
    scheme: Option<sourceview5::StyleScheme>,
    watchers: HashMap<PathBuf, gio::FileMonitor>,
}

#[relm4::component(pub)]
impl Component for Editor {
    type CommandOutput = EditorCommand;
    type Input = EditorMsg;
    type Output = EditorOutput;
    type Init = ();

    view! {
        #[root]
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_css_classes: &["alpaca-card"],
        }
    }

    fn init(_: (), root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let nb = gtk::Notebook::builder()
            .vexpand(true)
            .hexpand(true)
            .scrollable(true)
            .build();

        let this = Editor {
            nb: nb.clone(),
            pages: Vec::new(),
            root: None,
            lm: sourceview5::LanguageManager::default(),
            scheme: sourceview5::StyleSchemeManager::default().scheme("alpaca-dark"),
            watchers: HashMap::new(),
        };

        let snd = sender.clone();
        nb.connect_switch_page(move |_, _, _| {
            snd.input(EditorMsg::Switched);
        });

        // Ctrl+S saves the active page
        let svc = gtk::ShortcutController::new();
        let snd = sender.clone();
        let trig = gtk::ShortcutTrigger::parse_string("<Control>s");
        let act = gtk::CallbackAction::new(move |_w, _a| {
            snd.input(EditorMsg::SaveActive);
            glib::Propagation::Proceed // propagate
        });
        let sc = gtk::Shortcut::new(trig, Some(act));
        svc.add_shortcut(sc);
        root.add_controller(svc);

        let dist: TabDistributor = glib::Object::builder::<TabDistributor>().build();
        let tabs_wrap = gtk::Box::builder().orientation(gtk::Orientation::Vertical).build();
        // the wrap must take the full row width — a
        // HORIZONTAL Box gives a non-hexpand child only its own minimum (measured
        // 900px wrap → 280px nb); VERTICAL makes x the cross axis where children FILL
        tabs_wrap.set_layout_manager(Some(dist)); // widget refs the object; local keeps the borrow
        tabs_wrap.append(&nb);
        root.append(&tabs_wrap);

        let widgets = view_output!();
        ComponentParts { model: this, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            EditorMsg::Restore(root, tabs, active) => {
                self.restore(&root, tabs, active, &sender);
            }
            EditorMsg::OpenFile(path) => {
                self.open_file(&path, &sender);
            }
            EditorMsg::SaveActive => {
                self.save_active(&sender);
            }
            EditorMsg::ProbeTrace(tag) => {
                let (mut ws, mut ms) = (Vec::new(), Vec::new());
                let strip = self.nb.allocated_width();
                let act = self.nb.action_widget(gtk::PackType::End).map_or(0, |w| w.allocated_width());
                let n = self.nb.n_pages().max(1) as i32;
                let share = i32::max((strip.saturating_sub(act)) / n - TAB_CHROME, 0);
                for i in 0..self.nb.n_pages() {
                    let Some(pg) = self.nb.nth_page(Some(i)) else { continue };
                    let Some(label) = self.nb.tab_label(&pg) else { continue };
                    let Some(name) = find_tabname(&label) else { continue };
                    let (min, _nat, _mb, _nb) = name.measure(gtk::Orientation::Horizontal, -1);
                    ws.push(format!("{}", name.allocated_width()));
                    ms.push(format!("{min}"));
                }
                println!("PROBE taballoc {tag}: strip={strip} act={act} share={share} \
                          wrap={} widths=[{}] mins=[{}]",
                    self.nb.parent().map(|p| p.allocated_width()).unwrap_or(-1),
                    ws.join(","), ms.join(","));
            }
            EditorMsg::SaveOpen(paths) => {
                // commit flow flush: dirty buffers among the named abs paths go
                // to disk BEFORE the commit runs. Write errors are swallowed
                // and Flushed ALWAYS fires — rust must not strand T7's
                // pending-commit chain on an I/O error (eager-busy gate).
                for idx in 0..self.pages.len() {
                    let flush = self.pages[idx]
                        .path
                        .as_ref()
                        .is_some_and(|p| paths.contains(p))
                        && self.pages[idx].buf.is_modified();
                    if flush {
                        let _ = self.write_page(idx);
                    }
                }
                let _ = sender.output(EditorOutput::Flushed);
            }
            EditorMsg::OpenDiff { root, rel, letter } => {
                // root snapshot rides into the flight (App passes its live
                // root; no guard beyond the snapshot)
                sender.spawn_command(move |out| {
                    let (text, binary) = gitstatus::diff_for(
                        &root.to_string_lossy(),
                        &rel,
                        letter == "U",
                    )
                    // git failed/timeout → "" → the same no-diff gate
                    .unwrap_or_default();
                    out.emit(EditorCommand::DiffData { rel, letter, text, binary });
                });
            }
            EditorMsg::RefreshDiffs { root, letters } => {
                // per OPEN diff page still present in the change set; resolved
                // tabs stay as-is. The fetch flight is a responsiveness
                // improvement only, identical behavior when it lands.
                //
                // The flight dedupes byte-identical renewals (each page's
                // shown text + letter captured here, fresh ones compared
                // in-thread): pre-fix rust renewed on EVERY probe
                // tick and tore the visible page down with it (measured: 6
                // full rebuilds of one unchanged diff in 10s, both sides'
                // scroll adj flapping 320→0→320 — the "diff jumps around"
                // glitch). Same guard class as gitview's identical-porcelain
                // Apply skip. A stale page (diff_text None) reads as text ""
                // and letter "" → never deduped, always refreshed.
                let map: HashMap<String, String> = letters.into_iter().collect();
                let mut pairs: Vec<(String, String, String, String)> = Vec::new();
                for pg in &self.pages {
                    let Some(key) = pg.diff_of.as_deref() else { continue };
                    let Some(rel) = key.strip_prefix("diff:") else { continue };
                    if let Some(fresh) = map.get(rel) {
                        let (old_text, old_letter) = match pg.diff.as_ref() {
                            Some(d) => (d.diff_text.clone().unwrap_or_default(), pg.diff_letter.clone()),
                            None => (String::new(), String::new()),
                        };
                        pairs.push((rel.to_string(), fresh.clone(), old_text, old_letter));
                    }
                }
                sender.spawn_command(move |out| {
                    let mut fresh: Vec<(String, String, Sides, String)> = Vec::new();
                    for (rel, fresh_letter, old_text, old_letter) in pairs {
                        let (text, binary) = gitstatus::diff_for(
                            &root.to_string_lossy(),
                            &rel,
                            fresh_letter == "U",
                        )
                        .unwrap_or_default();
                        if binary {
                            continue; // the tab stays stale rather than die
                        }
                        if text == old_text && fresh_letter == old_letter {
                            continue; // already on screen — no teardown for it
                        }
                        let parsed = gitstatus::build_sides(&gitstatus::parse_unified(&text));
                        fresh.push((rel, text, parsed, fresh_letter));
                    }
                    out.emit(EditorCommand::FreshDiffs(fresh));
                });
            }
            EditorMsg::StateTick => {
                self.repaint_dots();
                let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
            }
            EditorMsg::Undo => {
                if let Some(idx) = self.active() {
                    let pg = &self.pages[idx];
                    if pg.buf.can_undo() {
                        pg.buf.undo();
                    }
                }
            }
            EditorMsg::Redo => {
                if let Some(idx) = self.active() {
                    let pg = &self.pages[idx];
                    if pg.buf.can_redo() {
                        pg.buf.redo();
                    }
                }
            }
            // clipboard ops ride the view's own actions
            EditorMsg::Cut => self.clip_op("cut-clipboard"),
            EditorMsg::Copy => self.clip_op("copy-clipboard"),
            EditorMsg::Paste => self.clip_op("paste-clipboard"),
            EditorMsg::SelectAll => {
                if let Some(idx) = self.active()
                    && let Some(w) = self.op_target(idx) {
                        let _ = w.activate_action("select-all", Some(&true.to_variant()));
                    }
            }
            EditorMsg::Switched => {
                self.refresh_restamps(&sender);
                let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
            }
            // probe-only fix-3 instrument: resolve the SAME widget the
            // production Edit-menu route targets on a diff page, and report
            // whether it is the focused (right) side view
            EditorMsg::ProbeClip => {
                let Some(idx) = self.active() else {
                    println!("PROBE clip-target: none");
                    return;
                };
                let right = self.pages[idx]
                    .diff
                    .as_ref()
                    .and_then(|d| d.sw_r.child())
                    .and_then(|w| w.downcast_ref::<sourceview5::View>().cloned());
                if let Some(ref rv) = right {
                    rv.grab_focus(); // the user's pointer/focus is on this side
                }
                // the production op route (op_target after fix-3): must land
                // on the focused right view now
                let hit = match &right {
                    Some(rv) => self
                        .op_target(idx)
                        .and_then(|w| w.downcast_ref::<sourceview5::View>().cloned())
                        .as_ref()
                        == Some(rv),
                    None => false,
                };
                println!(
                    "PROBE clip-target: {}",
                    if hit { "right-view" } else { "left-or-wrong" }
                );
            }
            // PROBE-ONLY diff scroll/policy asserts — NOT ProbeClip's job: the
            // step-4 (+400ms post-open) timing is racy on a live display
            // (measured 2026-10-13: vertical span still 0 and h values
            // unsettled on some runs with the SAME binary — the pairing is
            // not, the measurement is). The chain's PRE-SWITCH window runs
            // 5-11 (step 12's SetWorkspace closes every page), so the assert
            // rides step 9 — ~2s post-open, both sides' allocations and the
            // pairing long settled, still wsA.
            EditorMsg::ProbeDiffScroll => {
                let Some(idx) = self.active() else {
                    println!("PROBE assert: diff-presence: FAIL (no active page)");
                    return;
                };
                if let Some(dw) = self.pages[idx].diff.as_ref() {
                    let swept = PROBE_DIFF_SWEEP.with(|c| c.get());
                    probe_diff_scroll_asserts(dw, &self.pages[idx].view.clone());
                    if swept {
                        // post-renewal re-run: every assert above printed
                        // against a page whose buffers were swapped by the
                        // refresh_diff flight (an open tab + a git tick — the
                        // frequently-run path). PASS line here = asserts held
                        // across renewal: no flap, share + links survived.
                        println!("PROBE renewal-assert: PASS");
                    }
                } else {
                    println!("PROBE assert: diff-presence: FAIL (not a diff page)");
                }
                // arm once (probe-only): mutate the fixture + drive the
                // renewal directly
                if !PROBE_DIFF_SWEEP.with(|c| c.get()) {
                    PROBE_DIFF_SWEEP.with(|c| c.set(true));
                    let mut mutated = false;
                    // belt: the append must be a PROBE fixture, never a user
                    // workspace (the message is probe-only, but the chain's
                    // fixture conventions live at root = wsA)
                    if let Some(root) = &self.root
                        && root.file_name() == Some(std::ffi::OsStr::new("wsA"))
                    {
                        let f = root.join("a.txt");
                        if let Ok(mut fh) = std::fs::OpenOptions::new().append(true).open(&f) {
                            use std::io::Write;
                            let _ = write!(fh, "\nprobe renewal tick line\n");
                            mutated = true;
                        }
                    }
                    if mutated
                        && let Some(root) = self.root.clone()
                    {
                        // the flight refetches; the fresh (non-identical) diff
                        // defeats the byte-identical dedupe, so refresh_diff
                        // RUNS on the open page; 500ms = flight + landing
                        sender.input(EditorMsg::RefreshDiffs {
                            root,
                            letters: vec![("a.txt".into(), "M".into())],
                        });
                        let snd = sender.clone();
                        glib::timeout_add_local(
                            std::time::Duration::from_millis(500),
                            move || {
                                snd.input(EditorMsg::ProbeDiffScroll);
                                glib::ControlFlow::Break
                            },
                        );
                    }
                }
            }
            EditorMsg::FsEvent(path) => {
                self.arm_settle(path, &sender);
            }
            EditorMsg::FsSettled(path) => {
                self.fs_settled(&path, &sender);
            }
            EditorMsg::ClosePage(index, force) => {
                self.close_index(index, force, &sender);
            }
        }
    }

    fn update_cmd(
        &mut self,
        message: EditorCommand,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match message {
            EditorCommand::DiffData { rel, letter, text, binary } => {
                if binary {
                    self.error(
                        &format!("Binary file: {rel}"),
                        "No diff view for binary changes.",
                    );
                    return;
                }
                let sides = gitstatus::build_sides(&gitstatus::parse_unified(&text));
                // sides.old is the OLD side — len==0 means no content
                if sides.old.is_empty() {
                    self.error(
                        &format!("No diff: {rel}"),
                        "git produced no diff hunks for this path.",
                    );
                    return;
                }
                self.open_diff(&rel, &sides, &letter, &text, &sender);
            }
            EditorCommand::FreshDiffs(fresh) => {
                for (rel, text, sides, letter) in fresh {
                    self.refresh_diff(&rel, &sides, &letter, &text, &sender);
                }
            }
        }
    }
}

impl Editor {
    fn active(&self) -> Option<usize> {
        self.nb.current_page().map(|c| c as usize)
    }

    /// the toplevel window (dialogs are transient for it)
    fn window_widget(&self) -> Option<gtk::Window> {
        self.nb.root().and_downcast::<gtk::Window>()
    }

    /// modal CLOSE MessageDialog, transient for the window.
    fn error(&self, primary: &str, secondary: &str) {
        let Some(win) = self.window_widget() else { return };
        #[allow(deprecated)] // 4.10-deprecated upstream (S3 polish)
        let d = gtk::MessageDialog::builder()
            .transient_for(&win)
            .modal(true)
            .text(primary)
            .secondary_text(secondary)
            .buttons(gtk::ButtonsType::Close)
            .build();
        d.connect_response(|dd, _r| dd.destroy());
        d.present();
    }

    fn page_of(&self, path: &Path) -> Option<usize> {
        self.pages.iter().position(|pg| pg.path.as_deref() == Some(path))
    }

    /// diff tabs key off "diff:<rel>"; the exact
    /// path match `page_of` stays file-tab-only
    fn page_of_key(&self, key: &str) -> Option<usize> {
        self.pages.iter().position(|pg| pg.diff_of.as_deref() == Some(key))
    }

    /// basename of a rel path (os.path.basename — the last '/' segment)
    fn rel_base(rel: &str) -> &str {
        rel.rsplit_once('/').map(|(_, b)| b).unwrap_or(rel)
    }

    // ---- lifecycle ------------------------------------------------------------

    /// records the root, closes everything.
    fn set_root(&mut self, root: PathBuf) {
        self.root = Some(root);
        self.close_all();
    }

    /// set_root + open still-existing tabs + clamp by
    /// min(tab, n-1) (a saved index past the end re-clamps) + refresh.
    fn restore(&mut self, root: &Path, tabs: Vec<String>, active: i64, sender: &ComponentSender<Self>) {
        self.set_root(root.to_path_buf());
        for rel in tabs {
            let p = root.join(rel);
            if p.is_file() {
                // stale persisted tabs vanish silently
                self.open_file(&p, sender);
            }
        }
        let n = self.nb.n_pages() as i64;
        // GTK ignores a negative set_current_page; u32::try_from(None-maps it)
        self.nb
            .set_current_page(u32::try_from(active.min((n - 1).max(0))).ok());
        self.refresh_restamps(sender);
        let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
    }

    fn close_all(&mut self) {
        for i in (0..self.pages.len()).rev() {
            self.nb.remove_page(Some(i as u32));
            let p = self.pages.remove(i);
            if let Some(path) = p.path {
                self.cancel_watch(&path);
            }
        }
    }

    // ---- open -----------------------------------------------------------------

    fn open_file(&mut self, path: &Path, sender: &ComponentSender<Self>) {
        if let Some(k) = self.page_of(path) {
            self.nb.set_current_page(Some(k as u32));
            let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
            return;
        }
        let raw = match std::fs::read(path) {
            Ok(bytes) => Some(bytes),
            // vanished since the tree rendered it → tombstone tab, never a popup
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                self.error(
                    "Couldn't open file",
                    &format!("{}: {e}", basename(path)),
                );
                return;
            }
        };
        // refusing at open is the only safe point: a lossy buffer + Ctrl+S
        // would rewrite the file with truncated/replacement garbage
        let text = match &raw {
            Some(bytes) => match readable_text(bytes) {
                Some(t) => t,
                None => {
                    self.error(
                        "Unsupported file",
                        &format!("{}: binary, NUL bytes, or not UTF-8", basename(path)),
                    );
                    return;
                }
            },
            None => GONE_NOTE.to_owned(),
        };

        let buf = sourceview5::Buffer::builder().text(&text).build();
        if let Some(scheme) = &self.scheme {
            buf.set_style_scheme(Some(scheme));
        }
        let base = basename(path);
        if let Some(lang) = self.lm.guess_language(Some(base.as_str()), None) {
            buf.set_language(Some(&lang));
        }
        // GtkSource.Buffer(text=…) is born modified — an open must not show
        // the dirty dot
        buf.set_modified(false);
        if raw.is_none() {
            Self::tag_muted(&buf);
        }
        let snd = sender.clone();
        buf.connect_modified_changed(move |_| snd.input(EditorMsg::StateTick));
        let view = sourceview5::View::builder()
            .buffer(&buf)
            .show_line_numbers(true)
            .wrap_mode(gtk::WrapMode::None)
            .pixels_above_lines(2) // line pitch ~20 @ 13px mono (VS Code density)
            .left_margin(12)       // code column inset after the line-number gutter
            .css_classes(["alpaca-mono"])
            .build();
        let page_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        let rel: String = match &self.root {
            Some(root_dir) => relpath(path, root_dir),
            None => path.to_string_lossy().into_owned(),
        };
        let crumbs = rel.split('/').collect::<Vec<_>>().join("  ›  ");
        let crumb = gtk::Label::builder()
            .label(crumbs)
            .xalign(0.0)
            .ellipsize(pango::EllipsizeMode::Middle) // a deep path must not widen the page min
            .css_classes(["alpaca-breadcrumb"])
            .build();
        page_box.append(&crumb);
        // ref kept: reload restores its scroll
        let sw = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .child(&view)
            .build();
        page_box.append(&sw);

        let fs = if raw.is_some() { FileState::Live } else { FileState::Deleted };
        let load_mtime = if raw.is_some() { mtime_ns(path) } else { None };

        // tab head: dirty ● FIRST, badge slot, name, close
        let dot = gtk::Label::builder()
            .label("●")
            .css_classes(["alpaca-dirty"])
            .visible(false)
            .build();
        let name = gtk::Label::builder()
            .label(&base)
            .css_classes(["alpaca-tabname"])
            .ellipsize(pango::EllipsizeMode::Middle)
            .build();
        // tabname floor: GTK 4.22 pins scrollable-notebook tabs at their
        // minimum even with space to spare — bare ellipsize floors the text at
        // '…' and GTK4 GtkLabel has no min-width-chars (GTK3-only).
        name.set_size_request(72, -1);
        let close = gtk::Button::new();
        if let Some(vec) = vector::icon("x-dim.svg") {
            close.set_child(Some(&gtk::Image::from_paintable(Some(&vec))));
        } else {
            close.set_child(Some(
                &gtk::Image::builder().icon_name("window-close-symbolic").build(),
            ));
        }
        close.set_css_classes(&["alpaca-close"]);
        let head = gtk::Box::builder()
            .spacing(4)
            .css_classes(["alpaca-tab"])
            .valign(gtk::Align::Center)
            .build();
        head.append(&dot); // must stay first
        // per-file type badge; restamped by refresh
        let slot = gtk::Box::builder().spacing(4).valign(gtk::Align::Center).build();
        head.append(&slot);
        head.append(&name);
        head.append(&close);
        if let Some(want) = badges::pixbuf_for(&base, false) {
            slot.append(&gtk::Image::from_pixbuf(Some(&want)));
        }
        let index = self.nb.append_page(&page_box, Some(&head));
        self.nb.set_current_page(Some(index)); // append does not switch; a new tab must take focus
        // resolve the (possibly shifted) index at click time via the page widget
        let nb2 = self.nb.clone();
        let pg_widget = page_box.clone();
        let snd = sender.clone();
        close.connect_clicked(move |_| {
            // page Some only while the page exists
            let idx = nb2.page_num(&pg_widget).map(|i| i as usize);
            if let Some(idx) = idx {
                snd.input(EditorMsg::ClosePage(idx, false));
            }
        });
        self.pages.push(PageState {
            path: Some(path.to_path_buf()),
            buf,
            sw,
            view,
            page: page_box,
            dot,
            tabname: name,
            badge_slot: slot,
            fs,
            conflict: false,
            load_mtime,
            diff_of: None,
            diff_letter: String::new(),
            diff: None,
        });
        self.arm_watch(path, sender);
        if fs != FileState::Live {
            self.paint_tab_deleted(self.pages.len() - 1, true);
        }
        self.refresh_restamps(sender); // switch-page alone misses opens that don't change pages
        let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
    }

    // ---- tab state / dots -----------------------------------------------------

    fn paint_dot(&self, idx: usize) {
        let pg = &self.pages[idx];
        // red while locally modified, amber while ALSO changed on disk
        pg.dot.set_visible(pg.buf.is_modified() || pg.conflict);
        let mut classes = vec!["alpaca-dirty".to_owned()];
        if pg.conflict {
            classes.push("conflict".to_owned()); // amber
        }
        let classes: Vec<&str> = classes.iter().map(|s| s.as_str()).collect();
        pg.dot.set_css_classes(&classes);
    }

    fn repaint_dots(&self) {
        for i in 0..self.pages.len() {
            self.paint_dot(i);
        }
    }

    fn paint_tab_deleted(&self, idx: usize, on: bool) {
        let name = &self.pages[idx].tabname;
        if on {
            name.add_css_class("alpaca-deleted");
        } else {
            name.remove_css_class("alpaca-deleted");
        }
    }

    fn refresh_restamps(&self, sender: &ComponentSender<Self>) {
        // every tab carries its file's type badge (user overrode the mockup's
        // hash-for-inactive rule: a selected tab must not erase the others')
        for pg in &self.pages {
            if pg.diff_of.is_some() {
                continue; // diff tab: badge_slot already holds its letter chip
            }
            while let Some(c) = pg.badge_slot.first_child() {
                pg.badge_slot.remove(&c);
            }
            if let Some(path) = &pg.path
                && let Some(want) = badges::pixbuf_for(&basename(path), false) {
                    pg.badge_slot.append(&gtk::Image::from_pixbuf(Some(&want)));
                }
        }
        let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
    }

    // ---- close ----------------------------------------------------------------

    fn close_index(&mut self, index: usize, force: bool, sender: &ComponentSender<Self>) {
        if index >= self.pages.len() {
            return;
        }
        if self.pages[index].buf.is_modified() && !force {
            // dirty → "Discard changes to X?" CANCEL/Discard; the Discard
            // click re-enters forced. The index is resolved at response time
            // via the page widget — tabs may shift while the dialog is open
            let Some(win) = self.window_widget() else { return };
            let name = self.pages[index]
                .path
                .as_ref()
                .map(|p| basename(p))
                .unwrap_or_default();
            let d = discard_dialog(&win, &format!("Discard changes to {name}?"));
            let nb2 = self.nb.clone();
            let pg_widget = self.pages[index].page.clone();
            let snd = sender.clone();
            d.connect_response(move |dd, r| {
                dd.destroy(); // destroy in the response handler
                if r == gtk::ResponseType::Accept {
                    let idx = nb2.page_num(&pg_widget).map(|i| i as usize);
                    if let Some(idx) = idx {
                        snd.input(EditorMsg::ClosePage(idx, true));
                    }
                }
            });
            d.present();
            return;
        }
        self.nb.remove_page(Some(index as u32));
        let p = self.pages.remove(index);
        if let Some(path) = p.path {
            self.cancel_watch(&path);
        }
        self.refresh_restamps(sender); // closing the active tab flips tabs
    }

    // ---- save -----------------------------------------------------------------

    fn write_page(&mut self, idx: usize) -> Result<(), std::io::Error> {
        let path = self.pages[idx].path.clone().expect("write_page: file page");
        let text = self.pages[idx]
            .buf
            .text(&self.pages[idx].buf.start_iter(), &self.pages[idx].buf.end_iter(), false);
        std::fs::write(&path, text.as_str())?;
        // conflict cleared BEFORE set_modified(false) — the fired
        // modified-changed repaints the dot (order matters)
        self.pages[idx].conflict = false;
        self.pages[idx].buf.set_modified(false);
        if let Some(m) = mtime_ns(&path) {
            self.pages[idx].load_mtime = Some(m); // latch the save — the settle must not reload what we wrote
        }
        Ok(())
    }

    fn save_active(&mut self, _sender: &ComponentSender<Self>) {
        let Some(idx) = self.active() else { return };
        if self.pages[idx].path.is_none() {
            return; // diff page (S2): Ctrl+S is a no-op
        }
        if !self.pages[idx].buf.is_modified() {
            return; // clean Ctrl+S never resurrects a tombstone note
        }
        if let Err(e) = self.write_page(idx) {
            let name = self.pages[idx]
                .path
                .as_ref()
                .map(|p| basename(p))
                .unwrap_or_default();
            self.error("Couldn't save", &format!("{name}: {e}"));
        }
    }

    // ---- diff pages (S2) ------------------------------------------------------

    /// diff-side buffer — fresh GtkSource buffer (scheme +
    /// lang), bg tint over the marked lines, header fg, `set_modified` AFTER
    /// the tags ("diff tabs never dirty"). Full syntax highlighting both
    /// sides — the '=' rows get NO fg tag (user reversal 2026-10-14: the
    /// Rider-dim tag out-specced the scheme's token colors). Shared by the
    /// page build and the refresh refill.
    fn diff_buf(
        &self,
        lines: &[String],
        idxs: &BTreeSet<usize>,
        hdr: &BTreeSet<usize>,
        hexcol: &str,
        lang: Option<&sourceview5::Language>,
    ) -> sourceview5::Buffer {
        let buf = sourceview5::Buffer::builder().text(lines.join("\n")).build();
        if let Some(scheme) = &self.scheme {
            buf.set_style_scheme(Some(scheme));
        }
        if let Some(lang) = lang {
            buf.set_language(Some(lang));
        }
        // a bad hex stays transparent (fresh Gdk.RGBA())
        let tint = rgba(hexcol);
        let tag = gtk::TextTag::builder().background_rgba(&tint).build();
        let fg = gtk::TextTag::builder()
            .foreground_rgba(&rgba(DIFF_HDR_FG))
            .build();
        buf.tag_table().add(&tag);
        buf.tag_table().add(&fg);
        // negative-index clamp — parse never stores negatives, so the usize
        // compare kills a wrapped usize::MAX too (untracked full-del rows)
        for &i in idxs.iter() {
            if i >= buf.line_count() as usize || i == usize::MAX {
                continue;
            }
            if let Some(a) = buf.iter_at_line(i as i32) {
                let mut b = a;
                b.forward_to_line_end();
                buf.apply_tag(&tag, &a, &b);
            }
        }
        for &i in hdr.iter() {
            if i >= buf.line_count() as usize || i == usize::MAX {
                continue;
            }
            if let Some(a) = buf.iter_at_line(i as i32) {
                let mut b = a;
                b.forward_to_line_end();
                buf.apply_tag(&fg, &a, &b);
            }
        }
        // born modified (invariant); diff tabs never dirty
        buf.set_modified(false);
        buf
    }

    /// one diff side: the buffer above + non-editable unnumbered mono view.
    fn diff_side(
        &self,
        lines: &[String],
        idxs: &BTreeSet<usize>,
        hdr: &BTreeSet<usize>,
        hexcol: &str,
        lang: Option<&sourceview5::Language>,
    ) -> (sourceview5::View, sourceview5::Buffer) {
        let buf = self.diff_buf(lines, idxs, hdr, hexcol, lang);
        let view = sourceview5::View::builder()
            .buffer(&buf)
            .show_line_numbers(false) // padded sides would falsify numbers
            .editable(false)
            .wrap_mode(gtk::WrapMode::None)
            .pixels_above_lines(2)
            .left_margin(12)
            .css_classes(["alpaca-mono"])
            .build();
        (view, buf)
    }

    /// a CHANGES row's side-by-side diff page: left
    /// (del, red tint) / right (add, green tint), linked scroll, letter chip
    /// in the tab head ("diff tabs never dirty" → no dot). Reuses the file
    /// page's widget fields for the LEFT side.
    fn open_diff(
        &mut self,
        rel: &str,
        sides: &Sides,
        letter: &str,
        text: &str,
        sender: &ComponentSender<Self>,
    ) {
        let key = format!("diff:{rel}");
        if let Some(idx) = self.page_of_key(&key) {
            self.nb.set_current_page(Some(idx as u32));
            return;
        }
        let base = Self::rel_base(rel);
        let lang = self.lm.guess_language(Some(base), None);
        let (v_l, b_l) = self.diff_side(&sides.old, &sides.del, &sides.hdr, DIFF_DEL_BG, lang.as_ref());
        let (v_r, _b_r) = self.diff_side(&sides.new, &sides.add, &sides.hdr, DIFF_ADD_BG, lang.as_ref());

        // before side ships chrome-free (user ruling 2026-10-13): the after
        // side hosts the ONLY v bar and the ONLY h bar. Hiding rides the
        // widget layer (hide_scrollbars below): PolicyType::Never UNCLAMPS the
        // child's natural size on this build (measured via the S2 diff asserts
        // the day this landed: viewport grew to the full 1740px content
        // height, upper==page_size on both axes, nothing scrollable anywhere),
        // and css `display` is NOT a property here (theme parser rejects it —
        // probe13-19 logs: "No property named display"; the rule was a no-op).
        // Widget-visibility keeps the viewport clamped + pairing live.
        let sw_l = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .child(&v_l)
            .build();
        hide_scrollbars(&sw_l, &[gtk::Orientation::Horizontal, gtk::Orientation::Vertical]);
        let sw_r = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .child(&v_r)
            .build();
        // single full-width h bar (user ruling 2026-10-13): the panes' own
        // h bars hidden, ONE Scrollbar under the paned pair bound to sw_r's
        // hadj — the sw owns its adjustments and refresh_diff never
        // re-children, so a one-time bind is stable (adj identity
        // persists — the pairing law comment). sw_r's v bar stays.
        hide_scrollbars(&sw_r, &[gtk::Orientation::Horizontal]);
        let hbar = gtk::Scrollbar::new(gtk::Orientation::Horizontal, Some(&sw_r.hadjustment()));
        // style-match the standalone bar (user 2026-10-14: "its style differs
        // from the vertical scroll bar"): the panes' in-sw bars are styled by
        // the classes the sw's overlay machinery puts on them; a standalone
        // Scrollbar carries only `horizontal` and Breeze paints its classic
        // light trough. Inherit the reference bar's classes (sw_r's kept v
        // bar — the ONLY visible one), orientation swapped.
        let ref_cls: Vec<glib::GString> = {
            let mut c = sw_r.first_child();
            loop {
                match c {
                    Some(w) => {
                        if let Some(sb) = w.downcast_ref::<gtk::Scrollbar>()
                            && sb.orientation() == gtk::Orientation::Vertical
                        {
                            break sb.css_classes();
                        }
                        c = w.next_sibling();
                    }
                    None => break Vec::new(),
                }
            }
        };
        let mut cls: Vec<String> = ref_cls.iter().map(|s| s.to_string())
            .filter(|c| c != "vertical")
            .collect();
        cls.push("horizontal".into());
        if !cls.iter().any(|c| c == "alpaca-diffhbar") {
            cls.push("alpaca-diffhbar".into()); // css hook for capture-law tuning
        }
        hbar.set_css_classes(&cls.iter().map(String::as_str).collect::<Vec<_>>());
        // number panes (Task 4): the line-number columns. Same line-box
        // params as the mains (alpaca-mono + pixels_above_lines(2)) so row
        // heights match 1:1; fixed-width panes (see the size_request below —
        // hug-natural does NOT work under an hexpand'd sibling); focusable
        // (false) keeps clipboard ops on the content side. Buffers are
        // swapped on renewal exactly like the mains' — one shared adjustment
        // per side below means one set_buffer-law reset event, and
        // restore_frac on that same object re-applies once.
        let num_l = diff_num_view(&sides.old_no);
        let num_r = diff_num_view(&sides.new_no);
        let sw_num_l = gtk::ScrolledWindow::builder().child(&num_l).build();
        hide_scrollbars(&sw_num_l, &[gtk::Orientation::Horizontal, gtk::Orientation::Vertical]);
        let sw_num_r = gtk::ScrolledWindow::builder().child(&num_r).build();
        hide_scrollbars(&sw_num_r, &[gtk::Orientation::Horizontal, gtk::Orientation::Vertical]);
        // pane widths: an sw's natural width for a SCROLLABLE child is ~its
        // minimum (measured r3 red28: number panes collapsed to 0px — the
        // hexpand'd mains ate everything; even the hug-natural assumption was
        // wrong there). Fixed floors from the RENDERED field width — the old
        // view.measure() at open-time under-reported the mono field and the
        // hidden h bar let it clip digits mid-glyph (2026-10-14 clip fix);
        // ONE shared width so the two number columns line up.
        let numw = diff_num_pane_width(&sides.old_no, &num_l)
            .max(diff_num_pane_width(&sides.new_no, &num_r));
        sw_num_l.set_size_request(numw, -1);
        sw_num_r.set_size_request(numw, -1);
        // lockstep law: the num view's vadjustment IS its main's object — so
        // the num pane's own wheel drives the SAME adj the main scrolls; no
        // push race, no extra linked cells. The set MUST ride AFTER the num
        // sw's set_child AND after the MAIN sw's set_child — the
        // ScrolledWindow installs its own adjustments onto its child at
        // child-set (measured r2 green20: a pre-set_child share was
        // clobbered; dbg22: a share taken pre-sw_r bound to v_r's
        // pre-clobber own adj — set_child rewired v_r and left num_r
        // stranded. POST-child-set, the sw's adjustment-watcher rebinds its
        // bars to the child's new adj — measured on the left side). The h
        // axis stays private: the panes' page sizes differ, and one
        // adjustment can't hold two.
        if let Some(va) = v_l.vadjustment() {
            num_l.set_vadjustment(Some(&va));
        }
        if let Some(va) = v_r.vadjustment() {
            num_r.set_vadjustment(Some(&va));
        }
        // resizable split (user ruling 2026-10-13): the 1px alpaca-diffsep
        // Box becomes a paned divider (app.css's paned > separator 6px rule
        // makes the knob grabbable; no wide_handle — this stays compact).
        // GTK4 paned's OWN unset position is UNRELIABLE here (measured r2:
        // start child collapsed to its 1px minimum with text-view children),
        // and no allocation-notify exists — the one-shot 50% rides the sw's
        // hadjustment 'changed' (the same allocation-adjacent event
        // restore_frac legally uses: page_size lands when the split area
        // first allocates; the set lands before the user can touch it).
        let sw_l_wrap = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        sw_l_wrap.append(&sw_num_l); // numbers LEFT of the before side
        sw_l_wrap.append(&sw_l);
        let sw_r_wrap = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        sw_r_wrap.append(&sw_num_r); // numbers at the after pane's left edge
        sw_r_wrap.append(&sw_r);
        let paned = gtk::Paned::builder()
            .orientation(gtk::Orientation::Horizontal)
            .vexpand(true)
            .hexpand(true)
            .css_classes(["alpaca-diffpaned"]) // hairline separator paint (app.css)
            .start_child(&sw_l_wrap)
            .end_child(&sw_r_wrap)
            .build();
        let paned_pos = Rc::new(Cell::new(false));
        {
            let p2 = paned.clone();
            let pos_flag = paned_pos.clone();
            sw_l.hadjustment().connect_changed(move |adj| {
                if pos_flag.get() || adj.page_size() <= 0.0 {
                    return;
                }
                let w = p2.allocated_width();
                if w > 0 {
                    pos_flag.set(true);
                    // set_position fired MID-CASCADE is discarded by the
                    // paned's own allocation (S3's same-step law: "the split
                    // never applied") — readback was 482 with children still
                    // at the 1|958 unset compute; the set must land at idle,
                    // after the cascade, where it re-allocates for real
                    let pw = w / 2;
                    let p3 = p2.clone();
                    glib::idle_add_local_once(move || p3.set_position(pw));
                }
            });
        }

        let page = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        let crumb = gtk::Label::builder()
            .label(rel.split('/').collect::<Vec<_>>().join("  ›  "))
            .xalign(0.0)
            .ellipsize(pango::EllipsizeMode::Middle) // a deep path must not widen the page min
            .css_classes(["alpaca-breadcrumb"])
            .build();
        page.append(&crumb);
        page.append(&paned);
        page.append(&hbar);

        // tab head: letter chip slot FIRST, name, close — and no dirty dot.
        // an orphan dot for paint_dot parity (diff tabs never dirty)
        let slot = gtk::Box::builder().spacing(4).valign(gtk::Align::Center).build();
        if !letter.is_empty() {
            let chip = gtk::Label::builder()
                .label(letter)
                .css_classes([format!("alpaca-diffchip{}", letter.to_lowercase())])
                .build();
            slot.append(&chip);
        }
        let name = gtk::Label::builder()
            .label(base)
            .css_classes(["alpaca-tabname"])
            .ellipsize(pango::EllipsizeMode::Middle)
            .build();
        name.set_size_request(72, -1);
        let close = gtk::Button::new();
        if let Some(vec) = vector::icon("x-dim.svg") {
            close.set_child(Some(&gtk::Image::from_paintable(Some(&vec))));
        } else {
            close.set_child(Some(
                &gtk::Image::builder().icon_name("window-close-symbolic").build(),
            ));
        }
        close.set_css_classes(&["alpaca-close"]);
        let head = gtk::Box::builder()
            .spacing(4)
            .css_classes(["alpaca-tab"])
            .valign(gtk::Align::Center)
            .build();
        head.append(&slot);
        head.append(&name);
        head.append(&close);
        let orphan_dot = gtk::Label::builder()
            .label("●")
            .css_classes(["alpaca-dirty"])
            .visible(false)
            .build();

        let index = self.nb.append_page(&page, Some(&head));
        self.nb.set_current_page(Some(index)); // append does not switch; a new tab must take focus
        // resolve the (possibly shifted) index at click time via the page widget
        let nb2 = self.nb.clone();
        let pg_widget = page.clone();
        let snd = sender.clone();
        close.connect_clicked(move |_| {
            let idx = nb2.page_num(&pg_widget).map(|i| i as usize);
            if let Some(idx) = idx {
                snd.input(EditorMsg::ClosePage(idx, false));
            }
        });

        let diff = DiffWidgets {
            sw_l: sw_l.clone(),
            sw_r: sw_r.clone(),
            v_r: v_r.clone(),
            diff_text: Some(text.to_string()),
            linked_l: Rc::new(RefCell::new(LinkedAxes::default())),
            linked_r: Rc::new(RefCell::new(LinkedAxes::default())),
            paned: paned.clone(),
            bar: hbar.clone(),
            num_l: num_l.clone(),
            num_r: num_r.clone(),
            sw_num_l: sw_num_l.clone(),
            sw_num_r: sw_num_r.clone(),
        };
        // linking hooks once at wire time (views own their adjustments from
        // construction — pairing law comment at the machinery); dst captured
        // so the handler pushes into THIS page's opposite side
        diff_link(&v_l, &v_r, &diff.linked_l);
        diff_link(&v_r, &v_l, &diff.linked_r);

        self.pages.push(PageState {
            path: None,
            buf: b_l,
            view: v_l,
            sw: sw_l,
            page,
            dot: orphan_dot,
            tabname: name,
            badge_slot: slot,
            fs: FileState::Live,
            conflict: false,
            load_mtime: None,
            diff_of: Some(key),
            diff_letter: letter.to_string(),
            diff: Some(diff),
        });
        self.refresh_restamps(sender);
        let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
    }

    /// live renewal of an open diff page — same trigger/fraction/letter-chip
    /// contract, measured-churn-safe body (see the comment inside).
    fn refresh_diff(
        &mut self,
        rel: &str,
        sides: &Sides,
        letter: &str,
        text: &str,
        sender: &ComponentSender<Self>,
    ) {
        let key = format!("diff:{rel}");
        let Some(idx) = self.page_of_key(&key) else { return };
        let (frac_l, frac_r) = {
            let dw = self.pages[idx].diff.as_ref().unwrap(); // diff pages always carry theirs
            (frac_of(&dw.sw_l), frac_of(&dw.sw_r))
        };
        // refresh_diff — live renewal of an open diff page. NOT the
        // swap-the-child-VIEWS shape: measured churn law —
        // every widget swap re-allocates the visible page and its scroll
        // adjustments reset/flap; the swap raced restore_frac too. The views,
        // ScrolledWindows and scroll pairing here are RETAINED and only the
        // side buffers are swapped in (the reload path's shape, swap_text:
        // adj identity persists → linked_l/linked_r stay live, no re-wire,
        // no set_child). The byte-identical dedupe lives in the RefreshDiffs
        // flight — this path runs only when the diff really changed.
        let lang = self.lm.guess_language(Some(Self::rel_base(rel)), None);
        let (b_l, b_r) = (
            self.diff_buf(&sides.old, &sides.del, &sides.hdr, DIFF_DEL_BG, lang.as_ref()),
            self.diff_buf(&sides.new, &sides.add, &sides.hdr, DIFF_ADD_BG, lang.as_ref()),
        );
        {
            let (sw_l, sw_r, sw_num_l, sw_num_r, v_r, num_l, num_r) = {
                let dw = self.pages[idx].diff.as_ref().unwrap();
                (dw.sw_l.clone(), dw.sw_r.clone(), dw.sw_num_l.clone(), dw.sw_num_r.clone(), dw.v_r.clone(), dw.num_l.clone(), dw.num_r.clone())
            };
            self.pages[idx].view.set_buffer(Some(&b_l)); // left view kept
            v_r.set_buffer(Some(&b_r)); // right view kept
            // number panes ride along (Task 4): fresh sides' numbers, same
            // set_buffer shape — their vadj IS the main's (shared object),
            // so the reset happens once per side and restore_frac re-applies
            num_l.set_buffer(Some(&sourceview5::Buffer::builder()
                .text(gitstatus::diff_num_text(&sides.old_no))
                .build()));
            num_r.set_buffer(Some(&sourceview5::Buffer::builder()
                .text(gitstatus::diff_num_text(&sides.new_no))
                .build()));
            // refreshed sides can GAIN a number digit (99→100 lines) — the
            // fixed sw would clip the widened field mid-glyph again (the h
            // bar here is hidden). Re-derive, same shape as open_diff. The
            // re-stamp rides AFTER the buffers: pango needs the fields only.
            let numw = diff_num_pane_width(&sides.old_no, &num_l)
                .max(diff_num_pane_width(&sides.new_no, &num_r));
            sw_num_l.set_size_request(numw, -1);
            sw_num_r.set_size_request(numw, -1);
            self.pages[idx].buf = b_l;
            self.pages[idx].diff.as_mut().unwrap().diff_text = Some(text.to_string());
            restore_frac(&sw_l, frac_l);
            restore_frac(&sw_r, frac_r);
        }
        if letter != self.pages[idx].diff_letter {
            let slot = self.pages[idx].badge_slot.clone();
            let mut chip_child = slot.first_child();
            while let Some(c) = chip_child {
                let nxt = c.next_sibling();
                slot.remove(&c);
                chip_child = nxt;
            }
            if !letter.is_empty() {
                let chip = gtk::Label::builder()
                    .label(letter)
                    .css_classes([format!("alpaca-diffchip{}", letter.to_lowercase())])
                    .build();
                slot.append(&chip);
            }
            self.pages[idx].diff_letter = letter.to_string();
        }
        let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
    }

    // ---- clip / edit ops ------------------------------------------------------

    /// Edit-menu op target (review fix-3):
    /// buffer pages → the page's own view; diff pages → the CURRENTLY FOCUSED
    /// text view (either side panel), so Cut/Copy/Paste/Select-All act on the
    /// side the user actually has focus in; anything not focusable cleanly
    /// no-ops the activate_action. diff pages carry no editable buffer — the
    /// stored left view (PageState.view) is NOT the right target there.
    fn op_target(&self, idx: usize) -> Option<gtk4::Widget> {
        if self.pages[idx].diff.is_some() {
            self.nb
                .root()
                .and_then(|o| o.downcast::<gtk4::Window>().ok())
                .and_then(|w| gtk4::prelude::GtkWindowExt::focus(&w))
        } else {
            Some(self.pages[idx].view.clone().upcast())
        }
    }

    fn clip_op(&self, op: &str) {
        if let Some(idx) = self.active()
            && let Some(w) = self.op_target(idx) {
                let _ = w.activate_action(op, None);
            }
    }

    // ---- live file changes: no popups, disk is the truth ----------------------

    fn cancel_watch(&mut self, path: &Path) {
        if let Some(mon) = self.watchers.remove(path) {
            mon.cancel();
        }
    }

    /// one FileMonitor per open path.
    fn arm_watch(&mut self, path: &Path, sender: &ComponentSender<Self>) {
        self.cancel_watch(path);
        let file = gio::File::for_path(path);
        // an absent path may have no watchable inode: tombstone stays until reopened
        let mon = match file.monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE) {
            Ok(m) => m,
            Err(_) => return,
        };
        let snd = sender.clone();
        let p = path.to_path_buf();
        mon.connect_changed(move |_, _, _, _| {
            // No popups (live spec). Every event — delete, atomic replace
            // (agents save via temp+rename), re-create — funnels into ONE
            // debounced disk check.
            snd.input(EditorMsg::FsEvent(p.clone()));
        });
        self.watchers.insert(path.to_path_buf(), mon);
    }

    fn arm_settle(&self, path: PathBuf, sender: &ComponentSender<Self>) {
        // the cancel-then-arm reset shape is out: SourceId::remove
        // PANICS on an already-fired id (measured: one touch+delete race kills the
        // app). fs_settled is idempotent, so stale settles are harmless — arm
        // without cancelling.
        let snd = sender.clone();
        glib::timeout_add(std::time::Duration::from_millis(120), move || {
            snd.input(EditorMsg::FsSettled(path.clone()));
            glib::ControlFlow::Break
        });
    }

    fn fs_settled(&mut self, path: &Path, sender: &ComponentSender<Self>) {
        let Some(k) = self.page_of(path) else { return };
        let Some(mtime) = mtime_ns(path) else {
            self.fs_tombstone(k, sender);
            return;
        };
        if self.pages[k].fs == FileState::Live && Some(mtime) == self.pages[k].load_mtime {
            return; // our own open/Ctrl+S echo
        }
        if self.pages[k].buf.is_modified() {
            if self.pages[k].fs != FileState::Live {
                // dirty buffer on a placeholder tab: the note dies, text lives
                self.pages[k].fs = FileState::Live;
                self.paint_tab_deleted(k, false);
            }
            self.pages[k].conflict = true;
            self.paint_dot(k);
            return;
        }
        self.reload(k, mtime, sender); // clean buffer follows the disk (also un-tombstones)
    }

    fn reload(&mut self, idx: usize, mtime: i64, sender: &ComponentSender<Self>) {
        let Some(path) = self.pages[idx].path.clone() else { return };
        let Ok(raw) = std::fs::read(&path) else {
            self.fs_tombstone(idx, sender);
            return;
        };
        let Some(text) = readable_text(&raw) else {
            self.fs_binary(idx, sender);
            return;
        };
        // cursor line/col + scroll value preserved across the seamless swap
        let (line, col, scroll) = {
            let pg = &self.pages[idx];
            let ins = pg.buf.get_insert();
            let it = pg.buf.iter_at_mark(&ins);
            (it.line(), it.line_offset(), pg.sw.vadjustment().value())
        };
        self.swap_text(idx, &text, sender);
        // place_cursor clamped to the new buffer's shape
        let pg = &mut self.pages[idx];
        let n = pg.buf.line_count();
        if let Some(mut it) = pg.buf.iter_at_line(line.min(n - 1)) {
            it.forward_chars(col.min((it.chars_in_line() - 1).max(0)));
            pg.buf.place_cursor(&it);
        }
        pg.fs = FileState::Live;
        pg.conflict = false;
        pg.load_mtime = Some(mtime);
        pg.view.set_editable(true);
        self.paint_tab_deleted(idx, false);
        let sw = self.pages[idx].sw.clone();
        glib::idle_add_local_once(move || {
            let adj = sw.vadjustment();
            adj.set_value(scroll.min((adj.upper() - adj.page_size()).max(0.0)));
        });
        self.paint_dot(idx);
        let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
    }

    /// fresh buffer (style + language carried over); the view keeps its place
    fn swap_text(&mut self, idx: usize, text: &str, sender: &ComponentSender<Self>) {
        let (sch, lang) = {
            let pg = &self.pages[idx];
            (pg.buf.style_scheme(), pg.buf.language())
        };
        let nb = sourceview5::Buffer::builder().text(text).build();
        if let Some(s) = sch {
            nb.set_style_scheme(Some(&s));
        }
        if let Some(l) = lang {
            nb.set_language(Some(&l));
        }
        nb.set_modified(false); // born modified (invariant) — a reload must not raise the dot
        let snd = sender.clone();
        nb.connect_modified_changed(move |_| snd.input(EditorMsg::StateTick));
        let pg = &mut self.pages[idx];
        pg.view.set_buffer(Some(&nb));
        pg.buf = nb;
    }

    fn fs_tombstone(&mut self, idx: usize, sender: &ComponentSender<Self>) {
        // file gone while open: italic, dimmed tab; a CLEAN buffer becomes the
        // tombstone note, a DIRTY one keeps the user's text (Ctrl+S recreates
        // the file). A later re-created file un-tombstones via reload.
        if self.pages[idx].fs == FileState::Deleted {
            return;
        }
        {
            let pg = &mut self.pages[idx];
            pg.fs = FileState::Deleted;
            pg.conflict = false;
        }
        if !self.pages[idx].buf.is_modified() {
            self.set_placeholder(idx, GONE_NOTE, sender);
        } else {
            self.paint_dot(idx);
        }
        self.paint_tab_deleted(idx, true);
        let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
    }

    fn fs_binary(&mut self, idx: usize, sender: &ComponentSender<Self>) {
        // disk content unreadable: the buffer could never round-trip it, so
        // freeze the view and note why. The unmodified note can never be saved
        // back over the binary file (dirty save guard); a later readable file
        // un-freezes via reload.
        if self.pages[idx].fs == FileState::Binary {
            return;
        }
        {
            let pg = &mut self.pages[idx];
            pg.fs = FileState::Binary;
            pg.conflict = false;
            pg.view.set_editable(false);
        }
        if !self.pages[idx].buf.is_modified() {
            self.set_placeholder(idx, BINARY_NOTE, sender);
        }
        let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
    }

    fn set_placeholder(&mut self, idx: usize, text: &str, sender: &ComponentSender<Self>) {
        self.swap_text(idx, text, sender);
        Self::tag_muted(&self.pages[idx].buf);
        self.pages[idx].buf.set_modified(false);
    }

    /// muted italic whole-buffer tag (placeholder notes)
    fn tag_muted(buf: &sourceview5::Buffer) {
        let tag = gtk::TextTag::builder()
            .foreground_rgba(&gtk::gdk::RGBA::parse("#5a6375").unwrap())
            .style(gtk::pango::Style::Italic)
            .build();
        buf.tag_table().add(&tag);
        buf.apply_tag(&tag, &buf.start_iter(), &buf.end_iter());
    }

    fn snapshot(&self) -> EditorSnapshot {
        let open_tabs: Vec<String> = self
            .pages
            .iter()
            .filter_map(|pg| pg.path.as_ref())
            .map(|p| match &self.root {
                Some(root) => relpath(p, root),
                None => p.to_string_lossy().into_owned(),
            })
            .collect();
        // index, not a path; restore re-clamps via min(saved, n-1)
        let active_tab = self.nb.current_page().map(i64::from).unwrap_or(0);
        let has_dirty = self.pages.iter().any(|pg| pg.buf.is_modified());
        EditorSnapshot { open_tabs, active_tab, has_dirty }
    }

}

#[allow(deprecated)] // 4.10-deprecated upstream (S3 polish)
fn discard_dialog(win: &gtk::Window, text: &str) -> gtk::MessageDialog {
    let d = gtk::MessageDialog::builder()
        .transient_for(win)
        .modal(true)
        .text(text)
        .buttons(gtk::ButtonsType::Cancel)
        .build();
    d.add_button("Discard", gtk::ResponseType::Accept);
    d
}

// diff-page pairing law (2026-10-13, replaces the "placeholder
// swap at first allocation" story): a View owns BOTH its adjustments from
// construction; the ScrolledWindow binds those very objects to its
// scrollbars (adjtrace: view-getter == sw-getter — identity measured held
// across refresh_diff renewals — the only-buffer-swap contract); GTK's
// notify::allocation NEVER fires on this build, so linking hooks ONCE at
// open (LinkedAxes dedupe) and never retries — the notify/idle
// retries were measured inert and deleted.

/// fresh Gdk.RGBA() (transparent) that parse() fills in
/// place; a bad hex stays transparent (an invisible tag, not a panic)
fn rgba(hexcol: &str) -> gtk::gdk::RGBA {
    let mut c = gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 0.0);
    if let Ok(p) = gtk::gdk::RGBA::parse(hexcol) {
        c = p;
    }
    c
}

/// capture the scroll fraction BEFORE a re-child
fn frac_of(sw: &gtk::ScrolledWindow) -> f64 {
    let adj = sw.vadjustment(); // inherent getter: non-Option (only views are)
    let span = (adj.upper() - adj.page_size()).max(0.0);
    if span > 0.0 {
        (adj.value() / span).min(1.0)
    } else {
        0.0
    }
}

// measured law (broadway adjlaw probe): set_buffer's relayout RESETS the adj
// value (100→0 even with a growing upper; 100→10 on an identical set_text) —
// the reset rides the bounds-update burst of GtkAdjustment::changed, which is
// synchronous within one frame. Re-apply the captured fraction on every changed
// fire for a short window: the burst ends with frac*current-span in place
// BEFORE paint — no top-flash, no timed race. The window expires, so no stale
// frac sticks to later user scrolling (value changes never fire changed).
// Unallocated page: nothing fires; when it maps, the bounds change re-applies.
fn restore_frac(sw: &gtk::ScrolledWindow, frac: f64) {
    let adj = sw.vadjustment(); // inherent getter: non-Option
    let id = adj.connect_changed(move |a| {
        let span = (a.upper() - a.page_size()).max(0.0);
        if span > 0.0 {
            a.set_value((frac * span).min(span));
        }
    });
    let adj2 = adj.clone();
    glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
        adj2.disconnect(id); // window expired — later bounds changes are the user's business
    });
}

/// hide a ScrolledWindow's scrollbar WIDGETS while keeping the viewport
/// clamped and scrollable — filtered to the given orientations. NOT
/// PolicyType::Never (unclamps the child — window grew to the content
/// height, measured 2026-10-13) and NOT css `display: none` (this GTK has no
/// `display` property — the theme parser rejects it, measured in the
/// probe13-19 logs), both attempted and rolled back same day.
/// Widget-visibility walk = the menu-button-arrow walk pattern (CLAUDE.md).
/// GTK's overlay reveal is css-class/opacity machinery, not set_visible, so a
/// one-shot hide sticks — the ProbeDiffScroll asserts re-check AFTER driving
/// the scroll for exactly that.
fn hide_scrollbars(sw: &gtk::ScrolledWindow, axes: &[gtk::Orientation]) {
    let mut c = sw.first_child();
    while let Some(w) = c {
        let nxt = w.next_sibling();
        if let Some(sb) = w.downcast_ref::<gtk::Scrollbar>()
            && axes.contains(&sb.orientation())
        {
            w.set_visible(false);
        }
        c = nxt;
    }
}

/// PROBE-ONLY (ALPACA_PROBE_S2, chain step 9 — ~2s post-open, PRE-switch:
/// step 12's SetWorkspace closes every page and step 4 was measured racy on
/// the live display, 2026-10-13): the before side's scrolled window is truly
/// chrome-free (its internal GtkScrollbar widgets walked to !visible AFTER
/// the pairing drives below — GTK's overlay reveal must not resurrect them),
/// its policies still Automatic (Never would unclamp — see open_diff), and
/// BOTH pairing directions settle at equal values per axis.
fn probe_diff_scroll_asserts(dw: &DiffWidgets, view_l: &sourceview5::View) {
    let ok = |name: String, got: bool| {
        println!("PROBE assert: diff-{name}: {}", if got { "PASS" } else { "FAIL" });
    };
    let bars = |sw: &gtk::ScrolledWindow| -> Vec<gtk::Scrollbar> {
        let mut out = Vec::new();
        let mut c = sw.first_child();
        while let Some(w) = c {
            if let Some(sb) = w.downcast_ref::<gtk::Scrollbar>() {
                out.push(sb.clone());
            }
            c = w.next_sibling();
        }
        out
    };
    // sw's inherent getters are non-Option (the real adjs); the pairing hooks
    // the same objects through the views. Assert the settled value, not the
    // set target: the narrower side caps both (feedback settles = equal).
    // Skip an axis when EITHER side can't span (vacuous equalities mask a
    // dead pairing); drive and check r→l AND l→r — the user wheels either.
    for (axis, src, dst) in [
        ("v", dw.sw_r.vadjustment(), dw.sw_l.vadjustment()),
        ("h", dw.sw_r.hadjustment(), dw.sw_l.hadjustment()),
    ] {
        let span = (src.upper() - src.page_size()).max(0.0);
        let dspan = (dst.upper() - dst.page_size()).max(0.0);
        if span <= 0.0 || dspan <= 0.0 {
            println!("PROBE assert: diff-sync-{axis}: SKIPPED (no span)");
            continue;
        }
        src.set_value(span.min(60.0));
        ok(format!("sync-{axis}-rl"), (src.value() - dst.value()).abs() < 0.5);
        dst.set_value(dspan.min(60.0));
        ok(format!("sync-{axis}-lr"), (src.value() - dst.value()).abs() < 0.5);
    }
    // bar visibility AFTER the drives (overlay reveal resurrection check) —
    // ≥2 on each side so an unexpected GTK child-shape change reads FAIL, not
    // a vacuous pass.
    let bars_l = bars(&dw.sw_l);
    ok(
        "bars-l".into(),
        bars_l.len() >= 2 && bars_l.iter().all(|b| !b.is_visible()),
    );
    ok("policy-v".into(), dw.sw_l.vscrollbar_policy() == gtk::PolicyType::Automatic);
    ok("policy-h".into(), dw.sw_l.hscrollbar_policy() == gtk::PolicyType::Automatic);
    // merged bar (Task 3): one Scrollbar under the pair, bound to sw_r's
    // (stable-identity) hadj — the bind is checked by identity so a detached
    // adjustment (the placeholder RED build) reads FAIL, and the drive below
    // proves the whole chain bar→sw_r→(pairing)→sw_l. gtk4-rs' adjustment()
    // getter is non-Option: an unbound bar carries a fresh default adj.
    let badj = dw.bar.adjustment();
    let dr = dw.sw_r.hadjustment();
    ok("bar-adj".into(), format!("{badj:?}") == format!("{dr:?}"));
    // bars-r: h hidden (merged bar replaced it), v kept
    let bars_r = bars(&dw.sw_r);
    let r_h = bars_r
        .iter()
        .find(|b| b.orientation() == gtk::Orientation::Horizontal);
    let r_v = bars_r
        .iter()
        .find(|b| b.orientation() == gtk::Orientation::Vertical);
    ok(
        "bars-r".into(),
        r_h.map(|b| !b.is_visible()).unwrap_or(false)
            && r_v.map(|b| b.is_visible()).unwrap_or(false),
    );
    // the standalone h bar's styling classes (capture rig: it must inherit
    // the reference v bar's set — see open_diff)
    println!(
        "PROBE classes: ref-v={:?} bar={:?}",
        r_v.map(|b| b.css_classes()),
        dw.bar.css_classes()
    );
    // driving the MERGED bar must carry BOTH panes (h via the bar's bound adj)
    let span = (badj.upper() - badj.page_size()).max(0.0);
    if span > 0.0 {
        badj.set_value(span.min(60.0));
        ok(
            "bar-drive".into(),
            (dr.value() - dw.sw_l.hadjustment().value()).abs() < 0.5,
        );
    } else {
        println!("PROBE assert: diff-bar-drive: SKIPPED (no span/unbound)");
    }
    // paned divider (Task 2): assert the one-shot 50% init (rides the sw's
    // hadj 'changed' → idle set) landed and holds: position ≈ half of the
    // paned's live allocation.
    let pw = dw.paned.allocated_width() as f64;
    ok(
        "paned-50".into(),
        pw > 0.0 && (dw.paned.position() as f64 - pw / 2.0).abs() <= 4.0,
    );
    // number panes (Task 4): row counts 1:1 + the shared-vadjustment lockstep
    // law — the num view's vadjustment IS its main's object (GTK's sw adopts
    // the child's adj objects; sharing the OBJECT is lockstep with no push
    // races and restore_frac keeps working on the one shared adj). Row
    // height parity rides the views' own naturals: same line-box params
    // must give the same total height, or the numbers point at wrong rows.
    ok(
        "num-rows".into(),
        dw.num_l.buffer().line_count() == view_l.buffer().line_count()
            && dw.num_r.buffer().line_count() == dw.v_r.buffer().line_count(),
    );
    // full-file sides (Task 1 2026-10-14): huge-ctx diff covers the entire
    // file, so the first data row IS file line 1 (the old -U3 hunks started
    // at 2+ on this fixture) — sentinel that the -U switch stays put
    let buf_l = dw.num_l.buffer();
    let t = buf_l.text(&buf_l.start_iter(), &buf_l.end_iter(), true);
    ok(
        "num-filestart".into(),
        t.split('\n').nth(1).map(str::trim) == Some("1"),
    );
    // number panes' widths (measured 2026-10-14 live-render bug, user shot:
    // an sw's natural for a scrollable child ≈ its MINIMUM → under hexpand'd
    // siblings the columns collapsed to 0px and no digits painted — the old
    // asserts counted rows and compared adj identity, never width). NOW the
    // pane is stamped from the pango-rendered widest field, so the LIVE
    // allocation must cover field px + the view's left margin (the old floor
    // 20 passed while digits were clipped mid-glyph; the h bar is hidden —
    // an under-measure here can't be recovered by hand-scrolling).
    let field_w = |v: &sourceview5::View| -> i32 {
        let buf = v.buffer();
        let n = buf
            .text(&buf.start_iter(), &buf.end_iter(), true)
            .split('\n')
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(2)
            .max(2);
        let (px, _) = v.create_pango_layout(Some(&"0".repeat(n))).pixel_size();
        px + 4 // + left_margin(4)
    };
    ok(
        "num-width".into(),
        dw.num_l.allocated_width() >= field_w(&dw.num_l)
            && dw.num_r.allocated_width() >= field_w(&dw.num_r),
    );
    let (nat_l, nat_r) = (view_l.measure(gtk::Orientation::Vertical, -1).1, dw.v_r.measure(gtk::Orientation::Vertical, -1).1);
    let (nnum_l, nnum_r) = (dw.num_l.measure(gtk::Orientation::Vertical, -1).1, dw.num_r.measure(gtk::Orientation::Vertical, -1).1);
    ok("num-height".into(), nat_l > 0 && (nat_l - nnum_l).abs() <= 2
        && nat_r > 0 && (nat_r - nnum_r).abs() <= 2);
    match (dw.num_l.vadjustment(), view_l.vadjustment()) {
        (Some(na), Some(ma)) => ok("num-adj-l".into(), format!("{na:?}") == format!("{ma:?}")),
        _ => println!("PROBE assert: diff-num-adj-l: FAIL (adj None)"),
    }
    match (dw.num_r.vadjustment(), dw.v_r.vadjustment()) {
        (Some(na), Some(ma)) => {
            ok("num-adj-r".into(), format!("{na:?}") == format!("{ma:?}"));
            ok("num-sync".into(), (na.value() - ma.value()).abs() < 0.01);
        }
        _ => println!("PROBE assert: diff-num-adj-r: FAIL (adj None)"),
    }
}

/// the number-column side (Task 4): a
/// non-editable, unfocusable, fixed-narrow source view over the side's
/// per-row file numbers; line-box params MUST track diff_side's (row 1:1).
fn diff_num_view(no: &[u32]) -> sourceview5::View {
    let buf = sourceview5::Buffer::builder()
        .text(gitstatus::diff_num_text(no))
        .build();
    sourceview5::View::builder()
        .buffer(&buf)
        .show_line_numbers(false)
        .editable(false)
        .focusable(false)
        .wrap_mode(gtk::WrapMode::None)
        .pixels_above_lines(2)
        .left_margin(4)
        .css_classes(["alpaca-mono", "alpaca-diffnum"])
        .build()
}

/// the fixed number pane's width, from the RENDERED field width (2026-10-14
/// clip fix, user shot: a digit was cut mid-glyph at the pane's clip edge —
/// the sw's h bar is hidden so nothing recovers). The old view.measure() at
/// open time under-reported the mono field; a pango layout over the widest
/// padded field reads the view's css-resolved font (.alpaca-mono) directly.
/// Used at open AND on renewal (a refreshed side can grow a digit).
/// ponytail: relies on the css font resolving at measure time — probe
/// assert `num-width` fails loudly if it ever drifts (see the chain).
fn diff_num_pane_width(no: &[u32], view: &sourceview5::View) -> i32 {
    let probe = "0".repeat(gitstatus::diff_num_field_width(no)); // mono digits uniform
    let (px, _) = view.create_pango_layout(Some(&probe)).pixel_size();
    (px + 4 + 4).max(24) // left_margin 4 + antialias slack 4
}

/// the diff scroll-link machinery — drive `dst`'s scroll
/// from `v`'s, BOTH axes. True = a NEW link was made on either axis
/// (equal-value set_value fires no changed signal → no loop, so the
/// destination never needs a guard). Each axis links independently (see
/// LinkedAxes). MEASURED 2026-10-13 (broadway probes + app probe runs
/// on broadway AND the real display): a View owns BOTH its adjustments from
/// construction — hooks land at open_diff's wire time, the sw binds those
/// very objects to its scrollbars (adjtrace: view-getter == sw-getter,
/// identity held across refresh_diff renewals), and GTK's notify::allocation
/// NEVER fires for widgets here — the allocation-notify retry +
/// idle-resync timers were measured inert and are gone. `linked` keeps the
/// identity-dedup so a stray second call can't double-hook. Closed page:
/// v → handler → dst is a DAG (no cycle); ownership frees the chain.
fn diff_link(
    v: &sourceview5::View,
    dst: &sourceview5::View,
    linked: &Rc<RefCell<LinkedAxes>>,
) {
    let mut l = linked.borrow_mut();
    if let Some(va) = v.vadjustment() {
        if l.v.as_ref() != Some(&va) {
            l.v = Some(va.clone());
            let dst2 = dst.clone();
            va.connect_value_changed(move |a| {
                // view adjustments exist from construction (measured r2); the
                // ScrollableExt getter merely RETURNS Option on this API;
                // equal-value set_value fires nothing anyway
                if let Some(dadj) = dst2.vadjustment() {
                    dadj.set_value(a.value());
                }
            });
        }
    }
    if let Some(ha) = v.hadjustment() {
        if l.h.as_ref() != Some(&ha) {
            l.h = Some(ha.clone());
            let dst2 = dst.clone();
            ha.connect_value_changed(move |a| {
                if let Some(dadj) = dst2.hadjustment() {
                    dadj.set_value(a.value());
                }
            });
        }
    }
}

// PROBE-ONLY (ALPACA_PROBE_S2) — ProbeDiffScroll's first run arms the
// renewal sweep: mutate the fixture on disk, drive RefreshDiffs directly
// (deterministic landing — the 2s git-tick phase would be racy), then
// re-assert on the settled page: covers refresh_diff's buffer+number swap
// on a LIVE page (the frequently-run path: an open diff tab + a git tick).
thread_local! {
    static PROBE_DIFF_SWEEP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
mod tests {
    use super::readable_text;

    #[test]
    fn readable_text_rules() {
        assert_eq!(readable_text(b"hello\nworld").as_deref(), Some("hello\nworld"));
        assert!(readable_text(b"caf\xc3\xa9").is_some()); // valid UTF-8 non-ASCII opens
        assert_eq!(readable_text(b"caf\xe9"), None); // invalid UTF-8
        assert_eq!(readable_text(b"a\0b"), None); // NUL truncation trap
        assert_eq!(readable_text(b""), Some(String::new())); // empty file is fine
    }
}