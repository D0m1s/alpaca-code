#!/usr/bin/env python3
"""alpaca_code self-check: plain asserts, no framework. Run: python3 tests/test_selfcheck.py"""
import json, os, sys, tempfile

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
    from alpaca_code import panels
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
        assert cap.get("page") == 1 and cap.get("reset") is True
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
    assert status and status[-1] == ("Agent Console exited", "err"), status

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
                "panes": type("PN", (), {"set_root": lambda s, p: calls.append(("panes", p))})(),
                "editor": type("ED", (), {"restore": lambda s, p, tabs: calls.append(("editor", p, tabs)),
                                          "has_dirty": lambda s: False})(),
                "run_btn": _btn_fake(), "stop_btn": _btn_fake(),
                "win": type("W", (), {"set_title": lambda s, tt: calls.append(("title", tt))})(),
                "root": None,
            })()
            f.set_workspace = window.Window.__dict__["set_workspace"].__get__(f)
            f._set_workspace_now = window.Window.__dict__["_set_workspace_now"].__get__(f)
            f.set_workspace(t + "/proj")
            s = state.load()
            assert s["last_project"] == t + "/proj", s
            assert s["recents"] == [t + "/proj"], s
            assert ("title", "alpaca-code — proj") in calls
            assert ("tree", t + "/proj") in calls and ("panes", t + "/proj") in calls
            assert ("editor", t + "/proj", {"open_tabs": [], "active_tab": 0}) in calls
            assert f.run_btn.sensitive is True
            assert f.run_btn.tip == ("npm start" if runctl.detect(t + "/proj")["label"] == "npm start" else None)
            assert f.stop_btn.sensitive is False
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

def main():
    failed = 0
    for t in TESTS:
        try:
            t(); print(f"PASS {t.__name__}")
        except Exception as e:
            failed += 1; print(f"FAIL {t.__name__}: {e}")
    return 1 if failed else 0

if __name__ == "__main__":
    sys.exit(main())