# Changes view: git-change tree (group_tree rows) with checkboxes and a commit
# bar (spec §2). The store is REBUILT per refresh — filetree's measured
# invariant says value-only updates are safe under expanded rows, structural
# mutation collapses them, so surgical patching is off the table here.
import os
import threading
import alpaca_code.gi_env as ge
ge.require("Gdk", ("4.0",))
ge.require("Gtk", ("4.0",))
ge.require("GdkPixbuf", ("2.0",))
ge.require("Pango", ("1.0",))
from gi.repository import Gdk, GdkPixbuf, GLib, Gtk, Pango

from . import badges, gitstatus
from .treehover import HoverTree

LETTER_COLOR = {  # spec §2: M amber, A/U green, D red, R blue (+T typechange); C conflict red (I1)
    "M": "#f2c94c", "A": "#22c55e", "U": "#22c55e",
    "D": "#ef4444", "R": "#2f80ed", "T": "#2f80ed", "C": "#ef4444",
}
LETTER_DEFAULT = "#f2c94c"

class ChangesView(Gtk.Box):
    def __init__(self):
        super().__init__(orientation=Gtk.Orientation.VERTICAL)
        self.on_open = lambda rel, letter: None  # window → editor.open_diff
        self.before_commit = lambda paths: None  # window → editor.save_open
        self.on_refresh = lambda: None           # window → browser statusbar refresh
        self.root = None
        self._checked: set[str] = set()          # file rels only (subset of _files)
        self._files: set[str] = set()            # every file row in the changeset
        self._rows: list[tuple[str, str]] = []   # last changes() (filter() uses it)
        self._shown: list = []                   # rows the view currently renders (last _fill)
        self._visible: set[str] = set()          # file rels in _shown — every toggle lens
        self._needle: str = ""                   # active filter (re-applied on refresh)
        self._closed: set[str] = set()           # user-collapsed dir rels (dirs render OPEN unless closed)
        self._busy = False

        self.store = Gtk.TreeStore(str, str, str, str,            # name, rel, kind, letter
                                   GdkPixbuf.Pixbuf, GdkPixbuf.Pixbuf, GdkPixbuf.Pixbuf,  # chev, badge, chip
                                   bool, bool, bool, bool, bool)          # checked, inconsist, chev, letter, toggle
        self.view = HoverTree(model=self.store, headers_visible=False)
        self.view.set_activate_on_single_click(True)
        self.view.set_property("show-expanders", False)
        self.view.set_level_indentation(16)
        self.view.set_tooltip_column(1)

        # cell props at insert-time; text cell packed LAST with expand=True so
        # chevron/badge stay pinned left (filetree measured packing rule)
        cell_name = Gtk.CellRendererText(); cell_name.set_property("ypad", 2)
        cell_name.set_property("ellipsize", Pango.EllipsizeMode.MIDDLE)
        chev = Gtk.CellRendererPixbuf(); chev.set_property("ypad", 2)
        bad = Gtk.CellRendererPixbuf();  bad.set_property("ypad", 2)
        let = Gtk.CellRendererPixbuf();  let.set_property("ypad", 2)
        tog = Gtk.CellRendererToggle(); tog.set_property("activatable", True)
        col_main = Gtk.TreeViewColumn()          # kwargs form raises on this build
        col_main.pack_start(chev, False); col_main.add_attribute(chev, "pixbuf", 4)
        col_main.add_attribute(chev, "visible", 9)
        col_main.pack_start(bad, False); col_main.add_attribute(bad, "pixbuf", 5)
        col_main.pack_start(cell_name, True)
        col_main.add_attribute(cell_name, "text", 0)
        col_main.set_expand(True)   # spec §29: name column absorbs the spare width —
        # an ellipsized cell's natural is its MINIMUM (col0 negotiation measured
        # 54px + the toggle column swallowing ~346px of slack), so without this
        # the letter chip rides mid-panel next to 4-glyph names with ~110px of
        # dead space right of the checkbox
        col_let = Gtk.TreeViewColumn()
        col_let.pack_start(let, False); col_let.add_attribute(let, "pixbuf", 6)
        col_let.add_attribute(let, "visible", 10)
        col_tog = Gtk.TreeViewColumn()
        col_tog.pack_start(tog, False)
        col_tog.add_attribute(tog, "active", 7)
        col_tog.add_attribute(tog, "inconsistent", 8)
        col_tog.add_attribute(tog, "visible", 11)
        self.view.append_column(col_main)
        self.view.append_column(col_let)
        self.view.append_column(col_tog)
        self._toggle_col = col_tog
        self.view.set_css_classes(["alpaca-tree"])
        self.view.connect("row-expanded", self._on_expand_toggle, True)
        self.view.connect("row-collapsed", self._on_expand_toggle, False)
        self.view.connect("row-activated", self._on_activated)
        tog.connect("toggled", self._on_toggled)
        for sig in ("row-inserted", "row-deleted"):   # keep the band on the row under a stationary pointer
            self.store.connect(sig, lambda *a: self.view._refresh_hover())

        self.append(Gtk.ScrolledWindow(vexpand=True, child=self.view))

        bar = Gtk.Box(spacing=6, margin_start=12, margin_end=12, margin_top=6, margin_bottom=6)
        bar.set_css_classes(["alpaca-commitbar"])
        self.msg = Gtk.Entry(hexpand=True)
        self.msg.set_placeholder_text("Commit message…")
        self.msg.set_css_classes(["alpaca-msg"])
        self.msg.set_size_request(-1, 26)    # height floor, like .alpaca-search's 30
        self.msg.connect("activate", self._on_msg_enter)
        self.commit_btn = Gtk.Button(label="Commit"); self.commit_btn.set_css_classes(["alpaca-barbtn"])
        self.push_btn = Gtk.Button(label="Push");    self.push_btn.set_css_classes(["alpaca-barbtn"])
        self.commit_btn.connect("clicked", lambda b: self.commit_clicked())
        self.push_btn.connect("clicked", lambda b: self.push_clicked())
        self.result = Gtk.Label(xalign=0.0, visible=False)
        self.result.set_ellipsize(Pango.EllipsizeMode.END)   # 400-char git output must not widen the bar
        self.result.set_css_classes(["alpaca-commitresult"])
        bar.append(self.msg); bar.append(self.commit_btn); bar.append(self.push_btn)
        self.append(bar)
        self.append(self.result)

    # ---- data ------------------------------------------------------------------
    def set_root(self, root: str | None) -> None:
        self.root = root
        self._closed.clear()
        self._needle = ""                          # new repo: the old filter is meaningless
        self.refresh(keep_selection=False)         # a new repo = fresh selection

    def refresh(self, keep_selection: bool = True) -> None:
        """Rebuild from changes(). Fresh view selects all (spec §2); a refresh
        INTERSECTS the surviving selection with the new file set (deviation 1) —
        after a commit the committed paths vanish and the rest stay selected.
        An active filter re-applies (focus/commit refresh cycles keep the view
        the user is looking at)."""
        rows = gitstatus.changes(self.root) or []
        self._rows = rows
        files = {r[2] for r in gitstatus.group_tree(rows) if r[0] == "f"}   # r[2] = rel (group_tree: kind,name,rel,letter,depth)
        self._files = set(files)                 # BEFORE _fill: _sync_row needs it
        self._checked = (self._checked & files) if keep_selection else set(files)
        self._fill(self._filtered(rows))

    def filter(self, needle: str) -> None:
        """Search-route: re-render by name/rel substring over the remembered
        rows, no re-scan. _checked untouched (view-only); _files stays global."""
        self._needle = (needle or "").strip().lower()
        self._fill(self._filtered(self._rows))

    def _filtered(self, rows: list[tuple[str, str]]) -> list[tuple[str, str]]:
        """The needle lens (shared by filter() and refresh()): name/rel substring;
        empty needle → identity. _needle always tracks the rendered subset."""
        n = (self._needle or "").strip().lower()
        if not n:
            return rows
        return [r for r in rows
                if n in os.path.basename(r[0]).lower() or n in r[0].lower()]

    def _fill(self, rows: list[tuple[str, str]]) -> None:
        self.store.clear()
        self._shown = rows
        self._visible = {r[2] for r in gitstatus.group_tree(rows) if r[0] == "f"}
        if not rows:
            self.store.append(None, ["No changes", "", "e", "", badges.blank_pixbuf(),
                                     badges.blank_pixbuf(), None, False, False, False, False, False])
            self._sync()
            return
        self.store.append(None, ["Select all", "", "s", "", badges.blank_pixbuf(),
                                 badges.blank_pixbuf(), None, False, False, False, False, True])
        iters: dict[str, object] = {}
        for kind, name, rel, letter, _d in gitstatus.group_tree(rows):
            is_dir = kind == "d"
            badge = badges.folder_pixbuf() if is_dir else badges.pixbuf_for(name)
            lpix = (badges.letter_pixbuf(letter, LETTER_COLOR.get(letter, LETTER_DEFAULT))
                    if letter else None)
            parent = iters.get(rel.rsplit("/", 1)[0]) if "/" in rel else None
            it = self.store.append(parent, [
                name, rel, kind, letter,
                badges.chevron_pixbuf(False) if is_dir else badges.blank_pixbuf(),
                badge or badges.blank_pixbuf(), lpix,
                False, False, is_dir, bool(letter), True])
            if is_dir:
                iters[rel] = it
        self._sync()
        for rel, it in iters.items():            # dirs render open unless the user closed them
            if rel not in self._closed:
                self.view.expand_row(self.store.get_path(it), False)   # (DeprecationWarning prints once per process — measured harmless)

    # ---- checked-state sync (value-only writes: safe under expanded rows) -------
    def _sync(self) -> None:
        # lens: dir/masthead state is computed over what the USER SEES (_shown ×
        # _visible) with the lensed selection — invisible files never drive a
        # rendered checkbox (filter() renders a subset; toggles stay visible-only).
        st = (gitstatus.checked_dir_state(gitstatus.group_tree(self._shown),
                                          self._checked & self._visible)
              if self._shown else {})
        it = self.store.get_iter_first()
        while it is not None:
            self._sync_row(it, st)
            nxt = self.store.iter_next(it)       # capture BEFORE recursion (mutation risk)
            self._walk_children(self.store.iter_children(it), st)
            it = nxt
        self._buttons()

    def _walk_children(self, it, st) -> None:
        while it is not None:
            self._sync_row(it, st)
            nxt = self.store.iter_next(it)
            self._walk_children(self.store.iter_children(it), st)
            it = nxt

    def _sync_row(self, it, st) -> None:
        row = self.store[it]
        kind = row[2]
        if kind == "f":
            self.store[it][7] = row[1] in self._checked
        elif kind == "d":
            allc, somec = st.get(row[1], (False, False))
            self.store[it][7] = allc
            self.store[it][8] = not allc and somec
        elif kind == "s":
            vis = self._visible
            allc = bool(vis) and vis <= self._checked
            somec = bool(self._checked & vis)
            self.store[it][7] = allc
            self.store[it][8] = not allc and somec

    def _buttons(self) -> None:
        self.commit_btn.set_sensitive(not self._busy and bool(self._checked))
        self.push_btn.set_sensitive(not self._busy and bool(self._files))

    # ---- toggles ----------------------------------------------------------------
    def _on_toggled(self, render, path_str: str) -> None:
        row = self.store[path_str]
        kind, rel = row[2], row[1]
        if kind == "f":
            if rel in self._checked:
                self._checked.discard(rel)
            else:
                self._checked.add(rel)
        elif kind == "d":
            files = {r for r in self._visible if r.startswith(rel + "/")}
            if files and files <= self._checked:     # every descendant selected → clear them
                self._checked -= files
            else:                                    # none/partial → select all under this dir
                self._checked |= files
        elif kind == "s":                            # masthead: visible-only law (I2)
            vis = self._visible
            if vis and vis <= self._checked:
                self._checked -= vis
            else:
                self._checked |= vis
        self._sync()

    def _on_expand_toggle(self, view, it, tpath, expanded: bool) -> None:
        rel = self.store[it][1]
        if rel:
            (self._closed.discard if expanded else self._closed.add)(rel)
            self.store[it][4] = badges.chevron_pixbuf(expanded)
            if expanded:                         # hidden dirs don't expand (expand_row is
                cin = self.store.iter_children(it)   # no-op under a collapsed ancestor) — catch them up
                while cin is not None:
                    if self.store[cin][2] == "d" and self.store[cin][1] not in self._closed:
                        self.view.expand_row(self.store.get_path(cin), False)
                    cin = self.store.iter_next(cin)

    # ---- activation (spec §2: file click opens the diff) ------------------------
    def _on_activated(self, view, tpath, col) -> None:
        if col is self._toggle_col:
            return                                   # checkbox clicks never open a diff (Review Focus #1)
        row = self.store[tpath]
        if row[2] == "d" and row[1]:
            if row[1] in self._closed:
                view.expand_row(tpath, False)        # children are pre-loaded (non-lazy store)
            else:
                view.collapse_row(tpath)
        elif row[2] == "f":
            self.on_open(row[1], row[3])         # (rel, letter) — chip needs the letter

    # ---- commit / push (spec §4: workers are daemon threads; GTK via idle_add) --
    def commit_clicked(self) -> None:
        if self._busy or not self.root:
            return
        paths = sorted(self._checked)
        msg = self.msg.get_text().strip()
        if not msg:
            self._show_result(False, "Empty commit message")
            return
        if not paths:
            self._show_result(False, "No files selected")
            return
        self.before_commit(paths)        # window: flush dirty editor buffers for exactly these files
        root = self.root                 # snapshot: a set_root() during the flight must not reroute it (review I4)
        self._set_busy(True)
        threading.Thread(target=self._do_commit, args=(root, paths, msg),
                         daemon=True, name="alpaca-commit").start()

    def _do_commit(self, root, paths, msg) -> None:
        ok, text = gitstatus.commit(root, paths, msg)
        GLib.idle_add(self._commit_done, ok, text)

    def _commit_done(self, ok, text) -> bool:
        self._set_busy(False)
        self._show_result(ok, text)
        if ok:
            self.refresh()               # keep_selection=True: committed paths vanish, rest kept
            self.on_refresh()            # window: browser statusbar branch/dirty refresh
        return False                     # idle_add: run once

    def push_clicked(self) -> None:
        if self._busy or not self.root:
            return
        root = self.root                 # snapshot: same I4 law as commit
        self._set_busy(True)
        threading.Thread(target=self._do_push, args=(root,),
                         daemon=True, name="alpaca-push").start()

    def _do_push(self, root) -> None:
        ok, text = gitstatus.push(root)
        GLib.idle_add(self._push_done, ok, text)

    def _push_done(self, ok, text) -> bool:
        self._set_busy(False)
        self._show_result(ok, text)
        if ok:                       # spec §2: the list re-syncs after push too
            self.refresh()
            self.on_refresh()
        return False

    def _set_busy(self, busy: bool) -> None:
        self._busy = busy
        self._buttons()
        self.msg.set_sensitive(not busy)

    def _show_result(self, ok: bool, text: str) -> None:
        self.result.set_text(text or "Done.")
        self.result.set_css_classes(["alpaca-commitresult", "ok" if ok else "err"])
        self.result.set_visible(True)

    def _on_msg_enter(self, entry) -> None:
        if self.commit_btn.get_sensitive():
            self.commit_clicked()

    def has_files(self) -> bool:
        return len(self._files) > 0