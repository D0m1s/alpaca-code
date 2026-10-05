# Bottom notebook: Agent (raw claude TUI), Terminal ($SHELL), Output (run cmd).
# VTE owns the pty: spawn via spawn_async; children are session leaders → killpg terminates trees.
import os, signal, time
import alpaca_code.gi_env as ge
ge.require("Gdk", ("4.0",))
ge.require("Gtk", ("4.0",))
ge.require("Vte", ("4", "4.0", "3.91"))   # ledger ruling: this box's vte4 ships Vte-3.91
ge.require("Pango", ("1.0",))
from gi.repository import Gdk, GLib, Gtk, Pango, Vte

from . import badges, runctl

# Mockup (Agentic App – Dark IDE) VTE colors: card-bg terminals, ANSI slots mapped to
# the same hues as the editor scheme so claude/npm output reads like highlighted code.
VTE_FG, VTE_BG = "#e6e8ee", "#0d1017"
_ANSI = ["#3a4152", "#ef4444", "#22c55e", "#f2c94c", "#5aa9f8", "#c3a6f7", "#9cdcfe", "#e6e8ee",
         "#5a6375", "#f16a6a", "#4ee08a", "#f6d97e", "#6db5ff", "#cf97f4", "#aee0ff", "#f7f9fc"]

def vte_palette() -> tuple[str, str, list[str]]:
    """(foreground, background, 16 ANSI slots) as hex strings."""
    return (VTE_FG, VTE_BG, list(_ANSI))

def get_env() -> list[str]:
    # pure-logic env build lives in runctl (displayless-testable): VTE merges envv
    # onto the child's INHERITED environ, so ambient markers are OVERRIDDEN to empty
    # there, not merely omitted — see runctl.pane_environ.
    return runctl.pane_environ()

def spawn(term: Vte.Terminal, cwd: str, argv: list[str], on_ready=None) -> None:
    """Spawn argv on term's pty. The pid lands on `term.pid_holder` via the spawn callback.
    on_ready(t, pid, error): extra hook for callers that track the child (Output/run)."""
    def ready(t, pid, error):
        term.pid_holder = pid if pid > 0 else None
        if error is not None:
            print(f"spawn failed ({argv[0]}): {error.message}")
        if on_ready is not None:
            on_ready(t, pid, error)
    try:
        # keywords everywhere: this Vte build rejects the plan's positional form
        # ("Argument 9 does not allow None") but accepts the keyword form; introspected
        # signature matches 1:1, so it's positional-drift, not an API change.
        # SEARCH_PATH|SEARCH_PATH_FROM_ENVP: DEFAULT is plain execv — bare "claude"
        # would fail with ENOENT; lookup must use get_env()'s PATH (which appends
        # ~/.local/bin) or the PATH-append in get_env() has no effect at all.
        term.spawn_async(pty_flags=Vte.PtyFlags.DEFAULT, working_directory=cwd,
                         argv=argv, envv=get_env(),
                         spawn_flags=GLib.SpawnFlags.SEARCH_PATH | GLib.SpawnFlags.SEARCH_PATH_FROM_ENVP,
                         child_setup=None, timeout=-1, cancellable=None, callback=ready)
    except TypeError:  # binding-arg drift between VTE builds
        print("spawn_async signature mismatch; adapt to help(Vte.Terminal.spawn_async)")
        raise

class Panes(Gtk.Box):
    def __init__(self):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.set_css_classes(["alpaca-card"])
        self.set_overflow(Gtk.Overflow.HIDDEN)   # clip children to the card's rounded corners
        self.root = None
        self.on_status = lambda text, cls: None
        self._pids: dict[str, int | None] = {"output": None}
        self._run_starting = False        # spawned but pid not landed yet (async spawn callback)
        self._run_stop_pending = False    # Stop pressed during that window → kill the fresh pid
        self._spawned: set[str] = set()
        self._pane_spawned_at: dict[str, float] = {}
        self._kills_pending: dict[str, int] = {}
        self._respawns: dict[str, int] = {}

        self.agent = Vte.Terminal(); self.out = Vte.Terminal(); self.term = Vte.Terminal()
        fg, bg = Gdk.RGBA(), Gdk.RGBA()
        fg.parse(VTE_FG); bg.parse(VTE_BG)
        pal = []
        for hexstr in _ANSI:
            col = Gdk.RGBA(); col.parse(hexstr)
            pal.append(col)
        # Mockup console: mono 13px, line pitch ~20 — px-matched to the editor (set_absolute
        # size, points would scale differently); height scale lifts VTE's tight default cell.
        font = Pango.FontDescription.from_string("Noto Sans Mono")
        font.set_absolute_size(13 * Pango.SCALE)
        for t in (self.agent, self.out, self.term):
            t.set_scrollback_lines(10000)
            t.set_font(font)
            if hasattr(t, "set_cell_height_scale"):
                t.set_cell_height_scale(1.08)
            t.set_colors(fg, bg, pal)          # unthemed VTE default is otherwise light — mockup cards are dark
        self.out.connect("child-exited", self._run_child_exited)
        self.agent.connect("child-exited", self._pane_child_exited, "agent")
        self.term.connect("child-exited", self._pane_child_exited, "term")
        # scrollable: same min-width ruling as the editor notebook
        self.nb = Gtk.Notebook(vexpand=True, scrollable=True)
        self.nb.add_css_class("alpaca-panes")   # pane strip geometry differs from the editor's (40px pill)
        # page order is load-bearing: 0=agent, 1=term, 2=out (_ensure_current, launch_run)
        self.nb.append_page(Gtk.ScrolledWindow(child=self.agent), self._pane_tab("Agent", badges.icon("bot.svg")))
        self.nb.append_page(Gtk.ScrolledWindow(child=self.term), self._pane_tab("Terminal", badges.icon("prompt.svg")))
        self.nb.append_page(Gtk.ScrolledWindow(child=self.out), self._pane_tab("Output", badges.icon("terminal.svg")))
        self.nb.connect("switch-page", self._on_switch)
        self.append(self.nb)

    @staticmethod
    def _pane_tab(title: str, ipy) -> Gtk.Box:
        # `alpaca-panetab` carries the pill's 12px side insets — NOT the label:
        # label-side padding left the leading icon flush with the pill edge (measured).
        # spacing 6: tight icon-to-text (ink gap ≈7 measured); the 12px edge pads
        # give the pill its breathing room (user ruling 2026-10-05: wide pill,
        # tight icon-text pair)
        head = Gtk.Box(spacing=6, css_classes=["alpaca-panetab"])
        head.set_valign(Gtk.Align.CENTER)
        if ipy is not None:
            # default FILL drops the icon to the row top when the label is taller
            img = Gtk.Image.new_from_paintable(ipy)
            img.set_valign(Gtk.Align.CENTER)
            head.append(img)
        head.append(Gtk.Label(label=title, css_classes=["alpaca-panetabname"]))
        return head

    # ---- status: run/stop state sync for the window ---------------------------
    def _set_status(self, text: str, cls: str = "") -> None:
        self.on_status(text, cls)

    # ---- workspace ------------------------------------------------------------
    def set_root(self, root: str) -> None:
        out_pid = self._pids.get("output")
        if out_pid:
            self._kill_tree(out_pid)
        self._kills_pending = {}
        for t, key in ((self.agent, "agent"), (self.term, "term")):
            pid = getattr(t, "pid_holder", None)
            if pid and os.path.exists(f"/proc/{pid}"):
                self._kill_tree(pid)
                self._kills_pending[key] = 1     # that child's exit event is ours — swallow it
        self._spawned = set()
        self._respawns = {}
        self._pids = {"output": None}
        self._run_starting = False
        self._run_stop_pending = False
        for t in (self.agent, self.out, self.term):
            t.reset(True, True)
        self.root = root
        self._set_status("Ready", "")
        GLib.idle_add(self._ensure_current)

    def _ensure_current(self, page_ix: int | None = None) -> bool:
        """Spawn the visible tab's child once; called on set_root + every switch-page.

        page_ix: from switch-page — get_current_page() is still the OLD index during
        that signal's emission in GTK 4.22, so the caller must hand over the new one.
        """
        i = self.nb.get_current_page() if page_ix is None else page_ix
        if not self.root:
            return False
        if i == 0 and "agent" not in self._spawned:
            self._spawned.add("agent")
            self._respawns.pop("agent", None)   # deliberate re-entry re-arms the exit budget
            self._spawn_pane("agent")
        elif i == 1 and "term" not in self._spawned:
            self._spawned.add("term")
            self._respawns.pop("term", None)
            self._spawn_pane("term")
        return GLib.SOURCE_REMOVE

    def _spawn_pane(self, key: str) -> None:
        term = self.agent if key == "agent" else self.term
        self._pane_spawned_at[key] = time.time()
        spawn(term, self.root, ["claude"] if key == "agent" else [os.environ.get("SHELL", "/bin/bash")])

    def _pane_child_exited(self, term, status, key) -> None:
        # A pane's child must not go dead for the session when the user exits it:
        # revive it. set_root's own kills are pre-counted in _kills_pending (the exit
        # event can land after the bookkeeping cleared _spawned); a child that dies
        # right after spawn is treated as a broken child — no respawn loop. Revives
        # of churn-quick children are capped (a claude that always exits after ~3s
        # would otherwise respawn forever): three per budget, a ≥60s-long child
        # re-arms it, returning to a capped tab re-arms it too.
        if self._kills_pending.get(key):
            self._kills_pending[key] -= 1
            return
        if not self.root or key not in self._spawned:
            return
        lived = time.time() - self._pane_spawned_at.get(key, 0)
        if lived < 3:
            return
        if lived >= 60:
            self._respawns[key] = 0
        if self._respawns.get(key, 0) >= 3:
            self._spawned.discard(key)          # returning to the tab offers a fresh attempt
            self._set_status(f"{'Agent' if key == 'agent' else 'Terminal'} exited", "err")
            return
        self._respawns[key] = self._respawns.get(key, 0) + 1
        self._spawn_pane(key)

    def _on_switch(self, nb, page, index) -> None:
        self._ensure_current(index)

    # ---- run lifecycle (used by Task 8) -----------------------------------------
    def launch_run(self, argv: list, label: str) -> bool:
        if self.has_running_run():
            return False
        self._run_starting = True
        self._run_stop_pending = False
        self.nb.set_current_page(2)   # Output is page 2 now
        self.out.reset(True, True)
        def landed(t, pid, error):
            self._run_starting = False
            if error is not None or pid <= 0:
                self._set_status(f"Run failed to start: {error.message if error else argv[0]}", "err")
                return
            self._pids["output"] = pid
            if self._run_stop_pending:            # Stop was pressed before the pid existed
                self._run_stop_pending = False
                self._pids["output"] = None
                self._kill_tree(pid)
                return
            if self._alive(pid):
                self._set_status("Running", "ok")
            else:
                self._pids["output"] = None       # died instantly (e.g. script missing)
                self._set_status(f"Process ended ({label})", "err")
        spawn(self.out, self.root, argv, on_ready=landed)
        return True

    def _alive(self, pid) -> bool:
        return bool(pid) and os.path.exists(f"/proc/{pid}")

    def has_running_run(self) -> bool:
        if self._run_starting:                    # guard the async window before the pid lands
            return True
        return self._alive(self._pids["output"])

    def stop_run(self) -> None:
        pid = self._pids["output"]
        if pid:
            self._kill_tree(pid)
            self._pids["output"] = None
            self._run_starting = False
        elif self._run_starting:
            self._run_stop_pending = True         # landed() kills the fresh pid when it arrives
        self._set_status("Ready", "")

    def _run_child_exited(self, term, status):
        if self._pids["output"] is None and not self._run_starting:
            return   # set_root/stop cleanup exit — no run is being tracked; keep its status
        self._pids["output"] = None
        try:
            code = os.waitstatus_to_exitcode(status)
        except ValueError:
            code = status
        self._set_status(f"Exit {code}", "err" if code else "ok")

    # ---- kills --------------------------------------------------------------------
    def _live_agent_term_pids(self) -> list[int]:
        return [t.pid_holder for t in (self.agent, self.term)
                if getattr(t, "pid_holder", None) and os.path.exists(f"/proc/{t.pid_holder}")]

    @staticmethod
    def _kill_tree(pid: int) -> None:
        # VTE children are session leaders; pgid kill reaches npm/node grandchildren.
        try:
            os.killpg(os.getpgid(pid), signal.SIGHUP)
        except (ProcessLookupError, PermissionError):
            return
        def finish() -> bool:
            try:
                os.killpg(os.getpgid(pid), signal.SIGKILL)
            except (ProcessLookupError, PermissionError):
                pass
            return GLib.SOURCE_REMOVE
        GLib.timeout_add(2000, finish)   # timeout_add_seconds never fires under app.run() on this box (measured)