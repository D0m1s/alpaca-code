# Git Changes View Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** WORKSPACE/CHANGES toggle in the File Browser card; CHANGES lists git-dirty paths as a checkable tree, opens side-by-side syntax-highlighted diffs in editor tabs, and commits/pushes exactly the checked files.

**Architecture:** All git subprocess logic stays in the pure module `gitstatus.py` (displayless-testable). A new `gitview.py` owns the changes tree + commit bar; `filetree.py` hosts the mode tabs + a `Gtk.Stack`; `editor.py` gains read-only diff pages driven by ready-built line lists; the hover-band tree view moves to `treehover.py` (shared by both card lists); `window.py` glues them (existing tree.on_open pattern).

**Tech Stack:** Python 3.14, GTK4/PyGObject, GtkSourceView 5 (scheme + language manager), stdlib subprocess/threading/difflib-free uni parser, plain-assert selfcheck.

**Spec:** `docs/superpowers/specs/2026-10-05-git-changes-view-design.md` — the plan argues from it; read both.

## Global Constraints (from CLAUDE.md / spec, verbatim where measured)

- Never import `gi.repository` before `alpaca_code.gi_env.require(ns, versions)` in any new module.
- PyGObject: property-by-attribute assignment silently no-ops — use setters (`set_text`, `set_sensitive`…).
- `GtkSource.Buffer(text=…) is born modified=True` — every buffer creation calls `set_modified(False)`.
- `Gtk.Notebook.append_page` does NOT switch pages — `set_current_page()` after.
- PyGObject wrapper identity unstable — compare indices, never `is`.
- Tree cells cache props at insert: set renderer properties before any store fill; text cell packed with `expand=True` pins earlier cells left.
- CSS lives in `main.py` `CSS`, loaded at USER priority; rule-carrying insets go in `padding`, never `margin`.
- Tests are displayless plain asserts: `python3 tests/test_selfcheck.py` (must stay green). Widget behavior verified with throwaway `/tmp` probes on the live display (`Gtk.init()` returns None — void call), numeric asserts, mainloop pumping.
- subprocess: `GIT_TERMINAL_PROMPT=0` env + timeout on every commit/push call (never hang at a credential prompt); commit/push run off-thread (daemon `threading.Thread`, results via `GLib.idle_add`).
- CSS/paint changes are only visible after a REAL process restart (GtkApplication single-instance forwards spawns — kill old pid first).
- No pip deps; system git assumed present is NOT assumed (missing git → `None`, UI degrades).

**Spec deviations (deliberate):**
1. Spec §2 said `_checked` resets to all on every refresh. Refinement: `set_root`/mode reset → all checked; `refresh()` (focus, post-commit) → intersect with still-dirty paths (a partial selection must not be wiped by alt-tab).
2. Spec §3 said "scroll sync comes free". Per-side `ScrolledWindow`s with a small vadjustment cross-link (same-value `set_value` is a no-op → no loop) — boring and safe vs GTK text-view sizing gambles.
3. Spec §5 said a per-file `Editor.save_path(path)` helper; implemented as one batch helper `Editor.save_open(paths)` — same contract ("flush dirty editor pages for exactly these files"), one write loop.

## Review Focus

1. **Toggle-vs-row click split** — clicking a file row opens its diff; clicking its checkbox only toggles. A person expects the checkbox square, not the row, to drive selection. → Task 3 probe asserts `CellRendererToggle` handles its own `toggled`; real-click re-verified in Task 5's live run.
2. **Commit selection semantics** — commit must contain exactly the checked paths; a sibling staged from the CLI stays staged and absent from the commit. → Task 1 `test_gitstatus_commit_only_selected`.
3. **Push must never hang or prompt** — no-upstream / no-remote situations surface readable errors. → Task 1 `test_gitstatus_push_offline` (prompt env + timeout).
4. **Diff pages must not pollute the editor/persistence machinery** — Ctrl+S/Ctrl+Q, tab restore, state.json stay clean after opening/closing diffs. → Task 2 probe; Task 5 e2e probe re-checks state.json on disk.
5. **Untracked + no-HEAD repos** — an untracked file shows `U` and diffs all-added; a repo with no commits diffs without erroring. → Task 1 `test_gitstatus_diff_for_untracked_and_nohead`.

---

### Task 1: Pure git ops (gitstatus.py)

**Files:**
- Modify: `alpaca_code/gitstatus.py`
- Test: `tests/test_selfcheck.py`

**Interfaces:**
- Consumes: nothing new (std lib only).
- Produces (exact signatures, used by Tasks 2/3/5):
  - `changes(root: str) -> list[tuple[str, str]] | None` — `(relpath, letter)`, relpath uses `/`, letter ∈ `M A U D R`; `None` when not a repo / no git / timeout.
  - `parse_unified(text: str) -> list[tuple[int, int, int, int, list[tuple[str, str]]]]` — hunk `(a_start, a_n, b_start, b_n, lines)`; line kinds `"="` context, `"<"` removed, `">"` added.
  - `build_sides(hunks) -> tuple[list[str], list[str], set[int], set[int], set[int]]` — `(old_lines, new_lines, del_idx, add_idx, hdr_idx)`; sides equal-length; 0-based line indexes for tints.
  - `diff_for(root: str, rel: str, is_untracked: bool = False) -> tuple[str, bool]` — `(diff_text, binary)`; text matches what `commit --only` will commit (worktree vs HEAD).
  - `commit(root: str, paths: list[str], msg: str) -> tuple[bool, str]` — paths REL; `(ok, text)`; text ≤ 400 chars, never raises.
  - `push(root: str) -> tuple[bool, str]` — `(ok, text)`; never raises.

- [ ] **Step 1: Write the failing tests**

Append to `tests/test_selfcheck.py` (before `def main():`, matching its `@register` idiom — helpers defined next to the tests):

```python
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
    assert dl == {2, 6, 7} and al == {3, 8} and hdr == {0, 5, 10}
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
        open(t + "/gone.py", "w").write("d\n")
        g("add", "gone.py"); g("commit", "-m", "two")
        os.remove(t + "/gone.py")                      # worktree deletion
        assert gitstatus.changes(t) == [("gone.py", "D")]
        # rename: -z puts the ORIGINAL as the next record
        g("add", "-A")
        g("commit", "-m", "empty")
        subprocess.run(["git", "-C", t, "mv", "gone.py", "moved.py"], check=True)
        assert sorted(gitstatus.changes(t)) == [("gone.py", "D"), ("moved.py", "R")]
        # untracked DIRECTORY: `-u all` lists each file inside — the tree needs
        # per-file rows, never a `dir/` row (spec §2 tree shape)
        os.makedirs(t + "/nest/deep")
        open(t + "/nest/deep/f.py", "w").write("z\n")
        assert sorted(gitstatus.changes(t)) == [
            ("gone.py", "D"), ("moved.py", "R"), ("nest/deep/f.py", "U")]

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

```

- [ ] **Step 2: Run the suite — new tests must FAIL**

Run: `python3 tests/test_selfcheck.py`
Expected: `FAIL test_gitstatus_parse_unified_and_build_sides: ...has no attribute 'parse_unified'...` (AttributeError on the new functions); every previously-registered test still `PASS`.

- [ ] **Step 3: Implement in `alpaca_code/gitstatus.py`**

Add `import re` to the file header. Append (module stays gi-free) — final file, `gitstatus.py` is also consumed verbatim by Tasks 1's tests (`HUNK_ROW` is referenced by the test):

```python
HUNK_ROW = "@@ … @@"          # side-by-side hunk-header placeholder (both sides)
_HUNK = re.compile(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@")

def parse_unified(text: str) -> list:
    """Unified diff → hunks (a_start, a_n, b_start, b_n, lines); kinds
    '=' context / '<' removed / '>' added. File headers, mode/summary lines and
    no-newline markers ('\\') are skipped; unknown lines inside a hunk are
    ignored — never raises."""
    hunks = []
    cur = None
    for line in text.splitlines():
        m = _HUNK.match(line)
        if m:
            a_n = int(m[2]) if m[2] is not None else 1
            b_n = int(m[4]) if m[4] is not None else 1
            cur = (int(m[1]), a_n, int(m[3]), b_n, [])
            hunks.append(cur)
            continue
        if cur is None:
            continue
        if line.startswith("+"):
            cur[4].append((">", line[1:]))
        elif line.startswith("-"):
            cur[4].append(("<", line[1:]))
        elif line.startswith(" "):
            cur[4].append(("=", line[1:]))
    return hunks

def build_sides(hunks: list) -> tuple:
    """(old_lines, new_lines, del_idx, add_idx, hdr_idx) — ready-built editor
    sides (0-based line indexes for tinting). Equal lengths by construction:
    every event feeds exactly one side and pads the other."""
    old, new, dl, al, hdr = [], [], [], [], []
    for hunk in hunks:
        old.append(HUNK_ROW); new.append(HUNK_ROW); hdr.append(len(old) - 1)
        for kind, txt in hunk[4]:
            if kind == "=":
                old.append(txt); new.append(txt)
            elif kind == "<":
                old.append(txt); new.append("")
                dl.append(len(old) - 1)
            else:                                # '>'
                new.append(txt); old.append("")
                al.append(len(new) - 1)
    return (old, new, set(dl), set(al), set(hdr))

def changes(root: str) -> list[tuple[str, str]] | None:
    """Every dirty path — one merged row (staged letter preferred, else worktree
    letter; untracked → 'U'), relpaths with '/', path-sorted. None outside a
    repo / no git / timeout. `diff_for` on a row == exactly what `commit --only`
    commits for it (both are worktree-vs-HEAD)."""
    if not root or not os.path.isdir(os.path.join(root, ".git")):
        return None
    try:
        r = subprocess.run(
            ["git", "-C", root, "status", "--porcelain=v1", "-z", "--untracked-files=all"],
            capture_output=True, timeout=10)
    except (OSError, subprocess.TimeoutExpired):
        return None
    if r.returncode != 0:
        return None
    entries = [e for e in r.stdout.decode("utf-8", "replace").split("\0") if e]
    out = []
    i = 0
    while i < len(entries):
        e = entries[i]
        if len(e) < 4:
            i += 1; continue                     # truncated/stray record (submodules emit extras)
        xy, path = e[:2], e[3:]
        if xy[0] == "R":                         # -z format: ORIGINAL path is the next record
            out.append((path, "R"))
            if i + 1 < len(entries):
                out.append((entries[i + 1], "D"))
            i += 2; continue
        chosen = xy[0] if xy[0] != " " else xy[1]
        out.append((path, {"?": "U"}.get(chosen, chosen)))
        i += 1
    return sorted(set(out), key=lambda t: t[0])

def _git_out(root: str, args: list[str], timeout: int = 15) -> bytes | None:
    """stdout or None; rc 1 counts (no-index/compare returns it on differences)."""
    try:
        r = subprocess.run(["git", "-C", root, *args], capture_output=True, timeout=timeout)
    except (OSError, subprocess.TimeoutExpired):
        return None
    return r.stdout if r.returncode in (0, 1) else None

def diff_for(root: str, rel: str, is_untracked: bool = False) -> tuple[str, bool]:
    """(diff text, binary) for one path: worktree-vs-HEAD == the --only commit.
    Untracked, or a HEAD-less repo (`git diff HEAD` fails): --no-index vs
    /dev/null (all-added). Binary files → ("", True)."""
    if not root or not rel:
        return ("", False)
    stdout = None
    if not is_untracked:
        stdout = _git_out(root, ["diff", "HEAD", "--no-color", "-U3", "--", rel])
        if stdout is None:                       # no HEAD: staged files still need seeing
            stdout = _git_out(root, ["diff", "--cached", "--no-color", "-U3", "--", rel])
    if stdout is None:
        stdout = _git_out(root, ["diff", "--no-index", "--no-color", "-U3",
                                 "--", "/dev/null", os.path.join(root, rel)])
    text = stdout.decode("utf-8", "replace") if stdout else ""
    if "Binary files" in text or "GIT binary patch" in text:
        return ("", True)
    return (text, False)

def commit(root: str, paths: list[str], msg: str) -> tuple[bool, str]:
    """Commit exactly `paths` (REL; modified/deleted/untracked all fine), leaving
    every OTHER staged path staged (`--only` pathspec semantics). (ok, text ≤400)."""
    if not paths or not os.path.isdir(os.path.join(root, ".git")):
        return (False, "No files selected")
    if not msg or not msg.strip():
        return (False, "Empty commit message")
    env = {**os.environ, "GIT_TERMINAL_PROMPT": "0"}
    try:
        r = subprocess.run(["git", "-C", root, "add", "--", *paths], env=env,
                           capture_output=True, text=True, timeout=30)
        if r.returncode != 0:
            return (False, ((r.stderr or "git add failed").strip() or "git add failed")[:400])
        r = subprocess.run(["git", "-C", root, "commit", "--only", "-m", msg, "--", *paths],
                           env=env, capture_output=True, text=True, timeout=60)
    except (OSError, subprocess.TimeoutExpired) as e:
        return (False, f"git failed: {e}")
    if r.returncode != 0:
        return (False, ((r.stderr or "git commit failed").strip() or "git commit failed")[:400])
    first = (r.stdout or "").strip().splitlines()
    return (True, (first[0] if first else "Committed.")[:400])

def push(root: str) -> tuple[bool, str]:
    """`git push`; the no-upstream case retries `git push -u origin <branch>`.
    GIT_TERMINAL_PROMPT=0 + timeout — a credential prompt can never hang the UI.
    (ok, text ≤400); never raises."""
    if not root or not os.path.isdir(os.path.join(root, ".git")):
        return (False, "Not a git repository")
    env = {**os.environ, "GIT_TERMINAL_PROMPT": "0"}
    try:
        r = subprocess.run(["git", "-C", root, "push"], env=env,
                           capture_output=True, text=True, timeout=120)
    except (OSError, subprocess.TimeoutExpired) as e:
        return (False, f"git push failed: {e}")
    if r.returncode == 0:
        return (True, ((r.stderr or r.stdout or "Pushed.").strip())[:400])
    err = (r.stderr or r.stdout or "").strip()
    if "no upstream" in err and branch_of(root):         # first push of a new branch
        try:
            r = subprocess.run(["git", "-C", root, "push", "-u", "origin", branch_of(root)],
                               env=env, capture_output=True, text=True, timeout=120)
        except (OSError, subprocess.TimeoutExpired) as e:
            return (False, f"git push failed: {e}")
        if r.returncode == 0:
            return (True, ((r.stderr or r.stdout or "Pushed.").strip())[:400])
        err = (r.stderr or r.stdout or "git push failed").strip()
    return (False, (err or "git push failed")[:400])
```

- [ ] **Step 4: Run the suite — all green**

Run: `python3 tests/test_selfcheck.py`
Expected: every test `PASS` including the five new ones. `git` is a hard dep of this feature; if the box lacks git, `changes(t) is None` still holds and the temp-repo tests fail loudly — that is correct (do not soften them).

- [ ] **Step 5: Commit**

```bash
git add alpaca_code/gitstatus.py tests/test_selfcheck.py
git commit -m "gitstatus: changes, parse_unified/build_sides, diff_for, commit-only, push"
```

---
### Task 2: Editor diff pages (editor.py + CSS)

**Files:**
- Modify: `alpaca_code/editor.py`
- Modify: `alpaca_code/main.py` (CSS slice, Task-6 collects nothing new here — put CSS with the code that needs it)
- Verify: throwaway probe `/tmp/probe_diffpage.py` (widget behavior is probe-only per CLAUDE.md)

**Interfaces:**
- Consumes: shapes only from Task 1 (`build_sides` output arrives as a plain 5-tuple of two `list[str]` + three `list[int]`; `set_workspace` never calls this directly).
- Produces: `Editor.open_diff(rel: str, sidetuple: tuple, letter: str) -> None` where `sidetuple = (old_lines, new_lines, del_idx, add_idx, hdr_idx)` exactly as `gitstatus.build_sides` returns; `Editor.save_open(paths: list[str]) -> None`; shared `Editor._write_page(page) -> None` extracted from save_active's body. Diff pages carry `page.diff_of = "diff:<rel>"` and NO `path`/`buf`, never enter persistence, and task 3 calls `open_diff` after `diff_for` + `parse_unified` + `build_sides` with `build_sides`' tuple passed straight through.

The full page-iteration audit, verified against editor.py as read: the sites are `_page_of` path lookup, `_refresh_tab_state` badge restamp, `_dirty_dot` page find, `_watch`/`_cancel_watch` (path keyed), `_error` (buffer-agnostic), `save_active` (path+buf write), `_close_index` (buf modified + path unwatch), `_close_all` (path unwatch), `get_open_state` (relpath(path)), `has_dirty` (buf exists). Every site gets a guard or filters `getattr(page, "path", None)` — one convention, one pass.

- [ ] **Step 1: Write the failing test**

Add to `tests/test_selfcheck.py` (after the gitstatus tests):

```python
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
        editor.Editor.save_active(e)                      # diff page → no-op, no OSError
        assert open(fp).read() == "old"
        editor.Editor.save_open(e, [os.path.abspath(fp)])
        assert open(fp).read() == "new\n"
        # and a file page NOT in the named set stays open-unsaved
        editor.Editor.save_open(e, [])
        assert open(fp).read() == "new\n"
```

Note `Editor.__new__` (class-unbound call) — the set_workspace tests establish this fake-object idiom; widgets are never touched. `get_open_state` must tolerate an empty notebook (`active_tab` clamps).

- [ ] **Step 2: Run to see it fail**

Run: `python3 tests/test_selfcheck.py`
Expected: FAIL — `save_active` raises `AttributeError` (diff page has no `.path`: `open(page.path)` → AttributeError before any write), and `_page_of` raises `AttributeError` on `.path`.

- [ ] **Step 3: Implement (editor.py)**

3a. Guard every page-iteration site, per the audit above. Concretely, by site (read-before-edit, match the current code exactly):

- `_page_of(path)`:

```python
    def _page_of(self, path: str) -> int:
        for i in range(self.nb.get_n_pages()):
            page = self.nb.get_nth_page(i)
            if getattr(page, "path", None) == path or getattr(page, "diff_of", None) == path:
                return i
        return -1
```

- `save_active` + shared writer + commit-flow saver (all at once):

```python
    def _write_page(self, page) -> None:
        with open(page.path, "w", encoding="utf-8") as f:
            f.write(page.buf.props.text)
        page.buf.set_modified(False)
        # ponytail: sync stdlib write, as save_active was

    def save_active(self) -> None:
        page = self.nb.get_nth_page(self.nb.get_current_page())
        if page is None or getattr(page, "path", None) is None:
            return            # diff page: Ctrl+S is a no-op; there is no dirty buffer
        self._write_page(page)

    def save_open(self, paths: list[str]) -> None:
        """Commit flow (spec §5): flush dirty buffers among the named abs paths."""
        for i in range(self.nb.get_n_pages()):
            page = self.nb.get_nth_page(i)
            p = getattr(page, "path", None)
            if p in paths and page.buf.get_modified():
                self._write_page(page)
```

- `_close_index`: first meaningful line becomes

```python
        p = getattr(page, "path", None)
        if page.buf.get_modified() and not force:    # buf-less diff page → get_modified …
```
wait — diff pages have NO buf at all, so `page.buf` itself raises. The guard ordering that keeps diff pages closable: diff pages never dirty, so:
```python
    def _close_index(self, index: int, force: bool = False) -> None:
        page = self.nb.get_nth_page(index)
        if page is None:
            return
        buf = getattr(page, "buf", None)
        if buf is not None and buf.get_modified() and not force:
            ... existing dialog (uses os.path.basename(page.path) - only reached when buf exists, so path exists too)
        self.nb.remove_page(index)
        self._cancel_watch(getattr(page, "path", ""))
        self._refresh_tab_state()
```
`_cancel_watch("")` is safe (dict pop miss already). Keep the `page.path` use inside the dialog text (only reachable with a real file page).

- `_close_all`: same — `self._cancel_watch(getattr(page, "path", ""))`.

- `has_dirty`: `return any((pg := self.nb.get_nth_page(i)).buf.get_modified() ...) for i ...)` becomes guard-per-page:

```python
    def has_dirty(self) -> bool:
        for i in range(self.nb.get_n_pages()):
            buf = getattr(self.nb.get_nth_page(i), "buf", None)
            if buf is not None and buf.get_modified():
                return True
        return False
```

- `_dirty_dot(buf)`'s inner match: `if page.buf is buf` → `if getattr(page, "buf", None) is buf:`

- `_refresh_tab_state`: in the page loop, first line after `page = self.nb.get_nth_page(i)`:

```python
            if getattr(page, "diff_of", None) is not None:
                continue        # diff tab: badge_slot already holds its letter chip
```

- `get_open_state`:

```python
    def get_open_state(self) -> dict:
        pages = [self.nb.get_nth_page(i) for i in range(self.nb.get_n_pages())]
        paths = [p.path for p in pages if getattr(p, "path", None)]
        rel = [os.path.relpath(p_, self.root) if self.root else p_ for p_ in paths]
        return {"open_tabs": rel,
                "active_tab": max(self.nb.get_current_page(), 0)}
```

(active_tab is an index, not a path — keep as-is even if it points at a diff tab: closing tabs re-clamps via `min(saved.active, pages-1)` in restore.)

3b. Module-level tokens right after `readable_text` (NOT class attributes — `_diff_side` references them bare), then `open_diff` + `_diff_side` added after `open_file`:

```python
# side-by-side tints (spec §3): sit under #e6e8ee text on the #0d1017 card
DIFF_DEL_BG = "#25181c"
DIFF_ADD_BG = "#15261d"
DIFF_HDR_FG = "#5a6375"
```

```python
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
            a = buf.get_iter_at_line(i); b = a.copy(); b.forward_to_line_end()
            buf.apply_tag(bg, a, b)
        for i in sorted(hdr_idx):
            if i >= buf.get_line_count() or i < 0:
                continue
            a = buf.get_iter_at_line(i); b = a.copy(); b.forward_to_line_end()
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
        page.diff_of = f"diff:{rel}"     # no .path/.buf: persistence/save/dirty skip this page
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
        # Cross-link via notify::vadjustment (filetree measured gotcha: the
        # ScrolledWindow REPLACES the view's adjustment on realize, so a link
        # attached to the construct-time adj dies at map). Hook fires whenever
        # the live adj changes; one value-changed per live adj (deduped by
        # `_linked`). Equal-value set_value fires no changed signal → no loop;
        # the 0.01 epsilon also swallows clamp-rounding bounce.
        def _adj_hook(v, _pspec, dst):
            live = v.get_vadjustment()
            if getattr(v, "_linked", None) is live:
                return
            v._linked = live
            live.connect("value-changed", lambda a, o=dst: o.get_vadjustment()
                         .set_value(a.get_value()))   # mirror adj: same range, set_value no-ops when equal
        vl.connect("notify::vadjustment", _adj_hook, vr)
        vr.connect("notify::vadjustment", _adj_hook, vl)
        self._refresh_tab_state()   # restamp pass now SKIPS diff pages (guard), chip preserved
        self.on_state_changed()
```

Why the guard ordering above matters: `_refresh_tab_state` clears every
page's `badge_slot` children (its remove-loop runs for any page that has one),
and a diff page HAS a badge_slot — its letter chip. The `diff_of` guard must
come BEFORE the slot read, or every restamp wipes the chip.

3c. CSS slice into `main.py` `CSS` (after the `.alpaca-dirty` rule, before the breadcrumb block):

```css
/* --- side-by-side diff pages (spec §3) --- */
.alpaca-diffsep { min-width: 1px; background: #1c2230; }
.alpaca-diffchipm,.alpaca-diffchipa,.alpaca-diffchipu,.alpaca-diffchipd,.alpaca-diffchipr {
    font-size: 10px; font-weight: 700; }
.alpaca-diffchipm { color: #f2c94c; }
.alpaca-diffchipr { color: #2f80ed; }
.alpaca-diffchipa, .alpaca-diffchipu { color: #22c55e; }
.alpaca-diffchipd { color: #ef4444; }
```
(`.alpaca-breadcrumb`, `.alpaca-mono`, `.alpaca-tabname` apply verbatim — diff pages reuse them.)

- [ ] **Step 4: Run to green**

Run: `python3 tests/test_selfcheck.py` → all PASS including the new test.

- [ ] **Step 5: Probe — throwaway `/tmp/probe_diffpage.py`**

Full page-level verification with real widgets, numeric asserts: open a diff page on a temp repo file, assert page count 1, `page.diff_of` present, `get_open_state` empty tabs, save_active doesn't crash, re-open switches instead of duplicating, open real file tab next to it, `_page_of` returns diff page index for the key and the file index for the path, scroll cross-link moves the other side (set right's vadj value, pump mainloop, compare left's), close_index works on the diff page, `_refresh_tab_state` left the letter chip label intact after switch (find the chip label through `page.badge_slot.get_first_child().get_property("label") == "M"`).

```python
import os, sys, tempfile
sys.path.insert(0, "/home/dominykasm/FunProjects/misiuscode")
import alpaca_code.gi_env as ge
ge.require("Gtk", ("4.0",)); ge.require("GtkSource", ("5",)); ge.require("Gdk", ("4.0",))
ge.require("Pango", ("1.0",)); ge.require("GdkPixbuf", ("2.0",))
from gi.repository import Gtk, GLib
Gtk.init()  # void on this build; never truthy-test
from alpaca_code.editor import Editor
from alpaca_code import gitstatus

UNIT = ("@@ -1,3 +1,3 @@\n a = 1\n-b = 2\n+b = 3\n"
        "+b2 = 22\n+b3 = 23\n a = 1\n+x = 1\n")

with tempfile.TemporaryDirectory() as tmp:
    src = os.path.join(tmp, "one.py")
    with open(src, "w") as f:
        f.write("a = 1\nb = 2\n" + "\n".join(f"print({i})" for i in range(40)) + "\n")
    sides = gitstatus.build_sides(gitstatus.parse_unified(UNIT))
    assert len(sides[0]) == len(sides[1]), "task1 broke: unequal sides"   # build_sides guarantee
    ed = Editor()
    ed.root = tmp
    w = Gtk.Window(); w.set_child(ed); w.present()
    ctx = GLib.MainContext.default()
    for _ in range(30): ctx.iteration(may_block=False)
    ed.open_diff("one.py", sides, "M")
    assert ed.nb.get_n_pages() == 1
    pg = ed.nb.get_nth_page(0)
    assert pg.diff_of == "diff:one.py"
    assert not hasattr(pg, "path") and not hasattr(pg, "buf")
    assert ed.get_open_state() == {"open_tabs": [], "active_tab": 0}   # diff page not persisted
    ed.save_active()                     # no path → no-op (must not raise)
    for _ in range(10): ctx.iteration(may_block=False)
    ed.open_diff("one.py", sides, "M")   # re-open: switch to the same page, no dupe
    assert ed.nb.get_n_pages() == 1 and ed.nb.get_current_page() == 0
    ed.open_file(src)
    assert ed.nb.get_n_pages() == 2
    assert ed.get_open_state()["open_tabs"] == ["one.py"]   # the REAL file persists
    assert ed._page_of("diff:one.py") == 0 and ed._page_of(src) == 1
    # scroll cross-link: drag the LEFT side, right follows (post-realize adjustments)
    pair = ed.nb.get_nth_page(0).get_last_child()
    sw_l = pair.get_first_child()
    sw_r = sw_l.get_next_sibling().get_next_sibling()
    vl_adj = sw_l.get_child().get_vadjustment()
    vr_adj = sw_r.get_child().get_vadjustment()
    vl_adj.set_value(30.0)
    for _ in range(30): ctx.iteration(may_block=False)
    assert abs(vr_adj.get_value() - vl_adj.get_value()) <= 0.6, (vl_adj.get_value(), vr_adj.get_value())
    # restamp guard: machinery must not wipe the letter chip
    ed._refresh_tab_state()
    chip = ed.nb.get_nth_page(0).badge_slot.get_first_child()
    assert chip is not None and chip.get_property("label") == "M"
    ed.close_index(ed._page_of("diff:one.py"))
    assert ed.nb.get_n_pages() == 1 and ed._page_of(src) != -1
    print("PROBE OK")
```

Run: `python3 /tmp/probe_diffpage.py` → `PROBE OK`.
(Both sides come from real `build_sides` — equal line counts guaranteed, and this also integration-checks Task 1→2 handoff. If tinted lines don't paint, try scheme `def:`-level tags first, then fall back per the note below.)

- [ ] **Step 6: Suite + real-restart note**

Run: `python3 tests/test_selfcheck.py` → all PASS.
Clean: `rm /tmp/probe_diffpage.py`.
Visual confirmation needs the real app restarted (single-instance forwarding gotcha) — leave for Task 5's final verify step.

- [ ] **Step 7: Commit**

```bash
git add alpaca_code/editor.py alpaca_code/main.py tests/test_selfcheck.py
git commit -m "editor: read-only side-by-side diff pages (.diff_of pages, guarded machinery, scroll-linked sides)"
```

------
### Task 3: Changes view (gitview.py) + pure tree helpers

**Files:**
- Modify: `alpaca_code/gitstatus.py` (two pure helpers — same file as Task 1, stays gi-free)
- Create: `alpaca_code/gitview.py`
- Modify: `alpaca_code/badges.py` (one public wrapper over the existing cairo chip renderer)
- Modify: `alpaca_code/main.py` (commit-bar CSS)

**Interfaces:**
- Consumes: `gitstatus.changes(root) -> list[(rel, letter)] | None`, `gitstatus.commit/push` (Task 1); `Editor.open_diff(rel, sidetuple, letter)` + `Editor.save_open(paths)` (Task 2).
- Produces: `gitstatus.group_tree(rows: list[tuple[str, str]]) -> list[tuple]` — `(kind "d"|"f", name, rel, letter, depth)`; `gitstatus.checked_dir_state(rows, checked: set[str]) -> dict[str, tuple[bool, bool]]` — dir rel → `(all_checked, any_checked)`; `badges.letter_pixbuf(letter: str, hexcol: str) -> Pixbuf | None`; widget `gitview.ChangesView(Gtk.Box)` with attrs/wiring:
  - `root: str | None` — set via `set_root(root)`
  - `on_open: Callable[[str, str], None]` — `(rel, letter)` — window → `editor.open_diff` (the tab chip needs the letter)
  - `before_commit: Callable[[list[str]], None]` — window → `editor.save_open`
  - `on_refresh: Callable[[], None]` — window → browser statusbar refresh
  - `set_root(root) -> None`, `refresh(keep_selection: bool = True) -> None` (rebuild; default selects all, refresh intersects — deviation 1)
  - `filter(needle: str) -> None` (re-render the remembered rows, name-substring)
  - `has_files() -> bool`, `_busy` flag, `msg`/`commit_btn`/`push_btn` widgets.
- Store anatomy (12 cols, flat list): 0 name, 1 rel, 2 kind (`"s"` masthead / `"d"` dir / `"f"` file / `"e"` empty-state), 3 letter, 4 chevron pix, 5 badge pix, 6 letter-chip pix, 7 checked, 8 inconsistent, 9 chev visible, 10 letter visible, 11 toggle visible. Three columns over the store: main (chev/badge/name expand — name cell packed LAST with expand=True per the filetree packing ruling, so name/status/toggle flush left and right), letter, toggle.

**Why the toggle gets its own column:** `row-activated` carries the clicked column, so the handler can structurally refuse toggle-column activations (`col is self._toggle_col → return`) instead of betting on when this build suppresses row activation under an activatable cell (Review Focus #1).

- [ ] **Step 1: Write the failing tests (pure helpers only — widgets never in the suite)**

```python
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
```

- [ ] **Step 2: Run to see it fail**

Run: `python3 tests/test_selfcheck.py`
Expected: `FAIL test_gitstatus_group_tree_and_dir_state: ...no attribute 'group_tree'...`

- [ ] **Step 3: Implement the pure helpers (gitstatus.py, after `push`)**

```python
def group_tree(rows: list[tuple[str, str]]) -> list:
    """changes() rows → [(kind 'd'|'f', name, rel, letter, depth)]: every changed
    path's ancestor dirs become dir rows; parents before children, dirs before
    files at a level, paths sorted."""
    tree: dict = {}
    for rel, letter in sorted(rows):
        node = tree
        parts = rel.split("/")
        for p in parts[:-1]:
            node = node.setdefault(p, {})
        node.setdefault("", []).append((parts[-1], letter))   # "" = this level's files
    out = []

    def walk(node: dict, prefix: str, depth: int) -> None:
        for name, sub in sorted((k, v) for k, v in node.items() if k):
            drel = f"{prefix}/{name}" if prefix else name
            out.append(("d", name, drel, "", depth))
            walk(sub, drel, depth + 1)
        for name, letter in node.get("", []):
            out.append(("f", name, f"{prefix}/{name}" if prefix else name, letter, depth))

    walk(tree, "", 0)
    return out

def checked_dir_state(rows: list, checked: set[str]) -> dict:
    """group_tree rows → dir rel → (all descendant files checked, any checked).
    all ⊇ any (fully-checked dir → (True, True)); dirs exist only as file
    ancestors (group_tree), so each accumulates ≥1 file."""
    state: dict[str, tuple[bool, bool]] = {}
    for kind, _name, rel, _letter, _depth in rows:
        if kind != "f":
            continue
        ok = rel in checked
        parts = rel.split("/")
        acc = ""
        for p in parts[:-1]:
            acc = f"{acc}/{p}" if acc else p
            prev = state.get(acc, (True, False))
            state[acc] = (prev[0] and ok, prev[1] or ok)
    return state
```

- [ ] **Step 4: Run to green (helpers)**

Run: `python3 tests/test_selfcheck.py` → both new tests PASS, all prior PASS.

- [ ] **Step 5: `badges.letter_pixbuf` (public wrapper; `_render` + its `_PCS` cache do the work)**

In `badges.py` under `chevron_pixbuf`:

```python
def letter_pixbuf(letter: str, hexcol: str) -> GdkPixbuf.Pixbuf | None:
    """Status-letter chip for gitview's tree cells (bare colored glyph, no chip
    background) — same _render (label, color, None) shape as EXT_BADGE."""
    return _render((letter, hexcol, None))
```

Full module, final — every block below in one file, `alpaca_code/gitview.py`:

```python
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
from gi.repository import Gdk, GdkPixbuf, GLib, Gtk, Pango

from . import badges, gitstatus

LETTER_COLOR = {  # spec §2: M amber, A/U green, D red, R blue (+T typechange)
    "M": "#f2c94c", "A": "#22c55e", "U": "#22c55e",
    "D": "#ef4444", "R": "#2f80ed", "T": "#2f80ed",
}
LETTER_DEFAULT = "#f2c94c"

class ChangesView(Gtk.Box):
    def __init__(self):
        super().__init__(orientation=Gtk.Orientation.VERTICAL)
        self.on_open = lambda rel, letter: None  # window → editor.open_diff
        self.before_commit = lambda paths: None  # window → editor.save_open
        self.on_refresh = lambda: None           # window → browser statusbar refresh
        self.root = None
        self._checked: set[str] = set()          # file rels only (subset of _files)
        self._files: set[str] = set()            # every file row in the changeset
        self._rows: list[tuple[str, str]] = []   # last changes() (filter() uses it)
        self._open: set[str] = set()             # expanded dir rels (signal-fed; view readback is stale)
        self._busy = False

        self.store = Gtk.TreeStore(str, str, str, str,            # name, rel, kind, letter
                                   GdkPixbuf.Pixbuf, GdkPixbuf.Pixbuf, GdkPixbuf.Pixbuf,  # chev, badge, chip
                                   bool, bool, bool, bool, bool)          # checked, inconsist, chev, letter, toggle
        self.view = Gtk.TreeView(model=self.store, headers_visible=False)
        self.view.set_activate_on_single_click(True)
        self.view.set_property("show-expanders", False)
        self.view.set_level_indentation(16)
        self.view.set_tooltip_column(1)

        # cell props at insert-time; text cell packed LAST with expand=True so
        # chevron/badge stay pinned left (filetree measured packing rule)
        cell_name = Gtk.CellRendererText(); cell_name.set_property("ypad", 2)
        cell_name.set_property("ellipsize", Pango.EllipsizeMode.MIDDLE)
        chev = Gtk.CellRendererPixbuf(); chev.set_property("ypad", 2)
        bad = Gtk.CellRendererPixbuf();  bad.set_property("ypad", 2)
        let = Gtk.CellRendererPixbuf();  let.set_property("ypad", 2)
        tog = Gtk.CellRendererToggle(); tog.set_property("activatable", True)
        col_main = Gtk.TreeViewColumn()          # kwargs form raises on this build
        col_main.pack_start(chev, False); col_main.add_attribute(chev, "pixbuf", 4)
        col_main.add_attribute(chev, "visible", 9)
        col_main.pack_start(bad, False); col_main.add_attribute(bad, "pixbuf", 5)
        col_main.pack_start(cell_name, True)
        col_main.add_attribute(cell_name, "text", 0)
        col_let = Gtk.TreeViewColumn()
        col_let.pack_start(let, False); col_let.add_attribute(let, "pixbuf", 6)
        col_let.add_attribute(let, "visible", 10)
        col_tog = Gtk.TreeViewColumn()
        col_tog.pack_start(tog, False)
        col_tog.add_attribute(tog, "active", 7)
        col_tog.add_attribute(tog, "inconsistent", 8)
        col_tog.add_attribute(tog, "visible", 11)
        self.view.append_column(col_main)
        self.view.append_column(col_let)
        self.view.append_column(col_tog)
        self._toggle_col = col_tog
        self.view.set_css_classes(["alpaca-tree"])
        self.view.connect("row-expanded", self._on_expand_toggle, True)
        self.view.connect("row-collapsed", self._on_expand_toggle, False)
        self.view.connect("row-activated", self._on_activated)
        tog.connect("toggled", self._on_toggled)

        self.append(Gtk.ScrolledWindow(vexpand=True, child=self.view))

        bar = Gtk.Box(spacing=6, margin_start=12, margin_end=12, margin_top=6, margin_bottom=6)
        bar.set_css_classes(["alpaca-commitbar"])
        self.msg = Gtk.Entry(hexpand=True)
        self.msg.set_placeholder_text("Commit message…")
        self.msg.set_css_classes(["alpaca-msg"])
        self.msg.set_size_request(-1, 26)    # height floor, like .alpaca-search's 30
        self.msg.connect("activate", self._on_msg_enter)
        self.commit_btn = Gtk.Button(label="Commit"); self.commit_btn.set_css_classes(["alpaca-barbtn"])
        self.push_btn = Gtk.Button(label="Push");    self.push_btn.set_css_classes(["alpaca-barbtn"])
        self.commit_btn.connect("clicked", lambda b: self.commit_clicked())
        self.push_btn.connect("clicked", lambda b: self.push_clicked())
        self.result = Gtk.Label(xalign=0.0, visible=False)
        self.result.set_ellipsize(Pango.EllipsizeMode.END)   # 400-char git output must not widen the bar
        self.result.set_css_classes(["alpaca-commitresult"])
        bar.append(self.msg); bar.append(self.commit_btn); bar.append(self.push_btn)
        self.append(bar)
        self.append(self.result)

    # ---- data ------------------------------------------------------------------
    def set_root(self, root: str | None) -> None:
        self.root = root
        self._open.clear()
        self.refresh(keep_selection=False)       # a new repo = fresh selection

    def refresh(self, keep_selection: bool = True) -> None:
        """Rebuild from changes(). Fresh view selects all (spec §2); a refresh
        INTERSECTS the surviving selection with the new file set (deviation 1) —
        after a commit the committed paths vanish and the rest stay selected."""
        rows = gitstatus.changes(self.root) or []
        self._rows = rows
        files = {r[2] for r in gitstatus.group_tree(rows) if r[0] == "f"}   # r[2] = rel (group_tree: kind,name,rel,letter,depth)
        self._files = set(files)                 # BEFORE _fill: _sync_row needs it
        self._checked = (self._checked & files) if keep_selection else set(files)
        self._fill(rows)

    def filter(self, needle: str) -> None:
        """Search-route: re-render by name/rel substring over the remembered
        rows, no re-scan. _checked untouched (view-only); _files stays global."""
        n = (needle or "").strip().lower()
        self._fill([r for r in self._rows
                    if not n or n in os.path.basename(r[0]).lower() or n in r[0].lower()])

    def _fill(self, rows: list[tuple[str, str]]) -> None:
        self.store.clear()
        self._open.clear()          # rebuilt rows: remembered expansions are stale (filetree law)
        if not rows:
            self.store.append(None, ["No changes", "", "e", "", badges.blank_pixbuf(),
                                     badges.blank_pixbuf(), None, False, False, False, False, False])
            self._sync()
            return
        self.store.append(None, ["Select all", "", "s", "", badges.blank_pixbuf(),
                                 badges.blank_pixbuf(), None, False, False, False, False, True])
        iters: dict[str, object] = {}
        for kind, name, rel, letter, _d in gitstatus.group_tree(rows):
            is_dir = kind == "d"
            badge = badges.folder_pixbuf() if is_dir else badges.pixbuf_for(name)
            lpix = (badges.letter_pixbuf(letter, LETTER_COLOR.get(letter, LETTER_DEFAULT))
                    if letter else None)
            parent = iters.get(rel.rsplit("/", 1)[0]) if "/" in rel else None
            it = self.store.append(parent, [
                name, rel, kind, letter,
                badges.chevron_pixbuf(False) if is_dir else badges.blank_pixbuf(),
                badge or badges.blank_pixbuf(), lpix,
                False, False, is_dir, bool(letter), True])
            if is_dir:
                iters[rel] = it
        self._sync()

    # ---- checked-state sync (value-only writes: safe under expanded rows) -------
    def _sync(self) -> None:
        st = (gitstatus.checked_dir_state(gitstatus.group_tree(self._rows), self._checked)
              if self._rows else {})
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
            allc = bool(self._files) and self._files <= self._checked
            somec = bool(self._checked)
            self.store[it][7] = allc
            self.store[it][8] = not allc and somec

    def _buttons(self) -> None:
        self.commit_btn.set_sensitive(not self._busy and bool(self._checked))
        self.push_btn.set_sensitive(not self._busy and bool(self._files))

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
            files = {r for r in self._files if r.startswith(rel + "/")}
            if files and files <= self._checked:     # every descendant selected → clear them
                self._checked -= files
            else:                                    # none/partial → select all under this dir
                self._checked |= files
        elif kind == "s":                            # masthead: any→all, all→none
            if self._files and self._files <= self._checked:
                self._checked.clear()
            else:
                self._checked = set(self._files)
        self._sync()

    def _on_expand_toggle(self, view, it, tpath, expanded: bool) -> None:
        rel = self.store[it][1]
        if rel:
            (self._open.add if expanded else self._open.discard)(rel)
            self.store[it][4] = badges.chevron_pixbuf(expanded)

    # ---- activation (spec §2: file click opens the diff) ------------------------
    def _on_activated(self, view, tpath, col) -> None:
        if col is self._toggle_col:
            return                                   # checkbox clicks never open a diff (Review Focus #1)
        row = self.store[tpath]
        if row[2] == "d" and row[1]:
            if row[1] in self._open:
                view.collapse_row(tpath)
            else:
                view.expand_row(tpath, False)        # children are pre-loaded (non-lazy store)
        elif row[2] == "f":
            self.on_open(row[1], row[3])         # (rel, letter) — chip needs the letter

    # ---- commit / push (spec §4: workers are daemon threads; GTK via idle_add) --
    def commit_clicked(self) -> None:
        if self._busy or not self.root:
            return
        paths = sorted(self._checked)
        msg = self.msg.get_text().strip()
        if not msg:
            self._show_result(False, "Empty commit message")
            return
        if not paths:
            self._show_result(False, "No files selected")
            return
        self.before_commit(paths)        # window: flush dirty editor buffers for exactly these files
        self._set_busy(True)
        threading.Thread(target=self._do_commit, args=(paths, msg),
                         daemon=True, name="alpaca-commit").start()

    def _do_commit(self, paths, msg) -> None:
        ok, text = gitstatus.commit(self.root, paths, msg)
        GLib.idle_add(self._commit_done, ok, text)

    def _commit_done(self, ok, text) -> bool:
        self._set_busy(False)
        self._show_result(ok, text)
        if ok:
            self.refresh()               # keep_selection=True: committed paths vanish, rest kept
            self.on_refresh()            # window: browser statusbar branch/dirty refresh
        return False                     # idle_add: run once

    def push_clicked(self) -> None:
        if self._busy or not self.root:
            return
        self._set_busy(True)
        threading.Thread(target=self._do_push, daemon=True, name="alpaca-push").start()

    def _do_push(self) -> None:
        ok, text = gitstatus.push(self.root)
        GLib.idle_add(self._push_done, ok, text)

    def _push_done(self, ok, text) -> bool:
        self._set_busy(False)
        self._show_result(ok, text)
        if ok:                       # spec §2: the list re-syncs after push too
            self.refresh()
            self.on_refresh()
        return False

    def _set_busy(self, busy: bool) -> None:
        self._busy = busy
        self._buttons()
        self.msg.set_sensitive(not busy)

    def _show_result(self, ok: bool, text: str) -> None:
        self.result.set_text(text or "Done.")
        self.result.set_css_classes(["alpaca-commitresult", "ok" if ok else "err"])
        self.result.set_visible(True)

    def _on_msg_enter(self, entry) -> None:
        if self.commit_btn.get_sensitive():
            self.commit_clicked()

    def has_files(self) -> bool:
        return len(self._files) > 0
```

Notes for the executor: the masthead is the store's FIRST row (spec §2: "Select
all" on top); `filter()` deliberately never resets `_checked` (view-only);
`_sync` computes dir state from `checked_dir_state` ONCE per pass (O(n), not
per-row recomputes), and `_set_busy` re-gates the buttons so a failed commit
always re-enables.

CSS slice (main.py, after the statusbar block — letter colors are baked into
the cairo chips, so no classes for them):

```css
/* --- changes view: commit bar (spec §6) --- */
.alpaca-commitbar { border-top: 1px solid #1c2230; }
.alpaca-msg { background: #0a0d13; color: #e6e8ee; font-size: 13px;
              border: 1px solid #2b3448; border-radius: 8px;
              min-height: 10px; padding: 0 10px; }
.alpaca-msg:focus { border-color: #2f80ed; }
.alpaca-msg text { min-height: 14px; padding: 0; }
.alpaca-barbtn { background: #111723; color: #e6e8ee; border: 1px solid #2b3448;
                 border-radius: 8px; font-size: 12px; font-weight: 500;
                 padding: 0 12px; min-height: 26px; }
.alpaca-barbtn:hover { background: #1c2431; }
.alpaca-barbtn:disabled { color: #5a6375; background: #0a0d13; }
.alpaca-commitresult { color: #8a93a6; font-size: 11px; padding: 0 12px 4px; }
.alpaca-commitresult.ok { color: #22c55e; }
.alpaca-commitresult.err { color: #ef4444; }
```

`(entry text-node mins mirror .alpaca-search: Breeze floors entries ~36px —
the same min-height sweep applies)`

- [ ] **Step 7: Probe — `/tmp/probe_gitview.py` (numeric; real temp repo, real store)**

```python
import os, sys, tempfile, subprocess, time
sys.path.insert(0, "/home/dominykasm/FunProjects/misiuscode")
import alpaca_code.gi_env as ge
ge.require("Gtk", ("4.0",)); ge.require("Gdk", ("4.0",)); ge.require("GdkPixbuf", ("2.0",))
ge.require("Pango", ("1.0",))
from gi.repository import Gtk, GLib
Gtk.init()   # void on this build; never truthy-test
from alpaca_code.gitview import ChangesView

def dump(gv):
    out = []
    def walk(it):
        while it is not None:
            out.append((gv.store[it][2], gv.store[it][1], gv.store[it][7], gv.store[it][8]))
            nxt = gv.store.iter_next(it)
            walk(gv.store.iter_children(it))
            it = nxt
    walk(gv.store.get_iter_first())
    return out

with tempfile.TemporaryDirectory() as t:
    def g(*a): subprocess.run(["git", "-C", t, *a], capture_output=True, text=True, check=True)
    g("init", "-q"); g("config", "user.email", "t@t"); g("config", "user.name", "t")
    open(t + "/keep.py", "w").write("one\n"); g("add", "."); g("commit", "-q", "-m", "init")
    os.mkdir(t + "/src"); open(t + "/src/b.py", "w").write("two\n")
    open(t + "/top.py", "w").write("u\n")

    gv = ChangesView()
    gv.set_root(t)                       # sets root + refresh(keep_selection=False): select-all default
    w = Gtk.Window(); w.set_child(gv); w.present()
    ctx = GLib.MainContext.default()
    for _ in range(30): ctx.iteration(may_block=False)
    # default state: everything selected; dir src fully-checked (not inconsistent)
    assert dump(gv) == [("s", "", True, False), ("d", "src", True, False),
                        ("f", "src/b.py", True, False), ("f", "top.py", True, False)], dump(gv)
    assert gv.commit_btn.get_sensitive() and gv.push_btn.get_sensitive()

    gv._on_toggled(None, "2")            # uncheck src/b.py
    assert "src/b.py" not in gv._checked
    assert dump(gv)[0] == ("s", "", False, True)   # masthead: some → inconsistent
    assert dump(gv)[1] == ("d", "src", False, True)
    gv._on_toggled(None, "1")            # dir src: partial → cascade-check ALL its files
    assert gv._checked == gv._files
    gv._on_toggled(None, "0")            # masthead: all → none
    assert gv._checked == set() and not gv.commit_btn.get_sensitive()
    gv._on_toggled(None, "0")            # none → all
    assert gv._checked == gv._files

    # activation split (Review Focus #1): toggle column never opens the diff
    opened = []
    gv.on_open = lambda rel, letter: opened.append((rel, letter))
    gv._on_activated(gv.view, Gtk.TreePath.new_from_string("2"), gv._toggle_col)
    assert opened == []
    gv._on_activated(gv.view, Gtk.TreePath.new_from_string("2"), gv.view.get_column(0))
    assert opened == [("src/b.py", "M")]
    gv._on_activated(gv.view, Gtk.TreePath.new_from_string("1"), gv.view.get_column(0))
    assert opened == [("src/b.py", "M")]  # dir click still opened nothing (len unchanged)
    gv._on_expand_toggle(gv.view, gv.store.get_iter("1"), None, True)    # handler logic directly
    assert "src" in gv._open
    gv._on_expand_toggle(gv.view, gv.store.get_iter("1"), None, False)
    assert "src" not in gv._open

    # expand machinery through the VIEW (children pre-loaded, non-lazy store):
    # real expand_row → row-expanded signal → _open
    gv.view.expand_row(Gtk.TreePath.new_from_string("1"), False)
    for _ in range(10): ctx.iteration(may_block=False)
    assert "src" in gv._open
    gv.view.collapse_row(Gtk.TreePath.new_from_string("1"))
    assert "src" not in gv._open

    # filter: no re-scan, selection untouched
    gv.filter("top")
    assert dump(gv) == [("s", "", True, False), ("f", "top.py", True, False)], dump(gv)
    gv.filter("")
    assert len(dump(gv)) == 4
    assert gv._open == set()          # a rebuild invalidates remembered expansions (filetree law)

    # real commit through the bar: worker thread + idle marshalling
    done = []
    gv.before_commit = lambda paths: done.append(("prep", list(paths)))
    gv.on_refresh = lambda: done.append(("refresh",))
    def fake_show(ok, text, done=done):
        done.append(("result", ok, text))
    gv._show_result = fake_show
    gv.msg.set_text("from probe")
    gv.commit_clicked()
    for _ in range(300):                 # pump until the worker's idle lands (≤ hard cap)
        ctx.iteration(may_block=False)
        if any(d[0] == "result" for d in done): break
        time.sleep(0.01)
    assert ("prep", ["src/b.py", "top.py"]) in done, done
    assert any(d[0] == "result" and d[1] is True for d in done), done
    assert any(d[0] == "refresh" for d in done), done
    assert subprocess.run(["git", "-C", t, "show", "--name-only", "--format="],
                          capture_output=True, text=True).stdout.split() == [
        "src/b.py", "top.py"], "commit carried more than the selection"

    gv.refresh()                         # changeset now clean → empty-state row
    assert gv.has_files() is False
    assert dump(gv)[0][0] == "e" and dump(gv)[0][1] == "No changes"
    gv.push_clicked()                    # no remote: red result, no raise, busy released
    for _ in range(300):
        ctx.iteration(may_block=False)
        if not gv._busy: break
        time.sleep(0.01)
    assert gv._busy is False and not gv.commit_btn.get_sensitive()
    print("PROBE OK")
```

Run: `python3 /tmp/probe_gitview.py` → `PROBE OK`.
(`push` in the probe hits a remote-less repo → gitstatus returns (False, …) —
the _show_result assert covers the marshal; the real push needs Task 5's
manual pass on the user's actual repo.)
- [ ] **Step 8: real-restart visual pass + suite**

Run: `python3 tests/test_selfcheck.py` → all PASS; `rm /tmp/probe_gitview.py`.
The view mounts in Task 4/5 wiring; its pixel look gets the Task 5 real-app pass (restart, click CHANGES, screenshot vs n_browser.png conventions).

- [ ] **Step 9: Commit**

```bash
git add alpaca_code/gitview.py alpaca_code/gitstatus.py alpaca_code/badges.py alpaca_code/main.py tests/test_selfcheck.py
git commit -m "gitview: change tree with checkboxes, commit/push bar; group_tree helpers"
```

### Task 4: WORKSPACE/CHANGES tab buttons + Stack (filetree.py, treehover.py)

**Files:**
- Create: `alpaca_code/treehover.py` (public `HoverTree` — filetree's `_HoverTree` moved verbatim)
- Modify: `alpaca_code/filetree.py` (delete `_HoverTree`; mode buttons, Stack, search routing, CHANGES visibility, `refresh_git`)
- Modify: `alpaca_code/gitview.py` (swap the plain TreeView for `HoverTree` so the changes list gets the same hover band)
- Modify: `alpaca_code/main.py` (`.alpaca-tabbtn` rules; delete the now-dead `.alpaca-section` rule)

**Interfaces:**
- Consumes: Task 3's `ChangesView` (`from .gitview import ChangesView`; attrs `set_root`/`refresh`/`filter`, wiring callbacks `on_open`/`before_commit`/`on_refresh` left unwired — window owns them in Task 5).
- Produces (Task 5 relies on):
  - `treehover.HoverTree(Gtk.TreeView)` — the hover-band tree view (class body moved verbatim, public name).
  - `FileBrowser.changes: ChangesView` — constructed in `__init__`; callbacks left as the views' own no-ops.
  - `FileBrowser._mode: str` — `"tree"` or `"changes"` (inits `"tree"`); `FileBrowser._set_mode(mode: str) -> None`; `FileBrowser.refresh_git() -> None` (statusbar +, when in changes mode, the changes view).
  - `gitview.ChangesView.view` is a `HoverTree` with row-inserted/row-deleted hover-refresh connects parked on its store (filetree parity).

**Import direction (no cycle):** `filetree → gitview → (badges, gitstatus, treehover)`; `treehover` imports nothing app-side. This is why `HoverTree` moves to its own module rather than staying private in filetree.

- [ ] **Step 1: Extract the hover tree — create `alpaca_code/treehover.py`, patch filetree**

Create `alpaca_code/treehover.py` (the class body moves VERBATIM — module comment becomes the module docstring, class renamed):

```python
# Hover-band TreeView — shared by the file browser and the changes view.
#
# GTK4 tree rows expose no :hover state: gtktreeview folds the widget's own
# state into EVERY row's background paint (and never sets cell PRELIT —
# gtkcellrenderer.c:1653 strips it), so a `treeview:hover` CSS rule would tint
# the whole widget. Instead: pointer position tracked via a motion controller,
# kept as a TreeRowReference (stable across model shifts above the row;
# `.valid()` goes False when the row is deleted), painted in do_snapshot
# BEFORE the chain-up — rows' transparent CSS background lets the band show,
# while opaque paint (selected #1b3560, cells' text) draws on top of it.
import alpaca_code.gi_env as ge
ge.require("Gtk", ("4.0",))
ge.require("Gdk", ("4.0",))
ge.require("Pango", ("1.0",))
ge.require("Graphene", ("1.0",))
from gi.repository import Gdk, GLib, Gtk, Graphene


class HoverTree(Gtk.TreeView):
    def __init__(self, **kw):
        super().__init__(**kw)
        self._hover_xy = (-1.0, -1.0)   # last pointer pos in view coords; y<0 = outside
        self._hover_row = None          # Gtk.TreeRowReference | None
        motion = Gtk.EventControllerMotion()
        motion.connect("motion", self._on_motion)
        motion.connect("leave", self._on_leave)
        self.add_controller(motion)
        # defer past the treeview's own scroll sync (it connects after this one,
        # so a synchronous refresh here would read pre-scroll geometry). The
        # hook attaches to whatever adjustment is live at the moment — the tree's
        # own at construct time, the ScrolledWindow's once it's parented (the
        # property swap fires notify::vadjustment and we reconnect).
        self._scroll_adj = None
        self.connect("notify::vadjustment", self._hook_scroll_adj)
        self._hook_scroll_adj()

    def _hook_scroll_adj(self, *a):
        adj = self.get_vadjustment()
        if adj is self._scroll_adj:
            return
        self._scroll_adj = adj
        adj.connect("value-changed",
                    lambda a: GLib.idle_add(self._refresh_hover))

    def _on_motion(self, ctrl, x, y):
        self._hover_xy = (x, y)
        self._refresh_hover()

    def _on_leave(self, ctrl):
        self._hover_xy = (-1.0, -1.0)
        self._hover_row = None
        self.queue_draw()

    def _refresh_hover(self):
        x, y = self._hover_xy
        if y < 0:
            return
        res = self.get_path_at_pos(int(x), int(y))   # None below the last row
        old = self._hover_row.get_path() if self._hover_row and self._hover_row.valid() else None
        new = res[0] if res else None
        if old is not None and new is not None and old.compare(new) == 0:
            return     # same row: no repaint (scroll/mutations already damage)
        self._hover_row = Gtk.TreeRowReference.new(self.get_model(), new) if new else None
        self.queue_draw()

    def _hover_area(self):
        """Graphene.Rect of the hovered row's strip in widget coords, or None."""
        if not self._hover_row or not self._hover_row.valid():
            return None
        path = self._hover_row.get_path()
        cols = self.get_columns()
        if not path or not cols:
            return None
        # measured + gtktreeview.c (get_row_y_offset subtracts the scroll offset
        # itself): background rects arrive in VISIBLE/viewport coords already —
        # no get_visible_rect() conversion, that would double-subtract
        area = self.get_background_area(path, cols[0])
        if area is None or area.height <= 0:
            return None
        rect = Graphene.Rect()
        rect.init(area.x, area.y, area.width, area.height)
        return rect

    def do_snapshot(self, snapshot):
        area = self._hover_area()
        if area is not None:
            ok, rgba = self.get_style_context().lookup_color("alpaca-hover")
            if not ok:
                rgba = Gdk.RGBA(); rgba.parse("#111723")
            snapshot.append_color(rgba, area)
        Gtk.TreeView.do_snapshot(self, snapshot)
```

Then `alpaca_code/filetree.py`:
- Delete the entire `_HoverTree` class (line 60 `class _HoverTree(Gtk.TreeView):` through line 141 `Gtk.TreeView.do_snapshot(self, snapshot)`) and its trailing blank lines.
- Top imports: add `from .treehover import HoverTree` under `from . import badges`.
- Line 180: `self.view = _HoverTree(model=self.store, headers_visible=False)` → `self.view = HoverTree(model=self.store, headers_visible=False)`. Nothing else in filetree references the private name (grep `_HoverTree` after edit → zero hits).

- [ ] **Step 2: Run the suite (regression — a verbatim move must change nothing)**

Run: `python3 tests/test_selfcheck.py`
Expected: every registered test still PASS (widget code lives outside the suite; the move is file plumbing).

- [ ] **Step 3: gitview gets the hover band too**

In `alpaca_code/gitview.py` — under `from . import badges, gitstatus` add `from .treehover import HoverTree`, then replace the view construction:

```python
        self.view = HoverTree(model=self.store, headers_visible=False)
```

and directly under the store/view wiring (after the `row-activated`/`toggled` connects) add filetree's hover-refresh pair:

```python
        for sig in ("row-inserted", "row-deleted"):   # keep the band on the row under a stationary pointer
            self.store.connect(sig, lambda *a: self.view._refresh_hover())
```

- [ ] **Step 4: Run the suite (again, cheap)**

Run: `python3 tests/test_selfcheck.py`
Expected: every registered test PASS.

- [ ] **Step 5: Mode buttons + Stack + routing in `FileBrowser`**

5a. Replace the WORKSPACE label block:

```python
        self.append(Gtk.Label(label="WORKSPACE", xalign=0.0, margin_start=12, margin_bottom=2,
                              css_classes=["alpaca-section"]))
```

with:

```python
        # WORKSPACE / CHANGES mode tabs (spec §1): text-only section-label type.
        # has_frame(False) + the transparent .alpaca-tabbtn rule keep them bare
        # text; _set_mode moves the `alpaca-on` class as the selection.
        btns = Gtk.Box(spacing=10, margin_start=12, margin_top=6, margin_bottom=2)
        self.ws_btn = Gtk.Button(label="WORKSPACE")
        self.ch_btn = Gtk.Button(label="CHANGES")
        for b in (self.ws_btn, self.ch_btn):
            b.set_has_frame(False)
            b.set_css_classes(["alpaca-tabbtn"] + (["alpaca-on"] if b is self.ws_btn else []))
            btns.append(b)
        self.ch_btn.set_visible(False)   # outside-repo policy (spec §2); _refresh_status decides
        self.ws_btn.connect("clicked", lambda *_: self._set_mode("tree"))
        self.ch_btn.connect("clicked", lambda *_: self._set_mode("changes"))
        self.append(btns)
```

5b. Replace the plain tree-page append:

```python
        self.append(Gtk.ScrolledWindow(vexpand=True, child=self.view))
```

with:

```python
        tree_page = Gtk.ScrolledWindow(vexpand=True, child=self.view)
        # mode host (spec §1): zero-transition swap between the tree and the
        # changes view. vexpand on the Stack — GtkBox gives non-expanding
        # children only their minimum, and the card must fill below the tab row.
        self.changes = ChangesView()   # top import; window wires on_open/before_commit/on_refresh
        self.stack = Gtk.Stack(transition_type=Gtk.StackTransitionType.NONE, vexpand=True)
        self.stack.add_named(tree_page, "tree")
        self.stack.add_named(self.changes, "changes")
        self._mode = "tree"
        self.append(self.stack)
```

(`ChangesView` is imported at file top in Step 1's patch: add `from .gitview import ChangesView` under the `HoverTree` import. No cycle — see Interfaces.)

5c. Add the mode/search plumbing into `FileBrowser` (put the mode block right after `refresh_branch`):

```python
    # ---- mode tabs (spec §1) --------------------------------------------------
    def _set_mode(self, mode: str) -> None:
        """Swap the card between the file tree and the changes view. Entering
        CHANGES re-syncs the list (spec §2 refresh trigger: view becomes
        active); the selected tab's css class holds it bright, hover brightens
        the idle one."""
        if mode == self._mode:
            return
        self._mode = mode
        self.stack.set_visible_child_name(mode)
        for b, own in ((self.ws_btn, "tree"), (self.ch_btn, "changes")):
            b.set_css_classes(["alpaca-tabbtn"] + (["alpaca-on"] if mode == own else []))
        self.entry.set_placeholder_text("Search files…" if mode == "tree" else "Filter changes…")
        if mode == "changes":
            self.changes.refresh()

    def refresh_git(self) -> None:
        """Window-focus hook (window's notify::is-active): re-sync the statusbar
        and, when the changes view is open, the changes list."""
        self._refresh_status()
        if self._mode == "changes":
            self.changes.refresh()
```

and route the search by mode — change the top of `_on_search`:

```python
    def _on_search(self, entry) -> None:
        text = entry.get_text().strip()
        if self._mode == "changes":   # changes search never touches the file tree
            self.changes.filter(text)
            return
        if not text:
            self._populate_root()
            return
```

5d. CHANGES visible only in a repo — in `_refresh_status`, right after `for w in (self.branch_icon, …): w.set_visible(is_git)`:

```python
        self.ch_btn.set_visible(is_git)
        if not is_git and self._mode == "changes":
            self._set_mode("tree")    # repo vanished (HEAD deleted) while reading it
```

5e. Workspace swap resets both views of the card — in `set_root`, right after `self._populate_root()`:

```python
        self._set_mode("tree")        # a fresh workspace opens on the file tree (spec §1)
        self.changes.set_root(root)
```

(Ordering note: `entry.set_text("")` runs before this and may fire `_on_search` under the OLD mode — harmless: tree mode re-populates, changes mode filters an already-stale list that `_set_mode` + `changes.set_root` immediately replace.)

- [ ] **Step 6: CSS — replace the section rule in `alpaca_code/main.py`**

Delete (the class's only user, the WORKSPACE label, is gone):

```css
.alpaca-section { color: #8a93a6; font-size: 11px; font-weight: 500;
                  letter-spacing: 1px; margin-top: 6px; margin-bottom: 4px; }
```

and put in its place:

```css
/* workspace/changes mode buttons (spec §1): text-only section-label type —
   no frame fill; hover brightens, the selected mode's `alpaca-on` stays
   bright. min-height/padding sweep keeps Breeze's 32px button floor off a
   13px-ink label (app css is USER priority — wins). */
.alpaca-tabbtn { color: #8a93a6; font-size: 11px; font-weight: 500;
                 letter-spacing: 1px; min-height: 16px; padding: 0;
                 background: transparent; border: none; box-shadow: none;
                 outline: none; }
.alpaca-tabbtn:hover { color: #e6e8ee; }
.alpaca-tabbtn.alpaca-on { color: #e6e8ee; }
```

- [ ] **Step 7: Run the suite**

Run: `python3 tests/test_selfcheck.py`
Expected: every registered test PASS.

- [ ] **Step 8: Probe — `/tmp/probe_browsertabs.py` (numeric; both modes through the real FileBrowser)**

```python
import os, sys, tempfile, subprocess
sys.path.insert(0, "/home/dominykasm/FunProjects/misiuscode")
import alpaca_code.gi_env as ge
ge.require("Gtk", ("4.0",)); ge.require("Gdk", ("4.0",)); ge.require("GdkPixbuf", ("2.0",))
ge.require("Pango", ("1.0",)); ge.require("Graphene", ("1.0",))
from gi.repository import Gtk, GLib
Gtk.init()   # void on this build; never truthy-test
from alpaca_code.filetree import FileBrowser
from alpaca_code.treehover import HoverTree

def pump(n, ctx):
    for _ in range(n): ctx.iteration(may_block=False)

def g_names(gv):
    st = gv.store
    out, it = [], st.get_iter_first()
    while it is not None:
        out.append(st[it][0])
        it = st.iter_next(it)
    return out

fb = FileBrowser()
w = Gtk.Window(); w.set_child(fb); w.present()
ctx = GLib.MainContext.default()
pump(30, ctx)

# --- outside a repo: CHANGES hidden, mode pinned to the tree -----------------
with tempfile.TemporaryDirectory() as plain:
    fb.set_root(plain)
    pump(10, ctx)
    assert fb.ch_btn.get_visible() is False, "outside a repo CHANGES must hide (spec §2)"
    assert fb._mode == "tree" and fb.stack.get_visible_child_name() == "tree"
    assert isinstance(fb.view, HoverTree) and isinstance(fb.changes.view, HoverTree)
    assert "alpaca-on" in fb.ws_btn.get_css_classes()
    assert "alpaca-on" not in fb.ch_btn.get_css_classes()

# --- in a repo ----------------------------------------------------------------
with tempfile.TemporaryDirectory() as t:
    def g(*a): subprocess.run(["git", "-C", t, *a], capture_output=True, text=True, check=True)
    g("init", "-q"); g("config", "user.email", "t@t"); g("config", "user.name", "t")
    open(t + "/one.py", "w").write("a\n"); open(t + "/two.py", "w").write("b\n")
    g("add", "-A"); g("commit", "-q", "-m", "init")
    open(t + "/one.py", "w").write("a2\n")            # worktree change
    fb.set_root(t)
    pump(10, ctx)
    assert fb.ch_btn.get_visible() is True, "inside a repo CHANGES shows"

    fb.ch_btn.emit("clicked")                          # real signal path into _set_mode
    pump(20, ctx)
    assert fb._mode == "changes" and fb.stack.get_visible_child_name() == "changes"
    assert "alpaca-on" in fb.ch_btn.get_css_classes()
    assert "alpaca-on" not in fb.ws_btn.get_css_classes()
    assert g_names(fb.changes) == ["Select all", "one.py", "two.py"], g_names(fb.changes)
    assert all(fb.changes.store[i][7] for i in ("0", "1", "2"))   # select-all default
    # checkbox-vs-row split through the REAL browser copy (identity asserts are
    # banned — PyGObject wrappers — so this is the split's behavioral twin)
    changed = []
    fb.changes.on_open = lambda rel, letter: changed.append((rel, letter))
    fb.changes._on_activated(fb.changes.view, Gtk.TreePath.new_from_string("1"),
                             fb.changes._toggle_col)
    assert changed == []                     # checkbox click never opens a diff
    fb.changes._on_activated(fb.changes.view, Gtk.TreePath.new_from_string("1"),
                             fb.changes.view.get_columns()[0])
    assert changed == [("one.py", "M")], changed

    # search routes by mode: changes view filters, file tree untouched
    fb.entry.set_text("one")
    pump(10, ctx)
    assert g_names(fb.changes) == ["Select all", "one.py"], g_names(fb.changes)
    assert len(fb.changes._files) == 2                 # selection set untouched (view-only filter)
    fb.entry.set_text("")
    pump(10, ctx)
    assert g_names(fb.changes) == ["Select all", "one.py", "two.py"]

    # focus hook target re-syncs the list
    open(t + "/two.py", "w").write("b2\n")             # second change
    fb.refresh_git()
    pump(10, ctx)
    assert fb.changes._files == {"one.py", "two.py"}, fb.changes._files

    # back to workspace: file-tree search still routes through scan_project
    fb.ws_btn.emit("clicked")
    pump(20, ctx)
    assert fb._mode == "tree" and "alpaca-on" in fb.ws_btn.get_css_classes()
    fb.entry.set_text("one")          # tree-mode search: scan_project route
    pump(10, ctx)
    assert g_names(fb) == ["one.py"], g_names(fb)      # flat rel-path search rows
print("PROBE OK")
```

Notes for the executor: `g_names` walks only top rows — tree-mode search results are flat by construction, and the whole second branch runs inside the `t` (repo) lifetime, where `set_root(t)` populated the file tree with one.py/two.py. The tree-mode clear (`_on_search("")` repopulating from disk) is filetree's already-shipped, already-probed behavior; not re-probed here.

- [ ] **Step 9: Real-restart visual pass (spec §1 polish)**

`python3 /tmp/probe_browsertabs.py` → `PROBE OK`, then on this project's real window (`gitStatus`-clean repo, dirty files staged for effect): kill the old `bin/alpaca-code` process by exact pid (single-instance forwards CSS changes to the old process otherwise — CLAUDE.md), relaunch, and check with eyes:

1. WORKSPACE reads as bare muted text; hovering it brightens; it STAYS bright while the file tree shows.
2. CHANGES sits right of it, hidden in a non-repo workspace, visible here.
3. Click CHANGES → list swaps with no slide/fade (zero transition), WORKSPACE idles back to muted, CHANGES holds bright.
4. Hovering rows in the changes list paints the same hover band as the file tree (HoverTree shared).
5. Search box routes per mode (placeholder "Filter changes…" while in CHANGES).

- [ ] **Step 10: Commit**

```bash
git add alpaca_code/treehover.py alpaca_code/filetree.py alpaca_code/gitview.py alpaca_code/main.py
git commit -m "browser: WORKSPACE/CHANGES tabs + zero-transition stack; HoverTree module"
```

(Automation steps in this session stay subject to per-step approval — this file lists them for the executor role, but every git commit waits for your explicit yes.)

### Task 5: Window wiring (window.py) — diff opener, commit flush, focus re-sync

**Files:**
- Modify: `alpaca_code/window.py:126-133` (wiring block), new `_open_changes_diff` method

**Interfaces:**
- Consumes: `ChangesView.on_open(rel, letter)` / `.before_commit(paths)` / `.on_refresh()` (Task 3); `FileBrowser.refresh_git()` + `.changes` + `ch_btn/ws_btn/_mode` (Task 4); `Editor.open_diff(rel, sidetuple, letter)`, `Editor.save_open(paths)`, `Editor._error(primary, secondary)` (Task 2); `gitstatus.diff_for(root, rel, is_untracked=False) -> (text, is_binary)`, `parse_unified(text)`, `build_sides(hunks) -> 5-tuple` (Task 1).
- Produces: `Window._open_changes_diff(rel: str, letter: str) -> None` — one method, nothing exported. `set_workspace` needs NO editing: its existing `editor.restore(path, …)` path ends in `set_root → _close_all`, and Task 2 closed that loop (diff pages die with the workspace); `tree.set_root(path)` already resets the browser card after Task 4.

No new suite test: wiring runs against the real window (widgets), which stays probe-only per CLAUDE.md; the pure halves it composes (diff_for/parse/build_sides, save_open/open_diff machinery) are suite-covered by Tasks 1 and 2.

- [ ] **Step 1: Wire the changes view's three callbacks + the focus hook**

In `Window.__init__`, directly under the existing `self.tree.on_open = …` line:

```python
        from .filetree import FileBrowser
        self.tree = FileBrowser()
        self.tree.on_open = lambda p: self.editor.open_file(p) if self.editor else None
```

insert:

```python
        # Changes-view wiring (spec §5): the browser HOSTS ChangesView; the
        # window owns its side effects. on_open carries (rel, letter) — the
        # letter drives the tab chip and the untracked-diff branch.
        self.tree.changes.on_open = self._open_changes_diff
        self.tree.changes.before_commit = self.editor.save_open   # flush dirty buffers for exactly the checked files
        self.tree.changes.on_refresh = self.tree.refresh_branch   # statusbar re-syncs after commit/push
```

and replace the focus hook:

```python
        self.win.connect("notify::is-active",
                         lambda *_a: self.tree and self.tree.refresh_git())
```

(was `refresh_branch()` — Task 4's `refresh_git` supersedes it: it re-syncs the statusbar AND an open changes view; spec §2 refresh triggers.)

- [ ] **Step 2: Add `Window._open_changes_diff`**

Place it right after `persist_tabs` (before the `# --- run wiring ---` comment):

```python
    # --- changes view → editor (spec §5) ----------------------------------------
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
```

(Branches: `letter == "U"` → untracked path diff via `--no-index /dev/null` (works with no HEAD too); binary files get a notice, never a blank page; a path with no hunks (e.g. mode-only change this diff format ignores) gets a notice rather than an empty tab.)

- [ ] **Step 3: Probe — `/tmp/probe_e2e.py` (real window in a real temp repo, full flow, numeric asserts)**

```python
import os, sys, tempfile, subprocess, signal, time
sys.path.insert(0, "/home/dominykasm/FunProjects/misiuscode")
import alpaca_code.gi_env as ge
ge.require("Gdk", ("4.0",)); ge.require("Gtk", ("4.0",)); ge.require("GtkSource", ("5",))
ge.require("Vte", ("4", "4.0", "3.91")); ge.require("Pango", ("1.0",))
ge.require("GdkPixbuf", ("2.0",)); ge.require("Graphene", ("1.0",))
from gi.repository import Gtk, GLib
Gtk.init()   # void on this build

def pump(n):
    ctx = GLib.MainContext.default()
    for _ in range(n): ctx.iteration(may_block=False)

# state.json must survive the probe untouched: stub the WRITER (window/editor
# call it through module lookups at call time, so the stub binds everywhere).
from alpaca_code import state
state_writes = []
state.save = lambda d: state_writes.append(d.get("last_project"))

def gvrows(gv):
    st = gv.store
    out, it = [], st.get_iter_first()
    while it is not None:
        out.append((st[it][0], st[it][2], st[it][7]))
        it = st.iter_next(it)
    return out

def g(root, *a):
    subprocess.run(["git", "-C", root, *a], capture_output=True, text=True, check=True)

with tempfile.TemporaryDirectory() as t:
    g(t, "init", "-q"); g(t, "config", "user.email", "t@t"); g(t, "config", "user.name", "t")
    open(t + "/one.py", "w").write("a\n"); open(t + "/two.py", "w").write("b\n")
    g(t, "add", "-A"); g(t, "commit", "-q", "-m", "init")
    open(t + "/one.py", "w").write("a2\n")     # worktree change (tracked)
    open(t + "/two.py", "w").write("b2\n")     # second worktree change

    from alpaca_code import window as wmod
    win = wmod.Window(t)          # spawns a real window; panes spawn claude — killed in teardown
    win.win.present(); pump(40)   # idle set_root + _ensure_current land

    assert win.tree._mode == "tree" and win.tree.ch_btn.get_visible() is True
    win.tree.ch_btn.emit("clicked")            # mode switch through the real button signal
    pump(20)
    assert win.tree.stack.get_visible_child_name() == "changes"
    assert [r[0] for r in gvrows(win.tree.changes)] == ["Select all", "one.py", "two.py"]
    assert all(r[2] for r in gvrows(win.tree.changes)), gvrows(win.tree.changes)

    # row click → diff page in the editor (spec §3 via §5): tracked AND untracked branches
    col0 = win.tree.changes.view.get_column(0)
    win.tree.changes._on_activated(win.tree.changes.view, Gtk.TreePath.new_from_string("1"), col0)
    pump(30)
    assert win.editor._page_of("diff:one.py") != -1
    assert win.editor.nb.get_current_page() == win.editor._page_of("diff:one.py")
    win.tree.changes._on_activated(win.tree.changes.view, Gtk.TreePath.new_from_string("2"), col0)
    pump(30)
    assert win.editor._page_of("diff:two.py") != -1          # letter "U" → no-index branch
    assert win.editor.get_open_state()["open_tabs"] == []    # diff pages never persist

    # subset commit through the real bar (Review Focus: commit --only semantics)
    win.tree.changes._on_toggled(None, "1")                  # uncheck one.py
    assert win.tree.changes._checked == {"two.py"}
    win.tree.changes.msg.set_text("e2e two")
    win.tree.changes.commit_clicked()                        # before_commit → editor.save_open([])
    for _ in range(300):
        pump(5); time.sleep(0.005)
        if not win.tree.changes._busy: break
    assert win.tree.changes._busy is False
    assert subprocess.run(["git", "-C", t, "show", "--name-only", "--format="],
                          capture_output=True, text=True).stdout.split() == ["two.py"], \
        "commit carried more than the selection"
    # post-commit refresh (keep_selection=True → intersect): selection empties
    assert [r[0] for r in gvrows(win.tree.changes)] == ["Select all", "one.py"]
    assert win.tree.changes._checked == set()
    assert win.tree.git_label.get_text() == "1 changed", win.tree.git_label.get_text()

    # focus wiring target re-syncs the list
    win.tree.refresh_git()
    assert win.tree.changes._files == {"one.py"}, win.tree.changes._files

    # back to workspace; a REAL file page works and persists like always
    win.tree.ws_btn.emit("clicked"); pump(20)
    assert win.tree.stack.get_visible_child_name() == "tree"
    win.editor.open_file(os.path.join(t, "two.py"))
    assert win.editor.get_open_state()["open_tabs"] == ["two.py"], \
        win.editor.get_open_state()

    # workspace switch: diff pages die with the old project (editor.restore → _close_all)
    with tempfile.TemporaryDirectory() as t2:
        subprocess.run(["git", "-C", t2, "init", "-q"], capture_output=True, check=True)
        open(t2 + "/other.py", "w").write("o\n")
        win.set_workspace(t2)
        pump(40)
        assert win.editor.get_open_state()["open_tabs"] == ["other.py"]
        assert win.editor._page_of("diff:one.py") == -1
        assert win.editor._page_of(os.path.join(t, "two.py")) == -1
        assert win.tree._mode == "tree"
        assert win.tree.changes._files == {"other.py"}, win.tree.changes._files

    # teardown: pane children are session leaders — kill before the probe exits
    win.panes.set_root(None)      # _kill_tree SIGHUP on agent/term/output, exits swallowed
    pump(30); time.sleep(1.0); pump(10)
    for term in (win.panes.agent, win.panes.term, win.panes.out):
        pid = getattr(term, "pid_holder", None)
        if pid:
            try: os.killpg(os.getpgid(pid), signal.SIGKILL)
            except (ProcessLookupError, PermissionError): pass

assert set(state_writes) == {t, t2}, state_writes   # stub saw ONLY probe workspaces
print("PROBE OK")
```

(T2's scope-out happens after teardown; `state_writes` is read back in the outer scope — both dirs are in it. `git status` inside the probe needs no `check=True` on failures we don't drive.)

Run: `python3 /tmp/probe_e2e.py` → `PROBE OK`.

- [ ] **Step 4: Full suite**

Run: `python3 tests/test_selfcheck.py`
Expected: every registered test PASS.

- [ ] **Step 5: Real-restart visual pass (everything ships only after eyes on it)**

Kill the running `bin/alpaca-code` process by exact pid (a second spawn forwards to the
running primary and the user keeps the OLD CSS), relaunch with this project open, then:

1. WORKSPACE/CHANGES: text-only, hover brighten, selection holds bright (Task 4 checklist).
2. CHANGES lists this repo's real dirty files with letter chips; Select all row on top, tri-state on a partial selection.
3. Click a file → side-by-side diff opens in the editor card: added/removed tints, syntax highlighting, no line numbers, hunk header rows dim on both sides.
4. Check a subset, type a message, Commit → bar result line goes green, statusbar count updates, diff list re-syncs.
5. Push → red result without a remote (expected on this repo), no hang, buttons re-enable.
6. Window focus away and back → list re-syncs.

- [ ] **Step 6: Commit the feature**

```bash
git add -A
git commit -m "changes view: wire diff opener, commit flush and focus re-sync into the window" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

(Executors: every commit in every task awaits an explicit per-step go in this session — the standing rule from the spec-commit denial, plus the trailer on every one of those commits.)

---
