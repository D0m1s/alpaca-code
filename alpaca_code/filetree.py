# Right panel: search + lazily expanded file tree + status row (branch, file count).
#
# Tree invariant (measured on this build): never mutate the store under an
# EXPANDED row — the child fill that used to run inside row-expanded made the
# view collapse itself (folders "couldn't be opened"). Children are loaded from
# disk on every activation-expand of a collapsed row, before expand_row fires.
# Open state lives in _open (signal transitions are truthful; view row_expanded()
# readback is not — deprecated+stale after mutations).
import os
import threading
import alpaca_code.gi_env as ge
ge.require("Gtk", ("4.0",))
ge.require("Gdk", ("4.0",))
ge.require("GdkPixbuf", ("2.0",))
ge.require("Pango", ("1.0",))
ge.require("Graphene", ("1.0",))
from gi.repository import GdkPixbuf, Gdk, Gio, GLib, Gtk, Pango, Graphene

from . import badges
from .branchmenu import BranchMenu
from .treehover import HoverTree
from .gitview import ChangesView

SKIP_DIRS = {"node_modules", ".git", "obj", "bin", "vendor", "__pycache__"}

EXT_ICONS = {  # ext → themed mime icon (Papirus/Breeze names; anything else falls back)
    "py": "text-x-python", "json": "application-json", "md": "text-x-markdown",
}

def icon_of(name: str, is_dir: bool) -> str:
    if is_dir:
        return "folder-symbolic"
    ext = os.path.splitext(name)[1][1:].lower()
    return EXT_ICONS.get(ext, "text-x-generic-symbolic")

def scan_dir(path: str) -> list[tuple[str, bool]]:
    out = []
    try:
        with os.scandir(path) as it:
            for e in it:
                if e.name in SKIP_DIRS:
                    continue
                try:
                    out.append((e.name, e.is_dir(follow_symlinks=False)))
                except OSError:
                    pass
    except OSError:
        pass
    return sorted(out, key=lambda t: (not t[1], t[0].lower()))

def scan_project(root: str, needle: str, cap: int = 500) -> list[str]:
    """Full walk, skip-list honored; filename-substring matches as sorted relpaths."""
    m = []
    for base, dirs, files in os.walk(root):
        dirs[:] = sorted(d for d in dirs if d not in SKIP_DIRS)
        for f in sorted(files):
            rel = os.path.relpath(os.path.join(base, f), root)
            if needle.lower() in os.path.basename(rel).lower():
                m.append(rel)
                if len(m) >= cap:
                    return sorted(m)
    return sorted(m)  # count calls use big cap; ponytail: full walk; index/cache if trees get huge

class FileBrowser(Gtk.Box):
    def __init__(self):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.set_css_classes(["alpaca-card"])
        self.set_overflow(Gtk.Overflow.HIDDEN)   # clip children to the card's rounded corners
        self.on_open = lambda path: None
        self.root = None
        self._monitors: list[Gio.FileMonitor] = []
        self._head_mon: Gio.FileMonitor | None = None
        self._monitored: set[str] = set()   # dirs with a live monitor (cap)
        self._open: set[str] = set()        # currently expanded abs paths
        self._saved_tree: tuple[str, set[str], float] | None = None
        self._git_busy = False               # a commit/push flight owns the status row
        self._pulse_id = 0
        self._probe_busy = False             # a live-probe git call is in flight
        # Live git probe (2026-10-05 spec): every 2s the porcelain status runs in a
        # worker thread and lands on the UI thread (`timeout_add`, never
        # `timeout_add_seconds` — never fires on this build). Covers agent edits
        # anywhere in the tree — dir monitors only see dirs we've opened. Skipped
        # while a flight owns the row or a probe/commit is busy.
        self.on_git_changed = lambda rows: None   # window → live diff-tab renewal
        GLib.timeout_add(2000, self._git_tick)
        # ^ (root, open abs paths, scroll) — the real tree's sheet lives here while
        #   search results displace it; restored on clear, NOT persisted to disk

        head = Gtk.Box(spacing=4, margin_start=14, margin_end=10, margin_top=8, margin_bottom=6)
        head.append(Gtk.Label(label="File Browser", xalign=0.0, hexpand=True,
                              css_classes=["alpaca-panel-title"]))
        self.append(head)

        self.entry = Gtk.SearchEntry()
        self.entry.set_placeholder_text("Search files…")
        self.entry.set_css_classes(["alpaca-search"])
        # 30px floor: the entry renders ~36 (Breeze searchentry min, swept to
        # the floor in main.py CSS) — the request only keeps it honest below that
        self.entry.set_size_request(-1, 30)
        self.entry.set_hexpand(False)
        self.entry.set_vexpand(False)
        self.entry.set_margin_start(12)
        self.entry.set_margin_end(12)
        self.entry.set_margin_bottom(6)
        self.entry.connect("search-changed", self._on_search)
        self.entry.set_halign(Gtk.Align.FILL)
        self.append(self.entry)

        # WORKSPACE / CHANGES mode tabs (spec §1): text-only section-label type.
        # has_frame(False) + the transparent .alpaca-tabbtn rule keep them bare
        # text; _set_mode moves the `alpaca-on` class as the selection. Full
        # width: the ref's hairline under this strip is the box's own
        # border-bottom — margins sit OUTSIDE the border box (measured
        # breadcrumb gotcha), so the inset is padding and margin_top only.
        btns = Gtk.Box(spacing=10, margin_top=6, hexpand=True)
        btns.set_css_classes(["alpaca-modetabs"])
        self.ws_btn = Gtk.Button(label="WORKSPACE")
        self.ch_btn = Gtk.Button(label="CHANGES")
        for b in (self.ws_btn, self.ch_btn):
            b.set_has_frame(False)
            b.set_css_classes(["alpaca-tabbtn"] + (["alpaca-on"] if b is self.ws_btn else []))
            btns.append(b)
        self.ch_btn.set_visible(False)   # outside-repo policy (spec §2); _refresh_status decides
        self.ws_btn.connect("clicked", lambda *_: self._set_mode("tree"))
        self.ch_btn.connect("clicked", lambda *_: self._set_mode("changes"))
        self.append(btns)

        # display, abs path, is_dir, symbolic fallback icon, chevron pixbuf, mock badge, badge/icon visible
        self.store = Gtk.TreeStore(str, str, bool, str, GdkPixbuf.Pixbuf, GdkPixbuf.Pixbuf, bool, bool)
        self.view = HoverTree(model=self.store, headers_visible=False)
        for sig in ("row-inserted", "row-deleted"):   # keep the hover band on the row under a stationary pointer
            self.store.connect(sig, lambda *a: self.view._refresh_hover())
        self.view.set_activate_on_single_click(True)     # single click = open/toggle
        self.view.set_property("show-expanders", False)  # our own chevron cell, mock-style
        self.view.set_tooltip_column(1)                  # ellipsized names: hover shows the full path
        self.view.set_level_indentation(16)              # indent: 8 + 16·depth
        pix_chev = Gtk.CellRendererPixbuf(); pix_chev.set_property("xpad", 2); pix_chev.set_property("ypad", 2)
        pix_badge = Gtk.CellRendererPixbuf(); pix_badge.set_property("xpad", 2); pix_badge.set_property("ypad", 2)
        pix_icon = Gtk.CellRendererPixbuf(); pix_icon.set_property("xpad", 2); pix_icon.set_property("ypad", 2)
        # pixbuf cells center on the line box but glyph ink hangs ~1px below it
        # (same law as the tab labels) — bottom-pin the icons +1px at the 22px
        # row (slack 2 on 16px art): yalign set before inserts, props cache there.
        # chevron rides 4/6 so dir rows stay flush with folders. Measured 2026-10-05.
        pix_chev.set_property("yalign", 2.0 / 3.0)
        pix_badge.set_property("yalign", 1.0)
        pix_icon.set_property("yalign", 1.0)
        # rows measure cell props at insert and cache: text ypad 2 + 13px CSS font ≈ the 22px row (VS Code density)
        cell = Gtk.CellRendererText(); cell.set_property("xpad", 2); cell.set_property("ypad", 2)
        cell.set_property("ellipsize", Pango.EllipsizeMode.MIDDLE)  # before any insert — props cache at insert; tree min must not follow the longest name
        col = Gtk.TreeViewColumn()                 # kwargs form raises in this PyGObject build
        col.pack_start(pix_chev, False)
        col.add_attribute(pix_chev, "pixbuf", 4)
        col.pack_start(pix_badge, False)
        col.add_attribute(pix_badge, "pixbuf", 5)
        col.add_attribute(pix_badge, "visible", 6)
        col.pack_start(pix_icon, False)
        col.add_attribute(pix_icon, "icon-name", 3)
        col.add_attribute(pix_icon, "visible", 7)
        col.pack_start(cell, True)            # text last, expand — pins chevron/badge/icon left
        col.add_attribute(cell, "text", 0)
        self.view.append_column(col)
        self.view.set_css_classes(["alpaca-tree"])
        self.view.connect("row-expanded", self._on_row_expanded)
        self.view.connect("row-collapsed", self._on_row_collapsed)
        self.view.connect("row-activated", self._on_activated)
        tree_page = Gtk.ScrolledWindow(vexpand=True, child=self.view)
        # mode host (spec §1): zero-transition swap between the tree and the
        # changes view. vexpand on the Stack — GtkBox gives non-expanding
        # children only their minimum, and the card must fill below the tab row.
        self.changes = ChangesView()   # window wires on_open/before_commit/on_status
        self.stack = Gtk.Stack(transition_type=Gtk.StackTransitionType.NONE, vexpand=True)
        self.stack.add_named(tree_page, "tree")
        self.stack.add_named(self.changes, "changes")
        self._mode = "tree"
        self.append(self.stack)

        bar = Gtk.Box(spacing=6, margin_start=14, margin_end=12, margin_top=0, margin_bottom=0)
        bar.set_css_classes(["alpaca-statusbar"])
        self.dot = Gtk.Box(); self.dot.set_size_request(8, 8); self.dot.set_css_classes(["alpaca-status-dot"])
        self.dot.set_valign(Gtk.Align.CENTER); self.dot.set_visible(False)
        self.spin = Gtk.Spinner(); self.spin.set_size_request(10, 10)
        self.spin.set_css_classes(["alpaca-status-spin"])
        self.spin.set_valign(Gtk.Align.CENTER); self.spin.set_visible(False)
        self.git_label = Gtk.Label(label="", ellipsize=Pango.EllipsizeMode.MIDDLE); self.git_label.set_visible(False)
        self.count_label = Gtk.Label(label="")
        self.branchbtn = BranchMenu()            # icon + name + ▾ pill; popover in branchmenu.py
        self.branchbtn.on_status = self.show_git_status
        self.branchbtn.allow = lambda: not self._git_busy
        bar.append(self.branchbtn)
        bar.append(self.dot)
        bar.append(self.spin)
        bar.append(self.git_label)
        bar.append(Gtk.Box(hexpand=True))        # spacer pushes file count right
        bar.append(self.count_label)
        self.append(bar)

    # ---- root / population ------------------------------------------------
    def set_root(self, root: str) -> None:
        for m in [*self._monitors, self._head_mon]:
            if m:
                m.cancel()
        self._monitors = []
        self._head_mon = None
        self.root = root
        self._saved_tree = None   # dead before set_text fires a synchronous search-changed
        self.entry.set_text("")
        self._populate_root()
        self._set_mode("tree")        # a fresh workspace opens on the file tree (spec §1)
        self.changes.set_root(root)
        head = os.path.join(root, ".git", "HEAD")
        if os.path.isfile(head):
            self._head_mon = Gio.File.new_for_path(head).monitor_file(0, None)
            self._head_mon.connect("changed", lambda *a: self.refresh_branch())
        self._refresh_status()
        self._count()

    def refresh_branch(self) -> None:
        """Public alias — window/git-change callers. Recomputes branch + dirty state."""
        self._refresh_status()

    def show_git_status(self, kind: str, text: str) -> None:
        """Commit/push flight status — changes-view reports land here (window
        wiring). busy → spinner + phase text; ok → green dot pulse, the row
        re-syncs itself after 2s; err → red dot + the error's first line, the
        full git output as a tooltip. The busy guard keeps mid-flight refreshes
        (focus hook, HEAD monitor, mode switches) from clobbering the spinner."""
        if self._pulse_id:                       # a fresh state outranks a stale pulse
            GLib.source_remove(self._pulse_id)
            self._pulse_id = 0
        self._git_busy = kind == "busy"
        if kind == "busy":
            self.spin.set_visible(True); self.spin.start()
            self.dot.set_visible(False)
            self.git_label.set_text(text); self.git_label.set_tooltip_text("")
            return
        self.spin.stop(); self.spin.set_visible(False)
        self.dot.set_visible(True)
        self.git_label.set_visible(True)
        if kind == "ok":
            self.dot.set_css_classes(["alpaca-status-dot", "ok"])
            self.git_label.set_text(text)
            self.git_label.set_tooltip_text("")
            self._pulse_id = GLib.timeout_add(2000, self._pulse)
        else:
            self.dot.set_css_classes(["alpaca-status-dot", "err"])
            self.git_label.set_text(text.splitlines()[0] if text else "Git error")
            self.git_label.set_tooltip_text(text or None)

    def _pulse(self) -> bool:
        """End of the ok pulse: stop owning the row, re-sync to the real state
        (replaces the old post-commit on_refresh wiring)."""
        self._pulse_id = 0
        self._refresh_status()
        return False

    # ---- mode tabs (spec §1) --------------------------------------------------
    def _set_mode(self, mode: str) -> None:
        """Swap the card between the file tree and the changes view. Entering
        CHANGES re-syncs the list (spec §2 refresh trigger: view becomes
        active); the selected tab's css class holds it bright, hover brightens
        the idle one."""
        if mode == self._mode:
            return
        self._mode = mode
        self.stack.set_visible_child_name(mode)
        for b, own in ((self.ws_btn, "tree"), (self.ch_btn, "changes")):
            b.set_css_classes(["alpaca-tabbtn"] + (["alpaca-on"] if mode == own else []))
        self.entry.set_placeholder_text("Search files…" if mode == "tree" else "Filter changes…")
        if mode == "tree":
            # the shared entry text survives the tab switch (and its view may be
            # displaced by the last search): a needle re-filters the tree, an
            # empty box restores the displaced sheet instead of leaving the
            # last flat results stuck with no way back
            if self.entry.get_text().strip():
                self._on_search(self.entry)
            elif self._saved_tree:
                saved, self._saved_tree = self._saved_tree, None
                self._populate_root()
                GLib.idle_add(self._restore_saved_tree, saved)
        if mode == "changes":
            self.changes.refresh()                    # fresh data (re-applies the view's own needle)
            self.changes.filter(self.entry.get_text())  # entry text outranks a stale needle (tree-mode search wrote it)

    def refresh_git(self) -> None:
        """Window-focus hook (window's notify::is-active): re-sync everything git."""
        self._refresh_status()

    def _paint_status(self, branch: str | None, dirty: int | None) -> None:
        """Statusbar rendering only — git data comes in from callers (the sync
        path below or the live probe's worker thread). The commit/push flight
        owns the row while busy; `git status` runs OFF-THREAD in both paths."""
        if self._git_busy:
            return                           # commit/push flight owns the row (pulse re-syncs)
        is_git = branch is not None
        self.branchbtn.update(branch if is_git else None)
        for w in (self.dot, self.spin, self.git_label):
            w.set_visible(is_git)
        self.spin.set_visible(False)         # idle state: the dot, never the spinner
        self.ch_btn.set_visible(is_git)
        if not is_git and self._mode == "changes":
            self._set_mode("tree")    # repo vanished (HEAD deleted) while reading it
        if not is_git:
            return
        self.git_label.set_tooltip_text("")   # a past err's tooltip must not outlive the row's re-sync
        # count = porcelain rows (untracked listed per-file, -z -uall) — same
        # number the changes view shows; one git call feeds both.
        if dirty is None:
            self.git_label.set_text("")
            self.dot.set_visible(False)
        elif dirty == 0:
            self.git_label.set_text("No changes")
            self.dot.set_css_classes(["alpaca-status-dot", "ok"])
            self.dot.set_visible(True)
        else:
            self.git_label.set_text(f"{dirty} changed")
            self.dot.set_css_classes(["alpaca-status-dot", "warn"])
            self.dot.set_visible(True)

    def _refresh_status(self) -> None:
        """Sync path (set_root, focus hook, HEAD monitor, pulse): one git call,
        paints via _paint_status and feeds the diff-tab hook + open changes view."""
        from . import gitstatus
        branch = gitstatus.branch_of(self.root) if self.root else None
        raw = gitstatus.changes(self.root) if branch else None
        self._paint_status(branch, len(raw) if raw is not None else None)
        if raw is not None:
            self.on_git_changed(raw)
            if self._mode == "changes":
                self.changes.apply(raw, gitstatus.ahead(self.root))

    # ---- live git probe (2026-10-05 spec) --------------------------------------
    def _git_tick(self) -> bool:
        if self.root and not self._probe_busy and not self._git_busy and not self.changes._busy:
            root = self.root                      # snapshot: a workspace switch mid-flight drops it
            self._probe_busy = True
            threading.Thread(target=self._git_probe, args=(root,),
                             daemon=True, name="alpaca-git").start()
        return True

    def _git_probe(self, root: str) -> None:
        from . import gitstatus
        raw = gitstatus.changes(root)
        ahead = gitstatus.ahead(root) if raw is not None else 0
        GLib.idle_add(self._git_landed, root, raw, ahead)

    def _git_landed(self, root: str, raw: list, ahead: int) -> bool:
        self._probe_busy = False
        if root != self.root or self._git_busy or self.changes._busy:
            return False
        from . import gitstatus
        self._paint_status(gitstatus.branch_of(root), len(raw) if raw is not None else None)
        if raw is not None:
            self.on_git_changed(raw)
            if self._mode == "changes":
                self.changes.apply(raw, ahead)
        return False

    def _row_vals(self, name: str, path: str, is_dir: bool) -> list:
        """Cairo badge/folder pixbuf when badge art is available, else the symbolic
        icon cell."""
        pix = badges.pixbuf_for(name, is_dir)
        chev = badges.chevron_pixbuf(False) if is_dir else badges.blank_pixbuf()
        return [name, path, is_dir, icon_of(name, is_dir),
                chev, pix, pix is not None, pix is None]

    def _populate_root(self) -> None:
        self.store.clear()
        self._monitored = set()   # rebuild is fresh: stale monitor set / remembered expansions
        self._open = set()        # are gone with the rows they pointed at
        if self.root:
            for name, is_dir in scan_dir(self.root):
                self.store.append(None, self._row_vals(name, os.path.join(self.root, name), is_dir))

    def _count(self) -> None:
        self.count_label.set_text(f"{len(scan_project(self.root, '', cap=100000))} files")

    def _row_child(self, parent_iter, name: str):
        """First row named `name` under parent (None = toplevel), or None."""
        i = self.store.iter_children(parent_iter) if parent_iter else self.store.get_iter_first()
        while i is not None:
            if self.store[i][0] == name:
                return i
            i = self.store.iter_next(i)
        return None

    def _reexpand(self, d: str) -> None:
        """Replay one remembered dir: descend parent-first over its segments,
        lazy-loading each level's children BEFORE expanding it (the store is
        lazy — a collapsed row has no child rows — and mutating UNDER an
        expanded row would collapse it; `_load_children` already prunes stale
        `_open` entries under a freshly loaded level). Signals are synchronous,
        so each expand_row re-fills _open/chevrons/monitors inline. Voids on
        vanished paths (moved/deleted since the save)."""
        it = None
        for seg in os.path.relpath(d, self.root).split(os.sep):
            it = self._row_child(it, seg)
            if it is None:
                return                    # tree moved on (rebuild/vanished path)
            dd = self.store[it][1]
            if not self.store.iter_children(it):         # lazy level: load, then expand
                self._load_children(it, dd)
            self.view.expand_row(self.store.get_path(it), False)

    def _restore_saved_tree(self, saved: tuple) -> bool:
        """Stage 1 (idle): replay the saved open-dirs parents-first (shallow to
        deep — parents must expand before their descendants are replayed into
        them), then hand the scroll to stage 2 after allocations settle. A
        workspace opened meanwhile cancels the replay silently."""
        root, open_paths, _scroll = saved
        if self.root != root:
            return False
        for d in sorted(open_paths, key=lambda p: p.count(os.sep)):
            self._reexpand(d)
        GLib.idle_add(self._restore_scroll, _scroll)
        return False                    # idle_add: run once

    def _restore_scroll(self, value: float) -> bool:
        adj = self.view.get_vadjustment()
        adj.set_value(min(value, max(adj.get_upper() - adj.get_page_size(), 0.0)))
        return False                    # idle_add: run once

    # ---- search -------------------------------------------------------------
    def _on_search(self, entry) -> None:
        text = entry.get_text().strip()
        if self._mode == "changes":   # changes search never touches the file tree
            self.changes.filter(text)
            return
        if not text:
            self._populate_root()
            saved, self._saved_tree = self._saved_tree, None
            if saved and saved[0] == self.root and saved[1]:
                GLib.idle_add(self._restore_saved_tree, saved)
            return
        # first displacement of a real tree saves it; later keystrokes must NOT
        # overwrite the bundle with the flat list's collapsed/clamped state
        if self._saved_tree is None:
            self._saved_tree = (self.root, set(self._open),
                                self.view.get_vadjustment().get_value())
        self.store.clear()
        self._open = set()        # search view has no tree rows
        self._monitored = set()
        for rel in scan_project(self.root, text):
            p = os.path.join(self.root, rel)
            self.store.append(None, self._row_vals(rel, p, False))

    # ---- load children + monitors ---------------------------------------------
    def _load_children(self, parent_iter, d: str) -> None:
        """Replace collapsed parent's children with a fresh scan from disk.
        Collapsed-row mutations only — under an expanded row this would collapse it."""
        while True:
            c = self.store.iter_children(parent_iter)
            if not c:
                break
            self.store.remove(c)
        # fresh rows are collapsed; remembered open-states under d are stale
        self._open = {p for p in self._open if p == d or not p.startswith(d + os.sep)}
        for name, is_dir in scan_dir(d):
            self.store.append(parent_iter,
                              self._row_vals(name, os.path.join(d, name), is_dir))

    def _on_row_expanded(self, view, iter_, tpath) -> None:
        row = self.store[iter_]
        d = row[1]
        if not d:
            return
        self._open.add(d)
        row[4] = badges.chevron_pixbuf(True)   # same blue folder for open/closed, chevron signals state
        # monitors only for dirs we've actually shown the user (open once, cap)
        if row[2] and d not in self._monitored and len(self._monitors) < 50:
            # ponytail: 50-monitor cap; deeper dirs refresh only via their parent's events
            self._monitored.add(d)
            mon = Gio.File.new_for_path(d).monitor_directory(0, None)
            mon.connect("changed", self._on_dir_changed, d)
            self._monitors.append(mon)

    def _on_row_collapsed(self, view, iter_, tpath) -> None:
        row = self.store[iter_]
        if row[1]:
            self._open.discard(row[1])
        row[4] = badges.chevron_pixbuf(False)

    def _on_dir_changed(self, mon, f, other, event, d) -> None:
        if event not in (Gio.FileMonitorEvent.CREATED, Gio.FileMonitorEvent.DELETED,
                         Gio.FileMonitorEvent.MOVED):
            return
        self._count()
        if d not in self._open:      # collapsed: next expansion reloads from disk anyway
            return
        p = f.get_path() if f else None
        name = os.path.basename(p) if p else None
        if not name or name in SKIP_DIRS:
            return
        if event == Gio.FileMonitorEvent.DELETED:
            is_dir = False
        else:
            try:
                info = f.query_info("standard::type", Gio.FileQueryInfoFlags.NONE, None)
                is_dir = info.get_file_type() == Gio.FileType.DIRECTORY
            except OSError:
                is_dir = False
        # ponytail: surgical rows keep open dirs live without collapsing them;
        # new rows land unsorted (next collapse/re-expand restores scan order)
        for row_iter in self._iters_for(d):
            ci = self.store.iter_children(row_iter)
            while ci:
                nxt = self.store.iter_next(ci)   # remove invalidates the iter → look ahead first
                if self.store[ci][1] == p:
                    self.store.remove(ci)
                ci = nxt
            if event != Gio.FileMonitorEvent.DELETED:
                self.store.append(row_iter, self._row_vals(name, p, is_dir))

    def _iters_for(self, path: str) -> list:
        """All TreeIters whose row abs-path == path (same-named dirs may exist under several parents)."""
        found = []
        def walk(parent_iter):
            i = self.store.iter_children(parent_iter) if parent_iter else self.store.get_iter_first()
            while i is not None:
                row = self.store[i]
                if row[1] == path:
                    found.append(i.copy())   # copy → stays valid across later mutations
                if row[2]:
                    walk(i)
                i = self.store.iter_next(i)
        walk(None)
        return found

    # ---- activation ------------------------------------------------------------
    # activate_on_single_click is on: one click activates. Dir rows toggle
    # expand/collapse; the open/close decision uses our own _open set, never the
    # view's (deprecated) row_expanded() readback.
    def _on_activated(self, view, tpath, col) -> None:
        row = self.store[tpath]
        if row[2]:
            if row[1] in self._open:
                view.collapse_row(tpath)
            elif row[1]:
                self._load_children(row.iter, row[1])
                view.expand_row(tpath, False)
        elif row[1]:
            self.on_open(row[1])