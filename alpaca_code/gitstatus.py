"""Branch from .git/HEAD (file-read); dirty count needs the git binary.
Pure — no gi imports."""
import os
import re
import subprocess

def branch_of(root: str) -> str | None:
    if not root:
        return None
    try:
        with open(os.path.join(root, ".git", "HEAD"), encoding="utf-8") as f:
            content = f.read().strip()
    except OSError:
        return None
    if content.startswith("ref: refs/heads/"):
        return content[len("ref: refs/heads/"):]
    return None  # detached HEAD / worktree pointer → hide branch widget

def status(root: str) -> int | None:
    """Count of dirty paths (`git status --porcelain`); None outside a repo /
    when git is missing or times out. Calls must be cheap-and-off-thread enough —
    this is invoked from UI refresh, so keep it rare."""
    if not root or not os.path.isdir(os.path.join(root, ".git")):
        return None
    try:
        r = subprocess.run(["git", "-C", root, "status", "--porcelain",
                            "--untracked-files=normal"],
                           capture_output=True, text=True, timeout=5)
    except (OSError, subprocess.TimeoutExpired):
        return None
    if r.returncode != 0:
        return None
    return sum(1 for line in r.stdout.splitlines() if line.strip())

# --- changes view ops (spec 2026-10-05) -------------------------------------------

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
        if "U" in xy:                         # unmerged (UU/AA/DD/AU/UA/DU/UD): every
            chosen = "C"                      # unmerged code has a U — != untracked "?"
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
    # `add` is only needed to bring untracked/edited WORKTREE content into the
    # index; a path existing solely as a staged removal (git rm/mv/rename)
    # refuses `git add` (fatal pathspec, rc 128 measured) and needs no add —
    # the pathspec is already staged. Keep the FULL list for --only; add only
    # what exists (lexists keeps a dangling symlink's typechange addable).
    present = [p for p in paths if os.path.lexists(os.path.join(root, p))]
    try:
        if present:
            r = subprocess.run(["git", "-C", root, "add", "--", *present], env=env,
                               capture_output=True, text=True, timeout=30)
            if r.returncode != 0:
                return (False, ((r.stderr or "git add failed").strip() or "git add failed")[:400])
        r = subprocess.run(["git", "-C", root, "commit", "--only", "-m", msg, "--", *paths],
                           env=env, capture_output=True, text=True, timeout=60)
        if r.returncode != 0:
            return (False, ((r.stderr or "git commit failed").strip() or "git commit failed")[:400])
    except (OSError, subprocess.TimeoutExpired) as e:
        return (False, f"git failed: {e}")
    if r.returncode != 0:
        return (False, ((r.stderr or "git commit failed").strip() or "git commit failed")[:400])
    first = (r.stdout or "").strip().splitlines()
    return (True, (first[0] if first else "Committed.")[:400])

def ahead(root: str) -> int:
    """Local commits not on the upstream (`rev-list --count @{u}..HEAD`).
    No upstream / not a repo / timeouts → 0 (push() handles the -u bootstrap).
    Cheap (--count) and called from refresh() only, never per toggle-click."""
    if not root or not os.path.isdir(os.path.join(root, ".git")):
        return 0
    try:
        r = subprocess.run(["git", "-C", root, "rev-list", "--count", "@{upstream}..HEAD"],
                           capture_output=True, text=True, timeout=5)
    except (OSError, subprocess.TimeoutExpired):
        return 0
    try:
        return int((r.stdout or "").strip()) if r.returncode == 0 else 0
    except ValueError:
        return 0

def commit_then_push(root: str, paths: list[str], msg: str,
                     phase=None) -> tuple[bool, str, bool, str]:
    """Commit exactly `paths`, then — only when the commit landed — push.
    Empty `paths` = push-only (the auto-detect button rule: a checked nothing
    with unpushed commits). `phase(kind, text)` fires once mid-flight just
    before the push (worker-thread context; the UI marshals to the mainloop).
    Returns (commit_ok, commit_text, push_ok, push_text); push fields are
    (False, "") when no push was attempted."""
    cok, ctext = commit(root, paths, msg) if paths else (True, "")
    if not cok:
        return (False, ctext, False, "")
    if paths and phase:
        phase("busy", "Pushing…")
    pok, ptext = push(root)
    return (True, ctext, pok, ptext)

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