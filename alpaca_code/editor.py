# Tabbed editor: GtkSourceView pages, custom tab headers, Ctrl+S, file-deleted dialogs.
import os
import alpaca_code.gi_env as ge
ge.require("Gtk", ("4.0",))
ge.require("GtkSource", ("5",))
from gi.repository import Gio, GLib, Gtk, GtkSource

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

class Editor(Gtk.Box):
    def __init__(self):
        super().__init__(orientation=Gtk.Orientation.VERTICAL)
        self.root = None
        self.on_state_changed = lambda: None
        self.lm = GtkSource.LanguageManager.get_default()
        self.scheme = GtkSource.StyleSchemeManager.get_default().get_scheme("alpaca-dark")
        self.nb = Gtk.Notebook(vexpand=True)
        self._watchers: dict[str, Gio.FileMonitor] = {}
        self.append(self.nb)
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
            self._error("Couldn't open file", f"{os.path.basename(path)}: {e}")
            return
        text = readable_text(raw)
        if text is None:
            # refusing at open is the only safe point: a lossy buffer + Ctrl+S
            # would rewrite the file with truncated/replacement garbage
            self._error("Unsupported file", f"{os.path.basename(path)}: binary, NUL bytes, or not UTF-8")
            return
        buf = GtkSource.Buffer(text=text)
        if self.scheme:
            buf.set_style_scheme(self.scheme)
        lang = self.lm.guess_language(os.path.basename(path), None)
        if lang:
            buf.set_language(lang)
        buf.set_modified(False)     # GtkSource.Buffer(text=...) is born modified — an open must not show the dirty dot
        buf.connect("modified-changed", lambda b: self._dirty_dot(b))
        view = GtkSource.View(buffer=buf, show_line_numbers=True)
        view.set_wrap_mode(Gtk.WrapMode.NONE)
        view.set_css_classes(["alpaca-mono"])
        page = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        page.append(Gtk.Label(label=os.path.relpath(path, self.root) if self.root else path,
                              xalign=0.0, margin_start=8, margin_top=4, margin_bottom=4,
                              css_classes=["alpaca-breadcrumb"]))
        page.append(Gtk.ScrolledWindow(vexpand=True, child=view))
        page.path = path                                # attrs for lookup/save
        page.buf = buf

        dirty = Gtk.Label(label="●"); dirty.set_css_classes(["alpaca-dirty"]); dirty.set_visible(False)
        name = Gtk.Label(label=os.path.basename(path))
        close = Gtk.Button(child=Gtk.Image(icon_name="window-close-symbolic"))
        close.set_css_classes(["alpaca-close"])
        head = Gtk.Box(spacing=4); head.set_css_classes(["alpaca-tab"])
        head.append(dirty); head.append(name); head.append(close)
        index = self.nb.append_page(page, head)
        self.nb.set_current_page(index)  # GTK4 append does not switch; a new tab must take focus
        close.connect("clicked", lambda b, pg=page: self.close_index(self.nb.page_num(pg)))
        self._watch(path)
        self.on_state_changed()

    def _page_of(self, path: str) -> int:
        for i in range(self.nb.get_n_pages()):
            if self.nb.get_nth_page(i).path == path:
                return i
        return -1

    def _dirty_dot(self, buf) -> None:
        for i in range(self.nb.get_n_pages()):
            page = self.nb.get_nth_page(i)
            if page.buf is buf:
                head = self.nb.get_tab_label(page)
                head.get_first_child().set_visible(buf.get_modified())
        self.on_state_changed()

    # ---- close -----------------------------------------------------------------
    def close_index(self, index: int, force: bool = False) -> None:
        self._close_index(index, force)

    def _close_index(self, index: int, force: bool = False) -> None:
        page = self.nb.get_nth_page(index)
        if page is None:
            return
        if page.buf.get_modified() and not force:
            d = Gtk.MessageDialog(transient_for=self.get_root(), modal=True,
                                  text=f"Discard changes to {os.path.basename(page.path)}?",
                                  buttons=Gtk.ButtonsType.CANCEL)
            d.add_button("Discard", Gtk.ResponseType.ACCEPT)
            d.connect("response", lambda dd, r: (dd.destroy(), self._close_index(index, True))
                      if r == Gtk.ResponseType.ACCEPT else dd.destroy())
            d.present()
            return
        self.nb.remove_page(index)
        self._cancel_watch(page.path)
        self.on_state_changed()

    def _close_all(self) -> None:
        for i in range(self.nb.get_n_pages() - 1, -1, -1):
            page = self.nb.get_nth_page(i)
            self.nb.remove_page(i)
            self._cancel_watch(page.path)

    def has_dirty(self) -> bool:
        return any(self.nb.get_nth_page(i).buf.get_modified()
                   for i in range(self.nb.get_n_pages()))

    # ---- save --------------------------------------------------------------------
    def save_active(self) -> None:
        page = self.nb.get_nth_page(self.nb.get_current_page())
        if page is None:
            return
        try:
            with open(page.path, "w", encoding="utf-8") as f:
                f.write(page.buf.props.text)
        except OSError as e:
            self._error("Couldn't save", f"{os.path.basename(page.path)}: {e}")
            return
        page.buf.set_modified(False)
        # ponytail: sync stdlib write; switch to GtkSource save_async if saves ever block the UI

    # ---- deletion while open (Review Focus #5) -------------------------------------
    def _watch(self, path: str) -> None:
        self._cancel_watch(path)
        mon = Gio.File.new_for_path(path).monitor_file(0, None)
        mon.connect("changed", self._on_file_event, path)
        self._watchers[path] = mon

    def _cancel_watch(self, path: str) -> None:
        m = self._watchers.pop(path, None)
        if m:
            m.cancel()

    def _on_file_event(self, mon, f, other, event, path) -> None:
        if event != Gio.FileMonitorEvent.DELETED or self._page_of(path) == -1:
            return
        d = Gtk.MessageDialog(transient_for=self.get_root(), modal=True,
                              text="File was deleted — close its tab?", buttons=Gtk.ButtonsType.CLOSE)
        d.add_button("Close tab", Gtk.ResponseType.ACCEPT)

        def resp(dd, r):
            dd.destroy()
            k = self._page_of(path)
            if r == Gtk.ResponseType.ACCEPT and k != -1:
                self._close_index(k, force=True)

        d.connect("response", resp)
        d.present()

    def _error(self, primary: str, secondary: str) -> None:
        d = Gtk.MessageDialog(transient_for=self.get_root(), modal=True, text=primary,
                              secondary_text=secondary, buttons=Gtk.ButtonsType.CLOSE)
        d.connect("response", lambda dd, r: dd.destroy())
        d.present()

    # ---- persistence interface ----------------------------------------------------
    def get_open_state(self) -> dict:
        paths = [self.nb.get_nth_page(i).path for i in range(self.nb.get_n_pages())]
        rel = [os.path.relpath(p, self.root) if self.root else p for p in paths]
        return {"open_tabs": rel, "active_tab": max(self.nb.get_current_page(), 0)}

    def _on_switch(self, nb, page, index):
        self.on_state_changed()