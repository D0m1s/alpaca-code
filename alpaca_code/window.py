import os, subprocess
import alpaca_code.gi_env as ge
ge.require("Gtk", ("4.0",))
from gi.repository import Gio, GLib, Gtk

from . import runctl

class Window:
    """Layout owner: HeaderBar(File | Run, Stop); hpaned → left vpaned(editor/panes) + file browser right."""

    def __init__(self, root: str | None):
        self.win = Gtk.ApplicationWindow(title="alpaca_code")
        self.win.set_default_size(1586, 992)
        self.root = root
        self.tree = None
        self.editor = None
        self.panes = None

        self.menubtn = Gtk.MenuButton(label="File")
        self.run_btn = Gtk.Button(label="▶ Run"); self.run_btn.set_sensitive(False)
        self.run_btn.set_css_classes(["alpaca-run"])
        self.stop_btn = Gtk.Button(label="■ Stop"); self.stop_btn.set_sensitive(False)
        self.stop_btn.set_css_classes(["alpaca-stop"])
        header = Gtk.HeaderBar(); header.add_css_class("alpaca-header")
        header.pack_start(self.menubtn)
        run_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=2)
        run_row.append(self.run_btn); run_row.append(self.stop_btn)
        header.pack_end(run_row)
        self.win.set_titlebar(header)

        from .editor import Editor
        self.editor = Editor()
        self.editor.on_state_changed = self.persist_tabs

        from .panels import Panes
        self.panes = Panes()
        self.vpane = Gtk.Paned(orientation=Gtk.Orientation.VERTICAL, wide_handle=True)
        self.vpane.set_start_child(self.editor)
        self.vpane.set_end_child(self.panes)
        self.vpane.set_position(594)

        from .filetree import FileBrowser
        self.tree = FileBrowser()
        self.tree.on_open = lambda p: self.editor.open_file(p) if self.editor else None
        self.hpane = Gtk.Paned(orientation=Gtk.Orientation.HORIZONTAL, wide_handle=True)
        self.hpane.set_start_child(self.vpane)
        self.hpane.set_end_child(self.tree)
        self.hpane.set_position(1181)
        self.win.connect("notify::is-active",
                         lambda *_a: self.tree and self.tree.refresh_branch())

        # Set hpane directly as the window child: a wrapping Box wouldn't expand it
        # (GTK4 Box gives non-expanding children only their minimum — measured 43px tall).
        self.win.set_child(self.hpane)

        self.run_btn.connect("clicked", self._on_run)
        self.stop_btn.connect("clicked", self._on_stop)
        self.panes.on_status = self._on_run_status
        if self.root:
            # NB: `win.title = x` silently becomes a plain Python attribute in PyGObject
            # (measured: get_title() unchanged) — always use set_title().
            self.win.set_title("alpaca-code — " + os.path.basename(self.root.rstrip("/")))
            self.set_workspace(self.root)  # startup with a path/last_project populates everything

    def set_workspace(self, path: str) -> None:
        from . import state
        if self.editor.has_dirty():
            d = Gtk.MessageDialog(transient_for=self.win, modal=True,
                                  text="Discard unsaved changes?", buttons=Gtk.ButtonsType.CANCEL)
            d.add_button("Discard", Gtk.ResponseType.ACCEPT)
            d.connect("response", self._on_discard_switch(path))
            d.present()
            return
        self._set_workspace_now(path)

    def _on_discard_switch(self, path: str):
        def resp(dd, r):
            dd.destroy()
            if r == Gtk.ResponseType.ACCEPT:
                self._set_workspace_now(path)
        return resp

    def _set_workspace_now(self, path: str) -> None:
        from . import state
        state.save(state.remember(state.load(), path))
        self.root = path
        self.win.set_title("alpaca-code — " + os.path.basename(path.rstrip("/")))
        self.tree.set_root(path)
        self.panes.set_root(path)
        cmd = runctl.detect(path)
        self.run_btn.set_sensitive(cmd is not None)
        self.run_btn.set_tooltip_text((cmd and cmd["label"]) or "No dev/start script or .csproj found")
        self.stop_btn.set_sensitive(False)
        self.editor.restore(path, state.project_tabs(state.load(), path))

    def persist_tabs(self) -> None:
        if not self.root:
            return
        def flush():
            from . import state
            saved = state.load()
            st = self.editor.get_open_state()
            state.set_tabs(saved, self.root, st["open_tabs"], st["active_tab"])
            state.save(saved)
            return GLib.SOURCE_REMOVE
        GLib.timeout_add(1000, flush)  # debounced ≤1s; last event before quit may be lost — acceptable

    # --- run wiring ---
    def _on_run(self, btn) -> None:
        if not self.root or self.panes.has_running_run():
            return                                       # no double children
        from . import runctl
        cmd = runctl.detect(self.root)
        if cmd is None:
            self.run_btn.set_sensitive(False)
            self.run_btn.set_tooltip_text("No dev/start script or .csproj found")
            return
        self.stop_btn.set_sensitive(True)
        self.panes.launch_run(cmd["argv"], cmd["label"])
        self.run_btn.set_sensitive(False)                # Stop-only until run ends

    def _on_run_status(self, text: str, cls: str) -> None:
        # Re-enable Run when the child exits ("Exit N" / "Ready").
        if not self.panes.has_running_run():
            self.run_btn.set_sensitive(True)
        self.stop_btn.set_sensitive(self.panes.has_running_run())

    def _on_stop(self, btn) -> None:
        self.panes.stop_run()
        self.stop_btn.set_sensitive(False)
        self.run_btn.set_sensitive(True)

    # --- actions + File menu ------------------------------------------------------
    def register_actions(self, app) -> None:
        for name, cb in (
            ("open-project", self._act_open_project),
            ("new-project", self._act_new_project),
            ("quit", lambda *_: app.quit()),
        ):
            a = Gio.SimpleAction.new(name, None)
            a.connect("activate", lambda _a, _p, f=cb: f())
            app.add_action(a)
        rec = Gio.SimpleAction.new("open-recent", GLib.VariantType.new("s"))
        def open_recent(_a, param):
            p = param.get_string()
            self.set_workspace(p) if os.path.isdir(p) else self._recent_missing(p)
        rec.connect("activate", open_recent)
        app.add_action(rec)
        app.set_accels_for_action("app.quit", ["<Control>q"])
        self.menubtn.set_popover(Gtk.PopoverMenu.new_from_model(self._build_menu()))

    def _recent_missing(self, path: str) -> None:
        d = Gtk.MessageDialog(transient_for=self.win, modal=True, text=f"Project not found: {path}",
                              buttons=Gtk.ButtonsType.CLOSE)
        d.connect("response", lambda dd, r: dd.destroy())
        d.present()

    def _build_menu(self) -> Gio.Menu:
        menu = Gio.Menu()
        menu.append("Open Project…", "app.open-project")
        recents = Gio.Menu()
        from . import state
        for p in state.load()["recents"]:
            item = Gio.MenuItem.new(os.path.basename(p.rstrip("/")) + " — " + p)
            item.set_action_and_target_value("app.open-recent", GLib.Variant.new_string(p))
            recents.append_item(item)
        if recents.get_n_items():
            menu.append_submenu("Open Recent", recents)
        menu.append("New Project…", "app.new-project")
        menu.append("Quit", "app.quit")
        return menu

    def _act_open_project(self) -> None:
        dialog = Gtk.FileChooserNative.new("Open Project", self.win,
                                           Gtk.FileChooserAction.SELECT_FOLDER, "Open", "Cancel")
        dialog.connect("response", self._on_pick)
        dialog.show()

    def _on_pick(self, dialog, resp) -> None:
        f = dialog.get_file() if resp == Gtk.ResponseType.ACCEPT else None
        path = f.get_path() if f else None
        dialog.destroy()
        if path and os.path.isdir(path):
            self.set_workspace(path)
            self.menubtn.set_popover(Gtk.PopoverMenu.new_from_model(self._build_menu()))  # refresh recents

    def _act_new_project(self) -> None:
        d = Gtk.Dialog(title="New Project", transient_for=self.win, modal=True)
        d.set_default_size(480, 160)
        name_e = Gtk.Entry(placeholder_text="project name")
        parent_e = Gtk.Entry()
        parent_e.set_text(os.path.expanduser("~/FunProjects"))
        for w in (name_e, parent_e):
            w.set_hexpand(True)
            d.get_content_area().append(w)
        d.add_button("Create", Gtk.ResponseType.ACCEPT)
        d.add_button("Cancel", Gtk.ResponseType.CANCEL)
        d.present()

        def on_resp(dd, resp):
            parent = parent_e.get_text().strip(); name = name_e.get_text().strip()
            dd.destroy()
            if resp != Gtk.ResponseType.ACCEPT or not name:
                return
            target = os.path.join(parent, name)
            try:
                os.makedirs(target, exist_ok=False)
                subprocess.run(["git", "init", target], capture_output=True, text=True, timeout=10)
            except (OSError, subprocess.SubprocessError) as e:
                self._error_dialog(f"Couldn't create {target}", str(e))
                return
            self.set_workspace(target)
            self.menubtn.set_popover(Gtk.PopoverMenu.new_from_model(self._build_menu()))  # refresh recents
        d.connect("response", on_resp)

    def _error_dialog(self, primary: str, detail: str) -> None:
        d = Gtk.MessageDialog(transient_for=self.win, modal=True, text=primary,
                              secondary_text=detail, buttons=Gtk.ButtonsType.CLOSE)
        d.connect("response", lambda dd, r: dd.destroy())
        d.present()