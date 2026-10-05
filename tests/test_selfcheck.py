#!/usr/bin/env python3
"""alpaca_code self-check: plain asserts, no framework. Run: python3 tests/test_selfcheck.py"""
import json, os, subprocess, sys, tempfile

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, REPO)

TESTS = []
def register(fn):
    TESTS.append(fn); return fn

# --- state (Task 2) ----------------------------------------------------------
@register
def test_state_roundtrip_and_recents():
    from alpaca_code import state
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "state.json")
        s = state.load(path)
        assert s == {"last_project": None, "recents": [], "projects": {}}
        s = state.remember(s, "/a")
        s = state.remember(s, "/b")
        s = state.remember(s, "/a")
        assert s["last_project"] == "/a"
        assert s["recents"] == ["/a", "/b"]
        state.save(s, path)
        s2 = state.load(path)
        assert s2["last_project"] == "/a" and s2["recents"] == ["/a", "/b"]
        for i in range(12):
            s = state.remember(s, f"/p{i}")
        assert s["recents"][0] == "/p11" and len(s["recents"]) == 10

@register
def test_state_tabs():
    from alpaca_code import state
    s = state.default()
    assert state.project_tabs(s, "/a") == {"open_tabs": [], "active_tab": 0}
    s = state.set_tabs(s, "/a", ["x/y.py", "z.py"], 1)
    assert state.project_tabs(s, "/a") == {"open_tabs": ["x/y.py", "z.py"], "active_tab": 1}

@register
def test_state_load_tolerates_weird_types():
    # valid JSON but wrong types must not brick the app (Reviewer Important #4)
    from alpaca_code import state
    with tempfile.TemporaryDirectory() as tmp:
        p = os.path.join(tmp, "state.json")
        def write(s):
            with open(p, "w") as f:
                json.dump(s, f)
        write({"recents": 5})
        assert state.load(p) == state.default()
        write({"projects": []})
        assert state.load(p) == state.default()
        write({"last_project": 42})
        assert state.load(p) == state.default()
        write({"last_project": "/a", "recents": ["/a", 5, "", None],
               "projects": {"/a": {"open_tabs": ["x.py"], "active_tab": 0}, "bad": 3}})
        assert state.load(p) == {"last_project": "/a", "recents": ["/a"],
                                 "projects": {"/a": {"open_tabs": ["x.py"], "active_tab": 0}}}

# --- per-project tab shape hardening (Final review Important #3) ----------------
@register
def test_state_project_tabs_tolerates_junk():
    from alpaca_code import state
    s = {"projects": {"/p": {"open_tabs": 3}}}
    assert state.project_tabs(s, "/p") == {"open_tabs": [], "active_tab": 0}
    s = {"projects": {"/p": {"open_tabs": ["a.py", 5, None, "b.py"], "active_tab": "2"}}}
    assert state.project_tabs(s, "/p") == {"open_tabs": ["a.py", "b.py"], "active_tab": 2}
    assert state.project_tabs(s, "/missing") == {"open_tabs": [], "active_tab": 0}
    s = {"projects": {"/p": "garbage"}}
    assert state.project_tabs(s, "/p") == {"open_tabs": [], "active_tab": 0}
    s = {"projects": {
        "/p": {"open_tabs": ["a.py"], "active_tab": 1},
        "/q": {"open_tabs": ["b.py"], "active_tab": "not-a-number"}}}
    assert state.project_tabs(s, "/p") == {"open_tabs": ["a.py"], "active_tab": 1}
    assert state.project_tabs(s, "/q") == {"open_tabs": ["b.py"], "active_tab": 0}

# --- binary/NUL open rejection (Final review Important #2) -----------------------
@register
def test_editor_text_decider_rejects_binary():
    from alpaca_code import editor
    assert editor.readable_text(b"hello\n") == "hello\n"
    assert editor.readable_text(b"before\x00after") is None   # NUL → buffer truncates → Ctrl+S would clobber
    assert editor.readable_text("café-ÿ".encode("utf-8")) == "café-ÿ"  # valid non-ascii utf-8 still opens
    assert editor.readable_text(b"caf\xe9") is None           # invalid utf-8 → U+FFFD resave mangling

# --- mockup restyle: badges, scheme, vte palette, css tokens ----------------------
@register
def test_badges_chip_map_and_pixbuf():
    from alpaca_code import badges
    # ts family → filled blue chip w/ white "TS" (mockup signature)
    label, fg, chip = badges.for_file("App.tsx")
    assert (label, fg, chip) == ("TS", "#ffffff", "#2f80ed")
    assert badges.for_file("main.tsx") == ("TS", "#ffffff", "#2f80ed")
    # glyph-only types: chip None (bare colored glyph, mockup `#` / `{ }`)
    assert badges.for_file("styles.css") == ("#", "#4d8ef0", None)
    assert badges.for_file("package.json") == ("{ }", "#f2c94c", None)
    assert badges.for_file("README.md") == ("MD", "#2f80ed", None)
    assert badges.for_file("noext") is None                    # unknown ext → caller keeps symbolic icon
    assert badges.for_file("src", is_dir=True) is None         # dirs use badges.folder_pixbuf(), not chips
    # rendered pixbufs: right size, alpha rounded corner survived BGRA→RGBA
    pb = badges.pixbuf_for("App.tsx")
    assert pb.get_width() == 16 and pb.get_height() == 16
    assert pb.get_pixels()[3] == 0                             # (0,0) corner of a rounded chip is transparent
    assert badges.pixbuf_for("src", True) is badges.folder_pixbuf()
    assert badges.folder_pixbuf().get_width() == 16
    assert badges.pixbuf_for("noext") is None
    # design svg icons rasterize (mockup's own folder art, tree cells)
    assert badges.svg_icon("folder.svg", 16).get_width() == 16
    assert badges.svg_icon("missing.svg", 16) is None
    assert badges.blank_pixbuf().get_width() == 12    # file row chevron slot
    # mush guard (probe-measured): the old 4×-supersample→16px crush left the "TS"
    # text as faint mush (18 bright px); exact-size render gives ≥30
    ts = badges.pixbuf_for("App.tsx").get_pixels()
    assert sum(1 for i in range(256) if ts[4 * i + 3] > 200
               and (ts[4 * i] * .3 + ts[4 * i + 1] * .6 + ts[4 * i + 2] * .1) > 190) >= 30

@register
def test_vector_icons_parse_and_paint():
    """Widget icons are Gsk render nodes, not pixbufs: every data/icons/*.svg
    parses, records nodes into a Snapshot at its intrinsic size, cache-stable."""
    from alpaca_code import vector
    import alpaca_code.gi_env as ge
    ge.require("Gtk", ("4.0",))
    from gi.repository import Gtk
    names = [f for f in sorted(os.listdir(vector.ICONS)) if f.endswith(".svg")]
    assert names, "no icon art found"
    snap = Gtk.Snapshot()
    for n in names:
        v = vector.icon(n)
        assert v is not None, n
        assert v.units in (12.0, 16.0, 24.0), (n, v.units)   # art authored 1:1 to its display grid
        assert 0 < round(v.units) < 32
        v.do_snapshot(snap, 16, 16)                     # nodes must record without a display
    assert snap.to_node() is not None
    assert vector.icon(names[0]) is vector.icon(names[0])   # cache
    assert vector.icon("missing.svg") is None
    # widget sites go through badges.icon() (no pixbuf path for widgets)
    from alpaca_code import badges
    assert badges.icon("terminal.svg") is not None

@register
def test_style_scheme_matches_mockup_palette():
    import xml.etree.ElementTree as ET
    p = os.path.join(REPO, "alpaca_code", "data", "alpaca-dark.xml")
    styles = {s.get("name"): (s.get("foreground"), s.get("background"))
              for s in ET.parse(p).getroot().iter("style")}
    assert styles["text"] == ("#e6e8ee", "#0d1017")
    assert styles["keyword"] == ("#c3a6f7", None)     # keywords lavender
    assert styles["string"] == ("#f0a36e", None)      # strings orange
    assert styles["function"] == ("#f2d98b", None)    # methods/calls gold
    assert styles["special-constant"] == ("#5aa9f8", None)   # true/false bright blue
    assert styles["selection"] == (None, "#1b3560")
    assert styles["line-numbers"] == ("#5a6375", "#0d1017")
    # languages address styles through the def:* namespace (measured: the render
    # path asks for def:keyword, not "keyword" — bare names alone leave plain text)
    for def_name, fg in (("def:keyword", "#c3a6f7"), ("def:string", "#f0a36e"),
                         ("def:comment", "#5a6375"), ("def:function", "#f2d98b"),
                         ("def:identifier", "#9cdcfe"), ("def:special-constant", "#5aa9f8"),
                         ("def:preprocessor", "#f2d98b")):
        assert styles.get(def_name) == (fg, None), def_name

@register
def test_panels_vte_palette():
    from alpaca_code import panels
    fg, bg, palette = panels.vte_palette()
    assert bg == "#0d1017" and fg == "#e6e8ee"
    assert len(palette) == 16          # Vte.Terminal.set_colors() takes at most 16 ANSI slots
    assert palette[1] == "#ef4444"     # ANSI red → mockup stop-red
    assert palette[2] == "#22c55e"     # ANSI green → mockup run-green
    assert palette[4] == "#5aa9f8"     # ANSI blue → mockup accent

@register
def test_main_css_palette_tokens():
    from alpaca_code import main
    css = main.CSS
    for needle in ("#07090d",         # gutter / window
                   "#0d1017",         # card surface
                   "#0a0d13",         # tab strips + search field
                   "#111723",         # raised: active tab pill, run pill
                   "#2f80ed",         # accent
                   "#1b3560",         # selected tree row
                   "#2b3448",         # hairline borders
                   "border-radius: 4px",   # cards match the WM window radius (~4px measured)
                   ".alpaca-panetab { padding: 0 12px",   # pane-tab pill: uniform insets (icon
                                          # flush with pill edge bug, measured 0 vs 6; widened
                                          # to 12px 2026-10-05 — text↔pill-side room)
                   "min-height: 28px"):    # run pill ~28px
        assert needle in css, needle

# --- runctl (Task 3) -----------------------------------------------------------
@register
def test_runctl_detect():
    from alpaca_code import runctl
    with tempfile.TemporaryDirectory() as t:
        os.mkdir(t + "/empty")
        assert runctl.detect(t + "/empty") is None

        os.mkdir(t + "/dev")
        open(t + "/dev/package.json", "w").write('{"name": "x", "scripts": {"dev": "vite"}}')
        assert runctl.detect(t + "/dev") == {"argv": ["npm", "run", "dev"], "label": "npm run dev"}

        os.mkdir(t + "/start")
        open(t + "/start/package.json", "w").write('{"name": "x", "scripts": {"start": "node x"}}')
        assert runctl.detect(t + "/start")["argv"] == ["npm", "start"]

        os.mkdir(t + "/no")
        open(t + "/no/package.json", "w").write('{"name": "x", "scripts": {"build": "x"}}')
        assert runctl.detect(t + "/no") is None

        os.mkdir(t + "/corrupt")
        open(t + "/corrupt/package.json", "w").write("{not json")
        assert runctl.detect(t + "/corrupt") is None

        os.mkdir(t + "/listscripts")
        open(t + "/listscripts/package.json", "w").write('{"scripts": [1, 2]}')
        assert runctl.detect(t + "/listscripts") is None

        os.mkdir(t + "/nullroot")
        open(t + "/nullroot/package.json", "w").write("null")
        assert runctl.detect(t + "/nullroot") is None

        os.mkdir(t + "/dnetroot")
        open(t + "/dnetroot/App.csproj", "w").close()
        assert runctl.detect(t + "/dnetroot") == {"argv": ["dotnet", "run"], "label": "dotnet run"}

        os.mkdir(t + "/dnet")          # root has NO csproj; two at depth 1 → alphabetically first wins
        os.mkdir(t + "/dnet/other")
        os.mkdir(t + "/dnet/src")
        open(t + "/dnet/other/B.csproj", "w").close()
        open(t + "/dnet/src/App.csproj", "w").close()
        assert runctl.detect(t + "/dnet")["argv"] == ["dotnet", "run", "--project", "other"]

@register
def test_runctl_pane_environ_overrides_claude_markers():
    # VTE envv MERGES onto the child's inherited environ (measured probe) — an
    # omitted marker would leak through whole. Ambient claude markers must be
    # OVERRIDDEN to empty so the Agent pane's claude stops disabling transcript
    # saving (its check is truthiness on CLAUDE_CODE_CHILD_SESSION).
    from alpaca_code import runctl
    saved = {k: os.environ.get(k) for k in runctl._CLAUDE_SESSION_MARKERS}
    try:
        leak = ("CLAUDE_CODE_CHILD_SESSION", "CLAUDE_CODE_SESSION_ID", "CLAUDE_PID")
        for k, v in zip(leak, ("1", "aaaa-bbbb", "999")):
            os.environ[k] = v
        entry = dict(x.split("=", 1) for x in runctl.pane_environ())
        for k, v in zip(leak, ("1", "aaaa-bbbb", "999")):
            assert entry[k] == "", f"{k} leaked {v!r} to pane children"
        assert "PATH=" in " ".join(runctl.pane_environ())      # sane environ still handed through
    finally:
        for k, v in saved.items():
            if v is None:
                os.environ.pop(k, None)
            else:
                os.environ[k] = v

# --- gitstatus (Task 4) ---------------------------------------------------------
@register
def test_gitstatus_branch():
    from alpaca_code import gitstatus
    with tempfile.TemporaryDirectory() as t:
        assert gitstatus.branch_of(t) is None
        os.mkdir(t + "/.git")
        open(t + "/.git/HEAD", "w").write("ref: refs/heads/main\n")
        assert gitstatus.branch_of(t) == "main"
        open(t + "/.git/HEAD", "w").write("0123456789abcdef0123456789abcdef01234567\n")
        assert gitstatus.branch_of(t) is None          # detached → hidden
        open(t + "/.git/HEAD", "w").write("gitdir: elsewhere\n")
        assert gitstatus.branch_of(t) is None          # worktree pointer → hidden

@register
def test_gitstatus_ahead_and_commit_then_push():
    from alpaca_code import gitstatus
    with tempfile.TemporaryDirectory() as t:
        assert gitstatus.ahead(t) == 0                 # not a repo
        g = _gitrepo(t)
        assert gitstatus.ahead(t) == 0                 # repo, no upstream → 0
        open(t + "/f.py", "w").write("a\n")
        g("add", "."); g("commit", "-m", "one")
        subprocess.run(["git", "init", "--bare", "-q", t + "origin.git"], check=True)
        g("remote", "add", "origin", t + "origin.git")
        assert gitstatus.ahead(t) == 0                 # upstream unset until first push
        g("push", "-u", "origin", "HEAD")              # git supplies @{u} → in sync
        assert gitstatus.ahead(t) == 0
        # commit_then_push: with files → phase fires before the push, both steps land
        open(t + "/f.py", "w").write("b\n")
        phases = []
        cok, ctext, pok, ptext = gitstatus.commit_then_push(
            t, ["f.py"], "two", lambda kind, text: phases.append((kind, text)))
        assert cok and pok, (ctext, ptext)
        assert phases == [("busy", "Pushing…")]
        assert gitstatus.ahead(t) == 0
        # push-only (empty paths): commit skipped, no phase, push runs
        open(t + "/f.py", "w").write("c\n")
        g("add", "."); g("commit", "-m", "three")
        assert gitstatus.ahead(t) == 1
        cok, ctext, pok, ptext = gitstatus.commit_then_push(t, [], "", None)
        assert cok and pok, (ptext,)
        assert gitstatus.ahead(t) == 0
        # failed commit: push never attempted
        cok, ctext, pok, ptext = gitstatus.commit_then_push(t, ["missing.py"], "x", None)
        assert not cok and not pok and ctext and ptext == ""

# --- filetree (Task 5) -----------------------------------------------------------
def _tree(tmp):
    os.makedirs(tmp + "/src/components")
    os.makedirs(tmp + "/hooks")
    os.makedirs(tmp + "/node_modules/junk")
    os.makedirs(tmp + "/.git")
    open(tmp + "/src/main.tsx", "w").close()
    open(tmp + "/src/components/FileTree.tsx", "w").close()
    open(tmp + "/package.json", "w").close()
    open(tmp + "/.gitignore", "w").close()
    open(tmp + "/README.md", "w").close()

@register
def test_filetree_icon_map():
    from alpaca_code import filetree
    assert filetree.icon_of("src", True) == "folder-symbolic"
    assert filetree.icon_of("x.py", False) == "text-x-python"
    assert filetree.icon_of("pkg.json", False) == "application-json"
    assert filetree.icon_of("README.md", False) == "text-x-markdown"
    assert filetree.icon_of("main.tsx", False) == "text-x-generic-symbolic"   # unknown ext → fallback
    assert filetree.icon_of("noext", False) == "text-x-generic-symbolic"

@register
def test_filetree_scan():
    from alpaca_code import filetree
    with tempfile.TemporaryDirectory() as t:
        _tree(t)
        assert [n for n, d in filetree.scan_dir(t)] == ["hooks", "src", ".gitignore", "package.json", "README.md"]
        assert filetree.scan_dir(t + "/src/components") == [("FileTree.tsx", False)]
        assert filetree.scan_project(t, "") == [".gitignore", "README.md", "package.json", "src/components/FileTree.tsx", "src/main.tsx"]
        hits = filetree.scan_project(t, "tsx")
        assert hits == ["src/components/FileTree.tsx", "src/main.tsx"]
        assert len(filetree.scan_project(t, "", cap=2)) == 2

# --- panels (Task 7): pure paths only; widgets/VTE spawn verified by probes -------
@register
def test_panels_env_and_kill():
    import signal, subprocess, time
    from alpaca_code import panels, runctl
    lb = os.path.expanduser("~/.local/bin")

    saved = os.environ.get("PATH")
    try:
        os.environ["PATH"] = ""
        assert f"PATH={os.pathsep}{lb}" in panels.get_env()
        os.environ["PATH"] = lb + os.pathsep + "/usr/bin"
        env2 = panels.get_env()
        assert f"PATH={lb}{os.pathsep}/usr/bin" in env2          # present → not appended twice
        assert any(v.startswith("HOME=") for v in env2)          # full environ, k=v list
    finally:
        os.environ["PATH"] = saved

    # claude nesting markers must never matter in pane children (Agent claude
    # turns off transcript saving over an inherited CLAUDE_CODE_CHILD_SESSION).
    # VTE spawn_async MERGES envv onto the child's inherited environ, so omission in
    # envv does NOT scrub the child — override to EMPTY (claude's check is truthiness).
    saved_markers = {k: os.environ.get(k) for k in runctl._CLAUDE_SESSION_MARKERS}
    try:
        for k, v in saved_markers.items():
            os.environ[k] = v or "1"
        env3 = dict(x.split("=", 1) for x in panels.get_env())
        stray = {k: v for k, v in env3.items() if k in runctl._CLAUDE_SESSION_MARKERS and v}
        assert not stray, stray
        assert any(v.startswith("HOME=") for v in panels.get_env())  # scrub, not nuke
    finally:
        for k, v in saved_markers.items():
            os.environ.pop(k, None)
            if v is not None:
                os.environ[k] = v

    # _kill_tree: child is a session leader (start_new_session) like VTE spawn children.
    # Liveness via poll()/returncode — a signal-killed child lingers in /proc as a
    # zombie until reaped, so f"/proc/{pid}" existence is NOT an aliveness signal.
    from gi.repository import GLib
    p = subprocess.Popen(["sleep", "30"], start_new_session=True)
    try:
        panels.Panes._kill_tree(p.pid)
        deadline = time.time() + 4
        while p.poll() is None and time.time() < deadline:
            GLib.MainContext.default().iteration(may_block=False)   # runs the 2s SIGKILL escalation
            time.sleep(0.1)
        assert p.returncode is not None and p.returncode < 0, f"rc={p.returncode}"
        assert p.returncode in (-signal.SIGHUP, -signal.SIGKILL), f"rc={p.returncode}"
    finally:
        p.wait()

    # exit-status mapping: raw wait-status → ("Exit N", ok|err), without instantiating widgets
    for status, expect in ((0, ("Exit 0", "ok")), (512, ("Exit 2", "err"))):
        rec2b = []
        fake = type("F", (), {
            "_set_status": staticmethod(lambda t, c="": rec2b.append((t, c))),
            "_pids": {"output": 123},
            "_run_starting": False,})()
        panels.Panes._run_child_exited(fake, None, status)
        assert rec2b[-1] == expect, rec2b[-1]

# --- run lifecycle (Task 8): Launch's pid must land, status must not race it -----
def _make_panes_fake(pid_alive=lambda pid: False):
    """Fake Panes: real launch_run/has_running_run/stop_run bound, widget bits stubbed.
    pid_alive stands in for the /proc check so tests need no real child."""
    rec = []
    cap = {}
    fake = type("F", (), {
        # widget surface launch_run touches
        "nb": type("NB", (), {"set_current_page": lambda s, i: cap.setdefault("page", i)})(),
        "out": type("OUT", (), {"reset": lambda s, a, b: cap.setdefault("reset", True)})(),
        "root": "/tmp/x",
        "_set_status": staticmethod(lambda t, c="": rec.append((t, c))),
        "_pids": {"output": None},
        "_run_starting": False,
        "_run_stop_pending": False,
        "rec": staticmethod(lambda: rec),
        "cap": staticmethod(lambda: cap),
        "pid_alive": staticmethod(pid_alive),
    })()
    return fake, rec, cap

@register
def test_run_pid_landing_and_races():
    from alpaca_code import panels
    landed = {}

    def fake_spawn(term, cwd, argv, on_ready=None):
        landed["term"], landed["cwd"], landed["argv"], landed["on_ready"] = term, cwd, argv, on_ready
        # does NOT call on_ready: simulates the async window between launch and pid arrival
        captured_spawns.append((argv, on_ready))

    captured_spawns = []
    real_spawn = panels.spawn
    panels.spawn = fake_spawn
    try:
        def build(alive=lambda pid: True):
            f, rec, cap = _make_panes_fake(pid_alive=alive)
            for m in ("launch_run", "has_running_run", "stop_run"):
                setattr(f, m, panels.Panes.__dict__[m].__get__(f))
            f._alive = lambda pid: bool(pid) and f.pid_alive(pid)   # mirrors real _alive's None guard
            return f, rec, cap

        # launch: spawns in out's cwd, switches to Output tab, marks "starting" immediately
        f, rec, cap = build()
        assert f.launch_run(["npm", "run", "dev"], "npm run dev") is True
        assert cap.get("page") == 2 and cap.get("reset") is True   # Output = page 2
        assert landed["cwd"] == "/tmp/x"
        assert f.has_running_run() is True          # guard covers the starting window
        assert f.launch_run(["x"], "x") is False    # double-launch refused

        # pid lands asynchronously → tracker + "Running" status only then
        assert f._pids["output"] is None
        landed["on_ready"](f.out, 4242, None)
        assert f._pids["output"] == 4242 and f._run_starting is False
        assert f.has_running_run() is True
        assert any(r[0] == "Running" for r in rec)

        # natural exit keeps its mapping (pid tracked → reported)
        panels.Panes._run_child_exited(f, None, 0)
        assert f._pids["output"] is None and rec[-1] == ("Exit 0", "ok")

        # spawn error: starting guard consumed, error surfaced, nothing tracked
        f2, rec2, _ = build()
        assert f2.launch_run(["bad"], "bad") is True
        err = type("E", (), {"message": "Failed to execve: x"})()
        landed["on_ready"](f2.out, -1, err)
        assert f2._run_starting is False and f2._pids["output"] is None
        assert f2.has_running_run() is False
        assert any("failed to start" in r[0].lower() for r in rec2), rec2

        # Stop while still starting: no pid yet → pending; kill the fresh pid on landing
        f3, rec3, _ = build()
        killed = {}
        f3._kill_tree = lambda pid: killed.setdefault("pid", pid)
        f3.launch_run(["x"], "x")
        f3.stop_run()
        assert f3._run_stop_pending is True
        landed["on_ready"](f3.out, 777, None)
        assert killed.get("pid") == 777
        assert f3._pids["output"] is None and f3._run_stop_pending is False
        assert any(r[0] == "Running" for r in rec3) is False

        # child-exited during cleanup (pid None, not starting) must not clobber status
        f4, rec4, _ = build(alive=lambda pid: False)
        f4._pids["output"] = None
        panels.Panes._run_child_exited(f4, None, 0)
        assert all(r[0] != "Exit 0" for r in rec4), rec4
    finally:
        panels.spawn = real_spawn

# --- pane respawn cap (Final review Important #4): crash-loops must stop ----------
@register
def test_pane_respawn_capped():
    import time as _t
    from alpaca_code import panels
    def fake(key, respawns, born):
        rec_resp, rec_status = [], []
        f = type("F", (), {})()
        f._kills_pending = {}
        f.root, f._spawned = "/w", {key}
        f._pane_spawned_at = {key: _t.time() - born}
        f._respawns = respawns
        f._spawn_pane = lambda k: rec_resp.append(k)
        f._set_status = lambda text, cls="": rec_status.append((text, cls))
        f._pane_child_exited = panels.Panes.__dict__["_pane_child_exited"].__get__(f)
        return f, rec_resp, rec_status

    # 3 consecutive ≥3s deaths → 3 revives; the 4th is capped with an "err" status
    f, resp, status = fake("agent", {}, 10)
    for _ in range(3):
        f._pane_child_exited(None, 0, "agent")
    assert resp == ["agent"] * 3, resp
    f._pane_child_exited(None, 0, "agent")
    assert resp == ["agent"] * 3, resp                       # 4th death: no revive
    assert status and status[-1] == ("Agent exited", "err"), status

    # a long-lived cycle re-arms the budget (lived ≥60s)
    f2, resp2, _ = fake("term", {"term": 3}, 120)
    f2._pane_child_exited(None, 0, "term")
    assert resp2 == ["term"] and f2._respawns["term"] == 1

    # short-lived death (<3s) stays "broken child": no revive, no status, cap untouched
    f3, resp3, status3 = fake("agent", {"agent": 3}, 1)
    f3._pane_child_exited(None, 0, "agent")
    assert resp3 == [] and status3 == []

    # our own set_root kill exits: swallowed, cap not incremented
    f4, resp4, _ = fake("agent", {}, 10)
    f4._kills_pending["agent"] = 1
    f4._pane_child_exited(None, 0, "agent")
    assert resp4 == [] and f4._respawns == {}

# --- window wiring (Task 9): final set_workspace persists + rewires; menu shape ----
def _btn_fake():
    b = type("B", (), {})()
    b.sensitive, b.tip = None, None
    b.set_sensitive = lambda v: setattr(b, "sensitive", v)
    b.set_tooltip_text = lambda t: setattr(b, "tip", t)
    return b

def _patch_state(tmpfile):
    from alpaca_code import state
    real_load, real_save = state.load, state.save
    state.load = lambda p=tmpfile: real_load(p)
    state.save = lambda s, p=tmpfile: real_save(s, p)
    return state, real_load, real_save

@register
def test_window_set_workspace_persistence():
    from alpaca_code import window, runctl
    with tempfile.TemporaryDirectory() as t:
        state, real_load, real_save = _patch_state(os.path.join(t, "state.json"))
        try:
            os.mkdir(t + "/proj")
            open(t + "/proj/package.json", "w").write('{"name": "p", "scripts": {"start": "x"}}')
            calls = []
            f = type("F", (), {
                "tree": type("TR", (), {"set_root": lambda s, p: calls.append(("tree", p))})(),
                "panes": type("PN", (), {"set_root": lambda s, p: calls.append(("panes", p)),
                                         "has_running_run": lambda s: False})(),
                "editor": type("ED", (), {"restore": lambda s, p, tabs: calls.append(("editor", p, tabs)),
                                          "has_dirty": lambda s: False})(),
                "run_btn": _btn_fake(), "stop_btn": _btn_fake(),
                "run_row": type("RR", (), {"set_css_classes": lambda s, c: setattr(s, "classes", c)})(),
                "win": type("W", (), {"set_title": lambda s, tt: calls.append(("title", tt))})(),
                "_title": type("L", (), {"set_text": lambda s, tt: calls.append(("wintitle", tt))})(),
                "root": None,
            })()
            f.set_workspace = window.Window.__dict__["set_workspace"].__get__(f)
            f._set_workspace_now = window.Window.__dict__["_set_workspace_now"].__get__(f)
            f._refresh_run_style = window.Window.__dict__["_refresh_run_style"].__get__(f)
            f.set_workspace(t + "/proj")
            s = state.load()
            assert s["last_project"] == t + "/proj", s
            assert s["recents"] == [t + "/proj"], s
            assert ("title", "alpaca_code") in calls   # WM title stays; headerbar wintitle is a label
            assert calls[calls.index(("title", "alpaca_code")) + 1] == ("wintitle", f"alpaca-code — proj")
            assert ("tree", t + "/proj") in calls and ("panes", t + "/proj") in calls
            assert ("editor", t + "/proj", {"open_tabs": [], "active_tab": 0}) in calls
            assert f.run_btn.sensitive is True
            assert f.run_btn.tip == ("npm start" if runctl.detect(t + "/proj")["label"] == "npm start" else None)
            assert f.stop_btn.sensitive is False
            assert getattr(f.run_row, "classes", None) == ["alpaca-runpill"]  # idle: flat shell, no .running
        finally:
            state.load, state.save = real_load, real_save

@register
def test_build_menu_recents_and_shape():
    from alpaca_code import window
    from gi.repository import GLib as _GL
    with tempfile.TemporaryDirectory() as t:
        state, real_load, real_save = _patch_state(os.path.join(t, "state.json"))
        try:
            s = state.default()
            s = state.remember(s, "/tmp/alpacademo")
            s = state.remember(s, "/tmp/alpacademo2")
            state.save(s)
            f = type("F", (), {})()
            f._build_menu = window.Window.__dict__["_build_menu"].__get__(f)
            m = f._build_menu()
            labels = []
            for i in range(m.get_n_items()):
                v = m.get_item_attribute_value(i, "label", _GL.VariantType.new("s"))
                labels.append(v.get_string() if v else None)
            assert labels == ["Open Project…", "Open Recent", "New Project…", "Quit"], labels
            sub = m.get_item_link(1, "submenu")   # GLib.MENU_LINK_SUBMENU constant not exposed in PyGI
            sub_labels = []
            for i in range(sub.get_n_items()):
                v = sub.get_item_attribute_value(i, "label", _GL.VariantType.new("s"))
                sub_labels.append(v.get_string() if v else None)
            assert sub_labels == ["alpacademo2 — /tmp/alpacademo2", "alpacademo — /tmp/alpacademo"], sub_labels
            # newest first, action targets attached
            v = sub.get_item_attribute_value(0, "action", _GL.VariantType.new("s"))
            target = sub.get_item_attribute_value(0, "target", _GL.VariantType.new("s"))
            assert v.get_string() == "app.open-recent"
            assert target.get_string() == "/tmp/alpacademo2"
            # no recents → no submenu
            os.remove(os.path.join(t, "state.json"))
            m2 = f._build_menu()
            labels2 = [m2.get_item_attribute_value(i, "label", _GL.VariantType.new("s")).get_string()
                       for i in range(m2.get_n_items())]
            assert labels2 == ["Open Project…", "New Project…", "Quit"], labels2
        finally:
            state.load, state.save = real_load, real_save

# --- git changes view: pure git ops (2026-10-05 spec) -----------------------------
def _gitrepo(tmp):
    """Plain temp repo with identity configured; returns run(name...) helper."""
    import subprocess
    def g(*args):
        r = subprocess.run(["git", "-C", tmp, *args], capture_output=True, text=True)
        assert r.returncode == 0, (args, r.stderr)
        return r
    g("init", "-q")
    g("config", "user.email", "t@t"); g("config", "user.name", "t")
    return g

@register
def test_gitstatus_parse_unified_and_build_sides():
    from alpaca_code import gitstatus
    diff_text = (
        "diff --git a/x.py b/x.py\n"
        "--- a/x.py\n"
        "+++ b/x.py\n"
        "@@ -1,4 +1,5 @@\n"
        " keep\n"
        "-kill me\n"
        "+add me\n"
        " keep2\n"
        "diff --git a/x.py b/x.py\n"      # second file's headers must be skipped cleanly
        "@@ -10,3 +11,4 @@\n"
        " more\n"
        "-gone\n"
        "-gone2\n"
        "+here\n"
        "@@ -30 +31 @@\n"                  # ",1" omitted count form
        "= ctx-like junk line"             # not a real diff op: ignored, not crashed on
    )
    hunks = gitstatus.parse_unified(diff_text)
    assert [(h[0], h[1], h[2], h[3]) for h in hunks] == [(1, 4, 1, 5), (10, 3, 11, 4), (30, 1, 31, 1)]
    assert hunks[0][4] == [("=", "keep"), ("<", "kill me"), (">", "add me"), ("=", "keep2")]
    old, new, dl, al, hdr = gitstatus.build_sides(hunks)
    # hdr row per hunk keeps hunk1/hunk2 rows aligned on both sides
    assert old[:5] == [gitstatus.HUNK_ROW, "keep", "kill me", "", "keep2"]
    assert new[:5] == [gitstatus.HUNK_ROW, "keep", "", "add me", "keep2"]
    assert old[5] == new[5] == gitstatus.HUNK_ROW
    assert old[6:] == ["more", "gone", "gone2", "", gitstatus.HUNK_ROW]
    assert new[6:] == ["more", "", "", "here", gitstatus.HUNK_ROW]
    assert len(old) == len(new)
    assert dl == {2, 7, 8} and al == {3, 9} and hdr == {0, 5, 10}
    no_hunks = gitstatus.parse_unified("Binary files a/x and b/x differ\n")
    assert no_hunks == []      # caller shows a notice, not an empty page

@register
def test_gitstatus_changes_porcelain():
    from alpaca_code import gitstatus
    import subprocess
    with tempfile.TemporaryDirectory() as t:
        assert gitstatus.changes(t) is None            # not a repo
        g = _gitrepo(t)
        assert gitstatus.changes(t) == []              # repo, zero commits, clean index
        open(t + "/new.py", "w").write("x = 1\n")
        open(t + "/mod.py", "w").write("a\n")
        g("add", ".")
        # no HEAD yet: staged adds still report (staged letter wins)
        assert sorted(gitstatus.changes(t)) == [("mod.py", "A"), ("new.py", "A")]
        g("commit", "-m", "one")
        open(t + "/mod.py", "w").write("b\n")
        assert gitstatus.changes(t) == [("mod.py", "M")]
        open(t + "/untouched.py", "w").write("q\n")    # untracked
        assert sorted(gitstatus.changes(t)) == [("mod.py", "M"), ("untouched.py", "U")]
        g("add", "untouched.py")                       # staged new
        assert sorted(gitstatus.changes(t)) == [("mod.py", "M"), ("untouched.py", "A")]
        g("commit", "-a", "-m", "clean")               # staged adds persist until committed — clear the slate
        open(t + "/gone.py", "w").write("d\n")
        g("add", "gone.py"); g("commit", "-m", "two")
        os.remove(t + "/gone.py")                      # worktree deletion
        assert gitstatus.changes(t) == [("gone.py", "D")]
        # rename: -z puts the ORIGINAL as the next record — parser emits R(new) + D(original)
        subprocess.run(["git", "-C", t, "mv", "mod.py", "moved.py"], check=True)
        assert sorted(gitstatus.changes(t)) == [
            ("gone.py", "D"), ("mod.py", "D"), ("moved.py", "R")]
        # untracked DIRECTORY: `-u all` lists each file inside — the tree needs
        # per-file rows, never a `dir/` row (spec §2 tree shape)
        os.makedirs(t + "/nest/deep")
        open(t + "/nest/deep/f.py", "w").write("z\n")
        assert sorted(gitstatus.changes(t)) == [
            ("gone.py", "D"), ("mod.py", "D"), ("moved.py", "R"), ("nest/deep/f.py", "U")]

@register
def test_gitstatus_diff_for_untracked_and_nohead():
    from alpaca_code import gitstatus
    with tempfile.TemporaryDirectory() as t:
        g = _gitrepo(t)
        open(t + "/a.py", "w").write("one\n")
        g("add", "a.py")                               # staged, NO HEAD yet
        out, binary = gitstatus.diff_for(t, "a.py", is_untracked=False)
        assert binary is False and out
        hunks = gitstatus.parse_unified(out)
        assert hunks and all(k == ">" for k, _ in hunks[0][4])   # all-added vs the void
        g("commit", "-m", "init")                      # HEAD exists from here on
        open(t + "/a.py", "w").write("one\nTWO\n")
        out, binary = gitstatus.diff_for(t, "a.py", is_untracked=False)
        hunks = gitstatus.parse_unified(out)
        assert hunks and [k for k, _ in hunks[0][4]] == ["=", ">"]
        open(t + "/fresh.py", "w").write("hello\n")     # untracked: no-index trick
        out, binary = gitstatus.diff_for(t, "fresh.py", is_untracked=True)
        hunks = gitstatus.parse_unified(out)
        assert binary is False and hunks and all(k == ">" for k, _ in hunks[0][4])
        # binary file → flag, no parse attempt
        open(t + "/bin.dat", "wb").write(b"\x00\x01\x02")
        g("add", "bin.dat")
        out, binary = gitstatus.diff_for(t, "bin.dat", is_untracked=False)
        assert binary is True and out == ""
        ok, _text = gitstatus.commit(t, ["bin.dat"], "bin"); assert ok
        open(t + "/bin.dat", "ab").write(b"\x03")
        out, binary = gitstatus.diff_for(t, "bin.dat", is_untracked=False)
        assert binary is True

@register
def test_gitstatus_commit_only_selected():
    from alpaca_code import gitstatus
    with tempfile.TemporaryDirectory() as t:
        g = _gitrepo(t)
        open(t + "/sel.py", "w").write("one\n")
        open(t + "/other.py", "w").write("one\n")
        g("add", "."); g("commit", "-m", "init")
        open(t + "/sel.py", "w").write("two\n")
        open(t + "/other.py", "w").write("two\n")
        g("add", "other.py")                            # sibling staged via CLI, NOT selected
        g("add", "sel.py")                              # selection staged too (typical flow)
        # untracked file that must ride along (untracked+modified+deleted in one call)
        open(t + "/added.py", "w").write("brand\n")
        ok, text = gitstatus.commit(t, ["sel.py", "added.py"], "pick sel")
        assert ok, text
        names = g("show", "--name-only", "--format=").stdout.split()
        assert sorted(names) == ["added.py", "sel.py"], names          # exactly the selection
        assert g("show", "--format=%s", "-s").stdout.strip() == "pick sel"
        # sibling: still staged, not committed
        assert gitstatus.changes(t) == [("other.py", "M")]
        assert [("other.py", "M")] == [c for c in gitstatus.changes(t) if c[0] == "other.py"]
        # guards: empty message / empty selection / missing repo — never raise
        assert gitstatus.commit(t, ["other.py"], "   ")[0] is False
        assert gitstatus.commit(t, [], "x")[0] is False
        assert gitstatus.commit("/nope", ["x"], "x")[0] is False
        # non-repo has no HEAD — covered by /nope; worktree diff text == committed content:
        out, binary = gitstatus.diff_for(t, "other.py", is_untracked=False)
        new_l = [s for k, s in gitstatus.parse_unified(out)[0][4] if k == ">"]
        assert new_l == ["two"]                          # worktree content, not the older staged copy
        ok, text = gitstatus.commit(t, ["other.py"], "take two")
        assert ok and names  # committed worktree content

@register
def test_gitstatus_push_offline():
    from alpaca_code import gitstatus
    with tempfile.TemporaryDirectory() as t:
        assert gitstatus.push("/nope") == (False, "Not a git repository")
        g = _gitrepo(t)                                # no remote configured
        ok, text = gitstatus.push(t)
        assert ok is False and text, text              # readable error, no exception, no hang
        assert ("push" in text.lower() or "destination" in text.lower()
                or "remote" in text.lower()), text

@register
def test_editor_diff_pages_invisible_to_machinery():
    """Diff pages (.diff_of only, no .path/.buf) must be invisible to save,
    persistence and dirty-tracking, and findable by _page_of (spec §3)."""
    import tempfile, os
    from types import SimpleNamespace
    from alpaca_code import editor

    class FakeNB:
        def __init__(self, pages): self.pages = pages; self.cur = 0
        def get_n_pages(self): return len(self.pages)
        def get_nth_page(self, i):
            return self.pages[i] if 0 <= i < len(self.pages) else None
        def get_current_page(self): return self.cur

    diff_page = SimpleNamespace(diff_of="diff:one.py")
    file_page = SimpleNamespace()          # .path/.buf attached below
    e = editor.Editor.__new__(editor.Editor)   # no widget construction in tests
    e.root = "/w"
    e.nb = FakeNB([diff_page, file_page])
    # _page_of finds diff pages by key and skips others
    assert editor.Editor._page_of(e, "diff:one.py") == 0
    assert editor.Editor._page_of(e, "diff:missing.py") == -1
    # get_open_state: only the file page persists, rel to root
    file_page.path = "/w/one.py"
    assert editor.Editor.get_open_state(e) == {"open_tabs": ["one.py"], "active_tab": 0}
    diff_page_first = SimpleNamespace(diff_of="diff:a.py")
    e.nb.pages[:] = [diff_page_first, diff_page]
    assert editor.Editor.get_open_state(e) == {"open_tabs": [], "active_tab": 0}
    assert editor.Editor.has_dirty(e) is False            # no bufs anywhere
    # open_state with an active diff tab: page index returned verbatim
    e.nb.cur = 1
    assert editor.Editor.get_open_state(e)["active_tab"] == 1
    # _write_page + save_open: named dirty file written, diff page untouched
    e.nb.pages[:] = [diff_page]
    with tempfile.TemporaryDirectory() as td:
        fp = os.path.join(td, "one.py")
        open(fp, "w").write("old")
        file_page2 = SimpleNamespace(path=fp)
        file_page2.buf = SimpleNamespace(props=SimpleNamespace(text="new\n"),
                                         get_modified=lambda: True,
                                         set_modified=lambda m: None)
        e.nb.pages = [diff_page, file_page2]
        e.nb.cur = 0                        # active = the diff page (cur was 1 from the assert above)
        editor.Editor.save_active(e)                      # diff page → no-op, no OSError
        assert open(fp).read() == "old"
        editor.Editor.save_open(e, [os.path.abspath(fp)])
        assert open(fp).read() == "new\n"
        # and a file page NOT in the named set stays open-unsaved
        editor.Editor.save_open(e, [])
        assert open(fp).read() == "new\n"

@register
def test_gitstatus_group_tree_and_dir_state():
    from alpaca_code import gitstatus
    rows = [("src/b.py", "M"), ("src/a.py", "A"), ("top.py", "U"),
            ("sub/deep/x.py", "D"), ("sub/y.py", "M")]
    t = gitstatus.group_tree(rows)
    # parents before children; dirs then files at each level; depth from hierarchy
    assert [(r[0], r[2], r[4]) for r in t] == [
        ("d", "src", 0), ("f", "src/a.py", 1), ("f", "src/b.py", 1),
        ("d", "sub", 0), ("d", "sub/deep", 1), ("f", "sub/deep/x.py", 2),
        ("f", "sub/y.py", 1), ("f", "top.py", 0)]
    # names are the basename segments; file rel keeps the '/' form
    assert {r[2]: r[1] for r in t}["sub/deep/x.py"] == "x.py"
    assert gitstatus.group_tree([]) == []
    # dir state: (all descendants checked, any checked); a full dir is (True, True)
    checked = {"src/a.py", "sub/y.py"}
    assert gitstatus.checked_dir_state(t, checked) == {
        "src": (False, True), "sub": (False, True), "sub/deep": (False, False)}
    assert gitstatus.checked_dir_state(t, {"sub/deep/x.py", "sub/y.py"}) == {
        "src": (False, False), "sub": (True, True), "sub/deep": (True, True)}
    assert gitstatus.checked_dir_state(t, set()) == {
        "src": (False, False), "sub": (False, False), "sub/deep": (False, False)}

@register
def test_badges_letter_pixbuf():
    from alpaca_code import badges
    try:
        import cairo
    except ImportError:
        return   # no cairo → chips are None everywhere; nothing to assert
    p = badges.letter_pixbuf("M", "#f2c94c")
    assert p is not None and p.get_width() == 16 and p.get_height() == 16
    assert badges.letter_pixbuf("M", "#f2c94c") is p     # cached, same spec
    assert badges.letter_pixbuf("Q", "#ef4444") is not None

def main():
    failed = 0
    for t in TESTS:
        try:
            t(); print(f"PASS {t.__name__}")
        except Exception as e:
            failed += 1; print(f"FAIL {t.__name__}: {e}")
    return 1 if failed else 0

@register
def test_gitstatus_commit_staged_deletion_and_rename():
    # Review C2: `git add -- <paths>` died on staged-only paths (fatal pathspec,
    # rc 128) -> Commit DEAD for any changeset holding a staged rm/mv/rename.
    from alpaca_code import gitstatus
    with tempfile.TemporaryDirectory() as t:
        g = _gitrepo(t)
        for n in ("gone.py", "mod.py", "old.py", "keep.py"):
            open(t + "/" + n, "w").write("v\n")
        g("add", "."); g("commit", "-m", "init")
        open(t + "/mod.py", "w").write("v2\n")
        g("rm", "-q", "gone.py")                # staged deletion (D  gone.py)
        g("mv", "old.py", "renamed.py")         # staged rename pair (R + D old.py)
        ok, text = gitstatus.commit(t, ["gone.py", "renamed.py", "old.py", "mod.py"], "drop+rename")
        assert ok, text
        names = g("show", "--name-only", "--no-renames", "--format=").stdout.split()
        assert sorted(names) == ["gone.py", "mod.py", "old.py", "renamed.py"], names
        # staged-deletion-only selection: `git add --` runs with an EMPTY list
        open(t + "/keep.py", "w").write("v2\n"); g("add", "keep.py"); g("rm", "-q", "--cached", "keep.py")
        ok, text = gitstatus.commit(t, ["keep.py"], "drop keep")
        assert ok, text
        assert g("show", "--name-only", "--no-renames", "--format=").stdout.split() == ["keep.py"]
@register
def test_gitstatus_unmerged_letter_c():
    # Review I1: unmerged paths (UU/AA/…) took xy[0]='U' — indistinguishable
    # from untracked 'U' → the diff opener fed `git --no-index` and lied (all-
    # added green page). Conflicted rows carry letter "C"; `git diff HEAD`
    # still yields hunks for them.
    from alpaca_code import gitstatus
    with tempfile.TemporaryDirectory() as t:
        g = _gitrepo(t)
        open(t + "/c.py", "w").write("base\n")
        g("add", "."); g("commit", "-m", "init")
        g("checkout", "-qb", "b2")          # b2 forks from the base commit — else the merge is a fast-forward
        g("checkout", "-q", "master")
        open(t + "/c.py", "w").write("one\n")
        g("commit", "-am", "one")
        g("checkout", "-q", "b2")
        open(t + "/c.py", "w").write("two\n")
        g("commit", "-am", "two")
        g("checkout", "-q", "master")
        # --no-rebase: rc 1 is the CONFLICT (the helper's rc==0 assert must not fire)
        m = subprocess.run(["git", "-C", t, "merge", "--no-edit", "b2"],
                           capture_output=True, text=True,
                           env=dict(os.environ, GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@t",
                                    GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@t"))
        assert m.returncode != 0 and "CONFLICT" in m.stdout + m.stderr
        files = dict(gitstatus.changes(t))
        assert files["c.py"] == "C", files
        text, binary = gitstatus.diff_for(t, "c.py", is_untracked=(files["c.py"] == "U"))
        assert not binary and text.strip(), repr(text[:200])

# --- live file changes (2026-10-05 spec): _fs_settled decision table --------------
@register
def test_editor_fs_settle_matrix():
    """Debounced disk checks (real GtkSource buffers + fakeries): our-save echo,
    clean external edit → seamless reload, dirty buffer → amber conflict dot and
    untouched text, vanished clean file → italic tombstone note, clean Ctrl+S can
    never resurrect the file, dirty tombstone keeps the text and save recreates."""
    import tempfile
    from types import SimpleNamespace
    from alpaca_code import editor, main
    import alpaca_code.gi_env as ge
    ge.require("Gtk", ("4.0",))
    ge.require("GtkSource", ("5",))
    from gi.repository import Gtk, GtkSource

    assert ".alpaca-dirty.conflict" in main.CSS and ".alpaca-tabname.alpaca-deleted" in main.CSS

    class _Kid:
        def __init__(self, classes=()):
            self._classes = list(classes)
            self.visible = None
            self.dot_classes = None
            self.added, self.removed = [], []
            self._next = None
        def get_css_classes(self):
            return self._classes
        def get_first_child(self):
            return None
        def get_next_sibling(self):
            return self._next
        def set_visible(self, v):
            self.visible = v
        def set_css_classes(self, c):
            self.dot_classes = list(c)
        def add_css_class(self, c):
            self.added.append(c)
        def remove_css_class(self, c):
            self.removed.append(c)

    class _Head:
        def __init__(self, kids):
            self.kids = kids
        def get_first_child(self):
            return self.kids[0]

    class _FakeNB:
        def __init__(self, pages):
            self.pages = pages; self.cur = 0
        def get_n_pages(self):
            return len(self.pages)
        def get_nth_page(self, i):
            return self.pages[i] if 0 <= i < len(self.pages) else None
        def get_current_page(self):
            return self.cur
        def get_tab_label(self, page):
            return page.tab_head

    def make_page(t, fname, content, *, fs=None):
        fp = os.path.join(t, fname)
        with open(fp, "w") as f:
            f.write(content)
        buf = GtkSource.Buffer(text=content)
        buf.set_modified(False)
        view = GtkSource.View()
        scroll = {"v": 0.0, "u": 0.0}
        sw = SimpleNamespace(get_vadjustment=lambda: SimpleNamespace(
            get_value=(lambda: scroll["v"]), get_upper=(lambda: scroll["u"])))
        dot, name = _Kid(), _Kid(classes=["alpaca-tabname"])
        dot._next = name
        page = SimpleNamespace(path=fp, buf=buf, view=view,
                               sw=sw,
                               diff_of=None, tab_head=_Head([dot, name]),
                               _fs=fs, _conflict=False, _load_mtime=None)
        return page, fp, dot, name

    def fake_editor(page):
        e = editor.Editor.__new__(editor.Editor)     # no widget construction in tests
        e.root = os.path.dirname(page.path)
        e.nb = _FakeNB([page])
        e.on_state_changed = lambda: None
        e._pending = {}
        return e

    with tempfile.TemporaryDirectory() as t:
        page, fp, dot, name = make_page(t, "a.py", "v1\n")
        e = fake_editor(page)

        # our-own-open/save echo: same mtime → no work, no state churn
        page._load_mtime = os.stat(fp).st_mtime_ns
        assert editor.Editor._fs_settled(e, fp) is False
        assert page.buf.props.text == "v1\n" and page._fs is None

        # clean external edit → seamless reload, style carried, buffer clean
        open(fp, "w").write("v2\n" * 30)
        assert editor.Editor._fs_settled(e, fp) is False
        assert page.buf.props.text.splitlines()[0] == "v2"
        assert page._fs is None and page._conflict is False and page._load_mtime == os.stat(fp).st_mtime_ns
        assert page.buf.get_modified() is False
        assert page.view.get_buffer() is page.buf

        # external edit on a DIRTY buffer → conflict: text untouched, amber dot
        page.buf.set_modified(True)
        open(fp, "w").write("v3\n")
        assert editor.Editor._fs_settled(e, fp) is False
        assert page.buf.props.text == "v2\n" * 30 and page._conflict is True
        assert dot.dot_classes == ["alpaca-dirty", "conflict"]
        assert dot.visible is True and page._load_mtime != os.stat(fp).st_mtime_ns

        # Ctrl+S from the conflict: text wins, conflict clears, echo latched
        editor.Editor._write_page(e, page)
        assert open(fp).read() == "v2\n" * 30 and page._conflict is False
        assert editor.Editor._fs_settled(e, fp) is False    # our save's echo → no reload
        assert page._conflict is False

        # vanished clean file → tombstone: muted note, italic tab, never resurrected
        page.buf.set_modified(False)
        os.remove(fp)
        assert editor.Editor._fs_settled(e, fp) is False
        assert page._fs == "deleted" and page.buf.props.text == editor.GONE_NOTE
        assert "alpaca-deleted" in name.added
        editor.Editor.save_active(e)                        # clean buffer + missing file
        assert not os.path.exists(fp)                       # the note can't recreate the file

        # file re-created → tombstone lifts: reload, italic removed
        open(fp, "w").write("v4\n")
        assert editor.Editor._fs_settled(e, fp) is False
        assert page._fs is None and page.buf.props.text == "v4\n"
        assert "alpaca-deleted" in name.removed

        # vanished DIRTY file → text keeps living (Ctrl+S recreates)
        page.buf.set_modified(True)
        os.remove(fp)
        assert editor.Editor._fs_settled(e, fp) is False
        assert page._fs == "deleted" and page.buf.props.text == "v4\n"   # user text kept
        editor.Editor._write_page(e, page)
        assert open(fp).read() == "v4\n"                    # explicit save = file recreated

        # unreadable disk content on a clean tab → frozen note, still save-proof
        page.buf.set_modified(False)
        page._fs = None
        open(fp, "wb").write(b"\x00\x01\xff")
        page._load_mtime = None
        assert editor.Editor._fs_settled(e, fp) is False
        assert page._fs == "binary" and page.buf.props.text == editor.BINARY_NOTE
        editor.Editor.save_active(e)
        with open(fp, "rb") as f:
            assert f.read() == b"\x00\x01\xff"              # binary content untouched

if __name__ == "__main__":
    sys.exit(main())

