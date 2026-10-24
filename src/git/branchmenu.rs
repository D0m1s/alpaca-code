

use std::path::PathBuf;
use std::time::Duration;

use relm4::gtk::{self, glib, prelude::*};
use relm4::prelude::*;

use crate::render::vector;
use crate::git::gitstatus::{self, GitStatusKind};

#[derive(Debug)]
pub enum BranchMenuMsg {
    SetRoot(Option<PathBuf>),
    Update(Option<String>),
        Gate(bool),
    Pick(String),
    Create(String),
    Pull,
    SubOpen(u64),
    SubClose(u64),
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
                            Branches {
        root: PathBuf,
        cur: Option<String>,
        names: Vec<String>,
        popup_sub: bool,
        open_gen: u64,
    },
}

pub struct BranchMenu {
    root: Option<PathBuf>,
    gate: bool,
    flight: bool,
        chain_pull: bool,
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

                let strip = gtk::Box::builder().spacing(3).build();
        if let Some(ic) = vector::icon("branch.svg").map(|vec| gtk::Image::from_paintable(Some(&vec))) {
            strip.append(&ic);
        }
        let label = gtk::Label::builder()
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .build();
        strip.append(&label);
        root.set_child(Some(&strip));

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

                let pull_row = gtk::Button::builder()
            .has_frame(false)
            .css_classes(["alpaca-branchitem"])
            .build();
        let strip4 = gtk::Box::builder().spacing(8).build();
        if let Some(px) = vector::icon("chevron-down.svg").map(|vec| gtk::Image::from_paintable(Some(&vec))) {
            strip4.append(&px);
        }
        strip4.append(
            &gtk::Label::builder()
                .label("Pull")
                .xalign(0.0)
                .hexpand(true)
                .build(),
        );
        pull_row.set_child(Some(&strip4));
        menupage.append(&pull_row);
        menupage.append(&new_row);

        let stack = gtk::Stack::builder()
            .vhomogeneous(false)
            .hhomogeneous(false)
            .build();
        stack.add_named(&menupage, Some("menu"));
        stack.add_named(&newpage, Some("new"));
        stack.set_visible_child(&menupage);

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

                let snd = sender.clone();
        root.connect_clicked(move |_| snd.input(BranchMenuMsg::OpenMenu));
        let snd = sender.clone();
        pop.connect_closed(move |_| {
                                    let _ = snd.input_sender().send(BranchMenuMsg::Closed);
        });
        let snd = sender.clone();
        let e2 = entry.clone();
        entry.connect_activate(move |_e| {
            let text = e2.text().to_string();
            snd.input(BranchMenuMsg::Create(text));
        });
                let nst = stack.clone();
        let nent = entry.clone();
        let npage = newpage.clone();
        new_row.connect_clicked(move |_| {
            nst.set_visible_child(&npage);
            nent.set_text("");
            nent.grab_focus();
        });
        let snd = sender.clone();
        pull_row.connect_clicked(move |_| snd.input(BranchMenuMsg::Pull));

        let model = BranchMenu {
            root: None,
            gate: true,
            flight: false,
            chain_pull: false,
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
                                                self.root = r;
            }
            BranchMenuMsg::Update(b) => {
                                root.set_visible(b.is_some());
                if let Some(n) = b.as_deref()
                    && !n.is_empty() {
                        self.label.set_text(n);
                    }
            }
            BranchMenuMsg::Gate(g) => self.gate = g,
            BranchMenuMsg::Pick(name) => {
                                self.chain_pull = true;
                self.start(gitstatus::switch, name, "Switching…", "Switched ✓", &sender)
            }
            BranchMenuMsg::Create(name) => {
                                self.start(
                    gitstatus::create_switch,
                    name,
                    "Creating…",
                    "Branch created ✓",
                    &sender,
                )
            }
            BranchMenuMsg::Pull => {
                                self.start(gitstatus::pull, String::new(), "Pulling…", "Pulled ✓", &sender)
            }
            BranchMenuMsg::SubOpen(g) => {
                if g != self.open_gen {
                    return; 
                }
                self.open_armed = false;
                self.list_branches(&sender, true);
            }
            BranchMenuMsg::SubClose(g) => {
                if g != self.close_gen {
                    return;
                }
                self.sub.popdown();
            }
            BranchMenuMsg::OpenMenu => {
                self.list_branches(&sender, false);
                self.stack.set_visible_child(&self.menupage);
                root.add_css_class("alpaca-open"); 
                self.pop.popup();
            }
            BranchMenuMsg::Closed => {
                root.remove_css_class("alpaca-open");
                                self.open_gen += 1;
                self.close_gen += 1;
                self.open_armed = false;
                self.sub.popdown();
            }
            BranchMenuMsg::SubEnterRow => {
                self.close_gen += 1; 
                if self.open_armed || self.sub.is_visible() {
                    return;
                }
                self.open_gen += 1;
                self.open_armed = true;
                let g = self.open_gen;
                let snd = sender.clone();
                glib::timeout_add(Duration::from_millis(80), move || {
                                                            if snd.input_sender().send(BranchMenuMsg::SubOpen(g)).is_err() {
                        return glib::ControlFlow::Break;
                    }
                    glib::ControlFlow::Break
                });
            }
            BranchMenuMsg::SubLeaveRow => {
                self.open_gen += 1;
                self.open_armed = false;
                self.sched_close(&sender);
            }
            BranchMenuMsg::SubEnterList => {
                self.close_gen += 1; 
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
                                                                if self.chain_pull {
                    self.chain_pull = false;
                    if ok {
                        self.start(
                            gitstatus::pull,
                            String::new(),
                            "Pulling…",
                            "Pulled ✓",
                            &sender,
                        );
                        return;
                    }
                }
                let _ = sender.output(BranchMenuOutput::Status(
                    if ok { GitStatusKind::Ok } else { GitStatusKind::Err },
                    if ok { okmsg } else { text },
                ));
            }
            BranchCommand::Branches { root, cur, names, popup_sub, open_gen } => {
                if self.root.as_ref() != Some(&root) {
                    return; 
                }
                                                let do_pop = popup_sub
                    && self.open_gen == open_gen
                    && !self.sub.is_visible();
                self.paint_branches(&sender, cur.as_deref(), &names, do_pop);
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
                                    if snd.input_sender().send(BranchMenuMsg::SubClose(g)).is_err() {
                return glib::ControlFlow::Break;
            }
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
                        return;
        };
                self.flight = true;
        self.pop.popdown(); 
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

                    fn list_branches(&self, sender: &ComponentSender<Self>, popup_sub: bool) {
        let Some(root) = self.root.clone() else {
                        self.paint_branches(sender, None, &[], popup_sub);
            return;
        };
        let open_gen = self.open_gen; 
        sender.spawn_command(move |out| {
            let cur = gitstatus::branch_of(&root.to_string_lossy());
            let names = gitstatus::branches(&root.to_string_lossy()).unwrap_or_default();
            out.emit(BranchCommand::Branches { root, cur, names, popup_sub, open_gen });
        });
    }

                fn paint_branches(
        &self,
        sender: &ComponentSender<Self>,
        cur: Option<&str>,
        names: &[String],
        popup_sub: bool,
    ) {
        let mut child = self.branch_box.first_child();
        while let Some(c) = child {
            let nxt = c.next_sibling();
            self.branch_box.remove(&c);
            child = nxt;
        }
        if names.is_empty() {
                        let lbl = gtk::Label::builder()
                .label("No branches yet")
                .xalign(0.5)
                .margin_top(4)
                .margin_bottom(4)
                .margin_start(10)
                .margin_end(10) 
                .css_classes(["alpaca-hint"])
                .build();
            self.branch_box.append(&lbl);
            if popup_sub {
                self.sub.popup();
            }
            return;
        }
        for n in names {
            let n = n.clone(); 
            let mut classes = Vec::with_capacity(2);
            classes.push("alpaca-branchitem");
            if cur == Some(n.as_str()) {
                classes.push("alpaca-on");
            }
            let b = gtk::Button::builder()
                .has_frame(false)
                .css_classes(classes)
                .build();
                        let lbl = gtk::Label::builder()
                .label(if cur == Some(n.as_str()) {
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
        if popup_sub {
            self.sub.popup();
        }
    }
}

