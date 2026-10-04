# Right panel: search + lazily expanded file tree + status row (branch, file count).
import os
import alpaca_code.gi_env as ge
ge.require("Gtk", ("4.0",))
from gi.repository import Gio, Gtk

SKIP_DIRS = {"node_modules", ".git", "obj", "bin", "vendor", "__pycache__"}

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
        self.set_css_classes(["alpaca-surface2"])
        self.on_open = lambda path: None
        self.root = None
        self._monitors: list[Gio.FileMonitor] = []
        self._head_mon: Gio.FileMonitor | None = None
        self._expanded: set[str] = set()

        self.entry = Gtk.SearchEntry()
        self.entry.set_placeholder_text("Search files…")
        self.entry.set_css_classes(["alpaca-search"])
        self.entry.connect("search-changed", self._on_search)
        self.append(self.entry)

        self.store = Gtk.TreeStore(str, str, bool)            # display, abs path, is_dir
        self.view = Gtk.TreeView(model=self.store, headers_visible=False)
        cell = Gtk.CellRendererText()
        col = Gtk.TreeViewColumn()                 # kwargs form raises in this PyGObject build
        col.pack_start(cell, True)
        col.add_attribute(cell, "text", 0)
        self.view.append_column(col)
        self.view.set_css_classes(["alpaca-tree"])
        self.view.connect("row-expanded", self._on_row_expanded)
        self.view.connect("row-activated", self._on_activated)
        self.append(Gtk.ScrolledWindow(vexpand=True, child=self.view))

        row = Gtk.Box(spacing=12, margin_start=12, margin_end=12, margin_top=8, margin_bottom=8)
        row.set_css_classes(["alpaca-status-row"])
        self.branch_label = Gtk.Label(label="")
        self.count_label = Gtk.Label(label="")
        row.append(self.branch_label)
        row.append(Gtk.Box(hexpand=True))                     # spacer
        row.append(self.count_label)
        self.append(row)

    # ---- root / population ------------------------------------------------
    def set_root(self, root: str) -> None:
        for m in [*self._monitors, self._head_mon]:
            if m:
                m.cancel()
        self._monitors = []
        self._head_mon = None
        self.root = root
        self._expanded = set()
        self.entry.set_text("")
        self._populate_root()
        head = os.path.join(root, ".git", "HEAD")
        if os.path.isfile(head):
            self._head_mon = Gio.File.new_for_path(head).monitor_file(0, None)
            self._head_mon.connect("changed", lambda *a: self._refresh_branch())
        self._refresh_branch()
        self._count()

    def refresh_branch(self) -> None:
        self._refresh_branch()

    def _refresh_branch(self) -> None:
        from . import gitstatus
        self.branch_label.set_text(gitstatus.branch_of(self.root) or "")

    def _populate_root(self) -> None:
        self.store.clear()
        self._expanded = set()   # rebuild is fresh (search-clear path) — remembered expansions are stale
        if self.root:
            for name, is_dir in scan_dir(self.root):
                ident = self.store.append(None, [name, os.path.join(self.root, name), is_dir])
                if is_dir:
                    self.store.append(ident, ["", "", False])  # placeholder → expander arrow

    def _count(self) -> None:
        self.count_label.set_text(f"{len(scan_project(self.root, '', cap=100000))} files")

    # ---- search -------------------------------------------------------------
    def _on_search(self, entry) -> None:
        text = entry.get_text().strip()
        if not text:
            self._populate_root()
            return
        self.store.clear()
        for rel in scan_project(self.root, text):
            self.store.append(None, [rel, os.path.join(self.root, rel), False])

    # ---- lazy expand + monitors ---------------------------------------------
    def _on_row_expanded(self, view, iter_, tpath) -> None:
        row = self.store[iter_]
        d = row[1]
        if not row[2] or d in self._expanded:
            return
        self._expanded.add(d)
        self._fill_children(row.iter)
        if len(self._monitors) < 50:  # ponytail: 50-monitor cap; deeper dirs refresh on expand only
            mon = Gio.File.new_for_path(d).monitor_directory(0, None)
            mon.connect("changed", self._on_dir_changed, d)
            self._monitors.append(mon)

    def _fill_children(self, parent_iter) -> None:
        while True:
            c = self.store.iter_children(parent_iter)
            if not c:
                break
            self.store.remove(c)
        d = self.store[parent_iter][1]
        # re-added children are fresh placeholders: remembered expansions under d are stale
        self._expanded = {p for p in self._expanded if p == d or not p.startswith(d + os.sep)}
        for name, is_dir in scan_dir(d):
            ident = self.store.append(parent_iter, [name, os.path.join(d, name), is_dir])
            if is_dir:
                self.store.append(ident, ["", "", False])

    def _on_dir_changed(self, mon, f, other, event, d) -> None:
        if event not in (Gio.FileMonitorEvent.CREATED, Gio.FileMonitorEvent.DELETED,
                         Gio.FileMonitorEvent.MOVED):
            return
        for it in self._iters_for(d):
            self._fill_children(it)
        self._count()

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
    def _on_activated(self, view, tpath, col) -> None:
        row = self.store[tpath]
        if not row[2] and self.on_open:
            self.on_open(row[1])