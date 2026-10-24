#![allow(deprecated)]

use std::cmp::Reverse;

use std::path::{Path, PathBuf};

use gdk_pixbuf::prelude::*;
use relm4::gtk::{self, gdk, gio, prelude::*};
use relm4::prelude::*;

use crate::git::gitstatus;
use crate::render::vector;
use crate::support::state;

const LLAMA: &[u8] = include_bytes!("../../assets/alpaca-80.png");
const BY_URI: &str = "https://github.com/D0m1s";

type PendingFn = Box<dyn FnOnce()>;
type CardTexts = Vec<(String, Vec<(String, String, String)>)>;

#[derive(Debug)]
pub enum LauncherMsg {
    Build,
    PlusClick,
}

#[derive(Debug)]
pub enum LauncherOutput {
    Open(String),
    New(String),
    Rename { stem: String, display: String },
    Delete(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Proj {
    pub name: String,
    pub lang: Option<String>,
    pub branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WbEntry {
    pub name: String,
    pub display: Option<String>,
    pub mtime: Option<std::time::SystemTime>,
    pub projs: Vec<Proj>,
}

pub fn scan_workbenches(dir: &Path) -> Vec<WbEntry> {
    let mut out: Vec<WbEntry> = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in rd.flatten() {
        let path = e.path();
        if !path.extension().is_some_and(|x| x == "json") {
            continue;
        }
        let mtime = e.metadata().and_then(|m| m.modified()).ok();
        let Some(wb) = state::load_workbench_from(&path) else {
            continue;
        };
        let mut projs: Vec<Proj> = wb
            .projects
            .into_iter()
            .filter(|(root, _)| Path::new(root).is_dir())
            .map(|(root, sess)| Proj {
                name: crate::ui::app::py_basename(&root),
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
        out.push(WbEntry {
            name,
            display: wb.display,
            mtime,
            projs,
        });
    }
    out.sort_by_key(|a| Reverse(a.mtime));
    out
}

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
    let last = last
        .map(|l| l.to_string_lossy().into_owned())
        .unwrap_or_default();
    roots.sort();
    roots.sort_by_key(|r| r.to_string_lossy() == last);
    roots
}

fn lang_label(id: &str) -> String {
    for (k, label) in [
        ("rust", "Rust"),
        ("c", "C"),
        ("cpp", "C++"),
        ("c-sharp", "C#"),
        ("typescript", "TS"),
        ("typescript-jsx", "TS"),
        ("javascript", "JS"),
        ("js", "JS"),
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
    let head = s
        .chars()
        .map(|c| c.len_utf8())
        .next()
        .unwrap_or(0)
        .min(s.len());
    if head > 0 {
        s[..head].to_uppercase() + &s[head..]
    } else {
        s
    }
}

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

#[allow(deprecated)]
fn llama_texture() -> Option<gdk::Texture> {
    llama_pixbuf().map(|pix| gdk::Texture::for_pixbuf(&pix))
}

type DoOk = std::rc::Rc<std::cell::RefCell<Option<Box<dyn FnOnce(String)>>>>;

const P_TITLE: &str = "Rename workbench";
const P_OK: &str = "Rename";
const D_TITLE: &str = "Delete this workbench?";
const D_OK: &str = "Delete";
const PLUS_TITLE: &str = "Name the new workbench";
const PLUS_OK: &str = "Create";

fn entry_pop(
    anchor: &impl IsA<gtk::Widget>,
    title: &str,
    ok_label: &str,
    start: &str,
    on_ok: impl FnOnce(String) + 'static,
) -> gtk::Popover {
    let pop = gtk::Popover::builder().autohide(true).build();
    let bx = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_start(10)
        .margin_end(10)
        .margin_top(8)
        .margin_bottom(8)
        .build();
    bx.append(
        &gtk::Label::builder()
            .label(title)
            .css_classes(["alpaca-wbpoptitle"])
            .build(),
    );
    let entry = gtk::Entry::builder()
        .text(start)
        .activates_default(true)
        .css_classes(["alpaca-msg"])
        .build();
    entry.set_size_request(220, 30);
    bx.append(&entry);
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .halign(gtk::Align::End)
        .build();
    let cancel = pop_cancel_btn(&pop);
    let ok = gtk::Button::builder()
        .label(ok_label)
        .css_classes(["alpaca-wbok"])
        .build();
    row.append(&cancel);
    row.append(&ok);
    bx.append(&row);
    pop.set_child(Some(&bx));

    let cell: DoOk = std::rc::Rc::new(std::cell::RefCell::new(Some(Box::new(on_ok))));
    let snd = cell.clone();
    let cpop = pop.clone();
    let cent = entry.clone();
    ok.connect_clicked(move |_| commit_pop(&cpop, &cent, &snd));
    let snd = cell;
    let cpop = pop.clone();
    let cent = entry.clone();
    entry.connect_activate(move |_| commit_pop(&cpop, &cent, &snd));
    pop.set_parent(anchor);
    pop.popup();
    entry.grab_focus();
    pop
}

fn confirm_pop(anchor: &impl IsA<gtk::Widget>, on_ok: impl FnOnce() + 'static) -> gtk::Popover {
    let pop = gtk::Popover::builder().autohide(true).build();
    let bx = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_start(10)
        .margin_end(10)
        .margin_top(8)
        .margin_bottom(8)
        .build();
    bx.append(
        &gtk::Label::builder()
            .label(D_TITLE)
            .css_classes(["alpaca-wbpoptitle"])
            .build(),
    );
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .halign(gtk::Align::End)
        .build();
    let cancel = pop_cancel_btn(&pop);
    let ok = gtk::Button::builder()
        .label(D_OK)
        .css_classes(["alpaca-wbdel"])
        .build();
    row.append(&cancel);
    row.append(&ok);
    bx.append(&row);
    pop.set_child(Some(&bx));
    let fire: std::rc::Rc<std::cell::RefCell<Option<PendingFn>>> =
        std::rc::Rc::new(std::cell::RefCell::new(Some(Box::new(on_ok))));
    let cpx = pop.clone();
    let fire2 = fire;
    ok.connect_clicked(move |_| {
        cpx.popdown();
        if let Some(g) = fire2.borrow_mut().take() {
            g();
        }
    });
    pop.set_parent(anchor);
    pop.popup();
    pop
}

fn pop_cancel_btn(pop: &gtk::Popover) -> gtk::Button {
    let cancel = gtk::Button::builder()
        .label("Cancel")
        .css_classes(["alpaca-wbcancel"])
        .build();
    let cpc = pop.clone();
    cancel.connect_clicked(move |_| cpc.popdown());
    cancel
}

fn commit_pop(pop: &gtk::Popover, entry: &gtk::Entry, cell: &DoOk) {
    pop.popdown();
    let text = entry.text().trim().to_string();
    if let Some(f) = cell.borrow_mut().take() {
        f(text);
    }
}

fn pop_entry(pop: &gtk::Popover) -> Option<gtk::Entry> {
    let bx = pop.child()?.downcast::<gtk::Box>().ok()?;
    child_widgets(&bx)
        .into_iter()
        .find_map(|w| w.downcast::<gtk::Entry>().ok())
}

fn pop_ok_btn(pop: &gtk::Popover) -> Option<gtk::Button> {
    let bx = pop.child()?.downcast::<gtk::Box>().ok()?;
    child_widgets(&bx)
        .into_iter()
        .find_map(|w| w.downcast::<gtk::Box>().ok())
        .and_then(|row| {
            child_widgets(&row)
                .into_iter()
                .filter_map(|w| w.downcast::<gtk::Button>().ok())
                .next_back()
        })
}

fn popover_open_of(bx: &gtk::Box) -> bool {
    child_widgets(bx)
        .into_iter()
        .any(|w| w.downcast::<gtk::Popover>().is_ok_and(|p| p.is_visible()))
}

fn icon_box(name: &str, tooltip: &str) -> gtk::Box {
    let bx = gtk::Box::builder()
        .css_classes(["alpaca-launchicon"])
        .tooltip_text(tooltip)
        .build();
    if let Some(vec) = vector::icon(name) {
        let img = gtk::Image::from_paintable(Some(&vec));
        img.set_pixel_size(14);
        img.set_valign(gtk::Align::Center);
        bx.append(&img);
    } else {
        bx.append(&gtk::Label::builder().label("?").build());
    }
    bx
}

fn wire_click(bx: &gtk::Box, on_click: impl Fn() + 'static) {
    let click = gtk::GestureClick::new();
    click.connect_pressed(move |g, _n, _x, _y| {
        if let Some(seq) = g.current_sequence() {
            gtk::prelude::GestureExt::set_sequence_state(g, &seq, gtk::EventSequenceState::Claimed);
        }
        on_click();
    });
    bx.add_controller(click);
}

pub struct Launcher {
    pub entries: Vec<WbEntry>,
    cards: gtk::Box,
    hint: gtk::Label,
    add: gtk::Button,
    plus_pop: Option<gtk::Popover>,
    sender: ComponentSender<Self>,
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
            .hexpand(true)
            .xalign(0.0)
            .build();
        inner.append(&section);

        let cards = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        inner.append(&cards);

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
        if let Some(vec) = vector::icon("plus.svg") {
            let img = gtk::Image::from_paintable(Some(&vec));
            img.set_pixel_size(24);
            img.set_valign(gtk::Align::Center);
            img.set_halign(gtk::Align::Center);
            add.set_child(Some(&img));
        } else {
            let l = gtk::Label::builder().label("+").build();
            l.set_halign(gtk::Align::Center);
            add.set_child(Some(&l));
        }
        add.set_tooltip_text(Some("Open a folder as a new workbench"));
        let snd = sender.clone();
        add.connect_clicked(move |_b| {
            snd.input(LauncherMsg::PlusClick);
        });
        inner.append(&add);

        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .build();
        scroller.set_child(Some(&inner));

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
            add: add.clone(),
            plus_pop: None,
            sender: sender.clone(),
        };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            LauncherMsg::Build => self.build(sender),
            LauncherMsg::PlusClick => self.open_plus(&sender),
        }
    }
}

impl Launcher {
    fn open_plus(&mut self, sender: &ComponentSender<Self>) {
        if let Some(old) = self.plus_pop.take() {
            old.unparent();
        }
        let snd = sender.clone();
        let pop = entry_pop(&self.add, PLUS_TITLE, PLUS_OK, "", move |name| {
            let _ = snd.output(LauncherOutput::New(name));
        });
        self.plus_pop = Some(pop);
    }
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
        let label = e.display.as_deref().unwrap_or(&e.name);
        let stem = e.name.clone();
        let disp = label.to_string();
        let name = gtk::Label::builder()
            .label(label)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["alpaca-launchname"])
            .build();
        name.set_hexpand(true);
        let pencil = icon_box("pencil.svg", "Rename workbench");
        let trash = icon_box("trash.svg", "Delete workbench");
        pencil.set_visible(false);
        trash.set_visible(false);
        let pencil_send = sender.clone();
        let pencil_stem = stem.clone();
        let pencil_anchor = pencil.clone();
        wire_click(&pencil, move || {
            let anchor = pencil_anchor.clone();
            let snd = pencil_send.clone();
            let stem = pencil_stem.clone();
            entry_pop(&anchor, P_TITLE, P_OK, &disp, move |display| {
                if !display.is_empty() {
                    let _ = snd.output(LauncherOutput::Rename { stem, display });
                }
            });
        });
        let trash_send = sender.clone();
        let trash_stem = stem.clone();
        let trash_anchor = trash.clone();
        wire_click(&trash, move || {
            let anchor = trash_anchor.clone();
            let snd = trash_send.clone();
            let stem = trash_stem.clone();
            confirm_pop(&anchor, move || {
                let _ = snd.output(LauncherOutput::Delete(stem));
            });
        });
        pencil.set_visible(false);
        trash.set_visible(false);
        let namerow = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .build();
        namerow.append(&name);
        namerow.append(&pencil);
        namerow.append(&trash);
        inner.append(&namerow);
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
        let pencil_v = pencil.clone();
        let trash_v = trash.clone();
        let motion = gtk::EventControllerMotion::new();
        motion.connect_enter(move |_c, _x, _y| {
            pencil_v.set_visible(true);
            trash_v.set_visible(true);
        });
        let pencil_u = pencil.clone();
        let trash_u = trash.clone();
        motion.connect_leave(move |_c| {
            if !popover_open_of(&pencil_u) && !popover_open_of(&trash_u) {
                pencil_u.set_visible(false);
                trash_u.set_visible(false);
            }
        });
        btn.add_controller(motion);
        let snd = sender.clone();
        let name_stem = e.name.clone();
        btn.connect_clicked(move |_b| {
            let _ = snd.output(LauncherOutput::Open(name_stem.clone()));
        });
        btn
    }

    fn tile(&self, p: &Proj) -> gtk::Box {
        let tile = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(5)
            .hexpand(true)
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
                    img.set_pixel_size(12);
                    strip.append(&img);
                }
                let lbl = gtk::Label::builder()
                    .label(branch)
                    .max_width_chars(18)
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

    pub fn probe_click(&self, i: usize) {
        if let Some(btn) = child_widgets(&self.cards)
            .get(i)
            .and_then(|w| w.downcast_ref::<gtk::Button>())
        {
            btn.emit_clicked();
        }
    }

    pub fn probe_texts(&self) -> CardTexts {
        let mut out = Vec::new();
        for c in child_widgets(&self.cards) {
            let Some(btn) = c.downcast_ref::<gtk::Button>() else {
                continue;
            };
            let Some(inner) = btn.child().and_downcast::<gtk::Box>() else {
                continue;
            };
            let Some(row) = inner.first_child().and_downcast::<gtk::Box>() else {
                continue;
            };
            let Some(name) = row.first_child().and_downcast::<gtk::Label>() else {
                continue;
            };
            let mut tiles: Vec<(String, String, String)> = Vec::new();
            if let Some(tilesbox) = row.next_sibling().and_downcast::<gtk::Box>() {
                for t in child_widgets(&tilesbox) {
                    let Some(tile) = t.downcast_ref::<gtk::Box>() else {
                        continue;
                    };
                    let pname = tile
                        .first_child()
                        .and_downcast::<gtk::Label>()
                        .map(|l| l.text().to_string())
                        .unwrap_or_default();
                    let row = tile.first_child().and_then(|n| n.next_sibling());
                    let mut lang = String::new();
                    let mut branch = String::new();
                    for w in child_widgets_of(row) {
                        if let Some(l) = w.downcast_ref::<gtk::Label>() {
                            lang = l.text().to_string();
                        } else if let Some(strip) = w.downcast_ref::<gtk::Box>() {
                            branch = child_widgets(strip)
                                .iter()
                                .filter_map(|w| {
                                    w.downcast_ref::<gtk::Label>().map(|l| l.text().to_string())
                                })
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

    pub fn card_icons(&self) -> Vec<(gtk::Box, gtk::Box)> {
        child_widgets(&self.cards)
            .into_iter()
            .filter_map(|c| {
                let btn = c.downcast_ref::<gtk::Button>()?;
                let child = btn.child()?;
                let inner = child.downcast_ref::<gtk::Box>()?;
                let first = inner.first_child()?;
                let row = first.downcast_ref::<gtk::Box>()?;
                let mut it = child_widgets(row)
                    .into_iter()
                    .filter_map(|w| w.downcast::<gtk::Box>().ok());
                let pencil = it.next()?;
                let trash = it.next()?;
                Some((pencil, trash))
            })
            .collect()
    }

    pub fn probe_trash(&self, i: usize) -> bool {
        let icons = self.card_icons();
        let Some((pencil, trash)) = icons.get(i).cloned() else {
            return false;
        };
        pencil.set_visible(true);
        trash.set_visible(true);
        let Some(e) = self.entries.get(i).cloned() else {
            return false;
        };
        let snd = self.sender.clone();
        let stem = e.name.clone();
        let pop = confirm_pop(&trash, move || {
            let _ = snd.output(LauncherOutput::Delete(stem));
        });
        pop.is_visible()
    }
    pub fn probe_pencil(&self, i: usize) -> Option<gtk::Entry> {
        let icons = self.card_icons();
        let (pencil, trash) = icons.get(i).cloned()?;
        pencil.set_visible(true);
        trash.set_visible(true);
        let e = self.entries.get(i)?.clone();
        let snd = self.sender.clone();
        let stem = e.name.clone();
        let pop = entry_pop(
            &pencil,
            P_TITLE,
            P_OK,
            &e.display.clone().unwrap_or(e.name),
            move |display| {
                if !display.is_empty() {
                    let _ = snd.output(LauncherOutput::Rename { stem, display });
                }
            },
        );
        pop_entry(&pop)
    }

    pub fn probe_pencil_entry(&self, i: usize) -> Option<gtk::Entry> {
        let icons = self.card_icons();
        let (pencil, _) = icons.get(i)?;
        let pop = child_widgets(pencil)
            .into_iter()
            .find_map(|w| w.downcast::<gtk::Popover>().ok())?;
        pop_entry(&pop)
    }

    pub fn probe_delete_ok(&self, i: usize) -> bool {
        let icons = self.card_icons();
        let Some((_, trash)) = icons.get(i).cloned() else {
            return false;
        };
        let Some(pop) = child_widgets(&trash)
            .into_iter()
            .find_map(|w| w.downcast::<gtk::Popover>().ok())
        else {
            return false;
        };
        pop_ok_btn(&pop).is_some_and(|b| {
            b.emit_clicked();
            true
        })
    }

    pub fn probe_plus_entry(&self) -> Option<gtk::Entry> {
        self.plus_pop.as_ref().and_then(pop_entry)
    }
}

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
    fn scan_carries_display_and_falls_back() {
        let d = testdir("scan-display");
        write(
            &d.join("a.json"),
            r#"{"name":"a","display":"My Backend","projects":{}}"#,
        );
        write(&d.join("b.json"), r#"{"name":"b","projects":{}}"#);
        let got = scan_workbenches(&d);
        let a = got.iter().find(|e| e.name == "a").unwrap();
        let b = got.iter().find(|e| e.name == "b").unwrap();
        assert_eq!(a.display.as_deref(), Some("My Backend"));
        assert_eq!(b.display, None);
    }

    #[test]
    fn scan_orders_by_mtime_and_skips_junk() {
        let d = testdir("scan-order");
        write(&d.join("a.json"), r#"{"name":"a","projects":{}}"#);
        write(&d.join("new.json"), r#"{"name":"new","projects":{}}"#);
        write(&d.join("c.json"), "not json at all");
        write(&d.join("e.txt"), r#"{"name":"e","projects":{}}"#);
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
        let f = std::fs::File::options()
            .read(true)
            .open(d.join("a.json"))
            .unwrap();
        f.set_times(std::fs::FileTimes::new().set_modified(old))
            .unwrap();
        drop(f);
        let got = scan_workbenches(&d);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].name, "new");
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
        assert_eq!(got[0].projs.len(), 1);
        assert_eq!(got[0].projs[0].name, "rproj");
        assert_eq!(got[0].projs[0].lang.as_deref(), Some("c-sharp"));
        assert_eq!(got[0].projs[0].branch, None);
    }

    #[test]
    fn lang_label_map_and_fallback() {
        assert_eq!(lang_label("c-sharp"), "C#");
        assert_eq!(lang_label("typescript"), "TS");
        assert_eq!(lang_label("typescript-jsx"), "TS");
        assert_eq!(lang_label("js"), "JS");
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
            (
                a.to_string_lossy().into_owned(),
                state::ProjectSession::default(),
            ),
            (
                z.to_string_lossy().into_owned(),
                state::ProjectSession::default(),
            ),
            (
                g.to_string_lossy().into_owned(),
                state::ProjectSession::default(),
            ),
        ]
        .into();
        let wb = state::WorkbenchFile {
            name: "w".into(),
            display: None,
            projects,
        };
        let got = open_roots(&wb, Some(Path::new(z.to_str().unwrap())));
        assert_eq!(got, vec![a.clone(), z.clone()]);
        let got = open_roots(&wb, None);
        assert_eq!(got, vec![a.clone(), z.clone()]);
        let projects: std::collections::HashMap<String, state::ProjectSession> = [(
            g.to_string_lossy().into_owned(),
            state::ProjectSession::default(),
        )]
        .into();
        let wb = state::WorkbenchFile {
            name: "w".into(),
            display: None,
            projects,
        };
        assert!(open_roots(&wb, None).is_empty());
    }
}
