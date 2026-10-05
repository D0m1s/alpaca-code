import os, subprocess
import alpaca_code.gi_env as ge
ge.require("Gdk", ("4.0",))
ge.require("Gtk", ("4.0",))
ge.require("Pango", ("1.0",))
from gi.repository import Gdk, Gio, GLib, Gtk, Pango

from . import badges, runctl

def _img(ipy, px=None):
    """Vector icon (badges.icon paintable) → widget; Gtk.Image(kwargs=…) is
    unsupported on this build. valign CENTER: the default align (FILL) lands
    the image top-left in this build's allocation (measured: image alloc y=0
    in the 32px run cell → glyphs ride 7px high; WindowControls' own images,
    valign CENTER, sit at exactly (h−16)/2 — the in-tree control)."""
    img = Gtk.Image.new_from_paintable(ipy)
    img.set_valign(Gtk.Align.CENTER)
    if px:
        img.set_pixel_size(px)   # Gsk scales the paintable, stays sharp
    return img

def _no_arrow(btn):
    """Label-mode MenuButtons render a real 16×24 `down` arrow widget (invisible
    glyph, real width → menu labels sat 38px apart). set_always_show_arrow(False)
    no-ops on this build; icon-mode buttons hide it themselves (measured). Hiding
    the widget survives set_popover/set_label (probed)."""
    box = btn.get_first_child().get_first_child()
    if box is None:
        return
    c = box.get_first_child()
    while c:
        if "down" in c.get_css_classes():
            c.set_visible(False)
            return
        c = c.get_next_sibling()

def _default_size() -> tuple[int, int]:
    """1586×992 unless the monitor is smaller — a default larger than the screen
    opens half-offscreen (content clipped). ponytail: monitor 0."""
    disp = Gdk.Display.get_default()
    if disp:
        mons = disp.get_monitors()
        if mons.get_n_items():
            g = mons.get_item(0).get_geometry()
            return min(1586, g.width - 40), min(992, g.height - 140)
    return (1586, 992)

class Window:
    """Layout owner: HeaderBar(File | Run, Stop); hpaned → left vpaned(editor/panes) + file browser right."""

    def __init__(self, root: str | None):
        self.win = Gtk.ApplicationWindow(title="alpaca_code")
        self.win.set_default_size(*_default_size())
        self.root = root
        self.tree = None
        self.editor = None
        self.panes = None

        self.menubtn = Gtk.MenuButton(label="File")
        self.editbtn = Gtk.MenuButton(label="Edit")     # design's menu row: File Edit Window Help
        self.winbtn = Gtk.MenuButton(label="Window")
        self.helpbtn = Gtk.MenuButton(label="Help")
        for b in (self.menubtn, self.editbtn, self.winbtn, self.helpbtn):
            _no_arrow(b)
        # Native minimize/maximize/close buttons: the header-bar itself carries an
        # internal GtkWindowControls (this GTK build wraps the bar in a WindowHandle).
        # They render only if CSS leaves `background` alone on titlebuttons —
        # `.alpaca-header button { background: transparent }` erased their icons,
        # since Adwaita draws titlebutton icons as background-image (measured).
        # main.py forces gtk-decoration-layout so KDE Wayland (which supplies none)
        # still gets the full triple.
        self.run_btn = Gtk.Button()
        # Mockup's pill buttons: green play / red stop — design SVG (theme symbols
        # would grey both out under CSS).
        play = badges.icon("play.svg")
        self.run_btn.set_child(_img(play, 14) if play else Gtk.Image(icon_name="media-playback-start-symbolic"))
        self.run_btn.set_sensitive(False); self.run_btn.set_css_classes(["alpaca-run"])
        self.run_btn.set_tooltip_text("Run")
        self.stop_btn = Gtk.Button()
        stop = badges.icon("stop.svg")
        self.stop_btn.set_child(_img(stop, 14) if stop else Gtk.Image(icon_name="media-playback-stop-symbolic"))
        self.stop_btn.set_sensitive(False); self.stop_btn.set_css_classes(["alpaca-stop"])
        self.stop_btn.set_tooltip_text("Stop")
        header = Gtk.HeaderBar(); header.add_css_class("alpaca-header")
        self._title = Gtk.Label(label="alpaca-code", css_classes=["alpaca-wintitle"])
        self._title.set_ellipsize(Pango.EllipsizeMode.END)  # a long project name must not widen the window's min
        header.set_title_widget(self._title)   # the visible title (mockup drops it; a bare bar read as a bug)
        header.pack_start(self.menubtn)
        header.pack_start(self.editbtn)
        header.pack_start(self.winbtn)
        header.pack_start(self.helpbtn)
        run_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
        self.run_row = run_row
        run_row.set_css_classes(["alpaca-runpill"])
        # Hover swatches live on wrapper cells, not the buttons: a GtkButton's
        # CSS box paints 16+2·pad-tall around a 16px icon while its measure adds
        # a constant ~18px phantom (probe_pill11-16 — every pad/min-height combo
        # kept ~9px gutters). GtkBox cells paint their FULL allocation (fill),
        # so the cells carry :hover and the buttons stay flat forever.
        for btn in (self.run_btn, self.stop_btn):
            cell = Gtk.Box(css_classes=["alpaca-runcell"])
            cell.append(btn)
            run_row.append(cell)
        divider = Gtk.Box(css_classes=["alpaca-rundivider"], margin_top=4, margin_bottom=4)
        run_row.insert_child_after(divider, run_row.get_first_child())   # run | div | stop
        # The cell's hover paint = its allocation, so the row must stand 28 tall
        # (not FILL the header interior): CENTER in the 32px bar = 2px air each
        # side, band 28 and centered — same geometry the old bg-image band had.
        run_row.set_valign(Gtk.Align.CENTER)
        header.pack_end(run_row)
        self.win.set_titlebar(header)
        self.win.set_icon_name("io.alpaca.code")

        from .editor import Editor
        self.editor = Editor()
        self.editor.on_state_changed = self.persist_tabs

        from .panels import Panes
        self.panes = Panes()
        self.vpane = Gtk.Paned(orientation=Gtk.Orientation.VERTICAL, wide_handle=True)
        self.vpane.set_start_child(self.editor)
        self.vpane.set_end_child(self.panes)
        self.vpane.set_position(592)   # design: editor card 56..649 (593) − pane handle overlap

        from .filetree import FileBrowser
        self.tree = FileBrowser()
        self.tree.on_open = lambda p: self.editor.open_file(p) if self.editor else None
        # Changes-view wiring (spec §5): the browser HOSTS ChangesView; the
        # window owns its side effects. on_open carries (rel, letter) — the
        # letter drives the tab chip and the untracked-diff branch.
        self.tree.changes.on_open = self._open_changes_diff
        # before_commit gets REL paths (changes-view contract); save_open compares
        # ABS page paths — bind root NOW, per call, not per workspace setup.
        self.tree.changes.before_commit = lambda rels: self.editor.save_open(
            [os.path.join(self.root, r) for r in rels])   # flush dirty buffers for exactly the checked files
        self.tree.changes.on_refresh = self.tree.refresh_branch   # statusbar re-syncs after commit/push
        self.hpane = Gtk.Paned(orientation=Gtk.Orientation.HORIZONTAL, wide_handle=True)
        self.hpane.set_start_child(self.vpane)
        self.hpane.set_end_child(self.tree)
        self.hpane.set_position(1161)   # design: cards gap columns 1172..1180 → pane pos = card end - 10
        self.win.connect("notify::is-active",
                         lambda *_a: self.tree and self.tree.refresh_git())

        # Set hpane directly as the window child: a wrapping Box wouldn't expand it
        # (GTK4 Box gives non-expanding children only their minimum — measured 43px tall).
        self.win.set_child(self.hpane)   # 8px top gap removed 2026-10-04: cards flush under the menus row

        self.run_btn.connect("clicked", self._on_run)
        self.stop_btn.connect("clicked", self._on_stop)
        self.panes.on_status = self._on_run_status
        if self.root:
            # NB: `win.title = x` silently becomes a plain Python attribute in PyGObject
            # (measured: get_title() unchanged) — always use set_title().
            self.win.set_title("alpaca_code")   # mockup: no title text in the headerbar
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
        self.win.set_title("alpaca_code")
        base = os.path.basename(path.rstrip("/"))
        self._title.set_text(f"alpaca-code — {base}" if base else "alpaca-code")
        self.tree.set_root(path)
        self.panes.set_root(path)
        cmd = runctl.detect(path)
        self.run_btn.set_sensitive(cmd is not None)
        self.run_btn.set_tooltip_text((cmd and cmd["label"]) or "No dev/start script or .csproj found")
        self.stop_btn.set_sensitive(False)
        self._refresh_run_style()
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

    # --- changes view → editor (spec §5) -----------------------------------------
    def _open_changes_diff(self, rel: str, letter: str) -> None:
        """CHANGES row click → side-by-side diff page. Sides are built here;
        the editor stays git-free (Task 2's open_diff takes ready-built lines)."""
        from . import gitstatus
        if not self.root:
            return
        text, binary = gitstatus.diff_for(self.root, rel, is_untracked=(letter == "U"))
        if binary:
            self.editor._error(f"Binary file: {rel}", "No diff view for binary changes.")
            return
        sides = gitstatus.build_sides(gitstatus.parse_unified(text))
        if len(sides[0]) == 0:
            self.editor._error(f"No diff: {rel}", "git produced no diff hunks for this path.")
            return
        self.editor.open_diff(rel, sides, letter)

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
        self._refresh_run_style()

    def _refresh_run_style(self) -> None:
        # Shell appears only while a run is live (idle reads flat, like the menus).
        self.run_row.set_css_classes(
            ["alpaca-runpill", "running"] if self.panes.has_running_run()
            else ["alpaca-runpill"])

    def _on_run_status(self, text: str, cls: str) -> None:
        # Re-enable Run when the child exits ("Exit N" / "Ready").
        if not self.panes.has_running_run():
            self.run_btn.set_sensitive(True)
        self.stop_btn.set_sensitive(self.panes.has_running_run())
        self._refresh_run_style()

    def _on_stop(self, btn) -> None:
        self.panes.stop_run()
        self.stop_btn.set_sensitive(False)
        self.run_btn.set_sensitive(True)
        self._refresh_run_style()

    # --- actions + File menu ------------------------------------------------------
    def register_actions(self, app) -> None:
        for name, cb in (
            ("open-project", self._act_open_project),
            ("new-project", self._act_new_project),
            ("quit", lambda *_: app.quit()),
            ("undo", self._edit_op("undo")),
            ("redo", self._edit_op("redo")),
            ("cut", self._edit_op("cut")),
            ("copy", self._edit_op("copy")),
            ("paste", self._edit_op("paste")),
            ("select-all", self._edit_op("select")),
            ("win-minimize", lambda *_: self.win.minimize()),
            ("close", lambda *_: self.win.destroy()),
            ("about", self._act_about),
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
        self._refresh_menus()

    def _edit_op(self, op: str):
        """Edit-menu routes: undo/redo direct on the buffer, clipboard ops via the
        view's own widget actions (cut-clipboard etc.) so focus/selection stay right.
        Diff pages have no `buf` and end in the pair Box (no .get_child) — every op
        must be a clean no-op/reroute there, never a crash (review I3)."""
        def run() -> None:
            if not self.editor:
                return
            page = self.editor.nb.get_nth_page(self.editor.nb.get_current_page())
            if page is None:
                return
            buf = getattr(page, "buf", None)
            if op == "undo":
                if buf is not None and buf.get_can_undo():   # can_undo() absent on this build
                    buf.undo()
                return
            if op == "redo":
                if buf is not None and buf.get_can_redo():
                    buf.redo()
                return
            # clipboard/select: the current page's own view — on diff pages the
            # FOCUSED widget (one of the two sides; anything else drops the action)
            view = page.get_last_child().get_child() if buf is not None else self.win.get_focus()
            if view is None:
                return
            if op == "select":
                view.activate_action("select-all", GLib.Variant.new_boolean(True))
            else:
                view.activate_action(f"{op}-clipboard", None)
        return run

    def _act_about(self) -> None:
        d = Gtk.AboutDialog(transient_for=self.win, modal=True,
                            program_name="alpaca-code", version="1.0",
                            comments="Minimal agentic IDE — GTK4 wrapper for the claude CLI")
        d.present()

    def _refresh_menus(self) -> None:
        """File menu in exactly one place: the headerbar File button."""
        self.menubtn.set_popover(Gtk.PopoverMenu.new_from_model(self._build_menu()))
        self.editbtn.set_popover(Gtk.PopoverMenu.new_from_model(self._edit_menu()))
        self.winbtn.set_popover(Gtk.PopoverMenu.new_from_model(self._win_menu()))
        self.helpbtn.set_popover(Gtk.PopoverMenu.new_from_model(self._help_menu()))

    def _edit_menu(self) -> Gio.Menu:
        m = Gio.Menu()
        sec = Gio.Menu(); sec.append("Undo", "app.undo"); sec.append("Redo", "app.redo")
        m.append_section(None, sec)
        sec = Gio.Menu(); sec.append("Cut", "app.cut"); sec.append("Copy", "app.copy")
        sec.append("Paste", "app.paste")
        m.append_section(None, sec)
        sec = Gio.Menu(); sec.append("Select All", "app.select-all")
        m.append_section(None, sec)
        return m

    def _win_menu(self) -> Gio.Menu:
        m = Gio.Menu()
        m.append("Minimize", "app.win-minimize")
        m.append("Close", "app.close")
        return m

    def _help_menu(self) -> Gio.Menu:
        m = Gio.Menu()
        m.append("About alpaca-code", "app.about")
        return m

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
            self._refresh_menus()  # refresh recents

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
            self._refresh_menus()  # refresh recents
        d.connect("response", on_resp)

    def _error_dialog(self, primary: str, detail: str) -> None:
        d = Gtk.MessageDialog(transient_for=self.win, modal=True, text=primary,
                              secondary_text=detail, buttons=Gtk.ButtonsType.CLOSE)
        d.connect("response", lambda dd, r: dd.destroy())
        d.present()