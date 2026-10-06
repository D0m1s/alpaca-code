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
from gi.repository import GdkPixbuf, GLib, Gtk, Pango

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
        self.on_status = lambda kind, text: None  # window → filetree.show_git_status
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
                                   bool, bool, bool, bool, bool)          # checked, inconsist, badge-vis, chip-vis, toggle-vis
        # (letters bind the VALUE: blank pixbuf when absent — slots must not free.
        #  badge-vis/chip-vis are the per-row VISIBILITY lenses above, set in _fill.)
        self.view = HoverTree(model=self.store, headers_visible=False)
        self.view.set_activate_on_single_click(True)
        self.view.set_property("show-expanders", False)
        self.view.set_level_indentation(16)
        self.view.set_tooltip_column(1)

        # ONE column: [chev|checkbox|letter|badge+name] — all visual cells pack
        # into the tree column so level indentation shifts the whole strip
        # together (probe_gtvc: only col-0 cell areas indent); store cols
        # untouched so every toggle/sync lens above keeps its index.
        # Gap law (2026-10-05): WORKSPACE sets xpad 2 on every cell — 4px
        # art-to-art; CHANGES' let/bad/name cells had none (gaps 0-2px) and the
        # blank slots in dir/Select-all rows floated the checkbox ~20-35px.
        # xpad 2 everywhere = the uniform 4px (user ruling), and let/bad go
        # PER-ROW INVISIBLE on rows without chips — this GTK drops a hidden
        # cell's width per row (probe: hidden row's text sat 20px left of its
        # twins), so the checkbox hugs folder/label at the same 4px.
        cell_name = Gtk.CellRendererText(); cell_name.set_property("ypad", 2)
        cell_name.set_property("ellipsize", Pango.EllipsizeMode.MIDDLE)
        cell_name.set_property("xpad", 2)    # WORKSPACE parity: ink at art+4
        chev = Gtk.CellRendererPixbuf();  chev.set_property("ypad", 2)
        chev.set_property("xpad", 2)             # 12px art + 2px pad = 16: the toggle slot starts after it
        chev.set_property("yalign", 2.0 / 3.0)   # bottom-pin: same law as filetree's chevron
        bad = Gtk.CellRendererPixbuf();  bad.set_property("ypad", 2)
        let = Gtk.CellRendererPixbuf();  let.set_property("ypad", 2)
        # pixbuf cells center glyphs ~1px high vs text ink (the filetree bug, now
        # fixed there): bottom-pin chips +1px at the 22px row, before any insert.
        bad.set_property("xpad", 2)
        bad.set_property("yalign", 1.0)
        let.set_property("xpad", 2)
        let.set_property("yalign", 1.0)
        tog = Gtk.CellRendererToggle(); tog.set_property("activatable", False)
        col = Gtk.TreeViewColumn()               # kwargs form raises on this build
        col.pack_start(chev, False)
        col.add_attribute(chev, "pixbuf", 4)
        col.pack_start(tog, False)
        col.add_attribute(tog, "active", 7)
        col.add_attribute(tog, "inconsistent", 8)
        col.add_attribute(tog, "visible", 11)
        col.pack_start(let, False)
        col.add_attribute(let, "pixbuf", 6)
        col.add_attribute(let, "visible", 10)    # chip only on file rows (per-row collapse)
        col.pack_start(bad, False)
        col.add_attribute(bad, "pixbuf", 5)
        col.add_attribute(bad, "visible", 9)     # folder/file badge; off on masthead rows
        col.pack_start(cell_name, True)
        col.add_attribute(cell_name, "text", 0)
        col.set_expand(True)   # spec §29: name column absorbs the spare width —
        # an ellipsized cell's natural is its MINIMUM (col0 negotiation measured
        # 54px + the toggle column swallowing ~346px of slack), so without this
        # letter chips could ride mid-panel next to 4-glyph names
        self._col = col
        self._tog = tog
        self._tog_w = None                       # toggle slot width, measured lazily
        self.view.append_column(col)
        self.view.set_css_classes(["alpaca-tree"])
        self.view.connect("row-expanded", self._on_expand_toggle, True)
        self.view.connect("row-collapsed", self._on_expand_toggle, False)
        self.view.connect("row-activated", self._on_activated)   # toggle clicks route here too
        for sig in ("row-inserted", "row-deleted"):   # keep the band on the row under a stationary pointer
            self.store.connect(sig, lambda *a: self.view._refresh_hover())

        self.append(Gtk.ScrolledWindow(vexpand=True, child=self.view))

        bar = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6,
                      margin_start=12, margin_end=12, margin_top=6, margin_bottom=6)
        bar.set_css_classes(["alpaca-commitbar"])
        self.msg = Gtk.Entry()
        self.msg.set_placeholder_text("Commit message…")
        self.msg.set_css_classes(["alpaca-msg"])
        self.msg.set_hexpand(True)
        self.msg.set_size_request(-1, 30)
        self.msg.connect("activate", lambda _e: self.commit_clicked())   # Enter = commit
        self.btn = Gtk.Button(label="Commit and Push")
        self.btn.set_css_classes(["alpaca-barbtn"])
        self.btn.set_hexpand(True)           # full-width pill: same width as the field above
        self.btn.connect("clicked", lambda b: self.commit_clicked())
        self._ahead = 0                        # local-unpushed count, cached by refresh()
        bar.append(self.msg); bar.append(self.btn)
        self.append(bar)

    # ---- data ------------------------------------------------------------------
    def set_root(self, root: str | None) -> None:
        self.root = root
        self._closed.clear()
        self._needle = ""                          # new repo: the old filter is meaningless
        self.refresh(keep_selection=False)         # a new repo = fresh selection

    def refresh(self, keep_selection: bool = True) -> None:
        """Sync entry (set_root, commit-done): the git calls run HERE; landing
        is apply(). Fresh view selects all (spec §2); a refresh INTERSECTS the
        surviving selection with the new file set (deviation 1) — after a commit
        the committed paths vanish and the rest stay selected. An active filter
        re-applies."""
        raw = gitstatus.changes(self.root)
        self.apply(raw, gitstatus.ahead(self.root) if raw is not None else 0,
                   keep_selection)

    def apply(self, raw: list | None, ahead: int = 0, keep_selection: bool = True) -> None:
        """Landing pad for fresh git data — the sync refresh above, or the live
        probe's worker thread via idle_add (filetree._git_landed). Rebuilds rows,
        intersects the selection with the surviving file set, re-applies any
        active filter. None (no repo / timeout) is a clean no-op."""
        if raw is None:
            return
        self._ahead = ahead                      # cached: no git per toggle
        self._rows = raw
        files = {r[2] for r in gitstatus.group_tree(raw) if r[0] == "f"}   # r[2] = rel (group_tree: kind,name,rel,letter,depth)
        self._files = set(files)                 # BEFORE _fill: _sync_row needs it
        self._checked = (self._checked & files) if keep_selection else set(files)
        self._fill(self._filtered(raw))

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
                                     badges.blank_pixbuf(), badges.blank_pixbuf(), False, False,
                                     False, False, False])
            self._sync()
            return
        self.store.append(None, ["Select all", "", "s", "", badges.blank_pixbuf(),
                                 badges.blank_pixbuf(), badges.blank_pixbuf(), False, False,
                                 False, False, True])
        iters: dict[str, object] = {}
        for kind, name, rel, letter, _d in gitstatus.group_tree(rows):
            is_dir = kind == "d"
            badge = badges.folder_pixbuf() if is_dir else badges.pixbuf_for(name)
            lpix = (badges.letter_pixbuf(letter, LETTER_COLOR.get(letter, LETTER_DEFAULT))
                    if letter else badges.blank_pixbuf())
            parent = iters.get(rel.rsplit("/", 1)[0]) if "/" in rel else None
            it = self.store.append(parent, [
                name, rel, kind, letter,
                badges.chevron_pixbuf(False) if is_dir else badges.blank_pixbuf(),
                badge or badges.blank_pixbuf(), lpix,
                False, False, True, not is_dir, True])   # badge-vis (files+dirs), chip-vis (file rows: a letterless file keeps its slot)
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
        # one button: commit-with-push when files are checked, push-only when the
        # branch is ahead (nothing checked); ahead comes from the refresh cache
        self.btn.set_sensitive(not self._busy and (bool(self._checked) or self._ahead > 0))

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
            self.store[it][4] = badges.chevron_pixbuf(expanded)   # dir chevron state
            if expanded:                         # hidden dirs don't expand (expand_row is
                cin = self.store.iter_children(it)   # no-op under a collapsed ancestor) — catch them up
                while cin is not None:
                    if self.store[cin][2] == "d" and self.store[cin][1] not in self._closed:
                        self.view.expand_row(self.store.get_path(cin), False)
                    cin = self.store.iter_next(cin)

    # ---- activation (spec §2: file click opens the diff) ------------------------
    def _on_activated(self, view, tpath, col) -> None:
        if self._toggle_hit(tpath):              # checkbox clicks toggle only (Review Focus #1)
            self._on_toggled(self._tog, tpath.to_string())
            return
        row = self.store[tpath]
        if row[2] == "d" and row[1]:
            if row[1] in self._closed:
                view.expand_row(tpath, False)        # children are pre-loaded (non-lazy store)
            else:
                view.collapse_row(tpath)
        elif row[2] == "f":
            self.on_open(row[1], row[3])         # (rel, letter) — chip needs the letter

    def _toggle_hit(self, tpath) -> bool:
        """Press inside the toggle's own slot? With one merged column, the
        checkbox is found by press geometry: [chevron slot end, + toggle nat)."""
        px, _py = self.view._press_xy
        if px < 0:
            return False
        if self._tog_w is None:                  # measured once (24 with xpad 3)
            try:
                self._tog_w = self._tog.get_preferred_width(self.view)[1]
            except Exception:
                self._tog_w = 24
        ca = self.view.get_cell_area(tpath, self._col)
        if ca is None:
            return False
        x0 = ca.x + 16                           # chevron slot: 12px art + 2px pad (measured)
        return x0 <= px < x0 + self._tog_w

    # ---- commit+push (spec §4: workers are daemon threads; GTK via idle_add) ----
    def commit_clicked(self) -> None:
        if self._busy or not self.root:
            return
        paths = sorted(self._checked)
        if not paths and self._ahead <= 0:
            self.on_status("err", "No files selected")
            return
        msg = self.msg.get_text().strip()
        if paths and not msg:
            self.on_status("err", "Empty commit message")
            return
        if paths:
            self.before_commit(paths)    # window: flush dirty editor buffers for exactly these files
        root = self.root                 # snapshot: a set_root() during the flight must not reroute it (review I4)
        self._set_busy(True)
        self.on_status("busy", "Committing…" if paths else "Pushing…")
        threading.Thread(target=self._run_commit_push, args=(root, paths, msg),
                         daemon=True, name="alpaca-push").start()

    def _run_commit_push(self, root, paths, msg) -> None:
        cok, ctext, pok, ptext = gitstatus.commit_then_push(
            root, paths, msg, self._phase if paths else None)
        GLib.idle_add(self._done, cok, ctext, pok, ptext)

    def _phase(self, kind: str, text: str) -> None:
        GLib.idle_add(self.on_status, kind, text)

    def _done(self, cok, ctext, pok, ptext) -> bool:
        self._set_busy(False)
        if cok:
            self.msg.set_text("")        # user ruling: the message dies with a landed commit
            self.refresh()               # keep_selection=True: committed paths vanish, rest kept
        if not cok:
            self.on_status("err", ctext)
        elif not pok:                    # commit landed; list refreshed — the row re-syncs on
            self.on_status("err", ptext)  # the next natural refresh (the red line explains the count)
        else:
            self.on_status("ok", "Pushed ✓")   # row pulse; filetree re-syncs itself after 2s
        return False                     # idle_add: run once

    def _set_busy(self, busy: bool) -> None:
        self._busy = busy
        self._buttons()
        self.msg.set_sensitive(not busy)

    def has_files(self) -> bool:
        return len(self._files) > 0