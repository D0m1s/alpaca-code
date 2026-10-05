# Tabbed editor: GtkSourceView pages, custom tab headers, Ctrl+S, file-deleted dialogs.
import os
import alpaca_code.gi_env as ge
ge.require("Gdk", ("4.0",))
ge.require("Gtk", ("4.0",))
ge.require("GtkSource", ("5",))
ge.require("Pango", ("1.0",))
from gi.repository import Gdk, Gio, GLib, Gtk, GtkSource, Pango

from . import badges

class _TabDistributor(Gtk.BoxLayout):
    """Equal-share tab widths (VSCode semantics). Measured on this build:
    GTK 4.22 allocates scrollable-notebook tabs at their MINIMUM width, never
    natural, and the scrolled tail widens as the strip narrows — so text
    visibility is inversely coupled to window size. Fix: set per-tab size
    requests from the strip's live allocation inside this layout pass — every
    tab gets avail/n (capped by its label's natural): full text when tabs fit,
    proportional shrinking text as the card narrows, arrows only past n*CHROME.
    No floor: one below the share re-enters scroll mode. Recomputed only when
    strip width or tab count changes (a share below CHROME leaves labels blank
    and scrolled-out tabs at 0 — stock behavior again, acceptable there)."""
    CHROME = 62   # label → tab box: badge slot + ✕ + label padding (measured
                  # @ 13px type; no inter-tab margin — pills touch)

    def __init__(self, editor):
        super().__init__()
        self._editor = editor
        self._last = None

    def do_allocate(self, widget, w, h, baseline):
        Gtk.BoxLayout.do_allocate(self, widget, w, h, baseline)
        ed = self._editor
        n = ed.nb.get_n_pages()
        if n == 0 or (w, n) == self._last:
            return   # unchanged: no size_request churn, no re-allocation loop
        self._last = (w, n)
        act = ed.nb.get_action_widget(Gtk.PackType.END)
        act_w = act.measure(Gtk.Orientation.HORIZONTAL, -1)[1] if act else 0
        req = max((w - act_w) // n - self.CHROME, 0)      # header pad is 0
        for i in range(n):
            tl = ed.nb.get_tab_label(ed.nb.get_nth_page(i))
            c = tl.get_first_child() if tl else None
            while c:
                if "alpaca-tabname" in c.get_css_classes():
                    nat = c.measure(Gtk.Orientation.HORIZONTAL, -1)[1]
                    c.set_size_request(min(req, nat), -1)
                    break
                c = c.get_next_sibling()

def readable_text(raw: bytes) -> str | None:
    """Text the editor can represent losslessly, else None: NUL bytes (GtkSource
    truncates the buffer at \\0) or invalid UTF-8 (would resave as U+FFFD garbage)."""
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError:
        return None
    if "\x00" in text:
        return None
    return text

# side-by-side tints (spec §3): sit under #e6e8ee text on the #0d1017 card
DIFF_DEL_BG = "#25181c"
DIFF_ADD_BG = "#15261d"
DIFF_HDR_FG = "#5a6375"
# live-file tombstones (2026-10-05 spec): the muted italic body an editor tab
# shows when the disk leaves it behind (no popups); #5a6375 = the DIFF_HDR_FG gray
GONE_NOTE = "‹file has been deleted›"
BINARY_NOTE = "‹file changed on disk — not UTF-8 text›"

class Editor(Gtk.Box):
    def __init__(self):
        super().__init__(orientation=Gtk.Orientation.VERTICAL)
        self.set_css_classes(["alpaca-card"])
        self.set_overflow(Gtk.Overflow.HIDDEN)   # clip children to the card's rounded corners
        self.root = None
        self.on_state_changed = lambda: None
        self.lm = GtkSource.LanguageManager.get_default()
        self.scheme = GtkSource.StyleSchemeManager.get_default().get_scheme("alpaca-dark")
        # scrollable: a non-scrollable notebook's min width grows with every open
        # tab; GtkPaned propagates no child minimums, so the window just clips
        self.nb = Gtk.Notebook(vexpand=True, hexpand=True, scrollable=True)
        self._watchers: dict[str, Gio.FileMonitor] = {}
        self._pending: dict[str, int] = {}   # path → debounce timer (live-file spec)
        # wrapper box carries the tab distributor: its layout pass sees the strip's
        # live width and floors per-tab shares (GTK pins scrollable tabs at min)
        self._tabs_wrap = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)  # vertical: the nb
        # must take the full row width — in a horizontal Box an expand-less child
        # is allocated only its own minimum (measured: 900px wrap → 280px nb)
        self._tabs_wrap.set_layout_manager(_TabDistributor(self))
        self._tabs_wrap.append(self.nb)
        self.append(self._tabs_wrap)
        self.nb.connect("switch-page", self._on_switch)

        trig = Gtk.ShortcutTrigger.parse_string("<Control>s")
        act = Gtk.CallbackAction.new(self._on_s)
        sc = Gtk.ShortcutController()
        sc.add_shortcut(Gtk.Shortcut(trigger=trig, action=act))
        self.add_controller(sc)

    # ---- ctrl+s -------------------------------------------------------------
    def _on_s(self, widget, arg) -> bool:
        self.save_active()
        return True

    def _on_switch(self, nb, page, index):
        self._refresh_tab_state(index)

    # ---- lifecycle ------------------------------------------------------------
    def set_root(self, root: str) -> None:
        self.root = root
        self._close_all()

    def restore(self, root: str, saved: dict) -> None:
        self.set_root(root)
        for rel in saved.get("open_tabs", []):
            p = os.path.join(root, rel)
            if os.path.isfile(p):                      # stale persisted tabs vanish silently
                self.open_file(p)
        self.nb.set_current_page(min(saved.get("active_tab", 0), max(self.nb.get_n_pages() - 1, 0)))
        self._refresh_tab_state()

    # ---- open -----------------------------------------------------------------
    def open_file(self, path: str) -> None:
        k = self._page_of(path)
        if k != -1:
            self.nb.set_current_page(k)
            self.on_state_changed()
            return
        try:
            with open(path, "rb") as f:
                raw = f.read()
        except (OSError, ValueError) as e:
            if not isinstance(e, FileNotFoundError):
                self._error("Couldn't open file", f"{os.path.basename(path)}: {e}")
                return
            raw = None    # vanished since the tree rendered it → tombstone tab, never a popup
        if raw is not None:
            text = readable_text(raw)
            if text is None:
                # refusing at open is the only safe point: a lossy buffer + Ctrl+S
                # would rewrite the file with truncated/replacement garbage
                self._error("Unsupported file", f"{os.path.basename(path)}: binary, NUL bytes, or not UTF-8")
                return
        else:
            text = GONE_NOTE
        buf = GtkSource.Buffer(text=text)
        if self.scheme:
            buf.set_style_scheme(self.scheme)
        lang = self.lm.guess_language(os.path.basename(path), None)
        if lang:
            buf.set_language(lang)
        buf.set_modified(False)     # GtkSource.Buffer(text=...) is born modified — an open must not show the dirty dot
        if raw is None:
            self._tag_muted(buf)
        buf.connect("modified-changed", lambda b: self._dirty_dot(b))
        view = GtkSource.View(buffer=buf, show_line_numbers=True)
        view.set_wrap_mode(Gtk.WrapMode.NONE)
        view.set_pixels_above_lines(2)   # line pitch ~20 @ 13px mono (VS Code density)
        view.set_left_margin(12)         # code column inset after the line-number gutter
        view.set_css_classes(["alpaca-mono"])
        page = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        rel = os.path.relpath(path, self.root) if self.root else path
        page.append(Gtk.Label(label="  ›  ".join(rel.split(os.sep)),
                              xalign=0.0,
                              ellipsize=Pango.EllipsizeMode.MIDDLE,   # a deep path must not widen the page min
                              css_classes=["alpaca-breadcrumb"]))
        sw = Gtk.ScrolledWindow(vexpand=True, child=view)   # ref kept: reload restores its scroll
        page.append(sw)
        page.path = path                                # attrs for lookup/save
        page.buf = buf
        page.sw = sw
        page.view = view
        page._fs = None if raw is not None else "deleted"
        page._conflict = False
        try:
            page._load_mtime = os.stat(path).st_mtime_ns if raw is not None else None
        except OSError:
            page._load_mtime = None

        dirty = Gtk.Label(label="●"); dirty.set_css_classes(["alpaca-dirty"]); dirty.set_visible(False)
        name = Gtk.Label(label=os.path.basename(path)); name.set_css_classes(["alpaca-tabname"])
        # tabname floor: GTK 4.22 pins scrollable-notebook tabs at their minimum
        # even with space to spare — bare ellipsize floors the text at '…', and
        # GTK4 GtkLabel has no min-width-chars (GTK3-only). 72 (~9ch @ 13px) is the
        # pre-map floor; once mapped, _TabDistributor recomputes it per share.
        name.set_ellipsize(Pango.EllipsizeMode.MIDDLE)
        name.set_size_request(72, -1)
        close = Gtk.Button()
        cpix = badges.icon("x-dim.svg")
        if cpix:
            close.set_child(Gtk.Image.new_from_paintable(cpix))
        else:
            close.set_child(Gtk.Image(icon_name="window-close-symbolic"))
        close.set_css_classes(["alpaca-close"])
        head = Gtk.Box(spacing=4); head.set_css_classes(["alpaca-tab"]); head.set_valign(Gtk.Align.CENTER)
        head.append(dirty)                       # must stay first — _dirty_dot reads get_first_child()
        name_slot = Gtk.Box(spacing=4)           # per-file type badge; restamped by _refresh_tab_state
        name_slot.set_valign(Gtk.Align.CENTER)
        head.append(name_slot)
        head.append(name); head.append(close)
        page.badge_slot = name_slot
        index = self.nb.append_page(page, head)
        self.nb.set_current_page(index)  # GTK4 append does not switch; a new tab must take focus
        close.connect("clicked", lambda b, pg=page: self.close_index(self.nb.page_num(pg)))
        self._watch(path)
        if page._fs:
            self._paint_tab_deleted(page, True)
        self._refresh_tab_state()   # switch-page alone misses opens that don't change pages / back-switches
        self.on_state_changed()

    # ---- diff pages (spec §3) ---------------------------------------------------
    def _diff_side(self, lines: list[str], idxs: set[int], hdr_idx: set[int],
                   hexcol: str, lang) -> GtkSource.View:
        buf = GtkSource.Buffer(text="\n".join(lines))
        if self.scheme:
            buf.set_style_scheme(self.scheme)
        if lang:
            buf.set_language(lang)
        bg = buf.create_tag(background_rgba=self._rgba(hexcol))
        fg = buf.create_tag(foreground_rgba=self._rgba(DIFF_HDR_FG))
        for i in sorted(idxs):
            if i >= buf.get_line_count() or i < 0:
                continue
            _, a = buf.get_iter_at_line(i)      # this build: returns (ok, iter)
            b = a.copy(); b.forward_to_line_end()
            buf.apply_tag(bg, a, b)
        for i in sorted(hdr_idx):
            if i >= buf.get_line_count() or i < 0:
                continue
            _, a = buf.get_iter_at_line(i)
            b = a.copy(); b.forward_to_line_end()
            buf.apply_tag(fg, a, b)
        buf.set_modified(False)   # born modified (invariant); diff tabs never dirty
        v = GtkSource.View(buffer=buf)
        v.set_show_line_numbers(False)   # padded sides would falsify numbers (spec ceiling)
        v.set_editable(False)
        v.set_wrap_mode(Gtk.WrapMode.NONE)
        v.set_pixels_above_lines(2)
        v.set_left_margin(12)
        v.set_css_classes(["alpaca-mono"])
        return v

    def _rgba(self, hexcol: str) -> Gdk.RGBA:
        c = Gdk.RGBA()
        c.parse(hexcol)
        return c

    def open_diff(self, rel: str, sidetuple: tuple, letter: str = "") -> None:
        """Diff tab like a file tab: breadcrumb + two mono views, tinted (spec §3).
        sidetuple = (old_lines, new_lines, del_idx, add_idx, hdr_idx) from build_sides."""
        key = f"diff:{rel}"
        k = self._page_of(key)
        if k != -1:
            self.nb.set_current_page(k)
            return
        old, new, del_idx, add_idx, hdr_idx = sidetuple
        lang = self.lm.guess_language(os.path.basename(rel), None)
        vl = self._diff_side(old, del_idx, hdr_idx, DIFF_DEL_BG, lang)
        vr = self._diff_side(new, add_idx, hdr_idx, DIFF_ADD_BG, lang)
        # per-side internal scrolling + vadjustment cross-link: equal-value
        # set_value fires no changed signal → no loop, and per-side sw keeps
        # h-scroll per side (single sw + 2 views: h-scrollers lie about each side's range)
        sw_l = Gtk.ScrolledWindow(hexpand=True, vexpand=True, child=vl)
        sw_r = Gtk.ScrolledWindow(hexpand=True, vexpand=True, child=vr)
        sep = Gtk.Box(css_classes=["alpaca-diffsep"])
        pair = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        pair.append(sw_l); pair.append(sep); pair.append(sw_r)
        page = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        page.append(Gtk.Label(label="  ›  ".join(rel.split("/")), xalign=0.0,
                              ellipsize=Pango.EllipsizeMode.MIDDLE,
                              css_classes=["alpaca-breadcrumb"]))
        page.append(pair)
        page.diff_of = f"diff:{rel}"
        page.diff_letter = letter                        # tracked by refresh_diff's chip
        page.diff_sw = (sw_l, sw_r)                      # sides the live diff renewal swaps
        slot = Gtk.Box(spacing=4); slot.set_valign(Gtk.Align.CENTER)
        if letter:
            slot.append(Gtk.Label(label=letter,
                                  css_classes=[f"alpaca-diffchip{letter.lower()}"]))
        page.badge_slot = slot          # same attr name _refresh_tab_state reads
        name = Gtk.Label(label=os.path.basename(rel))
        name.set_css_classes(["alpaca-tabname"])
        name.set_ellipsize(Pango.EllipsizeMode.MIDDLE)
        name.set_size_request(72, -1)
        close = Gtk.Button()
        cpix = badges.icon("x-dim.svg")
        if cpix:
            close.set_child(Gtk.Image.new_from_paintable(cpix))
        else:
            close.set_child(Gtk.Image(icon_name="window-close-symbolic"))
        close.set_css_classes(["alpaca-close"])
        head = Gtk.Box(spacing=4); head.set_css_classes(["alpaca-tab"]); head.set_valign(Gtk.Align.CENTER)
        head.append(slot); head.append(name); head.append(close)
        idx = self.nb.append_page(page, head)
        self.nb.set_current_page(idx)   # append does not switch (invariant)
        close.connect("clicked", lambda b, pg=page: self.close_index(self.nb.page_num(pg)))
        # Cross-link (comment on _wire_diff_scroll): wired via methods so the live
        # diff renewal can re-wire fresh views identically.
        self._wire_diff_scroll(vl, vr)
        self._wire_diff_scroll(vr, vl)
        self._refresh_tab_state()   # restamp pass now SKIPS diff pages (guard), chip preserved
        self.on_state_changed()

    # ---- live diff renewal (2026-10-05 spec) -------------------------------------
    def diff_pages(self) -> list:
        """(rel, letter) pairs of the open diff tabs — window._on_git_changed's map."""
        out = []
        for i in range(self.nb.get_n_pages()):
            page = self.nb.get_nth_page(i)
            if getattr(page, "diff_of", None):
                out.append((page.diff_of[5:], getattr(page, "diff_letter", "")))
        return out

    def refresh_diff(self, rel: str, sides: tuple, letter: str) -> None:
        """Swap both side buffers in place (widget tree and tab strip stay put):
        per-side scroll is kept as a content fraction, fresh views are re-wired
        for cross-linking (their ScrolledWindow swaps the adj on first alloc,
        same as a fresh page)."""
        k = self._page_of(f"diff:{rel}")
        if k == -1:
            return
        page = self.nb.get_nth_page(k)
        fracs = []
        for sw in page.diff_sw:
            adj = sw.get_vadjustment()
            span = max(adj.get_upper() - adj.get_page_size(), 0.0)
            fracs.append(min(adj.get_value() / span, 1.0) if span else 0.0)
        old_l, new_l, dl, al, hdr = sides
        lang = self.lm.guess_language(os.path.basename(rel), None)
        view_l = self._diff_side(old_l, dl, hdr, DIFF_DEL_BG, lang)
        view_r = self._diff_side(new_l, al, hdr, DIFF_ADD_BG, lang)
        page.diff_sw[0].set_child(view_l)
        page.diff_sw[1].set_child(view_r)
        self._wire_diff_scroll(view_l, view_r)
        self._wire_diff_scroll(view_r, view_l)
        for swm, frac in zip(page.diff_sw, fracs):
            GLib.idle_add(self._scroll_frac, swm, frac)   # adj re-lands with the new allocation
        if letter != getattr(page, "diff_letter", ""):    # the chip follows the change (M→R/D…)
            page.diff_letter = letter
            slot = page.badge_slot
            c = slot.get_first_child()
            while c:
                nxt = c.get_next_sibling()
                slot.remove(c); c = nxt
            if letter:
                slot.append(Gtk.Label(label=letter,
                                      css_classes=[f"alpaca-diffchip{letter.lower()}"]))
        self.on_state_changed()

    def _scroll_frac(self, sw, frac: float) -> bool:
        adj = sw.get_vadjustment()
        span = max(adj.get_upper() - adj.get_page_size(), 0.0)
        adj.set_value(min(frac * span, span))    # unallocated adj reads 0-span → top (acceptable for brief churn)
        return False

    # ---- cross-linking two diff side views ---------------------------------------
    def _diff_link(self, v, dst) -> bool:
        live = v.get_vadjustment()
        if getattr(v, "_linked", None) is live:
            return False                     # already on the live adj
        v._linked = live
        # equal-value set_value fires no changed signal → no loop
        live.connect("value-changed", lambda a, o=dst: o.get_vadjustment().set_value(a.get_value()))
        return True

    def _diff_resync(self, v, dst) -> bool:
        if self._diff_link(v, dst):
            GLib.idle_add(lambda: self._diff_link(v, dst) or False)  # cover a post-emit swap in the same frame
        return False                          # handler return value (keep watching)

    def _wire_diff_scroll(self, v, dst) -> None:
        """Cross-link: the ScrolledWindow swaps the view's placeholder adjustment
        for its own at the page's first allocation, WITHOUT any property notify
        (measured on this build: notify::vadjustment never fires on view or sw) —
        so retry the link on every allocation change, deduped by pointer
        identity; handlers fetch the CURRENT live adj at event time."""
        self._diff_link(v, dst)
        v.connect("notify::allocation", lambda vv, o=dst: self._diff_resync(vv, o))

    def _page_of(self, path: str) -> int:
        for i in range(self.nb.get_n_pages()):
            page = self.nb.get_nth_page(i)
            if getattr(page, "path", None) == path or getattr(page, "diff_of", None) == path:
                return i
        return -1

    def _refresh_tab_state(self, active: int | None = None) -> None:
        """Every tab carries its file's type badge (user overrode the mockup's
        hash-for-inactive rule: a selected tab must not erase the others' icons).
        Kept for open/close restamps; uniforms after that."""
        # index — get_nth_page returns a FRESH wrapper per call, so `page is x` lies;
        # and switch-page arrives BEFORE get_current_page catches up (probe-measured,
        # the stale value kept the chip on the previously active tab) — hence callers
        # pass the index where they have it.
        if active is None:
            active = self.nb.get_current_page()
        for i in range(self.nb.get_n_pages()):
            page = self.nb.get_nth_page(i)
            if getattr(page, "diff_of", None) is not None:
                continue        # diff tab: badge_slot already holds its letter chip
            slot = getattr(page, "badge_slot", None)
            if slot is None:
                continue
            want = badges.pixbuf_for(os.path.basename(page.path))
            c = slot.get_first_child()          # GtkContainer.foreach is gone in GTK4
            while c:
                nxt = c.get_next_sibling()
                slot.remove(c)
                c = nxt
            if want:
                slot.append(Gtk.Image.new_from_paintable(Gdk.Texture.new_for_pixbuf(want)))
        self.on_state_changed()

    def _dirty_dot(self, buf) -> None:
        for i in range(self.nb.get_n_pages()):
            page = self.nb.get_nth_page(i)
            if getattr(page, "buf", None) is buf:
                self._paint_dot(page)
        self.on_state_changed()

    def _paint_dot(self, page) -> None:
        """Tab-head dirty dot: red while locally modified, amber while ALSO
        changed on disk (conflict — live spec: no dialog, next save wins)."""
        head = self.nb.get_tab_label(page)
        if head is None:
            return
        dot = head.get_first_child()
        conflict = getattr(page, "_conflict", False)
        dot.set_visible(page.buf.get_modified() or conflict)
        dot.set_css_classes(["alpaca-dirty"] + (["conflict"] if conflict else []))

    def _tabname_label(self, page):
        head = self.nb.get_tab_label(page)
        c = head.get_first_child() if head else None
        while c:
            if "alpaca-tabname" in c.get_css_classes():
                return c
            c = c.get_next_sibling()
        return None

    def _paint_tab_deleted(self, page, on: bool) -> None:
        lab = self._tabname_label(page)
        if lab is None:
            return
        if on:
            lab.add_css_class("alpaca-deleted")
        else:
            lab.remove_css_class("alpaca-deleted")

    # ---- close -----------------------------------------------------------------
    def close_index(self, index: int, force: bool = False) -> None:
        self._close_index(index, force)

    def _close_index(self, index: int, force: bool = False) -> None:
        page = self.nb.get_nth_page(index)
        if page is None:
            return
        buf = getattr(page, "buf", None)
        if buf is not None and buf.get_modified() and not force:
            d = Gtk.MessageDialog(transient_for=self.get_root(), modal=True,
                                  text=f"Discard changes to {os.path.basename(page.path)}?",
                                  buttons=Gtk.ButtonsType.CANCEL)
            d.add_button("Discard", Gtk.ResponseType.ACCEPT)
            d.connect("response", lambda dd, r: (dd.destroy(), self._close_index(index, True))
                      if r == Gtk.ResponseType.ACCEPT else dd.destroy())
            d.present()
            return
        self.nb.remove_page(index)
        self._cancel_watch(getattr(page, "path", ""))
        self._refresh_tab_state()   # refires on_state_changed; closing the active tab flips tabs

    def _close_all(self) -> None:
        for i in range(self.nb.get_n_pages() - 1, -1, -1):
            page = self.nb.get_nth_page(i)
            self.nb.remove_page(i)
            self._cancel_watch(getattr(page, "path", ""))

    def has_dirty(self) -> bool:
        for i in range(self.nb.get_n_pages()):
            buf = getattr(self.nb.get_nth_page(i), "buf", None)
            if buf is not None and buf.get_modified():
                return True
        return False

    # ---- save --------------------------------------------------------------------
    def _write_page(self, page) -> None:
        with open(page.path, "w", encoding="utf-8") as f:
            f.write(page.buf.props.text)
        page._conflict = False              # saved: our text is the disk's text again
        page.buf.set_modified(False)        # the fired modified-changed repaints the dot (order matters)
        try:
            page._load_mtime = os.stat(page.path).st_mtime_ns   # latch the save
            # echo — the settle must not "reload" what we just wrote
        except OSError:
            pass
        # ponytail: sync stdlib write, as save_active was

    def save_active(self) -> None:
        page = self.nb.get_nth_page(self.nb.get_current_page())
        if page is None or getattr(page, "path", None) is None:
            return            # diff page: Ctrl+S is a no-op; there is no dirty buffer
        if not page.buf.get_modified():
            return            # clean Ctrl+S is a no-op — what can never resurrect a tombstone note
        try:
            self._write_page(page)
        except OSError as e:
            self._error("Couldn't save", f"{os.path.basename(page.path)}: {e}")

    def save_open(self, paths: list[str]) -> None:
        """Commit flow (spec §5): flush dirty buffers among the named abs paths."""
        for i in range(self.nb.get_n_pages()):
            page = self.nb.get_nth_page(i)
            p = getattr(page, "path", None)
            if p in paths and page.buf.get_modified():
                self._write_page(page)

    # ---- live file changes (2026-10-05 spec): no popups, disk is the truth -----
    def _watch(self, path: str) -> None:
        self._cancel_watch(path)
        try:
            mon = Gio.File.new_for_path(path).monitor_file(0, None)
        except GLib.Error:
            return      # an absent path may have no watchable inode: tombstone stays until reopened
        mon.connect("changed", self._on_file_event, path)
        self._watchers[path] = mon

    def _cancel_watch(self, path: str) -> None:
        m = self._watchers.pop(path, None)
        if m:
            m.cancel()

    def _on_file_event(self, mon, f, other, event, path) -> None:
        # No popups (live spec). Every event — delete, atomic replace (agents save
        # via temp+rename), re-create — funnels into ONE debounced disk check.
        src = self._pending.pop(path, None)
        if src:
            GLib.source_remove(src)
        self._pending[path] = GLib.timeout_add(120, self._fs_settled, path)

    def _fs_settled(self, path: str) -> bool:
        k = self._page_of(path)
        if k == -1:
            return False
        page = self.nb.get_nth_page(k)
        try:
            mtime = os.stat(path).st_mtime_ns
        except OSError:
            self._fs_tombstone(page)
            return False
        if page._fs is None and mtime == page._load_mtime:
            return False                      # our own open/Ctrl+S echo
        if page.buf.get_modified():
            if page._fs is not None:          # dirty buffer on a placeholder tab: the note dies, text lives
                page._fs = None
                self._paint_tab_deleted(page, False)
            page._conflict = True
            self._paint_dot(page)
            return False
        self._reload(page, mtime)             # clean buffer follows the disk (also un-tombstones)
        return False

    def _reload(self, page, mtime: int) -> None:
        """Disk is the truth for a clean buffer: seamless swap (style/language
        carried, cursor line/col and scroll value preserved), state cleared,
        save-echo latched to the new mtime."""
        try:
            with open(page.path, "rb") as f:
                raw = f.read()
        except OSError:
            self._fs_tombstone(page)
            return
        text = readable_text(raw)
        if text is None:
            self._fs_binary(page)
            return
        ins = page.buf.get_iter_at_mark(page.buf.get_insert())
        line, col = ins.get_line(), ins.get_line_offset()
        scroll = page.sw.get_vadjustment().get_value()
        self._swap_text(page, text)
        n = page.buf.get_line_count()
        _, it = page.buf.get_iter_at_line(min(line, n - 1))
        it.forward_chars(min(col, max(it.get_chars_in_line() - 1, 0)))
        page.buf.place_cursor(it)
        page._fs = None
        page._conflict = False
        page._load_mtime = mtime
        page.view.set_editable(True)
        self._paint_tab_deleted(page, False)
        GLib.idle_add(self._scroll_back, page, scroll)
        self._paint_dot(page)
        self.on_state_changed()

    def _swap_text(self, page, text: str) -> None:
        """Fresh buffer (style + language carried over); the view keeps its place."""
        old = page.buf
        nb = GtkSource.Buffer(text=text)
        sch = old.get_style_scheme()
        if sch:
            nb.set_style_scheme(sch)
        lang = old.get_language()
        if lang:
            nb.set_language(lang)
        nb.set_modified(False)      # born modified (invariant) — a reload must not raise the dot
        nb.connect("modified-changed", lambda b: self._dirty_dot(b))
        page.view.set_buffer(nb)
        page.buf = nb

    def _fs_tombstone(self, page) -> None:
        """File gone while open: italic, dimmed tab; a CLEAN buffer becomes the
        tombstone note, a DIRTY one keeps the user's text (Ctrl+S recreates the
        file). A later re-created file un-tombstones via _reload."""
        if page._fs == "deleted":
            return
        page._fs = "deleted"
        page._conflict = False
        if not page.buf.get_modified():
            self._set_placeholder(page, GONE_NOTE)
        else:
            self._paint_dot(page)
        self._paint_tab_deleted(page, True)
        self.on_state_changed()

    def _fs_binary(self, page) -> None:
        """Disk content is unreadable (binary/NUL/bad UTF-8): the buffer could
        never round-trip it, so freeze the view (edit-off) and note why. The
        unmodified note can never be saved back over the binary file (save guard);
        a later readable file un-freezes via _reload."""
        if page._fs == "binary":
            return
        page._fs = "binary"
        page._conflict = False
        page.view.set_editable(False)
        if not page.buf.get_modified():
            self._set_placeholder(page, BINARY_NOTE)
        self.on_state_changed()

    def _set_placeholder(self, page, text: str) -> None:
        self._swap_text(page, text)
        self._tag_muted(page.buf)
        page.buf.set_modified(False)

    def _tag_muted(self, buf) -> None:
        """Muted italic whole-buffer tag (placeholder notes)."""
        tag = buf.create_tag(foreground_rgba=self._rgba("#5a6375"), style=Pango.Style.ITALIC)
        start, end = buf.get_bounds()
        buf.apply_tag(tag, start, end)

    def _scroll_back(self, page, value: float) -> bool:
        adj = page.sw.get_vadjustment()
        adj.set_value(min(value, max(adj.get_upper() - adj.get_page_size(), 0.0)))
        return False

    def _error(self, primary: str, secondary: str) -> None:
        d = Gtk.MessageDialog(transient_for=self.get_root(), modal=True, text=primary,
                              secondary_text=secondary, buttons=Gtk.ButtonsType.CLOSE)
        d.connect("response", lambda dd, r: dd.destroy())
        d.present()

    # ---- persistence interface ----------------------------------------------------
    def get_open_state(self) -> dict:
        pages = [self.nb.get_nth_page(i) for i in range(self.nb.get_n_pages())]
        paths = [p.path for p in pages if getattr(p, "path", None)]
        rel = [os.path.relpath(p_, self.root) if self.root else p_ for p_ in paths]
        return {"open_tabs": rel,
                "active_tab": max(self.nb.get_current_page(), 0)}   # index, not a path;
        # an index pointing at a diff tab is fine: restore re-clamps it via min(saved, n-1)