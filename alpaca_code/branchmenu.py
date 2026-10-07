# Status-bar branch pill (2026-10-06): flat button (icon + name + ▾) whose
# popover holds Switch Branch (hover → branch submenu on the right) and New
# Branch (entry → checkout -b from HEAD). Git flights copy the commit bar
# pattern: daemon thread, idle lands, the status row owns busy/err/ok. The
# pill hides outside a repo (the branch_of rule). Hover open/close rides two
# GLib timers so the pointer can cross from row to submenu without a close.
import threading
import alpaca_code.gi_env as ge
ge.require("Pango", ("1.0",))
ge.require("Gdk", ("4.0",))
ge.require("Gtk", ("4.0",))
from gi.repository import GLib, Gtk, Pango

from . import badges, gitstatus


class BranchMenu(Gtk.Button):
    def __init__(self):
        super().__init__()
        self.set_has_frame(False)
        self.set_css_classes(["alpaca-branchbtn"])
        self.root = None
        self.on_status = lambda kind, text: None   # wiring: shell.show_git_status
        self.allow = lambda: True                  # wiring: shell's not-_git_busy
        self._flight = False
        self._sub_to = 0                           # submenu close timer
        self._sub_open_to = 0                      # submenu open timer

        strip = Gtk.Box(spacing=3)             # git logo ↔ branch name ride tight (old pair)
        self.icon = Gtk.Image()
        px = badges.icon("branch.svg")
        if px:
            self.icon.set_from_paintable(px)
        self.label = Gtk.Label(label="", ellipsize=Pango.EllipsizeMode.MIDDLE)
        strip.append(self.icon); strip.append(self.label)
        self.set_child(strip)
        self.set_visible(False)
        self.connect("clicked", lambda b: self._open_menu())

        # main popover: page "menu" (the two rows) / page "new" (entry). TOP: it
        # must open above the bar; GTK flips if there is no room.
        self.menupage = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        self.newpage = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, margin_start=8,
                               margin_end=8, margin_top=6, margin_bottom=6)
        self.entry = Gtk.Entry()
        self.entry.set_placeholder_text("Branch Name")
        self.entry.set_css_classes(["alpaca-msg"])
        self.entry.set_size_request(-1, 30)
        self.entry.connect("activate", self._create)
        self.newpage.append(self.entry)

        switch_row = Gtk.Button()
        switch_row.set_has_frame(False)
        switch_row.set_css_classes(["alpaca-branchitem"])
        strip2 = Gtk.Box(spacing=8)
        ic = Gtk.Image()
        if px:
            ic.set_from_paintable(px)
        # xalign 0: GTK4 labels center by default — inside the hexpanded row boxes
        # that floated the text right (measured +→text gap 30px vs the switch row's 14)
        lbl = Gtk.Label(label="Switch Branch", xalign=0.0, hexpand=True)
        chev = Gtk.Image()
        cx = badges.icon("chevron.svg")
        if cx:
            chev.set_from_paintable(cx)
        strip2.append(ic); strip2.append(lbl); strip2.append(chev)
        switch_row.set_child(strip2)
        new_row = Gtk.Button()
        new_row.set_has_frame(False)
        new_row.set_css_classes(["alpaca-branchitem"])
        strip3 = Gtk.Box(spacing=8)
        pxi = Gtk.Image()
        npx = badges.icon("plus.svg")
        if npx:
            pxi.set_from_paintable(npx)
        strip3.append(pxi)
        strip3.append(Gtk.Label(label="New Branch", xalign=0.0, hexpand=True))
        new_row.set_child(strip3)
        new_row.connect("clicked", self._to_new_entry)
        self.menupage.append(switch_row); self.menupage.append(new_row)

        self.stack = Gtk.Stack()
        self.stack.set_vhomogeneous(False)
        self.stack.set_hhomogeneous(False)
        self.stack.add_named(self.menupage, "menu")
        self.stack.add_named(self.newpage, "new")
        self.stack.set_visible_child(self.menupage)

        # branch submenu: own popover anchored to the switch row, opening from
        # its right edge (flips left at the window edge). List is rebuilt fresh
        # every main-popover open. autohide OFF — the popup grab fires a
        # synthetic leave on the row while the pointer still sits on it and a
        # synthetic enter on hand-back: hover-open ping-pongs (flicker,
        # reported live). Non-grabbed popup keeps every event real.
        self._branch_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        scroll = Gtk.ScrolledWindow(max_content_height=240, propagate_natural_height=True,
                                    hscrollbar_policy=Gtk.PolicyType.NEVER,
                                    child=self._branch_box)
        self.sub = Gtk.Popover(child=scroll, position=Gtk.PositionType.RIGHT)
        self.sub.set_autohide(False)
        self.sub.set_parent(switch_row)

        self.pop = Gtk.Popover(child=self.stack, position=Gtk.PositionType.TOP)
        self.pop.set_parent(self)
        self.pop.connect("closed", self._on_closed)

        ctrl = Gtk.EventControllerMotion()
        ctrl.connect("enter", self._sub_enter_row)
        ctrl.connect("leave", self._sub_leave_row)
        switch_row.add_controller(ctrl)
        ctrl2 = Gtk.EventControllerMotion()
        ctrl2.connect("enter", self._sub_enter_list)
        ctrl2.connect("leave", self._sub_leave_list)
        scroll.add_controller(ctrl2)

    # ---- public (filetree only) ------------------------------------------------
    def update(self, branch: str | None) -> None:
        """Row repaint: branch name or None (outside a repo → the pill hides)."""
        self.set_visible(branch is not None)
        if branch:
            self.label.set_text(branch)

    # ---- popover show/hide ------------------------------------------------------
    def _open_menu(self):
        self._fill_branches()
        self.stack.set_visible_child(self.menupage)
        self.add_css_class("alpaca-open")        # clicked-pill state (user mockup)
        self.pop.popup()

    def _on_closed(self, *_a):
        self.remove_css_class("alpaca-open")
        self._cancel(self._sub_open_to); self._cancel(self._sub_to)
        self._sub_open_to = self._sub_to = 0
        self.sub.popdown()

    def _to_new_entry(self, *_a):
        self.stack.set_visible_child(self.newpage)
        # stale text from an abandoned attempt must go: this build select-alls
        # whenever a grab lands on an entry WITH text (probe: (0,11) on 'hello'),
        # so the single grab below would pre-select the old name. Clearing makes
        # every open grab land on an empty field.
        self.entry.set_text("")
        # ONE grab, directly (the flip maps the entry synchronously). Never hand
        # `grab_focus` to idle_add: it returns True so the idle re-arms forever,
        # and this build select-alls on every (re)grab — the next tick after each
        # keystroke re-selected the whole text (measured).
        self.entry.grab_focus()

    # ---- submenu hover (timers: the pointer crossing row→list must win) ---------
    def _sub_enter_row(self, *_a):
        self._cancel(self._sub_to); self._sub_to = 0
        if self._sub_open_to or self.sub.get_visible():
            return
        self._sub_open_to = GLib.timeout_add(80, self._open_sub)

    def _sub_leave_row(self, *_a):
        self._cancel(self._sub_open_to)
        self._sub_open_to = 0
        self._sched_close()

    def _sub_enter_list(self, *_a):
        self._cancel(self._sub_to); self._sub_to = 0

    def _sub_leave_list(self, *_a):
        self._sched_close()

    def _open_sub(self) -> bool:
        self._sub_open_to = 0
        self._fill_branches()
        self.sub.popup()
        return False                             # timeout_add: run once

    def _sched_close(self):
        self._cancel(self._sub_to)
        self._sub_to = GLib.timeout_add(250, self._close_sub)

    def _close_sub(self) -> bool:
        self._sub_to = 0
        self.sub.popdown()
        return False

    @staticmethod
    def _cancel(t: int):
        if t:
            GLib.source_remove(t)

    # ---- branch list -------------------------------------------------------------
    def _fill_branches(self):
        cur = gitstatus.branch_of(self.root)
        child = self._branch_box.get_first_child()
        while child is not None:
            nxt = child.get_next_sibling()
            self._branch_box.remove(child)
            child = nxt
        names = gitstatus.branches(self.root) or []
        if not names:                            # unborn HEAD: nothing to switch to
            lbl = Gtk.Label(label="No branches yet", xalign=0.5,
                            margin_top=4, margin_bottom=4,
                            margin_start=10, margin_end=10)   # rows' 10px inset, centered
            lbl.set_css_classes(["alpaca-hint"])
            self._branch_box.append(lbl)
            return
        for n in names:
            b = Gtk.Button()
            b.set_has_frame(False)
            b.set_css_classes(["alpaca-branchitem"] + (["alpaca-on"] if n == cur else []))
            b.set_child(Gtk.Label(label=("✓ " + n) if n == cur else n, xalign=0.0,
                                  hexpand=True))
            b.connect("clicked", lambda _b, name=n: self._pick(name))
            self._branch_box.append(b)

    # ---- git flights (commit-bar pattern: thread + idle land) --------------------
    def _pick(self, name: str):
        self._start(gitstatus.switch, name, "Switching…", "Switched ✓")

    def _create(self, *_a):
        self._start(gitstatus.create_switch, self.entry.get_text(),
                    "Creating…", "Branch created ✓")

    def _start(self, op, name: str, phase: str, okmsg: str):
        if self._flight or not self.allow():
            return
        root = self.root                         # snapshot: set_root mid-flight must not reroute it
        self._flight = True
        self.pop.popdown()                       # closed signal tidies the submenu too
        self.on_status("busy", phase)
        threading.Thread(
            target=lambda: GLib.idle_add(self._land, *op(root, name), okmsg),
            daemon=True, name="alpaca-branch").start()

    def _land(self, ok: bool, text: str, okmsg: str) -> bool:
        self._flight = False
        self.on_status("ok" if ok else "err", okmsg if ok else text)
        return False                             # idle_add: run once