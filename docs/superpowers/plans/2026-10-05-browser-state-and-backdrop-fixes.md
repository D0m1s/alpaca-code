# Browser Tab-State Memory & Backdrop Fixes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The File Browser card remembers user state — WORKSPACE keeps opened dirs + scroll across tab switches (and across a search displacement/restoration), CHANGES renders all dirs open unless the user collapsed them — and the mode-tab/commit-bar labels stop brightening to "selected" when the window loses focus.

**Architecture:** Three independent, single-file fixes. Task 1 (filetree.py): the *only* code path that today destroys the workspace tree's expansion/scroll is the shared search entry (typing displaces the tree; a stale needle survives tab switches; a later clear rebuilds collapsed) — so save a `(root, open dirs, scroll)` bundle the moment the entry displaces the tree and replay it on clear/re-entry. Task 2 (gitview.py): replace the "remembered expansions" set (`_open`, cleared every rebuild) with a "user-collapsed" set (`_closed`) — dirs render expanded unless the user closed them; rebuilds re-apply it. Task 3 (main.py): Breeze-Dark's direct `label:backdrop { color: #fcfcfc }` beat the app's button-set (inherited) color at USER priority; restate per child `label` with `:backdrop` rules, per the file's established pattern.

**Tech Stack:** GTK4 / PyGObject on Python 3.14, PyGObject + GtkTreeView, no pip deps, CSS via a USER-priority (800) app provider in main.py.

**Spec:** User request (chat 2026-10-05): 1) WORKSPACE starts collapsed as now but remembers opened dirs + scroll across tab switches; 2) CHANGES starts all-open and remembers what you selected; 3) unfocused window shows both tabs as selected. Amends `docs/superpowers/specs/2026-10-05-git-changes-view-design.md` §2: the expansion default (collapsed) and the "selection does not survive rebuilds" ruling are superseded — checked-state survival already deviates per spec deviation 1 (`docs/superpowers/plans/2026-10-05-git-changes-view.md`), and this plan applies the same survival law to expansion. (Note: "what you selected" in CHANGES = the checked boxes — probe-verified to already survive every refresh; only the visual collapse hid them. Nothing to build there.)

## Global Constraints

- Python 3.14, GTK4 PyGObject; system deps only, no pip installs.
- Never import `gi.repository` before `alpaca_code.gi_env.require(...)` sets versions.
- Never mutate a TreeStore under an EXPANDED row (measured collapse law). Fresh/rebuilt stores must re-apply expansion via `view.expand_row(path, False)` after filling — children are pre-loaded for the changes view, lazy-loaded per level for the workspace tree's replay.
- `view.row_expanded()` readback is deprecated + stale after mutations — expansion truth lives in the code's own set (`_open` / `_closed`); verify via that set + chevron col identity (`badges.chevron_pixbuf(True/False)` are cached singletons).
- Cell props cache at insert — keep inserting rows with the shut chevron; only the expand-row signal/handler owns col 4.
- `GLib.timeout_add_seconds` never fires on this box — use `GLib.timeout_add(ms)` (scroll restore uses `GLib.idle_add`).
- `Gtk.init()` on the live display returns None — never truthy-test it in probes; pump with `GLib.MainContext.default().iteration(may_block=False)` in a bounded loop.
- state.json schema untouched (all three fixes are view-local, in-memory state; `~/.config` not written).
- CSS app rules are USER priority (800); a stateless/direct rule can still lose to a direct rule on a CHILD node in `:backdrop` — restate child-node colors as `label:backdrop` rules. Never give CSS `background` to titlebuttons.
- CSS/PYTHON changes need a REAL restart of the app: second `bin/alpaca-code` spawn single-instance-forwards to the running primary. Kill by exact pid (never `pkill -f` — it kills the shell itself), then relaunch.
- Tests: displayless suite `python3 tests/test_selfcheck.py` (29 tests; never instantiates widgets). Widget behavior is verified with throwaway `/tmp` probes per CLAUDE.md; probes never run with application_id `io.alpaca.code` (single-instance).

## Review Focus

- **Double-search overwrite:** a second needle typed later must NOT overwrite the saved bundle with the flat list's junk state — expect the first real-tree bundle to survive and be restored on the eventual clear (Task 1 probe §C).
- **Bare round-trip stays lossless:** expanding dirs then WORKSPACE→CHANGES→WORKSPACE with an unchanged entry text must keep `_open` + scroll EXACTLY (already true today; the changes must not add a rebuild path there) (Task 1 probe §D).
- **Filter + collapsed-dir interplay:** in CHANGES, a user-collapsed dir must stay collapsed through refresh/filters; a dir that never existed before (new dirty file in a clean location) must render expanded (Task 2 probes §C and §E).
- **Backdrop = focus exonerated:** unfocused-window pixel equality between a `:focus` state and none was measured; the fix keys on `label:backdrop` only. Hover inside an unfocused window stays muted by design (noted, not styled) (Task 3 probe).
- **set_root resets everything:** opening/switching workspace = fresh collapsed tree, all-open CHANGES, `_closed` emptied, selection reset to all — and a pending restore idle must not resurrect the OLD root's state (root guard in `_restore_saved_tree`) (probes §F in both).

---

### Task 1: WORKSPACE — search-displaced tree keeps its state (filetree.py)

**Files:**
- Modify: `alpaca_code/filetree.py` (`__init__` ~line 72, `_set_mode` ~lines 195–210, `_on_search` lines 264–279, `set_root` lines 178–180, new helpers after `_populate_root`)
- Probe: `/tmp/probe_ws_tree.py` (throwaway, not committed)

**Interfaces:**
- Consumes: existing `FileBrowser._open` (signal-fed expansion truth), `_load_children(it, d)` (lazy child load; prunes `_open` under `d`), `self._mode`/`self.entry`/`self.store`/`self.root`.
- Produces: `_saved_tree: tuple[str, set[str], float] | None` (bundle: root, open abs paths, scroll offset — consumed internally by `_restore_saved_tree`/`_reexpand`; nothing outside filetree reads it), `_restore_saved_tree(saved) → False` (idle stage 1), `_restore_scroll(value) → False` (idle stage 2), `_reexpand(d) → None` (parent-first replay of one remembered dir; early-return on vanished paths), `_row_child(parent_iter, name) → iter | None`. No other module changes.

**Measured facts this task builds on** (probe-verified 2026-10-05, real-app copy `io.alpaca.probe3` + widget probes):
- A bare `_set_mode("changes")` → `_set_mode("tree")` round trip loses NOTHING today (`_open` identical, vadjustment identical, zero store mutations — 6 independent ways). Do NOT add save/restore around `_set_mode` itself.
- The only code-reachable destroyer: `_on_search` — nonempty text → `store.clear(); _open=set(); _monitored=set()` (upper collapses to the viewport, scroll clamps away — the position is unrecoverable after the first keystroke); empty → `_populate_root()` = fresh collapsed rebuild. The entry text survives tab switches (`_set_mode` never touches it), so the user's repro (filter → CHANGES → back → clear) collapses everything and reads as "the tab switch forgot my state".
- Related wart (today): clearing the needle while in CHANGES mode returns early (changes filter only), so switching back to WORKSPACE with an empty box shows the STALE FLAT results forever (no signal fires on entry at switch time).
- **Scope ruling (user, 2026-10-05): WORKSPACE only** — the save/restore bundle covers the file tree alone. `_on_search`'s changes-mode branch and the entry-as-changes-filter behavior in `_set_mode` stay exactly as they are; CHANGES state memory is Task 2's, not this.

- [ ] **Step 1: Write the probe asserting the new behavior; verify it FAILS**

Create `/tmp/probe_ws_tree.py`:

```python
import os, subprocess, sys, tempfile
sys.path.insert(0, "/home/dominykasm/FunProjects/misiuscode")
import alpaca_code.gi_env as ge
ge.require("Gtk", ("4.0",)); ge.require("Gdk", ("4.0",)); ge.require("GdkPixbuf", ("2.0",))
from gi.repository import GLib, Gtk
from alpaca_code.filetree import FileBrowser

Gtk.init()
root = tempfile.mkdtemp()
for d in ("d1", "d1/sub", "d2"):
    os.makedirs(os.path.join(root, d), exist_ok=True)
for i in range(40):                                   # enough rows to scroll
    open(os.path.join(root, f"file{i:02}.py"), "w").write("x\n")
open(os.path.join(root, "d1", "a.js"), "w").write("x\n")
open(os.path.join(root, "d1", "sub", "b.json"), "w").write("x\n")
for _ in range(10):
    open(os.path.join(root, "d2", f"n{os.urandom(2).hex()}.md"), "w").write("x\n")

win = Gtk.ApplicationWindow(); win.set_default_size(400, 700)
b = FileBrowser(); b.set_root(root); b.on_open = lambda p: None
win.set_child(b); win.present()
ctx = GLib.MainContext.default()
def pump(n=40, settle=0.0):
    # search-changed is debounced 150ms on this build (search-delay property) —
    # set_text needs wall time to settle before its event fires
    for _ in range(n):
        ctx.iteration(may_block=False)
    if settle:
        import time; time.sleep(settle)
        for _ in range(n * 10):
            ctx.iteration(may_block=False)

def toplevel_kinds():
    """bool is_dir per toplevel row (a flattened search store has only False)."""
    out = []
    i = b.store.get_iter_first()
    while i is not None:
        out.append(b.store[i][2])
        i = b.store.iter_next(i)
    return out

# §A expand d1, d1/sub AND d2 through the REAL activation branch — the nested
# d1/sub is what exercises _reexpand's multi-segment descent + lazy load later
def find_path(rel):
    """TreePath for a '/'-joined rel path of row names, or None."""
    it = None
    for seg in rel.split("/"):
        i = b.store.iter_children(it) if it else b.store.get_iter_first()
        found = None
        while i is not None:
            if b.store[i][0] == seg:
                found = i
                break
            i = b.store.iter_next(i)
        if found is None:
            return None
        it = found
    return b.store.get_path(it)
for rel in ("d1", "d1/sub", "d2"):
    p = find_path(rel)
    assert p is not None, rel
    b._on_activated(b.view, p, b.view.get_column(0))
pump()
assert len(b._open) == 3, b._open                       # d1, d1/sub, d2
adj = b.view.get_vadjustment(); adj.set_value(min(120.0, adj.get_upper() - adj.get_page_size()))
saved_expect = (root, set(b._open), adj.get_value())

# §B displacement: typing replaces the tree and captures the bundle ONCE
b.entry.set_text(".py")
pump()
assert b._saved_tree == saved_expect, b._saved_tree     # captured before clear, not junk
assert not any(toplevel_kinds())                        # flat results — no dir rows

# §C double-needle must not overwrite with flat-list state (Review Focus 1)
b.entry.set_text("file")
pump()
assert b._saved_tree == saved_expect
b.entry.set_text("")                                    # clear → rebuild + restore
pump()
assert b._open == saved_expect[1], f"{b._open} != {saved_expect[1]}"
assert abs(b.view.get_vadjustment().get_value() - saved_expect[2]) < 2.0, b.view.get_vadjustment().get_value()
assert b._saved_tree is None                            # bundle consumed

# §D bare round trip stays lossless (Review Focus 2) — no search involved
assert b._saved_tree is None and not b.entry.get_text().strip()
pre = (set(b._open), b.view.get_vadjustment().get_value())
b._set_mode("changes"); pump()
b._set_mode("tree"); pump()
assert pre == (set(b._open), b.view.get_vadjustment().get_value()), (pre[0], set(b._open), pre[1], b.view.get_vadjustment().get_value())

# §E stale needle re-filters the tree on re-entry (spec §1), keeps the earliest bundle
b._saved_tree = None
b.entry.set_text("json"); pump(settle=0.25); b._set_mode("changes"); pump()
b._set_mode("tree")     # needle still in box
pump()
assert b._saved_tree is not None                        # first displacement captured, not the second
assert not any(toplevel_kinds())                        # needle re-applied → flat results again
b.entry.set_text(""); pump(settle=0.25)
restored = {os.path.join(root, d) for d in ("d1", "d1/sub", "d2")}
assert b._open == restored, b._open ^ restored           # cleared → tree restored

# §F set_root is the deliberate full reset; a pending idle cannot resurrect it (root guard)
b.entry.set_text(".py"); pump()
b.entry.set_text(""); pump()
new_root = tempfile.mkdtemp()
b.set_root(new_root); pump()
assert b._open == set() and b._saved_tree is None
assert b.view.get_vadjustment().get_value() == 0.0
print("ALL WORKSPACE-STATE ASSERTS PASS")
```

Note: §D's tree geometry differs between bare-`FileBrowser` probe and the real app only in scroll magnitudes; invariants are set-identical / value-near. Run:

Run: `python3 /tmp/probe_ws_tree.py`
Expected: **FAIL** on the first `b._saved_tree` assert (`FileBrowser` has no attribute `_saved_tree` — and §C/§E collapse). Baseline check first: `python3 tests/test_selfcheck.py` → 29 PASS.

- [ ] **Step 2: Verify the probe fails for the right reason**

Run: `python3 /tmp/probe_ws_tree.py`
Expected: `AttributeError: 'FileBrowser' object has no attribute '_saved_tree'` (nothing implements the new state yet).

- [ ] **Step 3: Implement (filetree.py)**

1. `__init__` — next to `self._open` (line 72), add:

```python
        self._saved_tree: tuple[str, set[str], float] | None = None   # (root, open abs paths, scroll) — the search-displaced sheet lives here while search results show
```

2. `set_root` — after `self.root = root` (line 178), BEFORE `self.entry.set_text("")` (that `set_text` fires a synchronous `search-changed` when text was nonempty; the bundle must be dead before it):

```python
        self._saved_tree = None   # a fresh workspace starts collapsed (user ruling; the pending restore, if any, cannot cross roots — _restore_saved_tree re-guards anyway)
```

3. `_on_search` — capture the bundle BEFORE the destructive clear; restore on clear:

```python
    def _on_search(self, entry) -> None:
        text = entry.get_text().strip()
        if self._mode == "changes":   # changes search never touches the file tree
            self.changes.filter(text)
            return
        if not text:
            self._populate_root()
            saved, self._saved_tree = self._saved_tree, None
            if saved and saved[0] == self.root and saved[1]:
                GLib.idle_add(self._restore_saved_tree, saved)
            return
        # first displacement of a real tree saves it; later keystrokes must NOT
        # overwrite the bundle with the flat list's collapsed/clamped state
        if self._saved_tree is None:
            self._saved_tree = (self.root, set(self._open),
                                self.view.get_vadjustment().get_value())
        self.store.clear()
        self._open = set()        # search view has no tree rows
        self._monitored = set()
        for rel in scan_project(self.root, text):
            p = os.path.join(self.root, rel)
            pix = badges.pixbuf_for(rel, False)
            self.store.append(None, [rel, p, False, icon_of(rel, False), badges.blank_pixbuf(),
                                     pix, pix is not None, pix is None])
```

4. New helpers after `_populate_root` (before `_count`):

```python
    def _row_child(self, parent_iter, name: str):
        """First row named `name` under parent (None = toplevel), or None."""
        i = self.store.iter_children(parent_iter) if parent_iter else self.store.get_iter_first()
        while i is not None:
            if self.store[i][0] == name:
                return i
            i = self.store.iter_next(i)
        return None

    def _reexpand(self, d: str) -> None:
        """Replay one remembered dir: descend parent-first over its segments,
        lazy-loading each level's children BEFORE expanding it (the store is
        lazy — a collapsed row has no child rows — and mutating UNDER an
        expanded row would collapse it; `_load_children` already prunes stale
        `_open` entries under a freshly loaded level). Signals are synchronous,
        so each expand_row re-fills _open/chevrons/monitors inline. Voids on
        vanished paths (moved/deleted since the save)."""
        it = None
        for seg in os.path.relpath(d, self.root).split(os.sep):
            it = self._row_child(it, seg)
            if it is None:
                return                    # tree moved on (rebuild/vanished path)
            dd = self.store[it][1]
            if not self.store.iter_children(it):         # lazy level: load, then expand
                self._load_children(it, dd)
            self.view.expand_row(self.store.get_path(it), False)
```

```python
    def _restore_saved_tree(self, saved: tuple) -> bool:
        """Stage 1 (idle): replay the saved open-dirs parents-first (shallow to
        deep — parents must expand before their descendants are replayed into
        them), then hand the scroll to stage 2 after allocations settle. A
        workspace opened meanwhile cancels the replay silently."""
        root, open_paths, _scroll = saved
        if self.root != root:
            return False
        for d in sorted(open_paths, key=lambda p: p.count(os.sep)):
            self._reexpand(d)
        GLib.idle_add(self._restore_scroll, _scroll)
        return False                    # idle_add: run once

    def _restore_scroll(self, value: float) -> bool:
        adj = self.view.get_vadjustment()
        adj.set_value(min(value, max(adj.get_upper() - adj.get_page_size(), 0.0)))
        return False                    # idle_add: run once
```

5. `_set_mode` — after the placeholder line (line 206), before the existing `if mode == "changes":` block, add the tree-mode half (spec §1: the search bar filters the ACTIVE view; the entry text otherwise survives the switch, and an empty box must not leave the stale flat list stuck):

```python
        if mode == "tree":
            # the shared entry text survives the tab switch (and its view may be
            # displaced by the last search): a needle re-filters the tree, an
            # empty box restores the displaced sheet instead of leaving the
            # last flat results stuck with no way back
            if self.entry.get_text().strip():
                self._on_search(self.entry)
            elif self._saved_tree:
                saved, self._saved_tree = self._saved_tree, None
                self._populate_root()
                GLib.idle_add(self._restore_saved_tree, saved)
        if mode == "changes":
            ...
```

(Keep the existing `changes` block exactly as-is; its `refresh()`→`_fill` double-pass with `filter(entry)` is measured harmless.)

- [ ] **Step 4: Run the probe; verify PASS**

Run: `python3 /tmp/probe_ws_tree.py`
Expected: `ALL WORKSPACE-STATE ASSERTS PASS`. If §C's restore value disagrees by more than the guard allows, check that `_restore_scroll` runs AFTER the expansion pass (chained idle, not synchronous).

Then: Run: `python3 tests/test_selfcheck.py`
Expected: 29 PASS (displayless suite untouched).

- [ ] **Step 5: Commit**

```bash
git add alpaca_code/filetree.py
git commit -m "$(printf 'filetree: workspace tree survives search displacement\n\nBundle (root, open dirs, scroll) at the first keystroke, replayed on\nclear/re-entry; stale-needle re-filter + flat-stick clear-in-changes fix.\nProbes: /tmp/probe_ws_tree.py (6 sections).\n\nCo-Authored-By: Claude Code <noreply@anthropic.com>')"
```

---

### Task 2: CHANGES — dirs open unless the user collapsed them (gitview.py)

**Files:**
- Modify: `alpaca_code/gitview.py` (state decl line 36, `set_root` line 110, `_fill` line 146 + insertion block end, `_on_expand_toggle` lines 236–240, `_on_activated` dir branch lines 247–251)
- Probe: `/tmp/probe_changes2.py` (throwaway; planning prototype `/tmp/probe_changes_expand.py` = sections A/B of today's matrix, all measured working 2026-10-05)

**Interfaces:**
- Consumes: `gitstatus.group_tree(rows)` → `(kind, name, rel, letter, depth)` parents-before-children, dirs-before-files (insertion order drives the expansion loop); `gitstatus.changes(root)`; the store's columns 4 (chevron) / 7–8 (toggle); `_sync()` (disjoint cols from the chevron write — measured order-safe either way, run sync first).
- Produces: `self._closed: set[str]` — user-collapsed dir RELS (consumed by `_fill`'s expansion loop, `_on_expand_toggle`, `_on_activated`). Replaces `self._open`. Nothing outside gitview reads it. Checked-state contract unchanged (`_checked` already survives refresh/filters/tab switches; `set_root` = fresh all-selected).

**Measured facts this task builds on** (probe-verified on the real `ChangesView`, temp git repo `nt/`, `src/`, `src/deep/`, `sub/`, 5 changed files):
- `_fill` clears `_open` (gitview.py:146) → EVERY `_fill` caller collapses dirs: mode switch (filetree `_set_mode`), `refresh_git`/is-active hook (fires on focus LOSS too), post-commit/post-push refresh, and `filter()` (each keystroke). The checked set survives everywhere — user's "remember what you selected" dissolves once dirs stop collapsing under it.
- `store.clear()` emits NO `row-collapsed` — a saved collapse set survives rebuilds untouched.
- **`expand_row` on a row whose ANCESTOR is collapsed is a silent no-op on this build** (no signal, chevron stays shut) — hidden children need a catch-up when their parent is reopened.
- `row-expanded` fires synchronously from `expand_row` (no pump needed); the signal handler path repainting col 4 is the probe-verified sequence.

- [ ] **Step 1: Write the probe asserting the new rule; verify it FAILS**

Create `/tmp/probe_changes2.py`:

```python
import os, subprocess, sys, tempfile
sys.path.insert(0, "/home/dominykasm/FunProjects/misiuscode")
import alpaca_code.gi_env as ge
ge.require("Gtk", ("4.0",)); ge.require("Gdk", ("4.0",)); ge.require("GdkPixbuf", ("2.0",))
from gi.repository import GLib, Gtk
from alpaca_code.gitview import ChangesView
from alpaca_code import gitstatus

Gtk.init()
root = tempfile.mkdtemp()
subprocess.run(["git", "init", "-q", root])
for d in ("src", "src/deep", "nt", "sub"):
    os.makedirs(os.path.join(root, d), exist_ok=True)
for f in ("src/a.py", "src/deep/d.py", "nt/q.txt", "sub/y.py", "top.txt"):
    open(os.path.join(root, f), "w").write("1\n")
subprocess.run(["git", "-C", root, "add", "-A"], check=True)
subprocess.run(["git", "-C", root, "commit", "-qm", "init"], check=True)
for f in ("src/a.py", "src/deep/d.py", "nt/q.txt", "sub/y.py", "top.txt"):
    open(os.path.join(root, f), "w").write("2\n")

win = Gtk.ApplicationWindow(); win.set_default_size(420, 700)
v = ChangesView(); v.on_open = lambda *a: None
win.set_child(v); v.set_root(root); win.present()
ctx = GLib.MainContext.default()
def pump(n=40):
    for _ in range(n):
        ctx.iteration(may_block=False)
pump()

def rows():
    """(rel, TreePath, kind) in store insertion order; always refetch —
    TreePaths go stale across refills."""
    out = []
    def walk(it):
        i = v.store.iter_children(it) if it else v.store.get_iter_first()
        while i is not None:
            out.append((v.store[i][1], v.store.get_path(i), v.store[i][2]))
            walk(i)
            i = v.store.iter_next(i)
    walk(None)
    return out

# §A fresh sheet = every dir open, nothing closed
assert v._closed == set()
dirs = {rel for rel, _p, kind in rows() if kind == "d"}
assert dirs == {"src", "src/deep", "nt", "sub"}, dirs
for rel, tpath, kind in rows():
    if kind == "d":
        assert v.view.row_expanded(tpath), rel           # all 4 dirs open out of the box

# §B user collapse of "src" only: dir closes, files stay checked, others stay open
src_t = next(tp for rel, tp, k in rows() if rel == "src")
v._on_activated(v.view, src_t, v.view.get_column(0))
pump()
assert v._closed == {"src"}, v._closed
assert not v.view.row_expanded(src_t)
nt_t = next(tp for rel, tp, k in rows() if rel == "nt")
assert v.view.row_expanded(nt_t)                        # an unrelated dir stays open
assert v._checked == {"src/a.py", "src/deep/d.py", "nt/q.txt", "sub/y.py", "top.txt"}

# §C refresh/filters re-apply the collapse rule (Review Focus 3)
v.refresh(); pump()
assert v._closed == {"src"}, v._closed                  # store.clear() emits no row-collapsed
src_t = next(tp for rel, tp, k in rows() if rel == "src")
assert not v.view.row_expanded(src_t)                   # collapsed again after refresh
v.filter("deep"); pump()
assert next((tp for rel, tp, k in rows() if rel == "src/deep"), None) is not None
assert v._closed == {"src"}                             # survived the filtered re-fill
v.filter(""); pump()
src_t = next(tp for rel, tp, k in rows() if rel == "src")
assert v._closed == {"src"} and not v.view.row_expanded(src_t)

# §D reopening a dir cascades to its not-closed children (hidden expand_row is a silent no-op)
v._on_activated(v.view, src_t, v.view.get_column(0)); pump()
assert v._closed == set(), v._closed                    # src reopened; deep never was closed
src_t = next(tp for rel, tp, k in rows() if rel == "src")
deep_t = next(tp for rel, tp, k in rows() if rel == "src/deep")
assert v.view.row_expanded(src_t) and v.view.row_expanded(deep_t)   # catch-up ran

# §E new dir appearing in a refresh renders OPEN (rule: open unless user-closed)
os.makedirs(os.path.join(root, "newdir"), exist_ok=True)
open(os.path.join(root, "newdir", "n.txt"), "w").write("1\n")       # untracked (created after commit)
v.refresh(); pump()
nd = next((tp for rel, tp, k in rows() if rel == "newdir"), None)
assert nd is not None and v.view.row_expanded(nd)

# §F set_root = fresh sheet (expansions AND _closed AND selection)
v.set_root(root); pump()
src_t = next(tp for rel, tp, k in rows() if rel == "src")
assert v._closed == set() and v.view.row_expanded(src_t)
assert v._checked == {rel for rel, _l in gitstatus.changes(root)}, "set_root resets selection to ALL"
print("ALL CHANGES-VIEW ASSERTS PASS")
```

Run: `python3 /tmp/probe_changes2.py`
Expected: **FAIL** on §A's first assertion (fresh sheet renders all dirs collapsed today — `row_expanded` False).

- [ ] **Step 2: Verify the probe fails for the right reason**

Run: `python3 /tmp/probe_changes2.py`
Expected: `AssertionError` in §A ("src" not expanded). Baseline first: `python3 tests/test_selfcheck.py` → 29 PASS.

- [ ] **Step 3: Implement (gitview.py)**

1. State decl (line 36 area) — rename with inverted polarity:

```python
        self._closed: set[str] = set()           # user-collapsed dir rels (dirs render open unless closed)
```

2. `set_root` (line 110):

```python
        self._closed.clear()
```

3. `_fill` — DELETE the `self._open.clear()` line (146) and its comment; the rebuild is still full (spec §2 mandate: store rebuilt per refresh — surgical patching stays off the table), expansion is re-applied at the view level, which mutates nothing. After the insert loop's `self._sync()` (keep sync first — col writes {7,8} vs the chevron's {4} are disjoint; measured safe either way), append the expansion pass — `iters` is insertion-ordered (parents before children by `group_tree`):

```python
        self._sync()
        for rel, it in iters.items():            # dirs render open unless the user closed them
            if rel not in self._closed:
                self.view.expand_row(self.store.get_path(it), False)
```

(The `store.get_path(it)` DeprecationWarning prints once per process on this build — worked, measured; leave it. If warn-free output is wanted later, track child counters instead — polish, not now.)

4. `_on_expand_toggle` — invert polarity + add the reveal catch-up:

```python
    def _on_expand_toggle(self, view, it, tpath, expanded: bool) -> None:
        rel = self.store[it][1]
        if rel:
            (self._closed.discard if expanded else self._closed.add)(rel)
            self.store[it][4] = badges.chevron_pixbuf(expanded)
            if expanded:                         # hidden dirs don't expand (row-expanded is
                cin = self.store.iter_children(it)   # no-op under a collapsed ancestor) — catch them up
                while cin is not None:
                    if self.store[cin][2] == "d" and self.store[cin][1] not in self._closed:
                        self.view.expand_row(self.store.get_path(cin), False)
                    cin = self.store.iter_next(cin)
```

(The child `expand_row` re-enters this handler for grandchildren — terminates at the tree's depth.)

5. `_on_activated` dir branch — flip the membership test:

```python
        if row[2] == "d" and row[1]:
            if row[1] in self._closed:
                view.expand_row(tpath, False)        # children are pre-loaded (non-lazy store)
            else:
                view.collapse_row(tpath)
```

`_set_mode`'s double fill (`refresh()` then `filter(entry)` inside filetree) re-runs the expansion loop twice — the second pass skips `expand_row` on already-open dirs (signal handler state is idempotent); measured harmless. filetree.py needs nothing here.

- [ ] **Step 4: Run the probe; verify PASS**

Run: `python3 /tmp/probe_changes2.py`
Expected: `ALL CHANGES-VIEW ASSERTS PASS`. Then: Run: `python3 tests/test_selfcheck.py` → 29 PASS.

- [ ] **Step 5: Commit**

```bash
git add alpaca_code/gitview.py
git commit -m "$(printf 'gitview: dirs open unless user-collapsed (_closed replaces _open)\n\nRefresh/filter/mode-switch/focus rebuilds re-apply the collapse set;\nreopening a dir cascades to its not-closed hidden children (expand_row\nis silently no-op under a collapsed ancestor on this build).\nChecked-set survival unchanged; set_root stays the full reset.\n\nCo-Authored-By: Claude Code <noreply@anthropic.com>')"
```

---

### Task 3: Backdrop label colors — tabs and commit-bar buttons (main.py)

**Files:**
- Modify: `alpaca_code/main.py` (CSS block, after the `.alpaca-tabbtn` rules ~line 120 and the `.alpaca-barbtn:disabled` rule ~line 145)
- Probe: `/tmp/probeC/` driver + captures (2026-10-05 artifacts, rerunnable; recreate per gotchas below if /tmp was wiped)

**Interfaces:**
- Consumes: the app CSS provider at USER priority (`_theme_setup`); widget classes `.alpaca-tabbtn` (+ `.alpaca-on`) on the two mode buttons, `.alpaca-barbtn` on Commit/Push.
- Produces: no new state; three `label:backdrop` rules in `CSS`.

**Measured facts this task builds on** (real-widget probe `io.alpaca.probeC`, KWin geometry, double-captures):
- Unfocused window: BOTH tab labels paint `#fcfcfc` (252,252,252) — brighter than the selected `#e6e8ee` (230,232,238). Root cause: the app sets `color` on the BUTTON; the `label` child INHERITS it; Breeze-Dark matches a direct `label:backdrop { color: @theme_unfocused_text_color_breeze(#fcfcfc) }` and a direct match beats an inherited value at any provider priority. `:focus` exonerated (pixel-identical focus-on vs focus-off in both window states).
- Validated fix restored the exact focused values in all 4 unfocused states × both modes; full-window focused-vs-unfocused pixel diff after the fix = **zero** differing pixels (except the physical donor-window overlap band).
- Also leaks: `.alpaca-barbtn` (Commit/Push → `#fcfcfc` in backdrop). Measured clean (leave alone): `.alpaca-panel-title`, statusbar labels, `.alpaca-tree` cell text, "Select all" cell, `.alpaca-search`/`.alpaca-msg` entries, checkbox column.

- [ ] **Step 1: (Baseline probe — optional when /tmp/probeC is intact)**

Run the existing driver in `/tmp/probeC/` (artifacts from the 2026-10-05 diagnosis) to confirm today's leak: idle tab label + bar buttons at `#fcfcfc` while unfocused. If /tmp was wiped: recreate the probe per its gotchas — own GtkApplication id `io.alpaca.probeC` (NEVER `io.alpaca.code` — single-instance forwards), window with `FileBrowser` bound to a git repo, the REAL CSS loaded exactly like `_theme_setup` (provider, USER priority), `spectacle -a` DOUBLE captures a second apart (stale committed frames on this WM), pixel-scan the label regions (PIL) for `WORKSPACE`/`CHANGES`/Commit/Push inks, geometry from the KWin scripting rig — see memory `kwin-geometry-probe-rig` + `gapplication-single-instance-forward`. Wake/hold DPMS (`kscreen-doctor --dpms on` + `xdg-screensaver reset` loop) for the run.

- [ ] **Step 2: Add the restatements to `CSS`**

After the `.alpaca-tabbtn.alpaca-on` rule (line ~120), add with a comment:

```css
/* backdrop: the tab color is inherited from the button — Breeze's direct
   label:backdrop rule (#fcfcfc) beat it at any priority, brightening BOTH
   tabs in an unfocused window (read as "both selected"). Restate per child
   label node (direct-vs-direct now); :focus measured exonerated. */
.alpaca-tabbtn label:backdrop { color: #8a93a6; }
.alpaca-tabbtn.alpaca-on label:backdrop { color: #e6e8ee; }
```

and after the `.alpaca-barbtn:disabled` rule (line ~145), same mechanism (Commit/Push leaked to #fcfcfc too):

```css
.alpaca-barbtn label:backdrop { color: #e6e8ee; }
```

- [ ] **Step 3: Re-run the probe; verify PASS**

Run: the `/tmp/probeC` driver (or recreated equivalent) with the repo's updated CSS loaded (it `import`s `alpaca_code.main`'s `CSS` string — pick up the edit automatically).
Expected: all 4 unfocused states × both modes → tab labels and bar buttons at their FOCUSED values; whole-window diff vs focused ≈ 0 (donor-overlap band aside). Focused states unchanged.
Then: Run: `python3 tests/test_selfcheck.py` → 29 PASS.

- [ ] **Step 4: Restart the app the right way and eyeball the real window**

```bash
# kill by EXACT pid (pkill -f would kill this shell — memory: pkill-self-match)
ps -eo pid,cmd | awk '/alpaca.code|bin\/alpaca-code/ && !/awk/ {print $1, $2}'   # find the pid
kill <exact-pid>; sleep 1
bin/alpaca-code &    # fresh process picks up the CSS; a plain re-spawn would forward to the (dead) primary otherwise
```

Check: switch WORKSPACE↔CHANGES, unfocus the window — the active tab stays `#e6e8ee`, the idle one returns to `#8a93a6`.

- [ ] **Step 5: Commit**

```bash
git add alpaca_code/main.py
git commit -m "$(printf 'css: restate tab/commitbar label colors on :backdrop\n\nBreeze Dark label:backdrop (#fcfcfc) out-matched the button-inherited\ncolor, painting BOTH mode tabs bright in an unfocused window.\nDirect label:backdrop rules hold (probe C: zero-diff vs focused).\n\nCo-Authored-By: Claude Code <noreply@anthropic.com>')"
```

---

## Skipped (decided, one line each)

- **`is-active` on focus loss: gate the hook** (`window.py:141-142` fires `refresh_git()` on losing focus too — with Task 2's fix that becomes a harmless redundant re-scan; one-line gate if a later scan cost shows up).
- **TreeView activation-highlight memory in CHANGES** (checked boxes already survive probe-verified; the open diff tab is the de-facto "what I was looking at" — add row-highlight tracking only if the user asks).
- **Hover-in-backdrop brightening for the tabs** (optional rule, only if the user wants it).
- **Warn-free `get_path` alternatives** (DeprecationWarning prints once per process; harmless).