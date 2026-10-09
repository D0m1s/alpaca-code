//! Status-bar branch pill + Switch/New popover — python branchmenu.py port.
//!
//! Pill is a flat button (branch icon + name) whose popover holds Switch
//! Branch and New Branch; git flights copy the commit-bar pattern (command
//! thread, land via update_cmd, the status row owns busy/err/ok). The pill
//! hides outside a repo (the branch_of rule).
//!
//! Hover timers are GENERATION COUNTERS: python `_cancel` was a silent no-op
//! on an already-fired GLib source, rust `SourceId::remove` PANICS on one —
//! handlers instead bump the generation a timer closure captured; a stale
//! timer fires and its input is discarded by the gen check.

// The module's whole API surface is consumed by GitPanel and app wiring
// (proven by the landed callers — the T4 sweep's build gate holds the line:
// no module allow survives, warnings resurface loudly).

use std::path::PathBuf;
use std::time::Duration;

use relm4::gtk::{self, glib, prelude::*};
use relm4::prelude::*;

use crate::vector;
use crate::gitstatus::{self, GitStatusKind};

#[derive(Debug)]
pub enum BranchMenuMsg {
    SetRoot(Option<PathBuf>),
    Update(Option<String>),
    /// wiring: gate flights (shell's not-_git_busy)
    Gate(bool),
    Pick(String),
    Create(String),
    SubOpen(u64),
    SubClose(u64),
    // relm4 routing additions: python called these methods straight from
    // closures; a relm4 closure can't reach the component model, so each one
    // became an input (additive — T6 consumes only SetRoot/Update/Gate).
    OpenMenu,
    Closed,
    SubEnterRow,
    SubLeaveRow,
    SubEnterList,
    SubLeaveList,
}

#[derive(Debug)]
pub enum BranchMenuOutput {
    Status(GitStatusKind, String),
}

#[derive(Debug)]
pub enum BranchCommand {
    Landed {
        ok: bool,
        text: String,
        okmsg: String,
    },
}

pub struct BranchMenu {
    root: Option<PathBuf>,
    gate: bool,
    flight: bool,
    open_gen: u64,
    close_gen: u64,
    open_armed: bool,
    label: gtk::Label,
    pop: gtk::Popover,
    sub: gtk::Popover,
    stack: gtk::Stack,
    menupage: gtk::Box,
    branch_box: gtk::Box,
}

#[relm4::component(pub)]
impl Component for BranchMenu {
    type CommandOutput = BranchCommand;
    type Input = BranchMenuMsg;
    type Output = BranchMenuOutput;
    type Init = ();

    view! {
        #[root]
        gtk::Button {
            set_has_frame: false,
            set_css_classes: &["alpaca-branchbtn"],
        }
    }

    fn init(_: (), root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        root.set_visible(false);

        // pill strip: git logo ↔ branch name ride tight (old pair)
        let strip = gtk::Box::builder().spacing(3).build();
        if let Some(ic) = vector::icon("branch.svg").map(|vec| gtk::Image::from_paintable(Some(&vec))) {
            strip.append(&ic);
        }
        let label = gtk::Label::builder()
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .build();
        strip.append(&label);
        root.set_child(Some(&strip));

        // main popover: page "menu" (the two rows) / page "new" (entry).
        // TOP: it must open above the bar; GTK flips if there is no room.
        let menupage = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        let newpage = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .margin_start(8)
            .margin_end(8)
            .margin_top(6)
            .margin_bottom(6)
            .build();
        let entry = gtk::Entry::builder()
            .placeholder_text("Branch Name")
            .css_classes(["alpaca-msg"])
            .build();
        entry.set_size_request(-1, 30);
        newpage.append(&entry);

        let switch_row = gtk::Button::builder()
            .has_frame(false)
            .css_classes(["alpaca-branchitem"])
            .build();
        let strip2 = gtk::Box::builder().spacing(8).build();
        if let Some(ic) = vector::icon("branch.svg").map(|vec| gtk::Image::from_paintable(Some(&vec))) {
            strip2.append(&ic);
        }
        // xalign 0: GTK4 labels center by default — inside the hexpanded row
        // boxes that floated the text right (measured gap 30px vs the row's 14)
        let lbl = gtk::Label::builder()
            .label("Switch Branch")
            .xalign(0.0)
            .hexpand(true)
            .build();
        strip2.append(&lbl);
        if let Some(chev) = vector::icon("chevron.svg").map(|vec| gtk::Image::from_paintable(Some(&vec))) {
            strip2.append(&chev);
        }
        switch_row.set_child(Some(&strip2));

        let new_row = gtk::Button::builder()
            .has_frame(false)
            .css_classes(["alpaca-branchitem"])
            .build();
        let strip3 = gtk::Box::builder().spacing(8).build();
        if let Some(pxi) = vector::icon("plus.svg").map(|vec| gtk::Image::from_paintable(Some(&vec))) {
            strip3.append(&pxi);
        }
        strip3.append(
            &gtk::Label::builder()
                .label("New Branch")
                .xalign(0.0)
                .hexpand(true)
                .build(),
        );
        new_row.set_child(Some(&strip3));
        menupage.append(&switch_row);
        menupage.append(&new_row);

        let stack = gtk::Stack::builder()
            .vhomogeneous(false)
            .hhomogeneous(false)
            .build();
        stack.add_named(&menupage, Some("menu"));
        stack.add_named(&newpage, Some("new"));
        stack.set_visible_child(&menupage);

        // branch submenu: own popover anchored to the switch row, opening from
        // its right edge (flips left at the window edge). List rebuilt fresh on
        // every open. autohide OFF — the popup grab fires a synthetic leave on
        // the row while the pointer still sits on it (hover-open ping-pong
        // flicker, reported live); a non-grabbed popup keeps every event real.
        let branch_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        let scroll = gtk::ScrolledWindow::builder()
            .max_content_height(240)
            .propagate_natural_height(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&branch_box)
            .build();
        let sub = gtk::Popover::builder()
            .child(&scroll)
            .position(gtk::PositionType::Right)
            .autohide(false)
            .build();
        sub.set_parent(&switch_row);

        let pop = gtk::Popover::builder()
            .child(&stack)
            .position(gtk::PositionType::Top)
            .build();
        pop.set_parent(&root);

        // hover controllers — closures can't reach the model, so the
        // enter/leave bookkeeping routes through inputs (gen counter lives in
        // the update arms)
        let ctrl = gtk::EventControllerMotion::new();
        let snd = sender.clone();
        ctrl.connect_enter(move |_, _, _| snd.input(BranchMenuMsg::SubEnterRow));
        let snd = sender.clone();
        ctrl.connect_leave(move |_| snd.input(BranchMenuMsg::SubLeaveRow));
        switch_row.add_controller(ctrl);
        let ctrl2 = gtk::EventControllerMotion::new();
        let snd = sender.clone();
        ctrl2.connect_enter(move |_, _, _| snd.input(BranchMenuMsg::SubEnterList));
        let snd = sender.clone();
        ctrl2.connect_leave(move |_| snd.input(BranchMenuMsg::SubLeaveList));
        scroll.add_controller(ctrl2);

        // signals
        let snd = sender.clone();
        root.connect_clicked(move |_| snd.input(BranchMenuMsg::OpenMenu));
        let snd = sender.clone();
        pop.connect_closed(move |_| snd.input(BranchMenuMsg::Closed));
        let snd = sender.clone();
        let e2 = entry.clone();
        entry.connect_activate(move |_e| {
            let text = e2.text().to_string();
            snd.input(BranchMenuMsg::Create(text));
        });
        // new-entry page flip is pure widget ops — no model reads, direct
        let nst = stack.clone();
        let nent = entry.clone();
        let npage = newpage.clone();
        new_row.connect_clicked(move |_| {
            nst.set_visible_child(&npage);
            nent.set_text("");
            nent.grab_focus();
        });

        let model = BranchMenu {
            root: None,
            gate: true,
            flight: false,
            open_gen: 0,
            close_gen: 0,
            open_armed: false,
            label: label.clone(),
            pop: pop.clone(),
            sub: sub.clone(),
            stack: stack.clone(),
            menupage: menupage.clone(),
            branch_box: branch_box.clone(),
        };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: BranchMenuMsg, sender: ComponentSender<Self>, root: &Self::Root) {
        match msg {
            BranchMenuMsg::SetRoot(r) => {
                // python latent-bug fix: the pill wired set_root nowhere, so
                // flights would run on root=None — here it's a real input
                self.root = r;
            }
            BranchMenuMsg::Update(b) => {
                // row repaint: branch name or None (outside a repo → hide)
                root.set_visible(b.is_some());
                if let Some(n) = b.as_deref() {
                    if !n.is_empty() {
                        self.label.set_text(n);
                    }
                }
            }
            BranchMenuMsg::Gate(g) => self.gate = g,
            BranchMenuMsg::Pick(name) => {
                self.start(gitstatus::switch, name, "Switching…", "Switched ✓", &sender)
            }
            BranchMenuMsg::Create(name) => {
                // text UNTRIMMED — create_switch trims internally
                self.start(
                    gitstatus::create_switch,
                    name,
                    "Creating…",
                    "Branch created ✓",
                    &sender,
                )
            }
            BranchMenuMsg::SubOpen(g) => {
                if g != self.open_gen {
                    return; // stale timer, canceled by a gen bump
                }
                self.open_armed = false;
                self.fill_branches(&sender);
                self.sub.popup();
            }
            BranchMenuMsg::SubClose(g) => {
                if g != self.close_gen {
                    return;
                }
                self.sub.popdown();
            }
            BranchMenuMsg::OpenMenu => {
                self.fill_branches(&sender);
                self.stack.set_visible_child(&self.menupage);
                root.add_css_class("alpaca-open"); // clicked-pill state (user mockup)
                self.pop.popup();
            }
            BranchMenuMsg::Closed => {
                root.remove_css_class("alpaca-open");
                // python's double _cancel: kill both armed timers at once
                self.open_gen += 1;
                self.close_gen += 1;
                self.open_armed = false;
                self.sub.popdown();
            }
            BranchMenuMsg::SubEnterRow => {
                self.close_gen += 1; // pointer came BACK: cancel a pending close
                if self.open_armed || self.sub.is_visible() {
                    return;
                }
                self.open_gen += 1;
                self.open_armed = true;
                let g = self.open_gen;
                let snd = sender.clone();
                glib::timeout_add(Duration::from_millis(80), move || {
                    snd.input(BranchMenuMsg::SubOpen(g));
                    glib::ControlFlow::Break
                });
            }
            BranchMenuMsg::SubLeaveRow => {
                self.open_gen += 1;
                self.open_armed = false;
                self.sched_close(&sender);
            }
            BranchMenuMsg::SubEnterList => {
                self.close_gen += 1; // pointer made it into the list
            }
            BranchMenuMsg::SubLeaveList => self.sched_close(&sender),
        }
    }

    fn update_cmd(
        &mut self,
        message: BranchCommand,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match message {
            BranchCommand::Landed { ok, text, okmsg } => {
                self.flight = false;
                let _ = sender.output(BranchMenuOutput::Status(
                    if ok { GitStatusKind::Ok } else { GitStatusKind::Err },
                    if ok { okmsg } else { text },
                ));
            }
        }
    }
}

impl BranchMenu {
    fn sched_close(&mut self, sender: &ComponentSender<Self>) {
        self.close_gen += 1;
        let g = self.close_gen;
        let snd = sender.clone();
        glib::timeout_add(Duration::from_millis(250), move || {
            snd.input(BranchMenuMsg::SubClose(g));
            glib::ControlFlow::Break
        });
    }

    fn start(
        &mut self,
        op: fn(&str, &str) -> (bool, String),
        name: String,
        phase: &str,
        okmsg: &'static str,
        sender: &ComponentSender<Self>,
    ) {
        if self.flight || !self.gate {
            return;
        }
        let Some(root) = self.root.clone() else {
            // not a repo — the pill hides anyway; python flew on None
            return;
        };
        // snapshot: set_root mid-flight must not reroute it
        self.flight = true;
        self.pop.popdown(); // closed signal tidies the submenu too
        let _ = sender.output(BranchMenuOutput::Status(
            GitStatusKind::Busy,
            phase.to_string(),
        ));
        sender.spawn_command(move |out| {
            let (ok, text) = op(&root.to_string_lossy(), &name);
            out.emit(BranchCommand::Landed {
                ok,
                text,
                okmsg: okmsg.to_string(),
            });
        });
    }

    /// Branch list, SYNC on the UI thread (python parity — the popup handler
    /// blocks on two git subprocess calls, same class as the refresh sync).
    fn fill_branches(&self, sender: &ComponentSender<Self>) {
        let cur = self
            .root
            .clone()
            .and_then(|r| gitstatus::branch_of(&r.to_string_lossy()));
        let mut child = self.branch_box.first_child();
        while let Some(c) = child {
            let nxt = c.next_sibling();
            self.branch_box.remove(&c);
            child = nxt;
        }
        let names = self
            .root
            .clone()
            .and_then(|r| gitstatus::branches(&r.to_string_lossy()))
            .unwrap_or_default();
        if names.is_empty() {
            // unborn HEAD: nothing to switch to
            let lbl = gtk::Label::builder()
                .label("No branches yet")
                .xalign(0.5)
                .margin_top(4)
                .margin_bottom(4)
                .margin_start(10)
                .margin_end(10) // rows' 10px inset, centered
                .css_classes(["alpaca-hint"])
                .build();
            self.branch_box.append(&lbl);
            return;
        }
        for n in names {
            let mut classes = Vec::with_capacity(2);
            classes.push("alpaca-branchitem");
            if cur.as_deref() == Some(n.as_str()) {
                classes.push("alpaca-on");
            }
            let b = gtk::Button::builder()
                .has_frame(false)
                .css_classes(classes)
                .build();
            // current branch row ticks (✓); plain label otherwise
            let lbl = gtk::Label::builder()
                .label(if cur.as_deref() == Some(n.as_str()) {
                    format!("✓ {n}")
                } else {
                    n.clone()
                })
                .xalign(0.0)
                .hexpand(true)
                .build();
            b.set_child(Some(&lbl));
            let snd = sender.clone();
            b.connect_clicked(move |_| snd.input(BranchMenuMsg::Pick(n.clone())));
            self.branch_box.append(&b);
        }
    }
}

// widget-layer behaviors (hover ping-pong, popover grab, real Pick → branch
// flip on a scratch repo) are the T8 probe's — nothing here is pure.