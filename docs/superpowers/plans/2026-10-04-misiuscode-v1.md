# misiuscode v1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One-window GTK4 workspace that hosts a raw `claude` CLI terminal, a tabbed code editor, a file explorer, a plain terminal, and npm/dotnet run output — for one project at a time.

**Architecture:** Single Python/GTK4 app, ~10 small modules, plain callbacks between components (no framework, custom GObject signals). All process spawning goes through VTE (`Vte.Terminal.spawn_async`), which owns the pty; run-child termination via `killpg` because a VTE child is a session leader, so npm's node children share its pgid and die with it.

**Tech Stack:** Python 3.11+ (measured: 3.14.7 on this box) + PyGObject, GTK4, GtkSourceView 5, VTE 4. Zero pip dependencies.

**Spec:** `docs/superpowers/specs/2026-10-04-misiuscode-design.md` — read it first; the palette table, layout measurements (1586×992, right pane 395px, editor/pane split 594/324), and behavior contracts live there.

## Global Constraints

- Resolve gir versions only through `misiuscode/gi_env.py` (`Gtk`→`("4.0",)`, `GtkSource`→`("5",)`, `Vte`→`("4","4.0")`), never hardcoded without the probe. `Gtk-4.0` and `GLib-2.0` are already installed; `GtkSource-5` and `Vte` are not.
- Runtime deps: `pacman -S gtksourceview5 vte4` (Task 1). If it needs root, ask the user to run it. No pip installs.
- **No git in any task.** Git commands are blocked in the implementing session; Task 10 hands the user `git init`/commit commands verbatim. There are no commit steps anywhere in this plan.
- One project at a time: every component exposes `set_root(root)`; switching a workspace kills running children and rebuilds everything.
- All child processes inherit `os.environ` with `~/.local/bin` appended to `PATH` if the dir exists and is missing from PATH (GUI-launched apps often lack it; `claude` lives there).
- Dark theme only; palette and CSS tokens come from the spec's table. Do not follow the system light/dark.
- Tests are plain asserts in `tests/test_selfcheck.py` (no pytest); UI tasks get manual checklists instead. Pure-logic modules must not import `gi` at module scope (state, runctl, gitstatus) so tests run headless.
- Known ceilings get a `# ponytail:` comment naming ceiling + upgrade path: tree monitor cap (50), sync file save, no autosave, search = full walk, branch via HEAD-file read only.
- v1 does **not** reload an open editor tab when claude edits the file on disk (dialog only if the file is deleted). If the agent edits an open-but-unmodified tab, the tab keeps its stale copy on purpose.

## Review Focus

Spec-implied inputs no task's tests exercise (owner named; each is in that task's checklist):

1. **Corrupt/weird `package.json`** (invalid JSON, `scripts` a list, `null` root) → `detect` returns `None` cleanly. → Task 3 test.
2. **`claude` unreachable because of GUI-launched PATH** → `~/.local/bin` appended. → Task 7 manual check.
3. **Run clicked while a run is already executing** → ignored until Stop; no double child. → Task 8 manual check.
4. **Non-git dir / detached HEAD** → branch label hidden, no crash. → Task 4 test.
5. **File deleted on disk while open** → dialog offering close; save-into-nothing impossible because the tab closes or the user discards first. → Task 6 manual check.

---

### Task 1: Bootstrap — skeleton window, launcher, dark CSS, API probes

**Files:**
- Create: `bin/misiuscode`, `misiuscode/__init__.py`, `misiuscode/gi_env.py`, `misiuscode/main.py`, `misiuscode/window.py`
- Test: none (skeleton is manually checked)

**Interfaces:**
- Produces: `main.run(argv)` — entry, `argv[0]` = optional project path. `Window` class owning `.win` (ApplicationWindow), header widgets (`self.run_btn`, `self.stop_btn`, `self.menubtn`), layout panes (`self.vpane`, `self.hpane`), and `set_workspace(path)` with component wiring filled in by later tasks. Placeholder labels `self.editor_placeholder` / `self.panes_placeholder` / `self.tree_placeholder` exist until Tasks 5/6/7 replace them.

- [ ] **Step 1: Install missing runtime deps**

Run: `pacman -S gtksourceview5 vte4`
If pacman fails/needs root, ask the user to run that exact command.

- [ ] **Step 2: Probe gir versions (informational; gi_env probes anyway)**

```bash
python3 - <<'EOF'
import gi
for ns, vals in [("GtkSource", ("5",)), ("Vte", ("4", "4.0"))]:
    for v in vals:
        try:
            gi.require_version(ns, v)
            __import__("gi.repository", fromlist=[ns])
            print("OK", ns, v); break
        except (ValueError, ImportError) as e:
            print("MISS", ns, v, type(e).__name__)
EOF
```
Expected: `OK GtkSource 5` and `OK Vte <4 or 4.0>`. If anything is MISS after Step 1, stop and ask the user.

- [ ] **Step 3: Create `bin/misiuscode` (chmod +x)**

```python
#!/usr/bin/env python3
import os, sys
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from misiuscode.main import run
run(sys.argv[1:])
```

- [ ] **Step 4: Create `misiuscode/__init__.py`** — empty file.

- [ ] **Step 5: Create `misiuscode/gi_env.py`**

```python
"""Resolve gir version strings. Call before `from gi.repository import …` in every module."""
import gi

_CACHE: dict[str, str] = {}

def require(ns: str, versions: tuple[str, ...]) -> str:
    if ns not in _CACHE:
        for v in versions:
            try:
                gi.require_version(ns, v)
                _CACHE[ns] = v
                break
            except ValueError:
                pass
        else:
            raise ImportError(f"no gir for {ns} tried {versions}")
    return _CACHE[ns]
```

- [ ] **Step 6: Create `misiuscode/main.py`**

Create `misiuscode/main.py`:

```python
import os
import misiuscode.gi_env as ge
ge.require("Gtk", ("4.0",))
ge.require("GtkSource", ("5",))
from gi.repository import Gdk, Gtk, GtkSource
from . import window
from . import state

CSS = """
window { background: #07090d; }
.misius-surface1 { background: #0a0d13; }
.misius-surface2 { background: #0d1017; }
.misius-surface3 { background: #141720; }
.misius-header button { background: #0a0d13; color: #cbd5e1; border: none; border-radius: 6px; padding: 6px 12px; }
.misius-header menubutton > button { background: transparent; color: #cbd5e1; padding: 4px 10px; }
.misius-run { color: white; background: #22c55e; }
.misius-stop { color: white; background: #ef4444; }
.misius-run:disabled, .misius-stop:disabled { background: #1a1f29; color: #4b5563; }
.misius-status-dot { min-width: 8px; min-height: 8px; border-radius: 4px; background: #6b7282; }
.misius-status-dot.ok { background: #22c55e; }
.misius-status-dot.err { background: #ef4444; }
.misius-muted { color: #6b7282; font-size: 11px; }
.misius-mono { font-family: monospace; font-size: 11pt; }
.misius-tree { background: transparent; color: #cbd5e1; }
.misius-tree .view:selected { background: #1b3560; }
.misius-accent { color: #2f80ed; }
.misius-search { background: #0a0d13; color: #cbd5e1; }
.misius-status-row { color: #6b7282; font-size: 11px; padding: 8px 12px; background: #0a0d13; }
.misius-tab { padding: 4px 8px; color: #cbd5e1; }
.misius-tab.active { background: #111723; }
.misius-close { min-width: 18px; min-height: 18px; padding: 0; color: #6b7282; background: transparent; border: none; }
.misius-dirty { color: #ef4444; font-size: 9px; }
.misius-breadcrumb { color: #6b7282; font-size: 11px; padding: 4px 8px; }
notebook > header { background: #0a0d13; border: none; }
notebook > header > tabs > tab { background: #0a0d13; color: #cbd5e1; padding: 4px 6px; }
notebook > header > tabs > tab:checked { background: #111723; border-bottom: 2px solid #2f80ed; }
"""

def run(argv: list[str]) -> None:
    app = Gtk.Application(application_id="io.misius.misiuscode")

    def on_activate(a):
        scheme_dir = os.path.join(os.path.dirname(__file__), "data")
        GtkSource.StyleSchemeManager.get_default().append_search_path(scheme_dir)
        provider = Gtk.CssProvider()
        provider.load_from_string(CSS)
        Gtk.StyleContext.add_provider_for_display(
            Gdk.Display.get_default(), provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)
        path = argv[0] if argv and os.path.isdir(argv[0]) else state.get("last_project")
        if path and not os.path.isdir(path):
            path = None
        w = window.Window(path)
        w.register_actions(app)
        w.win.set_application(a)
        w.win.present()

    app.connect("activate", on_activate)
    app.run([])

if __name__ == "__main__":
    run([])
```

- [ ] **Step 7: Create `misiuscode/window.py`** (skeleton; Tasks 5–9 extend `set_workspace` and wire components)

```python
import os
import misiuscode.gi_env as ge
ge.require("Gtk", ("4.0",))
from gi.repository import Gio, GLib, Gtk

class Window:
    """Layout owner: HeaderBar(File | Run, Stop); hpaned → left vpaned(editor/panes) + file browser right."""

    def __init__(self, root: str | None):
        self.win = Gtk.ApplicationWindow(title="misiuscode")
        self.win.set_default_size(1586, 992)
        self.root = root
        self.tree = None
        self.editor = None
        self.panes = None

        self.menubtn = Gtk.MenuButton(label="File")
        self.run_btn = Gtk.Button(label="▶ Run"); self.run_btn.set_sensitive(False)
        self.run_btn.set_css_classes(["misius-run"])
        self.stop_btn = Gtk.Button(label="■ Stop"); self.stop_btn.set_sensitive(False)
        self.stop_btn.set_css_classes(["misius-stop"])
        header = Gtk.HeaderBar()
        header.pack_start(self.menubtn)
        run_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=2)
        run_row.append(self.run_btn); run_row.append(self.stop_btn)
        header.pack_end(run_row)
        self.win.set_titlebar(header)

        self.editor_placeholder = Gtk.Label(label="editor — Task 6")
        self.panes_placeholder = Gtk.Label(label="panes — Task 7")
        self.vpane = Gtk.Paned(orientation=Gtk.Orientation.VERTICAL, wide_handle=True)
        self.vpane.set_start_child(self.editor_placeholder)
        self.vpane.set_end_child(self.panes_placeholder)
        self.vpane.set_position(594)

        self.tree_placeholder = Gtk.Label(label="files — Task 5")
        self.hpane = Gtk.Paned(orientation=Gtk.Orientation.HORIZONTAL, wide_handle=True)
        self.hpane.set_start_child(self.vpane)
        self.hpane.set_end_child(self.tree_placeholder)
        self.hpane.set_position(1181)

        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        outer.append(self.hpane)
        self.win.set_child(outer)
        if self.root:
            self.win.title = "misiuscode — " + os.path.basename(self.root.rstrip("/"))

    def set_workspace(self, path: str) -> None:
        # Tasks 5–9 fill this in; keep a stub so the skeleton runs.
        self.root = path
        self.win.title = "misiuscode — " + os.path.basename(path.rstrip("/"))

    # --- registered in Task 9 ---
    def register_actions(self, app) -> None:
        pass
```

- [ ] **Step 8: Manual check**

Run: `python3 bin/misiuscode` → dark window opens: File button top-left, Run/Stop cluster top-right (disabled), two placeholder panes. Close via ✕ exits cleanly. Run: `python3 bin/misiuscode /tmp` → title `misiuscode — tmp`. A `Gtk-WARNING ... Locale not supported` line at startup is harmless (C locale fallback).

---

### Task 2: state.py — persistence (TDD)

**Files:**
- Create: `misiuscode/state.py`
- Create: `tests/test_selfcheck.py` (created here; later tasks append sections)

**Interfaces:**
- Produces (all pure, no gi):
  - `state.default() -> dict` — `{"last_project": None, "recents": [], "projects": {}}`
  - `state.load(path=STATE_PATH) -> dict` — missing/corrupt → default, never raises
  - `state.save(state, path=STATE_PATH) -> None` — makedirs, atomic tmp+rename
  - `state.remember(state, project_path) -> dict` — sets `last_project`, prepends to `recents`, dedupes, caps 10
  - `state.project_tabs(state, project_path) -> dict` — `{"open_tabs": list[str], "active_tab": int}`
  - `state.set_tabs(state, project_path, open_tabs, active) -> dict`

- [ ] **Step 1: Write the failing test — `tests/test_selfcheck.py`**

```python
#!/usr/bin/env python3
"""misiuscode self-check: plain asserts, no framework. Run: python3 tests/test_selfcheck.py"""
import json, os, sys, tempfile

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, REPO)

TESTS = []
def register(fn):
    TESTS.append(fn); return fn

# --- state (Task 2) ----------------------------------------------------------
@register
def test_state_roundtrip_and_recents():
    from misiuscode import state
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
    from misiuscode import state
    s = state.default()
    assert state.project_tabs(s, "/a") == {"open_tabs": [], "active_tab": 0}
    s = state.set_tabs(s, "/a", ["x/y.py", "z.py"], 1)
    assert state.project_tabs(s, "/a") == {"open_tabs": ["x/y.py", "z.py"], "active_tab": 1}

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
```

- [ ] **Step 2: Run to verify failure**

Run: `python3 tests/test_selfcheck.py`
Expected: FAIL `ModuleNotFoundError: No module named 'misiuscode.state'` (both tests).

- [ ] **Step 3: Implement `misiuscode/state.py`**

```python
"""state.json persistence: ~/.config/misiuscode/state.json"""
import json, os

STATE_PATH = os.path.expanduser("~/.config/misiuscode/state.json")

def default() -> dict:
    return {"last_project": None, "recents": [], "projects": {}}

def load(path: str = STATE_PATH) -> dict:
    try:
        with open(path, encoding="utf-8") as f:
            s = json.load(f)
        return {**default(), **s} if isinstance(s, dict) else default()
    except (OSError, ValueError):
        return default()

def save(s: dict, path: str = STATE_PATH) -> None:
    os.makedirs(os.path.dirname(path), exist_ok=True)
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(s, f, indent=2)
    os.replace(tmp, path)

def remember(s: dict, project_path: str) -> dict:
    s["last_project"] = project_path
    s["recents"] = ([project_path] + [p for p in s["recents"] if p != project_path])[:10]
    return s

def project_tabs(s: dict, project_path: str) -> dict:
    d = s["projects"].get(project_path, {})
    return {"open_tabs": list(d.get("open_tabs", [])), "active_tab": int(d.get("active_tab", 0))}

def set_tabs(s: dict, project_path: str, open_tabs: list, active: int) -> dict:
    s.setdefault("projects", {})[project_path] = {"open_tabs": list(open_tabs), "active_tab": int(active)}
    return s
```

- [ ] **Step 4: Run to verify pass** — `python3 tests/test_selfcheck.py` → PASS both.

---

### Task 3: runctl.py — project detection (TDD)

**Files:**
- Create: `misiuscode/runctl.py`
- Modify: `tests/test_selfcheck.py` (append section)

**Interfaces:**
- Consumes: nothing.
- Produces: `runctl.detect(root: str) -> dict | None` → `{"argv": list[str], "label": str}` or `None`. Used by Task 8's Run wiring.

- [ ] **Step 1: Append failing test**

```python
# --- runctl (Task 3) -----------------------------------------------------------
@register
def test_runctl_detect():
    from misiuscode import runctl
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
```

- [ ] **Step 2: Run to verify failure** — `python3 tests/test_selfcheck.py` → FAIL (no module `runctl`).

- [ ] **Step 3: Implement `misiuscode/runctl.py`**

```python
"""Detect how to run a project: npm scripts.dev → scripts.start, or dotnet run. Pure — no gi imports."""
import glob, json, os

def detect(root: str) -> dict | None:
    pj = os.path.join(root, "package.json")
    if os.path.isfile(pj):
        try:
            with open(pj, encoding="utf-8") as f:
                scripts = (json.load(f) or {}).get("scripts")
        except (OSError, ValueError, AttributeError):
            scripts = None
        if isinstance(scripts, dict):
            if "dev" in scripts:
                return {"argv": ["npm", "run", "dev"], "label": "npm run dev"}
            if "start" in scripts:
                return {"argv": ["npm", "start"], "label": "npm start"}
        return None
    if glob.glob(os.path.join(root, "*.csproj")):
        return {"argv": ["dotnet", "run"], "label": "dotnet run"}
    nested = sorted(glob.glob(os.path.join(root, "*", "*.csproj")))
    if nested:
        d = os.path.relpath(os.path.dirname(nested[0]), root)
        return {"argv": ["dotnet", "run", "--project", d], "label": f"dotnet run --project {d}"}
    return None
```

Notes: `(json.load(f) or {})` handles a JSON `null` body; list-valued `scripts` → AttributeError → None → "no scripts"; the `*/*.csproj` glob only matches files (dirs don't end in `.csproj`), and sorted order decides ties (alphabetical first per spec).

- [ ] **Step 4: Run to verify pass** — `python3 tests/test_selfcheck.py` → PASS all.

---

### Task 4: gitstatus.py — branch parsing (TDD)

**Files:**
- Create: `misiuscode/gitstatus.py`
- Modify: `tests/test_selfcheck.py` (append)

**Interfaces:**
- Produces: `gitstatus.branch_of(root: str) -> str | None` — branch name, or `None` (missing/inaccessible `.git/HEAD`, detached SHA, or `.git` being a worktree pointer file). Pure file read; no `git` subprocess; no gi imports.

- [ ] **Step 1: Append failing test**

```python
# --- gitstatus (Task 4) ---------------------------------------------------------
@register
def test_gitstatus_branch():
    from misiuscode import gitstatus
    with tempfile.TemporaryDirectory() as t:
        assert gitstatus.branch_of(t) is None
        os.mkdir(t + "/.git")
        open(t + "/.git/HEAD", "w").write("ref: refs/heads/main\n")
        assert gitstatus.branch_of(t) == "main"
        open(t + "/.git/HEAD", "w").write("0123456789abcdef0123456789abcdef01234567\n")
        assert gitstatus.branch_of(t) is None          # detached → hidden
        open(t + "/.git/HEAD", "w").write("gitdir: elsewhere\n")
        assert gitstatus.branch_of(t) is None          # worktree pointer → hidden
```

- [ ] **Step 2: Run to verify failure** — expected FAIL.

- [ ] **Step 3: Implement `misiuscode/gitstatus.py`**

```python
"""Branch name from .git/HEAD. Pure — no gi imports, no git binary."""
import os

def branch_of(root: str) -> str | None:
    try:
        with open(os.path.join(root, ".git", "HEAD"), encoding="utf-8") as f:
            content = f.read().strip()
    except OSError:
        return None
    if content.startswith("ref: refs/heads/"):
        return content[len("ref: refs/heads/"):]
    return None  # detached HEAD / worktree pointer → hide branch widget
```

- [ ] **Step 4: Run to verify pass** — all PASS.

---

### Task 5: filetree.py — right panel (scan fns TDD, widget integrated)

**Files:**
- Create: `misiuscode/filetree.py`
- Modify: `misiuscode/window.py`, `tests/test_selfcheck.py` (append)

**Interfaces:**
- Consumes: `gitstatus.branch_of` (Task 4).
- Produces (used by window.py now and Task 10):
  - Pure: `SKIP_DIRS: set[str]`; `scan_dir(path) -> list[tuple[str, bool]]` dirs-first; `scan_project(root, needle, cap=500) -> list[str]` relpaths.
  - Widget: `FileBrowser(Gtk.Box)` — callback attr `on_open(path: str)`; `set_root(root)`, `set_branch(branch=None)`, `refresh_branch()`. Owns the `.git/HEAD` FileMonitor itself (Task 5 wires it; Task 10 only adds the window-focus refresh hook).

- [ ] **Step 1: Append failing test (scan logic only)**

```python
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
    from misiuscode import filetree
    with tempfile.TemporaryDirectory() as t:
        _tree(t)
        assert [n for n, d in filetree.scan_dir(t)] == ["src", "hooks", ".gitignore", "package.json", "README.md"]
        assert filetree.scan_dir(t + "/src/components") == [("FileTree.tsx", False)]
        assert filetree.scan_project(t, "") == [".gitignore", "README.md", "package.json", "src/main.tsx", "src/components/FileTree.tsx"]
        hits = filetree.scan_project(t, "tsx")
        assert hits == ["src/main.tsx", "src/components/FileTree.tsx"]
        assert len(filetree.scan_project(t, "", cap=2)) == 2
```

(`_tree` writes no `node_modules` files; `scan_project` skips that dir entirely, which the expected output encodes.)

- [ ] **Step 2: Run to verify failure** — expected FAIL.

- [ ] **Step 3: Implement `misiuscode/filetree.py`** (full file):

```python
# Right panel: search + lazily expanded file tree + status row (branch, file count).
import os
import misiuscode.gi_env as ge
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
        self.add_css_class("misius-surface2")
        self.on_open = lambda path: None
        self.root = None
        self._monitors: list[Gio.FileMonitor] = []
        self._head_mon: Gio.FileMonitor | None = None
        self._expanded: set[str] = set()

        self.entry = Gtk.SearchEntry()
        self.entry.set_placeholder_text("Search files…")
        self.entry.add_css_class("misius-search")
        self.entry.connect("search-changed", self._on_search)
        self.append(self.entry)

        self.store = Gtk.TreeStore(str, str, bool)            # display, abs path, is_dir
        self.view = Gtk.TreeView(model=self.store, headers_visible=False)
        cell = Gtk.CellRendererText()
        self.view.append_column(Gtk.TreeViewColumn(title="", cell=cell, text=0))
        self.view.add_css_class("misius-tree")
        self.view.connect("row-expanded", self._on_row_expanded)
        self.view.connect("row-activated", self._on_activated)
        self.append(Gtk.ScrolledWindow(vexpand=True, child=self.view))

        row = Gtk.Box(spacing=12, margin_start=12, margin_end=12, margin_top=8, margin_bottom=8)
        row.add_css_class("misius-status-row")
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
```

(`Gtk.TreeIter` reuse across `_fill_children` calls is safe: we only mutate that row's children.)

- [ ] **Step 4: Run to verify pass** — `python3 tests/test_selfcheck.py` → PASS all.

- [ ] **Step 5: Wire into `window.py`** — in `__init__`, replace the `self.tree_placeholder` line and add open plumbing:

```python
        from .filetree import FileBrowser
        self.tree = FileBrowser()
        self.tree.on_open = lambda p: self.editor.open_file(p) if self.editor else None
        self.hpane.set_end_child(self.tree)
```

(editor isn't built yet → safe no-op until Task 6 lands. Delete `self.tree_placeholder`.)

Extend `set_workspace`:

```python
    def set_workspace(self, path: str) -> None:
        self.root = path
        self.win.title = "misiuscode — " + os.path.basename(path.rstrip("/"))
        self.tree.set_root(path)
```

- [ ] **Step 6: Manual check**

Fixture + run:

```bash
mkdir -p /tmp/misiusdemo/src/components /tmp/misiusdemo/hooks
touch /tmp/misiusdemo/src/main.tsx /tmp/misiusdemo/package.json /tmp/misiusdemo/README.md
```
Run: `python3 bin/misiuscode /tmp/misiusdemo`
Expected: search box top; tree shows dirs first (`src`, `hooks`), then `package.json`, `README.md`; no `node_modules`; file count `3 files`; branch label empty (no git). Search `tsx` → flat list of two; clearing restores the tree.

---

### Task 6: editor.py — tabbed GtkSourceView editor

**Files:**
- Create: `misiuscode/editor.py`, `misiuscode/data/misius-dark.xml`
- Modify: `misiuscode/window.py`
- Test: manual checklist (GTK widget; Review Focus #5)

**Interfaces:**
- Produces `Editor(Gtk.Box)` with attrs/methods used by window.py:
  - `on_state_changed = cb()` (fires on open/close/switch)
  - `set_root(root)`, `open_file(abs_path)`, `save_active()`, `get_open_state() -> {"open_tabs": [rel], "active_tab": int}`, `restore(root, saved)`.

- [ ] **Step 1: Create `misiuscode/data/misius-dark.xml`** (Tokyo-Night-ish scheme, tuned to Figma)

```xml
<?xml version="1.0" encoding="UTF-8"?>
<style-scheme id="misius-dark" _name="misius dark" version="1.0">
  <style name="text"         foreground="#c0caf5" background="#0d1017"/>
  <style name="line-numbers" foreground="#3a4150" background="#0d1017"/>
  <style name="current-line" background="#161b26"/>
  <style name="selection"    background="#1b3560"/>
  <style name="comment"      foreground="#565f89" italic="true"/>
  <style name="keyword"      foreground="#7aa2f7"/>
  <style name="string"       foreground="#9ece6a"/>
  <style name="number"       foreground="#ff9e64"/>
  <style name="boolean"      foreground="#ff9e64"/>
  <style name="null-value"   foreground="#ff9e64"/>
  <style name="escaped-char" foreground="#e0af68"/>
  <style name="markup"       foreground="#e0af68"/>
  <style name="heading"      foreground="#2ac3de"/>
  <style name="symbol"       foreground="#2ac3de"/>
  <style name="function"     foreground="#7aa2f7"/>
  <style name="def:name"     foreground="#bb9af7"/>
  <style name="error"        foreground="#ef4444"/>
</style-scheme>
```

- [ ] **Step 2: Implement `misiuscode/editor.py`** (full file):

```python
# Tabbed editor: GtkSourceView pages, custom tab headers, Ctrl+S, file-deleted dialogs.
import os
import misiuscode.gi_env as ge
ge.require("Gtk", ("4.0",))
ge.require("GtkSource", ("5",))
from gi.repository import Gio, GLib, Gtk, GtkSource

class Editor(Gtk.Box):
    def __init__(self):
        super().__init__(orientation=Gtk.Orientation.VERTICAL)
        self.root = None
        self.on_state_changed = lambda: None
        self.lm = GtkSource.LanguageManager.get_default()
        self.scheme = GtkSource.StyleSchemeManager.get_default().get_scheme("misius-dark")
        self.nb = Gtk.Notebook(vexpand=True)
        self._watchers: dict[str, Gio.FileMonitor] = {}
        self.append(self.nb)

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
            with open(path, encoding="utf-8", errors="replace") as f:
                text = f.read()
        except (OSError, ValueError) as e:
            self._error("Couldn't open file", f"{os.path.basename(path)}: {e}")
            return
        buf = GtkSource.Buffer(text=text)
        if self.scheme:
            buf.set_style_scheme(self.scheme)
        lang = self.lm.guess_language(os.path.basename(path), None)
        if lang:
            buf.set_language(lang)
        buf.connect("modified-changed", lambda b: self._dirty_dot(b))
        view = GtkSource.View(buffer=buf, show_line_numbers=True)
        view.set_wrap_mode(Gtk.WrapMode.NONE)
        view.add_css_class("misius-mono")
        page = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        page.append(Gtk.Label(label=os.path.relpath(path, self.root) if self.root else path,
                              xalign=0.0, margin_start=8, margin_top=4, margin_bottom=4,
                              css_classes=["misius-breadcrumb"]))
        page.append(Gtk.ScrolledWindow(vexpand=True, child=view))
        page.path = path                                # attrs for lookup/save
        page.buf = buf

        dirty = Gtk.Label(label="●"); dirty.add_css_class("misius-dirty")
        name = Gtk.Label(label=os.path.basename(path))
        close = Gtk.Button(child=Gtk.Image(icon_name="window-close-symbolic"))
        close.add_css_class("misius-close")
        head = Gtk.Box(spacing=4); head.add_css_class("misius-tab")
        head.append(dirty); head.append(name); head.append(close)
        index = self.nb.append_page(page, head)
        close.connect("clicked", lambda b, i=index: self.close_index(i))
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
        page = self.nb.get_nth_page(index)
        if page is None:
            return
        if page.buf.get_modified() and not force:
            d = Gtk.MessageDialog(transient_for=self.get_root(), modal=True,
                                  text=f"Discard changes to {os.path.basename(page.path)}?",
                                  buttons=Gtk.ButtonsType.CANCEL)
            d.add_button("Discard", Gtk.ResponseType.ACCEPT)
            d.connect("response", lambda dd, r: (dd.destroy(), self.close_index(index, True))
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
                self.close_index(k, force=True)

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
```

plus in `__init__`: `self.nb.connect("switch-page", self._on_switch)`.

- [ ] **Step 3: Wire into `window.py` + `main.py`.** In `Window.__init__` replace `self.editor_placeholder`:

```python
        from .editor import Editor
        self.editor = Editor()
        self.editor.on_state_changed = self.persist_tabs
        self.vpane.set_start_child(self.editor)
```

Add methods:

```python
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
```

In `set_workspace` (Task 5 version, now extended):

```python
    def set_workspace(self, path: str) -> None:
        self.root = path
        self.win.title = "misiuscode — " + os.path.basename(path.rstrip("/"))
        self.tree.set_root(path)
        from . import state
        saved_on_open = state.project_tabs(state.load(), path)
        self.editor.restore(path, saved_on_open)
```

In `main.py::run`, delete nothing else — `Editor` imports GtkSource itself, so the standalone `ge.require("GtkSource", ...)` already there covers the scheme lookup (keep as-is).

- [ ] **Step 4: Manual check** (Review Focus #5)

Run: `python3 bin/misiuscode /tmp/misiusdemo`
1. Double-click `package.json` and `src/main.tsx` → two tabs; JSON shows colored strings/keywords; TSX may render plain text (acceptable v1).
2. Type → red `●` in the tab header; Ctrl+S → dot clears; file content on disk matches.
3. Edit then `rm src/main.tsx` in a shell → "File was deleted — close its tab?" dialog; Close tab works; Keep-open path: reopen the same file later creates a fresh tab.
4. Edit then click tab `✕` → "Discard changes?" dialog; Discard closes; Cancel keeps; save-then-close closes silently.
5. Breadcrumb under each page shows `src/main.tsx`-style relative path.

---

### Task 7: panels.py — Agent / Output / Terminal VTE tabs

**Files:**
- Create: `misiuscode/panels.py`
- Modify: `misiuscode/window.py`
- Test: manual checklist (Review Focus #2)

**Interfaces:**
- Produces `Panes(Gtk.Box)`:
  - `set_root(root: str) -> None` (kill children, reset terminals, lazy respawn pending)
  - `on_status = cb(text: str, cls: str)` with `cls ∈ {"", "ok", "err"}` — wired by window.py to (unused in v1 beyond internal dot) and to Run/Stop button state by Task 8.
  - `launch_run(argv: list[str], label: str) -> bool` — False when a run child is alive.
  - `has_running_run() -> bool`; `stop_run() -> None`; `clear_active() -> None`.
- Tab order fixed: 0 = Agent Console (raw `claude`), 1 = Output (run target), 2 = Terminal (`$SHELL`).

- [ ] **Step 1: Implement `misiuscode/panels.py`** (full file):

```python
# Bottom notebook: Agent Console (raw claude TUI), Output (run cmd), Terminal ($SHELL).
# VTE owns the pty: spawn via spawn_async; children are session leaders → killpg terminates trees.
import os, signal
import misiuscode.gi_env as ge
ge.require("Gtk", ("4.0",))
ge.require("Vte", ("4", "4.0"))
from gi.repository import GLib, Gtk, Vte

def get_env() -> list[str]:
    env = dict(os.environ)
    p = os.path.expanduser("~/.local/bin")
    if os.path.isdir(p) and p not in env.get("PATH", "").split(os.pathsep):
        env["PATH"] = env.get("PATH", "") + os.pathsep + p
    return [f"{k}={v}" for k, v in env.items()]

def spawn(term: Vte.Terminal, cwd: str, argv: list[str]) -> None:
    """Spawn argv on term's pty. The pid lands on `term.pid_holder` via the spawn callback."""
    def ready(t, pid, error):
        term.pid_holder = pid
    try:
        term.spawn_async(Vte.PtyFlags.DEFAULT, cwd, argv, get_env(),
                         GLib.SpawnFlags.DEFAULT, None, -1, None, ready)
    except TypeError:  # binding-arg drift between VTE builds
        print("spawn_async signature mismatch; adapt to help(Vte.Terminal.spawn_async)")
        raise

class Panes(Gtk.Box):
    def __init__(self):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.root = None
        self.on_status = lambda text, cls: None
        self._pids: dict[str, int | None] = {"output": None}
        self._spawned: set[str] = set()

        strip = Gtk.Box(spacing=8, margin_start=12, margin_end=12,
                        margin_top=8, margin_bottom=8)
        strip.add_css_class("misius-status-row")
        self.dot = Gtk.Box(css_classes=["misius-status-dot"])
        self.status_label = Gtk.Label(label="Ready")
        self.status_label.add_css_class("misius-muted")
        self.trash = Gtk.Button(icon_name="edit-clear-all-symbolic")
        self.trash.add_css_class("misius-close")
        self.trash.connect("clicked", self._clear_active)
        strip.append(self.dot); strip.append(self.status_label)
        strip.append(Gtk.Box(hexpand=True))       # spacer
        strip.append(self.trash)
        self.append(strip)

        self.agent = Vte.Terminal(); self.out = Vte.Terminal(); self.term = Vte.Terminal()
        for t in (self.agent, self.out, self.term):
            t.set_scrollback_lines(10000)
        self.out.connect("child-exited", self._run_child_exited)
        self.nb = Gtk.Notebook(vexpand=True)
        for title, t in (("Agent Console", self.agent), ("Output", self.out), ("Terminal", self.term)):
            self.nb.append_page(Gtk.ScrolledWindow(child=t), Gtk.Label(label=title))
        self.nb.connect("switch-page", self._on_switch)
        self.append(self.nb)

    # ---- status -------------------------------------------------------------
    def _set_status(self, text: str, cls: str = "") -> None:
        self.status_label.set_text(text)
        for c in ("ok", "err"):
            self.dot.remove_css_class(c)
        if cls:
            self.dot.add_css_class(cls)
        self.on_status(text, cls)

    # ---- workspace ------------------------------------------------------------
    def set_root(self, root: str) -> None:
        out_pid = self._pids.get("output")
        if out_pid:
            self._kill_tree(out_pid)
        for pid in self._live_agent_term_pids():
            self._kill_tree(pid)
        self._spawned = set()
        self._pids = {"output": None}
        for t in (self.agent, self.out, self.term):
            t.reset(True, True)
        self.root = root
        self._set_status("Ready", "")
        GLib.idle_add(self._ensure_current)

    def _ensure_current(self) -> bool:
        """Spawn the visible tab's child once; called on set_root + every switch-page."""
        i = self.nb.get_current_page()
        if not self.root:
            return False
        if i == 0 and "agent" not in self._spawned:
            self._spawned.add("agent")
            spawn(self.agent, self.root, ["claude"])
        elif i == 2 and "term" not in self._spawned:
            self._spawned.add("term")
            spawn(self.term, self.root, [os.environ.get("SHELL", "/bin/bash")])
        return GLib.SOURCE_REMOVE

    def _on_switch(self, nb, page, index) -> None:
        self._ensure_current()

    # ---- run lifecycle (used by Task 8) -----------------------------------------
    def launch_run(self, argv: list, label: str) -> bool:
        if self.has_running_run():
            return False
        self.nb.set_current_page(1)
        self.out.reset(True, True)
        spawn(self.out, self.root, argv)
        self._set_status("Running", "ok")
        return True

    def has_running_run(self) -> bool:
        pid = self._pids["output"]
        return bool(pid and os.path.exists(f"/proc/{pid}"))

    def stop_run(self) -> None:
        pid = self._pids["output"]
        if pid:
            self._kill_tree(pid)
            self._pids["output"] = None
        self._set_status("Ready", "")

    def _run_child_exited(self, term, status):
        self._pids["output"] = None
        try:
            code = os.waitstatus_to_exitcode(status)
        except ValueError:
            code = status
        self._set_status(f"Exit {code}", "err" if code else "ok")

    # ---- misc -----------------------------------------------------------------
    def clear_active(self) -> None:   # trash button
        self._clear_active(None)

    def _clear_active(self, *_a) -> None:
        t = (self.agent, self.out, self.term)[max(self.nb.get_current_page(), 0)]
        t.reset(True, True)

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
        GLib.timeout_add_seconds(2, finish)
```

- [ ] **Step 2: Wire into `window.py`:** replace `self.panes_placeholder`:

```python
        from .panels import Panes
        self.panes = Panes()
        self.vpane.set_end_child(self.panes)
        self.vpane.set_position(594)
```

and extend `set_workspace` with `self.panes.set_root(path)`.

- [ ] **Step 3: Manual check** (Review Focus #2)

Run: `python3 bin/misiuscode /tmp/misiusdemo`
1. Bottom strip: `Agent Console | Output | Terminal` tabs; above-right the trash; dot + `Ready` above-left, right.
2. Agent Console immediately shows the raw claude TUI running in `/tmp/misiusdemo` (Ollama env inherited untouched).
3. Terminal tab: `echo $PATH | tr ':' '\n' | grep 'local/bin'` prints the `~/.local/bin` entry; `whoami`, resize, colors all normal.
4. Trash clears the visible tab's screen without killing its process.
5. Restarting on another path kills previous children (`set_root` path) — verify with `pgrep -af claude` before/after.

---

### Task 8: Run/Stop wiring

**Files:**
- Modify: `misiuscode/window.py`
- Test: manual checklist (Review Focus #3)

**Interfaces:**
- Consumes: `runctl.detect`, `Panes.launch_run/stop_run/has_running_run`.
- Produces: `_on_run(btn)`, `_on_stop(btn)`, `_on_run_status(text, cls)` — Panes owns the status dot/label and calls `on_status`; the window only manages Run/Stop button sensitivity from those callbacks.

- [ ] **Step 1: Connect buttons in `window.py`** (end of `__init__`):

```python
        self.run_btn.connect("clicked", self._on_run)
        self.stop_btn.connect("clicked", self._on_stop)
        self.panes.on_status = self._on_run_status
```

Add methods (import at top: `from . import runctl`):

```python
    def _on_run(self, btn) -> None:
        if not self.root or self.panes.has_running_run():
            return                                       # Review Focus #3: no double children
        cmd = runctl.detect(self.root)
        if cmd is None:
            self.run_btn.set_sensitive(False)
            self.run_btn.tooltip_text = "No dev/start script or .csproj found"
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
```

In `set_workspace`, after panes exist:

```python
        cmd = runctl.detect(path)
        self.run_btn.set_sensitive(cmd is not None)
        self.run_btn.tooltip_text = (cmd and cmd["label"]) or "No dev/start script or .csproj found"
        self.stop_btn.set_sensitive(False)
```

- [ ] **Step 2: Manual check**

```bash
mkdir -p /tmp/demo-npm && printf '{"name":"x","scripts":{"dev":"python3 -m http.server 8765"}}' > /tmp/demo-npm/package.json
mkdir -p /tmp/demo-nostart
touch /tmp/demo-nostart/package.json && printf '{}' > /tmp/demo-nostart/empty.json && rm /tmp/demo-nostart/empty.json && printf '{"name":"y"}' > /tmp/demo-nostart/package.json
```
Run: `python3 bin/misiuscode /tmp/demo-npm`
1. Tooltip `npm run dev`; Run → Output tab activates, server logs streamed, dot green `Running`.
2. Run again while running → ignored; `pgrep -f http.server | wc -l` stays 1.
3. Stop → process gone (`pgrep -f http.server` empty), dot red `Exit -15`, Run re-enabled.
4. Same for `/tmp/demo-nostart` → Run disabled with the no-command tooltip.
5. Ctrl+C typed inside the Output terminal also stops the server (pty semantics) and dot shows the same.

---

### Task 9: Menus + persistence wiring

**Files:**
- Modify: `misiuscode/window.py`, `misiuscode/main.py`
- Test: manual checklist

**Interfaces:**
- Consumes: `state.*` (Task 2), `runctl` not needed here.
- Produces: File popover (Open Project…, Open Recent ▸ dynamic, New Project…, Quit), Ctrl+Q; startup restores `last_project` unless `argv[0]` is a valid dir.

- [ ] **Step 1: Fill `register_actions` + menu in `window.py`** (replace the Task-1 stub and `menubtn` placeholder wiring; required import at top: `import subprocess`):

```python
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
        rec.connect("activate", lambda _a, param: self.set_workspace(param.get_string() if os.path.isdir(param.get_string()) else self._recent_missing(param.get_string())))
        app.add_action(rec)
        app.set_accels_for_action("app.quit", ["<Control>q"])
        self.menubtn.set_popover(Gtk.PopoverMenu.new_from_model(self._build_menu()))

    def _recent_missing(self, path: str) -> None:
        d = Gtk.MessageDialog(transient_for=self.win, modal=True, text=f"Project not found: {path}",
                              buttons=Gtk.ButtonsType.CLOSE)
        d.connect("response", lambda dd, r: dd.destroy())
        d.present()

    def _build_menu(self) -> GLib.Menu:
        menu = GLib.Menu()
        menu.append("Open Project…", "app.open-project")
        recents = GLib.Menu()
        from . import state
        for p in state.load()["recents"]:
            item = GLib.MenuItem.new(os.path.basename(p.rstrip("/")) + " — " + p)
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
```

- [ ] **Step 2: Startup persistence — replace the whole `Window.set_workspace`** (this is the final version; it supersedes the Task 5/6/8 partial versions):

```python
    def set_workspace(self, path: str) -> None:
        from . import state
        state.save(state.remember(state.load(), path))
        self.root = path
        self.win.title = "misiuscode — " + os.path.basename(path.rstrip("/"))
        self.tree.set_root(path)
        self.panes.set_root(path)
        cmd = runctl.detect(path)                            # `from . import runctl` at module top (Task 8)
        self.run_btn.set_sensitive(cmd is not None)
        self.run_btn.tooltip_text = (cmd and cmd["label"]) or "No dev/start script or .csproj found"
        self.stop_btn.set_sensitive(False)
        self.editor.restore(path, state.project_tabs(state.load(), path))
```

(`main.run` from Task 1 already opens `last_project` when `argv[0]` isn't a dir.)

- [ ] **Step 3: Manual check**

1. File ▸ Open Project… → pick `/tmp/misiusdemo`; title/tree/run-detect refresh; Open Recent gains entries.
2. File ▸ New Project… → creates `zzproj` under `/tmp`, `git init` runs (may fail silently if `git` is absent from the app's env — workspace still opens; error dialog only on `makedirs` failure), recents updated.
3. Recents missing path → "Project not found" dialog instead of a broken workspace.
4. Quit via menu/`Ctrl+Q` closes; relaunch without args reopens `last_project` with its tabs restored and active tab correct (`cat ~/.config/misiuscode/state.json` sanity).
5. Recents cap/dedupe: open several, reopen oldest, check order.

---

### Task 10: Final polish + walkthrough + repo handoff

**Files:**
- Modify: `misiuscode/window.py` (branch refresh on focus)
- Test: full manual walkthrough (this is the last gate)

**Interfaces:**
- Consumes: everything built so far.

- [ ] **Step 1: Branch refresh on window focus** — in `Window.__init__` after `self.tree` exists:

```python
        self.win.connect("notify::is-active",
                         lambda *_a: self.tree and self.tree.refresh_branch())
```

(`FileBrowser.refresh_branch` exists since Task 5.)

- [ ] **Step 2: Full manual walkthrough — one run, all checks**

Using `/tmp/demo-npm` (from Task 8) plus any personal project:

1. [ ] `python3 tests/test_selfcheck.py` → all PASS, exit 0.
2. [ ] `python3 bin/misiuscode /tmp/demo-npm` → dark layout matches the Figma: header (File | Run, Stop right), editor over bottom panel left, file browser right with search + status row.
3. [ ] Editor: open 2 files, edit, dirty dot, Ctrl+S, discard dialog, delete-file-on-disk dialog (Review Focus #5).
4. [ ] Agent Console = raw claude TUI in project root; a trivial edit task edits a real file; Ctrl+C inside the pane kills only the running action.
5. [ ] Terminal tab = regular shell in project root; `$PATH` contains `~/.local/bin` (Review Focus #2); `git status`, `npm` available.
6. [ ] Run/Stop cycle including double-click-Run ignored (Review Focus #3).
7. [ ] Branch label shows for a git dir, empty for non-git (Task 4's detached logic verified via `git checkout --detach HEAD` then reopen).
8. [ ] Menus: Open Project / Open Recent (cap 10, dedupe) / New Project (empty dir + git init) / Quit; restart reopens last project + tabs.
9. [ ] Window resize: panes stay usable; tree re-scrolls; no crashes in stderr.

- [ ] **Step 3: Hand the user the repo commands (they run them — git is blocked here)**

Print to the user verbatim after the walkthrough passes:

```bash
cd ~/FunProjects/misiuscode
git init
git add -A
git commit -m "misiuscode v1: GTK4 wrapper — editor, file browser, claude/terminal/Output panes, npm+dotnet run"
```

## Self-review (applied before saving)

- **Spec coverage:** palette/layout → T1 CSS (+T6 scheme); Theming → T1; state → T2; detection rules & fallback order → T3; git branch read+monitor+focus refresh → T4/T5/T10; lazy tree + skip-list + search + count → T5; editor tabs/dirty/Ctrl+S/breadcrumb/deleted-dialog → T6; three VTE tabs + PATH fix + kill semantics + project switch → T7; Run/Stop + tooltip disable → T8; menus (File only) + New Project = empty dir + git init + recents cap + last_project reopen + tab persistence → T9; final gate → T10. Out-of-scope items (Problems, chat input, Edit/Window/Help menus, git status beyond branch, symbol breadcrumbs, desktop file) appear in no task.
- **Placeholders:** none — every module ships as a full code block; the only implementer-written line is one `child-exited` connect, quoted in Task 7.
- **Type consistency:** `detect() -> {argv, label} | None` identical in T3/T8; `state` API names match across T2/T9; `Panes.spawn(term, cwd, argv)` signature used the same in T7; `Editor.restore/get_open_state` shapes match `state.project_tabs`; `on_status(text, cls)` consistent.
- **Review Focus owners:** see the list at top — each maps to a named test or manual step.