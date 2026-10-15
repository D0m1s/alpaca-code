//! launcher.rs — the Workbench Launcher hub (2026-10-15): a GtkStack page in
//! the main window listing every saved workbench file
//! (~/.config/alpaca-code/workbenches/<uuid>.json, most recent save first) as
//! one card per workbench — name + up-to-3 project tiles (basename, the saved
//! language chip, a live branch read) — plus the big `+` card and the footer
//! credit link (the mockup).
//!
//! Every listing fact is a DISK READ at Build (the launcher is only ever
//! shown, so there is no live-update machinery): language = the SAVED
//! ProjectSession.language — that field was stored for exactly this display;
//! branch = gitstatus::branch_of (a .git/HEAD read, the app's own sync path).
//! Opening rides app.rs's shared routes: card → LauncherOutput::Open →
//! AppMsg::LauncherOpen (adopt the session + OpenWorkspace per project); `+`
//! → LauncherOutput::New → AppMsg::LauncherAdd (folder picker, fresh
//! identity). Probe: ALPACA_PROBE_LN=<rig dir> (app.rs chain; production
//! never runs it, env unset).

use std::path::{Path, PathBuf};

use gdk_pixbuf::prelude::*;
use relm4::gtk::{self, gdk, gio, prelude::*};
use relm4::prelude::*;

use crate::{gitstatus, state, vector};

/// The llama, baked one-shot (magick Lanczos 1254→80) from the user-supplied
/// "Cute Fluffy Llama Icon.png" — frozen artifact like the seti set; the bake
/// is NOT tracked. Shown 1:1 at its 80px bake (no resample anywhere).
const LLAMA: &[u8] = include_bytes!("../assets/alpaca-80.png");
/// The launcher's credit link (user decision 2026-10-15).
const BY_URI: &str = "https://github.com/D0m1s";

#[derive(Debug)]
pub enum LauncherMsg {
    /// rebuild the card list from disk (every boot / menu Open Workbench)
    Build,
}

#[derive(Debug)]
pub enum LauncherOutput {
    /// card activated → open this workbench (the file stem = the name)
    Open(String),
    /// big `+` → folder picker → a FRESH workbench (identity cleared)
    New,
}

/// One project inside a workbench card: display facts only, no session data.
#[derive(Debug, PartialEq)]
pub struct Proj {
    pub name: String,
    /// the SAVED GtkSourceView language id (None → no chip)
    pub lang: Option<String>,
    /// live .git/HEAD read (None → not a repo / detached HEAD → no branch)
    pub branch: Option<String>,
}

/// One workbench card's facts, straight from its file.
#[derive(Debug, PartialEq)]
pub struct WbEntry {
    /// the file stem == workbench_path()'s name — what LauncherOpen loads by
    pub name: String,
    /// file mtime = the last save — the card order (most recent first)
    pub mtime: Option<std::time::SystemTime>,
    pub projs: Vec<Proj>,
}

/// All parseable *.json workbench files in `dir`, most recent save first;
/// per-project roots filtered to surviving dirs (the boot `p.is_dir()` law).
pub fn scan_workbenches(dir: &Path) -> Vec<WbEntry> {
    let mut out: Vec<WbEntry> = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out; // no dir yet → no cards (first-run law)
    };
    for e in rd.flatten() {
        let path = e.path();
        if !path.extension().is_some_and(|x| x == "json") {
            continue;
        }
        let mtime = e.metadata().and_then(|m| m.modified()).ok();
        let Some(wb) = state::load_workbench_from(&path) else {
            continue; // corrupt → skip, never brick the launcher
        };
        let mut projs: Vec<Proj> = wb
            .projects
            .into_iter()
            .filter(|(root, _)| Path::new(root).is_dir())
            .map(|(root, sess)| Proj {
                name: crate::app::py_basename(&root),
                lang: sess.language,
                branch: gitstatus::branch_of(&root),
            })
            .collect();
        projs.sort_by(|a, b| a.name.cmp(&b.name));
        let name = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        out.push(WbEntry { name, mtime, projs });
    }
    out.sort_by(|a, b| b.mtime.cmp(&a.mtime)); // desc; None mtime sorts last
    out
}

/// The roots a launcher card opens, in the order App opens them:
/// alphabetical, with `last` (state.json's last_project — the project that
/// was active at save) LAST, because open_workspace leaves the last-processed
/// root ACTIVE. Vanished roots filter out (the boot is_dir law).
pub fn open_roots(wb: &state::WorkbenchFile, last: Option<&Path>) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = wb
        .projects
        .keys()
        .filter(|k| Path::new(k).is_dir())
        .map(PathBuf::from)
        .collect();
    if roots.is_empty() {
        return roots;
    }
    let last = last.map(|l| l.to_string_lossy().into_owned()).unwrap_or_default();
    roots.sort();
    roots.sort_by_key(|r| r.to_string_lossy() == last); // stable: `last` rides last
    roots
}

/// GtkSourceView language id → chip text ("C#", "TS" — mockup labels).
/// Unmapped ids prettify ("some-lang" → "Some lang").
fn lang_label(id: &str) -> String {
    for (k, label) in [
        ("rust", "Rust"),
        ("c", "C"),
        ("cpp", "C++"),
        ("c-sharp", "C#"),
        ("typescript", "TS"),
        ("typescript-jsx", "TS"), // x.tsx maps to typescript-jsx here (measured)
        ("javascript", "JS"),
        ("js", "JS"),             // x.js/x.mjs map to id "js" here (measured)
        ("python", "Python"),
        ("python3", "Python"),
        ("go", "Go"),
        ("kotlin", "Kotlin"),
        ("swift", "Swift"),
        ("ruby", "Ruby"),
        ("php", "PHP"),
        ("java", "Java"),
        ("html", "HTML"),
        ("css", "CSS"),
        ("json", "JSON"),
        ("xml", "XML"),
        ("markdown", "MD"),
        ("yaml", "YAML"),
        ("shell", "Shell"),
        ("sh", "Shell"),
    ] {
        if k == id {
            return label.to_owned();
        }
    }
    let s = id.replace('-', " ");
    let head = s.chars().map(|c| c.len_utf8()).next().unwrap_or(0).min(s.len());
    if head > 0 {
        s[..head].to_uppercase() + &s[head..]
    } else {
        s
    }
}

/// Chip color rides css CLASSES (a Gtk4 Label has no style attribute) —
/// the seti palette the app already uses for its file badges. Unmapped ids →
/// muted. (`.alpaca-lc-*` rules live in app.css; each re-states :backdrop,
/// so an unfocused window keeps the chips tinted.)
fn lang_class(id: &str) -> &'static str {
    for (k, class) in [
        ("c-sharp", "alpaca-lc-blue"),
        ("c", "alpaca-lc-blue"),
        ("cpp", "alpaca-lc-purple"),
        ("typescript", "alpaca-lc-blue"),
        ("typescript-jsx", "alpaca-lc-blue"),
        ("javascript", "alpaca-lc-yellow"),
        ("js", "alpaca-lc-yellow"),
        ("python", "alpaca-lc-blue"),
        ("python3", "alpaca-lc-blue"),
        ("go", "alpaca-lc-blue"),
        ("rust", "alpaca-lc-grey"),
        ("kotlin", "alpaca-lc-orange"),
        ("swift", "alpaca-lc-orange"),
        ("ruby", "alpaca-lc-red"),
        ("php", "alpaca-lc-purple"),
        ("java", "alpaca-lc-red"),
        ("html", "alpaca-lc-blue"),
        ("css", "alpaca-lc-blue"),
        ("json", "alpaca-lc-yellow"),
        ("yaml", "alpaca-lc-grey"),
        ("shell", "alpaca-lc-grey"),
        ("sh", "alpaca-lc-grey"),
    ] {
        if k == id {
            return class;
        }
    }
    "alpaca-lc-muted"
}

fn llama_pixbuf() -> Option<gdk_pixbuf::Pixbuf> {
    let p = gdk_pixbuf::PixbufLoader::new();
    p.write(LLAMA).ok()?;
    p.close().ok()?;
    p.pixbuf()
}

/// The llama as a texture paintable — for_pixbuf is 4.20-deprecated upstream
/// but gdk4 0.11.5 ships no replacement binding (checked); the 80px bake
/// draws 80px 1:1. Targeted allow, not module-wide.
#[allow(deprecated)]
fn llama_texture() -> Option<gdk::Texture> {
    llama_pixbuf().map(|pix| gdk::Texture::for_pixbuf(&pix))
}

pub struct Launcher {
    /// card facts parallel the widgets' strip order (probe reads both)
    pub entries: Vec<WbEntry>,
    cards: gtk::Box,
    hint: gtk::Label,
}

#[relm4::component(pub)]
impl Component for Launcher {
    type CommandOutput = ();
    type Input = LauncherMsg;
    type Output = LauncherOutput;
    type Init = ();

    view! {
        #[root]
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_hexpand: true,
            set_vexpand: true,
            set_css_classes: &["alpaca-launch"],
        }
    }

    fn init(_: (), root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        // ——— the scrolling column: masthead → section rule → cards → big + ———
        let inner = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(14)
            .hexpand(true)
            .valign(gtk::Align::Start)
            .build();

        let mast = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(14)
            .build();
        if let Some(tex) = llama_texture() {
            let img = gtk::Image::from_paintable(Some(&tex));
            // pixel_size pins the draw (the 16px measure-floor law would
            // otherwise resample the 80px texture small); valign matches
            // the title's row
            img.set_pixel_size(80);
            img.set_valign(gtk::Align::Center);
            mast.append(&img);
        }
        let title = gtk::Label::builder()
            .label("Alpaca Code")
            .css_classes(["alpaca-launchtitle"])
            .build();
        title.set_valign(gtk::Align::Center);
        mast.append(&title);
        inner.append(&mast);

        let section = gtk::Label::builder()
            .label("Workbenches")
            .css_classes(["alpaca-launchsection"])
            .hexpand(true) // the hairline under it spans the column
            .xalign(0.0)
            .build();
        inner.append(&section);

        let cards = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        inner.append(&cards);

        // first-run: no files yet → nothing below but the big +
        let hint = gtk::Label::builder()
            .label("No workbenches saved — Ctrl+Shift+S saves the current session as one")
            .css_classes(["alpaca-hint"])
            .build();
        hint.set_visible(false);
        inner.append(&hint);

        let add = gtk::Button::builder()
            .has_frame(false)
            .css_classes(["alpaca-launchadd"])
            .build();
        // plus.svg rides the paintable path — Gsk re-tessellates per size
        // (sharp at any pixel size, the vector law); 24px = the big card's +.
        // Child set DIRECTLY (a wrapper Box's hexpand doesn't propagate
        // through a Button — the child gets the fill allocation and the
        // wrapper's own expand ignored): halign/valign Center on the child.
        if let Some(vec) = vector::icon("plus.svg") {
            let img = gtk::Image::from_paintable(Some(&vec));
            img.set_pixel_size(24);
            img.set_valign(gtk::Align::Center);
            img.set_halign(gtk::Align::Center);
            add.set_child(Some(&img));
        } else {
            // art loss (never on a shipped build): the text stand-in
            let l = gtk::Label::builder().label("+").build();
            l.set_halign(gtk::Align::Center);
            add.set_child(Some(&l));
        }
        add.set_tooltip_text(Some("Open a folder as a new workbench"));
        let snd = sender.clone();
        add.connect_clicked(move |_b| {
            let _ = snd.output(LauncherOutput::New);
        });
        inner.append(&add);

        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .build();
        scroller.set_child(Some(&inner));

        // ——— footer: the credit link, bottom-right, deliberately tiny ———
        let footer = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .build();
        let air = gtk::Box::builder().hexpand(true).build();
        footer.append(&air);
        let by = gtk::Button::builder()
            .has_frame(false)
            .label("By @D0m1s")
            .css_classes(["alpaca-launchby"])
            .build();
        by.set_tooltip_text(Some(BY_URI));
        by.connect_clicked(|_b| {
            let _ = gio::AppInfo::launch_default_for_uri(BY_URI, None::<&gio::AppLaunchContext>);
        });
        footer.append(&by);
        footer.set_margin_top(4);

        root.append(&scroller);
        root.append(&footer);

        let model = Launcher {
            entries: Vec::new(),
            cards,
            hint,
        };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            LauncherMsg::Build => self.build(sender),
        }
    }
}

impl Launcher {
    /// the one rebuild: clear + re-fill from a fresh disk scan (button clicks
    /// rebind with the new cards)
    fn build(&mut self, sender: ComponentSender<Self>) {
        self.entries = scan_workbenches(&state::workbenches_dir());
        while let Some(c) = self.cards.first_child() {
            self.cards.remove(&c);
        }
        for e in &self.entries {
            let btn = self.card(e, sender.clone());
            self.cards.append(&btn);
        }
        self.hint.set_visible(self.entries.is_empty());
    }

    fn card(&self, e: &WbEntry, sender: ComponentSender<Self>) -> gtk::Button {
        let btn = gtk::Button::builder()
            .has_frame(false)
            .css_classes(["alpaca-launchcard"])
            .build();
        let inner = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        let name = gtk::Label::builder()
            .label(&e.name)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["alpaca-launchname"])
            .build();
        inner.append(&name);
        if !e.projs.is_empty() {
            let tiles = gtk::Box::builder()
                .orientation(gtk::Orientation::Horizontal)
                .spacing(8)
                .build();
            for p in &e.projs {
                tiles.append(&self.tile(p));
            }
            inner.append(&tiles);
        }
        btn.set_child(Some(&inner));
        btn.set_tooltip_text(Some(&format!("Open workbench {}", e.name)));
        let snd = sender.clone();
        let name = e.name.clone();
        btn.connect_clicked(move |_b| {
            let _ = snd.output(LauncherOutput::Open(name.clone()));
        });
        btn
    }

    fn tile(&self, p: &Proj) -> gtk::Box {
        let tile = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(5)
            .hexpand(true) // tiles split the card width evenly
            .css_classes(["alpaca-launchtile"])
            .build();
        let name = gtk::Label::builder()
            .label(&p.name)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        tile.append(&name);
        if p.lang.is_some() || p.branch.is_some() {
            let row = gtk::Box::builder()
                .orientation(gtk::Orientation::Horizontal)
                .spacing(6)
                .build();
            if let Some(lang) = p.lang.as_deref() {
                let chip = gtk::Label::builder()
                    .label(lang_label(lang))
                    .css_classes(["alpaca-launchlang", lang_class(lang)])
                    .xalign(0.0)
                    .build();
                row.append(&chip);
            }
            let air = gtk::Box::builder().hexpand(true).build();
            row.append(&air);
            if let Some(branch) = p.branch.as_deref() {
                let strip = gtk::Box::builder()
                    .orientation(gtk::Orientation::Horizontal)
                    .spacing(4)
                    .build();
                if let Some(vec) = vector::icon("branch.svg") {
                    let img = gtk::Image::from_paintable(Some(&vec));
                    img.set_pixel_size(12); // art is 16-grid: 1:1 at the row's scale
                    strip.append(&img);
                }
                let lbl = gtk::Label::builder()
                    .label(branch)
                    .max_width_chars(18) // long branch names must not blow the tile
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .css_classes(["alpaca-launchbranch"])
                    .xalign(0.0)
                    .build();
                strip.append(&lbl);
                row.append(&strip);
            }
            tile.append(&row);
        }
        tile
    }

    // —— PROBE-ONLY surface (ALPACA_PROBE_LN; production never calls) ————

    /// drive card i through its REAL Button activation (emit_clicked → the
    /// connect_clicked closure → LauncherOutput — the full production chain)
    pub fn probe_click(&self, i: usize) {
        if let Some(btn) = child_widgets(&self.cards)
            .get(i)
            .and_then(|w| w.downcast_ref::<gtk::Button>())
        {
            btn.emit_clicked();
        }
    }

    /// the widget truth: (card name, [(proj name, chip text, branch text)])
    /// straight off the live widgets
    pub fn probe_texts(&self) -> Vec<(String, Vec<(String, String, String)>)> {
        let mut out = Vec::new();
        for c in child_widgets(&self.cards) {
            let Some(btn) = c.downcast_ref::<gtk::Button>() else {
                continue;
            };
            let Some(inner) = btn.child().and_downcast::<gtk::Box>() else {
                continue;
            };
            let Some(name) = inner.first_child().and_downcast::<gtk::Label>() else {
                continue;
            };
            let mut tiles: Vec<(String, String, String)> = Vec::new();
            if let Some(tilesbox) = name.next_sibling().and_downcast::<gtk::Box>() {
                for t in child_widgets(&tilesbox) {
                    let Some(tile) = t.downcast_ref::<gtk::Box>() else {
                        continue;
                    };
                    let pname = tile
                        .first_child()
                        .and_downcast::<gtk::Label>()
                        .map(|l| l.text().to_string())
                        .unwrap_or_default();
                    // the chip/branch row: the chip label sits DIRECTLY under
                    // the row, the branch label under the strip Box
                    let row = tile.first_child().and_then(|n| n.next_sibling());
                    let mut lang = String::new();
                    let mut branch = String::new();
                    for w in child_widgets_of(row) {
                        if let Some(l) = w.downcast_ref::<gtk::Label>() {
                            lang = l.text().to_string();
                        } else if let Some(strip) = w.downcast_ref::<gtk::Box>() {
                            branch = child_widgets(strip)
                                .iter()
                                .filter_map(|w| w.downcast_ref::<gtk::Label>().map(|l| l.text().to_string()))
                                .next_back()
                                .unwrap_or_default();
                        }
                    }
                    tiles.push((pname, lang, branch));
                }
            }
            out.push((name.text().to_string(), tiles));
        }
        out
    }
}

/// children in strip order (a Box's own child list walk)
fn child_widgets(parent: &gtk::Box) -> Vec<gtk::Widget> {
    let mut v = Vec::new();
    let mut c = parent.first_child();
    while let Some(w) = c {
        let next = w.next_sibling();
        v.push(w);
        c = next;
    }
    v
}

fn child_widgets_of(row: Option<gtk::Widget>) -> Vec<gtk::Widget> {
    row.and_downcast::<gtk::Box>()
        .map(|b| child_widgets(&b))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn testdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("alpaca-launch-{}", name));
        if d.exists() {
            std::fs::remove_dir_all(&d).unwrap();
        }
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(p: &Path, text: &str) {
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn scan_orders_by_mtime_and_skips_junk() {
        let d = testdir("scan-order");
        write(&d.join("a.json"), r#"{"name":"a","projects":{}}"#);
        write(&d.join("new.json"), r#"{"name":"new","projects":{}}"#);
        write(&d.join("c.json"), "not json at all");
        write(&d.join("e.txt"), r#"{"name":"e","projects":{}}"#);
        // mtimes: a.json old, new.json fresh — `new` must ride first
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
        let f = std::fs::File::options().read(true).open(d.join("a.json")).unwrap();
        f.set_times(std::fs::FileTimes::new().set_modified(old)).unwrap();
        drop(f);
        let got = scan_workbenches(&d);
        assert_eq!(got.len(), 2); // e.txt skipped (ext), c.json skipped (corrupt)
        assert_eq!(got[0].name, "new"); // most recent save first
        assert_eq!(got[1].name, "a");
    }

    #[test]
    fn scan_filters_vanished_roots() {
        let d = testdir("scan-vanished");
        let real = d.join("rproj");
        std::fs::create_dir_all(&real).unwrap();
        write(
            &d.join("w.json"),
            &format!(
                r#"{{"name":"w","projects":{{"{}":{{"language":"c-sharp","vpane":0,"hpane":0,"scroll":null,"expanded":[],"changes_closed":[]}},"gone":{}}}}}"#,
                real.to_str().unwrap(),
                r#"{"language":"c-sharp","vpane":0,"hpane":0,"scroll":null,"expanded":[],"changes_closed":[]}"#,
            ),
        );
        let got = scan_workbenches(&d);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].projs.len(), 1); // "gone" (no dir) filtered
        assert_eq!(got[0].projs[0].name, "rproj");
        assert_eq!(got[0].projs[0].lang.as_deref(), Some("c-sharp"));
        assert_eq!(got[0].projs[0].branch, None); // no .git → no branch
    }

    #[test]
    fn lang_label_map_and_fallback() {
        assert_eq!(lang_label("c-sharp"), "C#");
        assert_eq!(lang_label("typescript"), "TS");
        assert_eq!(lang_label("typescript-jsx"), "TS"); // x.tsx's real id
        assert_eq!(lang_label("js"), "JS"); // x.js/x.mjs's real id
        assert_eq!(lang_label("rust"), "Rust");
        assert_eq!(lang_label("sh"), "Shell");
        assert_eq!(lang_label("made-up"), "Made up");
        assert_eq!(lang_label(""), "");
    }

    #[test]
    fn lang_class_fallback_and_known() {
        assert_eq!(lang_class("c-sharp"), "alpaca-lc-blue");
        assert_eq!(lang_class("typescript-jsx"), "alpaca-lc-blue");
        assert_eq!(lang_class("js"), "alpaca-lc-yellow");
        assert_eq!(lang_class("rust"), "alpaca-lc-grey");
        assert_eq!(lang_class("who-knows"), "alpaca-lc-muted");
    }

    #[test]
    fn open_roots_alpha_with_last_project_last() {
        let d = testdir("roots");
        let a = d.join("alpha");
        let z = d.join("zulu");
        let g = d.join("gone");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&z).unwrap();
        let projects: std::collections::HashMap<String, state::ProjectSession> = [
            (a.to_string_lossy().into_owned(), state::ProjectSession::default()),
            (z.to_string_lossy().into_owned(), state::ProjectSession::default()),
            (g.to_string_lossy().into_owned(), state::ProjectSession::default()),
        ]
        .into();
        let wb = state::WorkbenchFile { name: "w".into(), projects };
        // alphabetical, vanished dropped, and `last` must ride LAST
        let got = open_roots(&wb, Some(Path::new(z.to_str().unwrap())));
        assert_eq!(got, vec![a.clone(), z.clone()]);
        // no last_project at all → plain alphabetical, dir order
        let got = open_roots(&wb, None);
        assert_eq!(got, vec![a.clone(), z.clone()]);
        // every root vanished → empty (the caller stays on the launcher)
        let projects: std::collections::HashMap<String, state::ProjectSession> =
            [(g.to_string_lossy().into_owned(), state::ProjectSession::default())].into();
        let wb = state::WorkbenchFile { name: "w".into(), projects };
        assert!(open_roots(&wb, None).is_empty());
    }
}