# Alpaca-code Rust Port — Stage 2 (git features) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to
> implement this plan task-by-task. The user's established execution method is
> **native inline** (executing-plans, decided for stage 1). Steps use checkbox
> (`- [ ]`) syntax for tracking. `superpowers:test-driven-development` is loaded
> at setup; `superpowers:systematic-debugging` is loaded on any red that isn't
> a spec-expected fail.

**Goal:** Port stage 2 — the git feature surface (status parser, CHANGES view +
commit bar, branch pill + Switch/New Branch popover, left-panel mode strip +
live probe + HEAD monitor, editor side-by-side diff pages) — into
`alpaca-code-rs/`, keeping the python app untouched as the UI reference.

**Architecture:** Pure git logic lands in `gitstatus.rs` (no gi imports,
displayless-testable). Hover-tree infra moves to `treehover.rs` with a
press-position capture (the changes view hit-tests toggle x-geometry with it).
`gitview.rs` hosts the CHANGES view as its own component; `branchmenu.rs` the
status-bar pill; `gitpanel.rs` the host that pairs the file tree with the
changes view (shared search entry, mode strip, status row) and swaps into
app.rs where the bare file tree lived. `editor.rs` grows diff pages
(self-fetching via spawn_command flights, editor stays git-subprocess-aware
only through the `gitstatus` pure API). app.rs carries the commit-flush chain.

**Tech Stack:** relm4 0.11, gtk4-rs 0.11 (V4_22 feature), sourceview5 0.11,
vte4 0.10, glib/gio 0.22, gdk-pixbuf 0.22. **No new crates** — no regex;
hand-roll the hunk-header parser.

**Spec:** `docs/superpowers/specs/2026-10-07-rust-port-design.md` §Stage 2 AND
`docs/superpowers/specs/2026-10-05-git-changes-view-design.md`. **Where the
live python code differs from those specs, the python code wins** — it was
re-verified line-by-line immediately before this plan was written
(gitstatus.py 313 L, gitview.py 360 L, branchmenu.py 234 L, filetree.py's
FileBrowser, editor.py diff regions, window.py wiring). Port each referenced
python method verbatim; line citations in the tasks point at the governing code.

## Global Constraints

Every task implicitly includes these. Exact values copied from the verified
sources:

- **NO git commits** (standing user waiver, "Looks good, ignore commit steps").
  No `git add`/`git commit` anywhere; task-done ledger lines are the records.
- Python code is untouched: nothing outside `alpaca-code-rs/` changes except
  `CLAUDE.md` (T8) and this repo's own docs.
- App-id stays `io.alpaca.rs`; shared `~/.config/alpaca-code/state.json` is the
  sequential contract with python (last writer wins — never launch both apps on
  the same project concurrently and toggle tabs). S2 adds nothing to the schema:
  diff tabs are excluded from snapshots already (path is None on diff pages).
- Displayless gate — every task: `cargo test --manifest-path
  alpaca-code-rs/Cargo.toml`; `cargo build` (same manifest) before any relaunch,
  because `cargo test` does NOT refresh the runnable binary.
- Timers: `glib::timeout_add(Duration::from_millis(...))` (or
  `timeout_add_local`) only — `timeout_add_seconds` never fires under
  `app.run()` on this box (twice measured). **Never `SourceId::remove`** on a
  possibly-fired id — it PANICS on already-fired ids where python's
  `source_remove` is a silent no-op (measured law, editor.rs comment). One-shot
  and debounce timers use **generation counters**: every timer captures the
  counter's current value at schedule time and no-ops unless it still matches;
  cancel = increment. relm4-idiomatic shape: timer closures carry a cloned
  `Sender<ComponentInput>` and emit an INPUT; the handler checks the generation
  and mutates state. Pulse/submenu/scroll-frac timers work this way.
- Git contracts (python-exact, gitstatus.py): every call is `git -C root`;
  `GIT_TERMINAL_PROMPT=0` ONLY on checkout/commit/add/push; per-call timeouts —
  5s branches/ahead, 10s changes status, 15s `git_out` default, 30s add, 60s
  checkout/commit, 120s push; error text = trimmed, first 400 CODE POINTS
  (`chars().take(400)`, python counts code points); `_git_out` accepts rc 0 and
  1 (no-index compares return 1 on differences); **never panic / never raise on
  git failure** — return the error state instead.
- No new crates. No regex — the unified-diff hunk-header parser is hand-rolled.
- CSS: every S2 class (`alpaca-diffchip*`, `alpaca-diffsep`, `alpaca-commitbar`,
  `alpaca-msg`, `alpaca-barbtn`, `alpaca-modetabs`, `alpaca-tabbtn`,
  `alpaca-status-dot/ok/warn/err`, `alpaca-statusbar`, `alpaca-branchbtn`,
  `alpaca-branchitem`, `alpaca-on`, `alpaca-hint`, `alpaca-open`, `alpaca-tree`)
  is already verbatim in `alpaca-code-rs/assets/app.css`. Tasks must USE these,
  never restyle or add selectors.
- Tree/store laws (filetree.rs / gitview.rs, measured): the changes store is
  REBUILT per refresh (filetree's invariant: value-only writes are safe under
  expanded rows, structural mutation visually collapses them — surgical
  patching is off the table here); cell renderer props cache at insert — set
  xpad/ypad/yalign/ellipsize BEFORE any store fill; on this build a per-row
  hidden cell still keeps its column slot but drops its width — chip column
  cells must go per-row invisible; toggles are NOT activatable (`Activatable`
  setting off) — clicks are routed by x-geometry (T2/T3 `_toggle_hit`).
- Test hygiene: repo-local git config in fixtures
  (`git config user.email/user.name/commit.gpgsign false` inside the scratch
  repo), never process env (rust 2024 `env::set_var` is unsafe + parallel-test
  races). Tests run against the real git binary in `std::env::temp_dir()`.
- relm4 idioms (editor.rs/filetree.rs precedent): `#[relm4::component(pub)]`,
  init epilogue `let widgets = view_output!();` then `ComponentParts` (no
  `root` field), `T::builder().launch(x).connect_receiver(cb)` (returns the
  Controller, no detach), `type CommandOutput` + `fn update_cmd(message,
  sender, root)` declared IN the `#[relm4::component]` impl, `ComponentSender<C>`
  is generic over the COMPONENT, flights via
  `sender.spawn_command(move |out| ...)` (impl = spawn_blocking) landing in
  `update_cmd` on the main thread — no raw `std::thread` in components.
- `#![allow(deprecated)]` blocks precede the `use` items they permit
  (whole-block placement works; inner placement must precede every item, E0442).
- Widget-layer behavior beyond the suite's reach is gated by cargo build + the
  T8 probe (env-gated programmatic flight + screenshot) — S1 precedent: the
  respawn-band bug shipped past 21 green unit tests because the bookkeeping was
  widget-layer; probes, not print-trust, are the live gate. Steps below pin
  which probe covers which review class.

## Review Focus

The input classes the spec implies that no task's unit tests can exercise —
each is deliberately probed at T8 (or pinned by the named unit test):

- **Workspace switch mid-flight** (commit/push or branch flight; probe lands
  between SetWorkspace and Restore): every flight snapshots `root` at click
  time (python `review I4`); the landing must target only the snapshotted root
  and never a switched-away workspace; a commit landed on a stale root must not
  paint onto the new workspace's status row. Pinned by the T8
  `ALPACA_PROBE_S2` flight, which injects a real `SetWorkspace` mid-commit and
  asserts exactly one commit on the scratch repo, zero on the switched-to repo.
- **Flushed-ack-always contract**: the commit chain is Editor `SaveOpen` →
  unconditional `Flushed` output → gitpanel `StartCommit`. `save_open` swallows
  write errors (python leaves OSError unguarded and would kill the chain mid
  flush; rust deliberately catches — ledger ruling at execution) and emits
  `Flushed` even with zero dirty pages so a clean commit can never strand the
  eager-busy state. Reviewer: check no early-return path skips the `Flushed`.
- **Non-repo / unborn HEAD / root=None**: `.git` missing → `changes` None,
  CHANGES pill hidden, mode strip forced back to tree, branch pill hidden,
  `push` → "(Not a git repository"); unborn HEAD → diff falls back to
  `--cached` then `--no-index /dev/null`. Pinned by T1 guard tests + T8
  screenshot on a non-git dir.
- **Porcelain edge cases**: `-z` rename twins (R row + synthetic D twin on the
  ORIGINAL path), unmerged `U`-code classes → letter C, short/truncated
  records, trailing NUL — T1 `parse_porcelain` synthetic tests pin every branch.
- **Unborn-HEAD diff chain**: T1 `diff_for` tests pin HEAD-fail → `--cached`
  → `--no-index /dev/null` (all-added) and the binary sniff.
- **Busy-gate interplay**: the probe tick skips while a git flight owns the
  status row (git_busy / probe_busy / changes busy); an ok pulse re-syncs the
  row and must not survive a following state. The T8 double-commit probe pins
  exactly-one-commit; the pulse's generation-counter no-panic path runs inside
  the same probe window.

---

### Task 1: `gitstatus.rs` — pure git parser + subprocess helpers

**Files:**
- Create: `alpaca-code-rs/src/gitstatus.rs`
- Modify: `alpaca-code-rs/src/main.rs` (add `mod gitstatus;`)
- Test: in-module `#[cfg(test)]` (scratch git repos in `std::env::temp_dir()`)

**Interfaces:**
- Consumes: nothing (leaf module, std only).
- Produces: `pub type Row = (String, String)`; `pub type Rows = Vec<Row>`;
  `pub enum GitStatusKind { Busy, Ok, Err }` (derive Clone, Copy, Debug,
  PartialEq); `pub struct TreeRow { pub kind: &'static str, pub name: String,
  pub rel: String, pub letter: String, pub depth: usize }`;
  `pub struct Hunk { pub lines: Vec<(char, String)> }`;
  `pub struct Sides { pub old: Vec<String>, pub new: Vec<String>,
  pub del: BTreeSet<usize>, pub add: BTreeSet<usize>, pub hdr: BTreeSet<usize>, }`
  (derive Clone, Debug); `pub struct FlightOut { pub cok: bool, pub ctext:
  String, pub pok: bool, pub ptext: String }` (derive Clone, Debug);
  `pub const HUNK_ROW: &str = "@@ … @@"`; and the functions below. Later
  tasks import `crate::gitstatus::*` — exact names:
  - `pub fn branch_of(root: &str) -> Option<String>`
  - `pub fn branches(root: &str) -> Option<Vec<String>>`
  - `pub fn switch(root: &str, name: &str) -> (bool, String)`
  - `pub fn create_switch(root: &str, name: &str) -> (bool, String)`
  - `pub fn changes(root: &str) -> Option<Rows>`
  - `pub fn parse_porcelain(stdout: &[u8]) -> Option<Rows>` (pure, split out
    of `changes` so -z edge cases are unit-testable without a repo)
  - `pub fn parse_unified(text: &str) -> Vec<Hunk>`
  - `pub fn build_sides(hunks: &[Hunk]) -> Sides`
  - `pub fn git_out(root: &str, args: &[&str], timeout_s: u64) -> Option<Vec<u8>>`
    (python `_git_out` — private there, pub here because tests in this module
    and no other consumer needs it)
  - `pub fn diff_for(root: &str, rel: &str, is_untracked: bool)
    -> Option<(String, bool)>`
  - `pub fn commit(root: &str, paths: &[String], msg: &str) -> (bool, String)`
  - `pub fn ahead(root: &str) -> usize`
  - `pub fn push(root: &str) -> (bool, String)`
  - `pub fn commit_then_push(root: &str, paths: &[String], msg: &str,
    phase: Option<&(dyn Fn() + Send + Sync)>) -> FlightOut`
  - `pub fn group_tree(rows: &Rows) -> Vec<TreeRow>`
  - `pub fn checked_dir_state(rows: &[TreeRow],
    checked: &BTreeSet<String>) -> BTreeMap<String, (bool, bool)>`

- [ ] **Step 1: Write the failing tests**

Append to `alpaca-code-rs/src/main.rs`'s module list (`mod gitstatus;`) and
create `src/gitstatus.rs` with the test module first. Fixture helpers (repo-LOCAL
config per Global Constraints):

```rust
//! Git surface — branch/status/porcelain/diff/commit/push, python
//! `alpaca_code/gitstatus.py` port. Pure (no gi imports): safe to run and test
//! displayless. Every call is `git -C root`; never raises (returns error states).

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub type Row = (String, String);
pub type Rows = Vec<Row>;

/// Status-row vocabulary (python's on_status kinds) — lives here so
/// gitview/branchmenu/gitpanel share it without import cycles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitStatusKind {
    Busy,
    Ok,
    Err,
}

pub const HUNK_ROW: &str = "@@ … @@";

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir();
        let p = d.join(format!(
            "alpaca-gittest-{}-{}-{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// Fixture git call, panicking INSIDE the test only (assert keeps the
    /// production-never-panic law honest for production paths).
    fn git(repo: &Path, args: &[&str]) {
        let out = Command::new("git").arg("-C").arg(repo).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "fixture git {:?} failed: {} / {}",
            args,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn repo(tag: &str) -> PathBuf {
        let p = scratch(tag);
        git(&p, &["init", "-q", "-b", "main"]);
        git(&p, &["config", "user.email", "t@example.com"]);
        git(&p, &["config", "user.name", "t"]);
        git(&p, &["config", "commit.gpgsign", "false"]);
        p
    }

    fn w(repo: &Path, rel: &str, body: &str) {
        let p = repo.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, body).unwrap();
    }

    fn rows_of(v: &[(&str, &str)]) -> Rows {
        v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn branch_of_reads_head() {
        let p = repo("branch");
        w(&p, "f.txt", "x");
        // name comes from the repo itself — never assume the default branch name
        let name = String::from_utf8(
            Command::new("git").arg("-C").arg(&p)
                .args(["symbolic-ref", "--short", "HEAD"])
                .output().unwrap().stdout,
        ).unwrap().trim().to_string();
        assert_eq!(branch_of(p.to_str().unwrap()).as_deref(), Some(name.as_str()));
        git(&p, &["checkout", "-q", "--detach"]);
        assert_eq!(branch_of(p.to_str().unwrap()), None, "detached HEAD hides the pill");
        // detached writes a raw sha — HEAD is no longer "ref:…" but stays readable
        std::fs::write(p.join(".git").join("HEAD"), b"junk without prefix\n").unwrap();
        assert_eq!(branch_of(p.to_str().unwrap()), None);
        std::fs::write(p.join(".git").join("HEAD"), b"ref: refs/heads/feature/x\n").unwrap();
        assert_eq!(branch_of(p.to_str().unwrap()).as_deref(), Some("feature/x"));
        clean(&p);
        assert_eq!(branch_of("/no/such/root"), None);
    }

    #[test]
    fn branches_sorted_refname() {
        let p = repo("branches");
        w(&p, "f.txt", "x");
        git(&p, &["add", "."]);
        git(&p, &["commit", "-q", "-m", "c"]);
        git(&p, &["checkout", "-q", "-b", "feat/b"]);
        git(&p, &["checkout", "-q", "-b", "feat/a"]);
        let bs = branches(p.to_str().unwrap()).unwrap();
        let bi = bs.iter().position(|b| b == "feat/a").unwrap();
        let bj = bs.iter().position(|b| b == "feat/b").unwrap();
        assert!(bi < bj, "refname sort: {:?}", bs);
        assert!(bs.iter().any(|b| b == "main" || b == "master"));
        assert_eq!(branches("/no/such/root"), None);
    }

    #[test]
    fn switch_guards() {
        let p = repo("switch");
        assert_eq!(switch(p.to_str().unwrap(), ""), (false, "Bad branch name".into()));
        assert_eq!(switch(p.to_str().unwrap(), "-x"), (false, "Bad branch name".into()));
        assert_eq!(create_switch(p.to_str().unwrap(), "-b"), (false, "Bad branch name".into()));
        let (ok, text) = create_switch(p.to_str().unwrap(), "newbie");
        assert!(ok, "checkout -b failed: {text}");
        assert_eq!(branch_of(p.to_str().unwrap()).as_deref(), Some("newbie"));
    }

    #[test]
    fn changes_letters_and_untracked() {
        let p = repo("letters");
        w(&p, "m.txt", "one\n");
        w(&p, "u1.txt", "x");
        w(&p, "untracked/z.txt", "y");
        git(&p, &["add", "u1.txt"]);
        w(&p, "m.txt", "two\n");
        let got = changes(p.to_str().unwrap()).unwrap();
        let want = rows_of(&[("m.txt", "M"), ("u1.txt", "A"),
                             ("untracked/z.txt", "U")]);
        assert_eq!(got, want);
    }

    #[test]
    fn changes_rename_twin() {
        let p = repo("rename");
        w(&p, "old.txt", "hello");
        git(&p, &["add", "."]);
        git(&p, &["commit", "-q", "-m", "c"]);
        git(&p, &["mv", "old.txt", "new.txt"]);
        let got = changes(p.to_str().unwrap()).unwrap();
        assert_eq!(
            got,
            rows_of(&[("new.txt", "R"), ("old.txt", "D")]),
            "-z rename pair: new path + ORIGINAL path (D twin)"
        );
    }

    #[test]
    fn changes_unmerged_is_c() {
        let p = repo("unmerged");
        w(&p, "f.txt", "base");
        git(&p, &["add", "."]);
        git(&p, &["commit", "-q", "-m", "base"]);
        git(&p, &["checkout", "-q", "-b", "side"]);
        w(&p, "f.txt", "side\n");
        git(&p, &["commit", "-qam", "side"]);
        git(&p, &["checkout", "-q", "main"]);
        w(&p, "f.txt", "main\n");
        git(&p, &["commit", "-qam", "main"]);
        let _ = Command::new("git").arg("-C").arg(&p).args(["merge", "side"]).output().unwrap();
        let got = changes(p.to_str().unwrap()).unwrap();
        assert_eq!(got, rows_of(&[("f.txt", "C")]), "UU → C");
    }

    #[test]
    fn parse_porcelain_synthetic() {
        // truncated record skipped; ? → U; U-codes → C; rename pair order
        let bytes = b"?? one.txt\0UU f.txt\0R  new.bin\0old.bin\0A  \0 \0?  two.txt\0";
        let got = parse_porcelain(bytes).unwrap();
        let want = rows_of(&[
            ("f.txt", "C"),
            ("new.bin", "R"),
            ("old.bin", "D"),
            ("one.txt", "U"),
            ("two.txt", "U"),
        ]);
        assert_eq!(got, want);
        assert_eq!(parse_porcelain(&[]).map(|r| r.len()), Some(0));
    }

    #[test]
    fn changes_guards() {
        assert_eq!(changes(""), None);
        assert_eq!(changes("/no/such/root"), None); // no .git dir
        let notrepo = scratch("notrepo");
        assert_eq!(changes(notrepo.to_str().unwrap()), None); // exists, not a repo
    }

    #[test]
    fn unified_and_sides_exact() {
        let text = "diff --git a/f b/f\n--- a/f\n+++ b/f\n\
                    @@ -1,2 +1,2 @@\n keep\n-gone\n+add\n keep2\n";
        let hunks = parse_unified(text);
        assert_eq!(hunks.len(), 1);
        let s = build_sides(&hunks);
        assert_eq!(s.old.len(), s.new.len(), "equal lengths by construction");
        // hunk row first, then context/remove/add/context
        assert_eq!(s.old[0], HUNK_ROW);
        assert!(s.hdr.contains(&0), "hunk-row index 0 on both sides");
        // idx1 '=', idx2 '<', idx3 '>', idx4 '='
        assert_eq!(s.old[2], "gone");
        assert_eq!(s.new[2], "");
        assert_eq!(s.new[3].clone(), "add");
        assert_eq!(s.old[3], "");
        assert!(s.del.contains(&2) && !s.del.contains(&3));
        assert!(s.add.contains(&3) && !s.add.contains(&2));
        // no-count form "@@ -1 +1 @@" parses, defaults n=1
        assert_eq!(parse_unified("@@ -5 +9 @@\n a\n").len(), 1);
        assert_eq!(parse_unified("index 123..abc 100644\n").len(), 0); // before first hunk
        assert_eq!(parse_unified("").len(), 0); // never raises
    }

    #[test]
    fn diff_for_chain() {
        let p = repo("diffchain");
        w(&p, "tracked.txt", "a\n");
        git(&p, &["add", "."]);
        // UNBORN HEAD: tracked+staged → --cached catches it
        let (t, b) = diff_for(p.to_str().unwrap(), "tracked.txt", false).unwrap();
        assert!(!b && t.contains("+"), "staged file diffs via --cached");
        w(&p, "untracked.txt", "un\n");
        let (t, b) = diff_for(p.to_str().unwrap(), "untracked.txt", true).unwrap();
        assert!(!b && t.contains("+"), "no-index vs /dev/null: all-added");
        assert!(t.contains("untracked.txt"));
        assert_eq!(diff_for(p.to_str().unwrap(), "", false).unwrap(), ("".into(), false));
    }

    #[test]
    fn diff_for_binary_sniff() {
        let p = repo("bin");
        std::fs::write(p.join("blob.bin"), &[0x00u8, 1, 2, 3]).unwrap();
        git(&p, &["add", "."]);
        let (t, b) = diff_for(p.to_str().unwrap(), "blob.bin", false).unwrap();
        assert!(b && t.is_empty(), "binary flagged by markers, got: {t}");
    }

    #[test]
    fn commit_guards_and_present_filter() {
        let p = repo("commit");
        w(&p, "keep.txt", "v1\n");
        git(&p, &["add", "."]);
        git(&p, &["commit", "-q", "-m", "base"]);
        // staged removal whose worktree path no longer exists (present-filter law)
        w(&p, "gone.txt", "v\n");
        git(&p, &["add", "."]);
        git(&p, &["commit", "-q", "-m", "second"]);
        git(&p, &["rm", "-q", "--cached", "gone.txt"]);
        std::fs::remove_file(p.join("gone.txt")).unwrap();
        // staged-only removal stays addable as a --only pathspec
        let paths = vec!["gone.txt".to_string()];
        let (ok, text) = commit(p.to_str().unwrap(), &paths, "rm gone");
        assert!(ok, "staged-only removal commits without add: {text}");
        assert_eq!(commit(p.to_str().unwrap(), &[], "x"), (false, "No files selected".into()));
        assert_eq!(commit(p.to_str().unwrap(), &["f".into()], " "),(false, "Empty commit message".into()));
    }

    #[test]
    fn ahead_and_push_bootstrap() {
        let p = repo("ahead");
        let mut w2 = scratch("remote");
        let out = Command::new("git").arg("init").arg("--bare").arg("-q")
            .arg(w2.join("remote.git")).output().unwrap();
        assert!(out.status.success());
        w2 = w2.join("remote.git");
        w(&p, "a.txt", "one\n");
        git(&p, &["add", "."]);
        git(&p, &["commit", "-q", "-m", "c1"]);
        assert_eq!(ahead(p.to_str().unwrap()), 0, "no upstream → 0");
        // point origin at the bare remote — still no upstream BRANCH configured
        git(&p, &["remote", "add", "origin", w2.to_str().unwrap()]);
        w(&p, "b.txt", "two\n");
        git(&p, &["add", "."]);
        git(&p, &["commit", "-q", "-m", "c2"]);
        assert_eq!(ahead(p.to_str().unwrap()), 0, "no upstream branch yet → 0");
        let (ok, text) = push(p.to_str().unwrap()); // no-upstream → push -u origin main
        assert!(ok, "bootstrap push failed: {text}");
        let up = String::from_utf8(Command::new("git").arg("-C").arg(&p)
            .args(["rev-parse", "--abbrev-ref", "@{upstream}"]).output().unwrap().stdout)
            .unwrap();
        assert!(up.starts_with("origin/"));
        w(&p, "c.txt", "three\n");
        git(&p, &["add", "."]);
        git(&p, &["commit", "-q", "-m", "c3"]);
        assert_eq!(ahead(p.to_str().unwrap()), 1, "one unpushed commit");
        let (ok, _) = push(p.to_str().unwrap());
        assert!(ok);
        assert_eq!(ahead(p.to_str().unwrap()), 0);
        assert_eq!(push("/no/such/root"), (false, "Not a git repository".into()));
    }

    #[test]
    fn commit_then_push_phase_contract() {
        let p = repo("ctp");
        let mut w2 = scratch("remote2");
        let out = Command::new("git").arg("init").arg("--bare").arg("-q")
            .arg(w2.join("remote.git")).output().unwrap();
        assert!(out.status.success());
        w2 = w2.join("remote.git");
        w(&p, "f.txt", "x\n");
        git(&p, &["add", "."]);
        git(&p, &["commit", "-q", "-m", "c1"]);
        git(&p, &["remote", "add", "origin", w2.to_str().unwrap()]);
        let calls = std::cell::Cell::new(0);
        let kind = std::cell::RefCell::new("");
        let phase = |k: &str| {
            calls.set(calls.get() + 1);
            *kind.borrow_mut() = k;
        };
        // paths empty = push-only: zero phase calls
        let r = commit_then_push(p.to_str().unwrap(), &[], "ignored", Some(&phase));
        assert!(r.cok && r.pok, "push-only: {r:?}");
        assert_eq!(calls.get(), 0);
        w(&p, "f.txt", "y\n");
        let r = commit_then_push(p.to_str().unwrap(), &["f.txt".to_string()], "v2",
                                 Some(&phase));
        assert!(r.cok && r.pok, "full flight failed: {r:?}");
        assert_eq!((calls.get(), kind.borrow().as_str()), (1, "busy"), "one 'busy' phase call");
        // failing commit: no phase, no push attempt
        let r = commit_then_push(p.to_str().unwrap(), &["f.txt".to_string()], "  ", Some(&phase));
        assert!(!r.cok && !r.pok && calls.get() == 1, "commit fail stops the flight: {r:?}");
        assert_eq!(r.ctext, "Empty commit message");
    }

    #[test]
    fn group_tree_shapes() {
        let rows = rows_of(&[("e.txt", "U"), ("a/b/c.txt", "M"), ("a/d.txt", "A")]);
        let got = group_tree(&rows);
        let kinds: Vec<&str> = got.iter().map(|r| r.kind).collect();
        assert_eq!(
            got.iter().map(|r| (r.rel.as_str(), r.kind)).collect::<Vec<_>>(),
            vec![
                ("a", "d"), ("a/b", "d"), ("a/b/c.txt", "f"),
                ("a/d.txt", "f"), ("e.txt", "f"),
            ],
            "parents before children, dirs before files at a level"
        );
        assert_eq!(kinds[0], "d");
        // depths: a=0, a/b=1, c.txt=2, d.txt=1, e.txt=0
        let depths: Vec<usize> = got.iter().map(|r| r.depth).collect();
        assert_eq!(depths, vec![0, 1, 2, 1, 0]);
        // letters ride the FILE rows only (dirs carry "")
        assert_eq!(got.iter().find(|r| r.name == "c.txt").unwrap().letter, "M");
        assert!(got.iter().all(|r| r.kind != "d" || r.letter.is_empty()));
    }

    #[test]
    fn dir_state_all_any() {
        let rows = group_tree(&rows_of(&[
            ("e.txt", "U"), ("a/b/c.txt", "M"), ("a/d.txt", "A"),
        ]));
        let ck = BTreeSet::from(["a/b/c.txt".to_string()]);
        let st = checked_dir_state(&rows, &ck);
        assert_eq!(st.get("a/b"), Some(&(false, true)) /* some, not all */);
        assert_eq!(st.get("a"), Some(&(false, true)));
        assert_eq!(st.get("e.txt"), None);
        // all three checked → all + any true
        let ck: BTreeSet<String> = ["a/b/c.txt", "a/d.txt", "e.txt"]
            .iter().map(|s| s.to_string()).collect();
        let st = checked_dir_state(&rows, &ck);
        assert_eq!(st.get("a/b"), Some(&(true, true)));
        assert_eq!(st.get("a"), Some(&(true, true)));
        let st = checked_dir_state(&rows, &BTreeSet::new());
        assert_eq!(st.get("a"), Some(&(false, false)));
    }
}
```

These tests FAIL TO COMPILE now (`branch_of` unresolved) — that is the RED.
Scratch repos are left in `std::env::temp_dir()` (tmp cleaner reaps them).

- [ ] **Step 2: Run — verify RED**

Run: `cargo test --manifest-path alpaca-code-rs/Cargo.toml gitstatus 2>&1 | tail -20`
Expected: FAIL — unresolved names (`branch_of` not found). Not a compile-err
on anything else: if the RED shows unrelated breakage, fix the test file first.

- [ ] **Step 3: Implement `gitstatus.rs` (production code below the tests)**

Everything below OUTSIDE the test module. Port law: gitstatus.py lines 7-73
(branch/branches/switch), 77-155 (HUNK_ROW/parse/build/changes), 158-183
(git_out/diff_for), 185-246 (commit/ahead/commit_then_push), 248-273 (push),
274-313 (group_tree/checked_dir_state). `status()` (lines 59-73) is DEAD in
python — no port. The subprocess runner (one shared helper, replaces six
try/except blocks):

```rust
fn has_git(root: &str) -> bool {
    !root.is_empty() && std::path::Path::new(root).join(".git").is_dir()
}

fn clip400(s: &str) -> String {
    s.trim().chars().take(400).collect()
}

/// subprocess.run(["git","-C",root,*args], capture_output, timeout) shape with
/// the pipe-deadlock fixed: stdout/stderr are drained to Vec on READER THREADS
/// (plain try_wait polling with piped children fills the 64KiB pipes on big
/// diffs). env GIT_TERMINAL_PROMPT=0 only when env_prompt (Command::env
/// merges onto the inherited environ — exactly python's {**os.environ, …}).
/// Timeout / spawn failure / kill → None; never panics.
fn git_run(
    root: &str,
    args: &[&str],
    env_prompt: bool,
    timeout: Duration,
) -> Option<std::process::Output> {
    if root.is_empty() {
        return None;
    }
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(root).args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if env_prompt {
        cmd.env("GIT_TERMINAL_PROMPT", "0");
    }
    let mut child = cmd.spawn().ok()?;
    let mut out_h = child.stdout.take()?;
    let mut err_h = child.stderr.take()?;
    let t1 = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = out_h.read_to_end(&mut v);
        v
    });
    let t2 = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = err_h.read_to_end(&mut v);
        v
    });
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = t1.join().ok().unwrap_or_default();
                let stderr = t2.join().ok().unwrap_or_default();
                return Some(std::process::Output { status, stdout, stderr });
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    // reap on the way out (kill → readers get EOF → join)
                    let _ = child.kill();
                    let _ = t1.join();
                    let _ = t2.join();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            // Err (child went away): treat as dead
            Err(_) => {
                let _ = t1.join();
                let _ = t2.join();
                return None;
            }
        }
    }
}
```

Then (signatures exact as Produces):

```rust
pub fn branch_of(root: &str) -> Option<String> {
    if root.is_empty() {
        return None;
    }
    let f = std::fs::read_to_string(std::path::Path::new(root).join(".git").join("HEAD"));
    let content = f.ok()?.trim().to_string();
    content.strip_prefix("ref: refs/heads/").map(|s| s.to_owned())
    // no prefix → None: detached HEAD / worktree pointer → hide branch widget
}

pub fn branches(root: &str) -> Option<Vec<String>> {
    if !has_git(root) {
        return None;
    }
    let r = git_run(root, &["for-each-ref", "refs/heads", "--sort=refname",
                             "--format=%(refname:short)"], false,
                    Duration::from_secs(5))?;
    if !r.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&r.stdout);
    Some(s.lines().filter(|l| !l.is_empty())
          .map(|l| l.to_owned()).collect())
}

fn _checkout(root: &str, args: &[&str]) -> (bool, String) {
    let Some(r) = git_run(root, args, true, Duration::from_secs(60)) else {
        return (false, "git checkout failed: timed out".into());
    };
    if !r.status.success() {
        let text = {
            let se = String::from_utf8_lossy(&r.stderr);
            if se.trim().is_empty() { String::from_utf8_lossy(&r.stdout).to_string() }
            else { se.to_string() }
        };
        return (false, if text.trim().is_empty() { "git checkout failed" } else { text.trim() }.to_string()
            .chars().take(400).collect::<String>());
    }
    (true, String::new())
}

pub fn switch(root: &str, name: &str) -> (bool, String) {
    if name.is_empty() || name.starts_with('-') {
        return (false, "Bad branch name".into());
    }
    _checkout(root, &["checkout", name])
}

/// `checkout -b` from HEAD (create + switch, user ruling). Git validates the
/// name; the leading-dash guard keeps a name like "-force" from parsing as a
/// flag. python create_switch also strips whitespace around the name.
pub fn create_switch(root: &str, name: &str) -> (bool, String) {
    let name = name.trim();
    if name.is_empty() || name.starts_with('-') {
        return (false, "Bad branch name".into());
    }
    _checkout(root, &["checkout", "-b", name])
}
```

(The `(False, f"git checkout failed: {e}")` on OSError/TimeoutExpired: the
python message embeds the exception; rust keeps a stable text — see the
`_checkout` sketch above. Keep texts ≤400.)

`parse_porcelain` (pure, from changes):

```rust
pub fn parse_porcelain(stdout: &[u8]) -> Option<Rows> {
    let text = String::from_utf8_lossy(stdout).to_string(); // decode replace
    let entries: Vec<&str> = text.split('\0').filter(|e| !e.is_empty()).collect();
    let mut out: Rows = Vec::new();
    let mut i = 0usize;
    while i < entries.len() {
        let e = entries[i];
        if e.len() < 4 {  // truncated/stray record (submodules emit extras)
            i += 1;
            continue;
        }
        let xy = &e[..2];
        // XY+space are ASCII, so byte slicing [3..] == python code-point [3:]
        let path = &e[3..];
        if xy.as_bytes()[0] == b'R' {
            // -z format: ORIGINAL path is the next record
            out.push((path.to_string(), "R".to_string()));
            if i + 1 < entries.len() {
                out.push((entries[i + 1].to_string(), "D".to_string()));
            }
            i += 2;
            continue;
        }
        let x = xy.as_bytes()[0] as char;
        let y = xy.as_bytes()[1] as char;
        let mut chosen = if x != ' ' { x.to_string() } else { y.to_string() };
        if x == 'U' || y == 'U' {
            chosen = "C".to_string(); // every unmerged code has a U; != untracked '?'
        }
        if chosen == "?" {
            chosen = "U".to_string();
        }
        out.push((path.to_string(), chosen));
        i += 1;
    }
    // python: sorted(set(out), key=lambda t: t[0]) — dedup + path sort
    out.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    out.dedup();
    Some(out)
}
```

`changes` / `git_out` / `diff_for` / `commit` / `ahead` / `push` /
`commit_then_push` / `group_tree` / `checked_dir_state` — port each from the
python body with these mapping notes:

- `changes`: `has_git` guard → `git_run(..., &["status","--porcelain=v1","-z","--untracked-files=all"], false, 10s)` → non-success → None → `parse_porcelain(&r.stdout)`.
- `git_out`: `git_run(root, args, false, Duration::from_secs(timeout_s))` → `!success || (code != 0 && code != 1)` → None; else Some(stdout). Exit code: `r.exit_code()` — but careful: a killed child reports 137; python would have raised TimeoutExpired instead — the None-from-timeout already covers that branch.
- `diff_for`: python lines 166-183 verbatim: empty root/rel → `Some(("".into(), false))` (python returns the tuple directly — never None); `is_untracked=false` → `git_out(root, ["diff","HEAD","--no-color","-U3","--",rel], 15)`; None → `git_out(root, ["diff","--cached",…], 15)`; still None (covers untracked) → `git_out(root, ["diff","--no-index","--no-color","-U3","--","/dev/null", &format!("{root}/{rel}")], 15)`; decode lossy or ""; `"Binary files" in text || "GIT binary patch" in text → ("", true)`; else `(text, false)`.
- `commit`: guards → `("No files selected")` on empty paths or no `.git`; blank msg → "Empty commit message"; `present = paths.filter(|p| { let q = root.join(p); q.symlink_metadata().is_ok() })` (python `os.path.lexists` — dangling symlink counts; `Path::exists` does NOT follow-free, it lexists — symlink_metadata is exact); present → `git_run(add --, …, true, 30s)`; non-success → `(false, clip400(stderr or "git add failed"))`; then `git_run(["commit","--only","-m",msg,"--"] + paths, true, 60s)`; non-success → `(false, clip400(stderr or "git commit failed"))`; timeout path → "git failed: timed out"; success → first stdout line (trimmed, stripped) if any else "Committed.", clip400.
- `ahead`: `has_git` else 0; `git_run(["rev-list","--count","@{upstream}..HEAD"], false, 5s)`; non-success → 0; stdout trim parse::<usize> unwrap or 0 (python's ValueError→0).
- `push`: `has_git` else `(false,"Not a git repository")`; `git_run(["push"], true, 120s)` — Err/timeout → "git push failed: timed out"; rc 0 → `(true, clip400(stderr or stdout or "Pushed."))`; else err=clip400… then `"no upstream" in err && branch_of(root) == Some` → `git_run(["push","-u","origin",branch], true, 120s)`; rc 0 same success; else final `(false, err or "git push failed")`.
- `commit_then_push`: per Produces contract (see code sketch in Review Focus); `if paths.is_empty() { (true,"") } else { commit(...) }` then `if !cok return`; `if !paths.is_empty(), phase → call it once with the "busy","Pushing…" phase (the closure bakes the text)`; `push(root)`.
- `group_tree`: python 274-296 → build the nested `BTreeMap<String, BTreeMap…>`? python uses plain dicts with sorted at emit. Rust direct shape:
  ```rust
  type Node = BTreeMap<String, Node2>;  // dirs
  // simpler: recursive enum-free tree as (dirs: BTreeMap<String, Node>, files: Vec<(String,String)>)
  ```
  Use a struct `struct Lev { dirs: BTreeMap<String, Lev>, files: Vec<(String, String)> }`, build by walking parts; walk(dirs sorted by BTreeMap order (byte-order — python `sorted` on str = code-point order; equal on ASCII paths, which status paths are except unicode names — acceptable, note it), emit `("d", name, drel, "", depth)` then recurse, then files (python node.get("") preserves INSERTION order of the sorted input rows — sorted(rows) pre-sorted tuples; file order at a level = sorted by (rel, letter) = name+letter — port: `sort_by` the (name, letter) pair). depth++ on recursion; kind literal `&'static str` via `"d"`/`"f"` — matches `kind: &'static str`.
- `checked_dir_state`: per T1 test — over rows, f-rows only, per part-prefix accumulate `(prev_all && ok, prev_any || ok)` — exact python 298-313 port, keyed `BTreeMap<String,(bool,bool)>`.

`let _ = ` any unused `ExitStatus` import — drop the unused import (GREEN
build will name it).

- [ ] **Step 4: Run — verify GREEN**

Run: `cargo test --manifest-path alpaca-code-rs/Cargo.toml 2>&1 | tail -15`
Expected: PASS — all gitstatus tests green AND the whole S1 suite still green
(filetree scan tests etc. untouched).

- [ ] **Step 5: task-done**

Run: `scripts/task-done docs/superpowers/plans/2026-10-07-rust-port-stage2.md 1 -- cargo test --manifest-path alpaca-code-rs/Cargo.toml`
(No commit — standing waiver; the ledger line is the record.)

---

### Task 2: `treehover.rs` — HoverTree moves out of filetree.rs + press capture

**Files:**
- Create: `alpaca-code-rs/src/treehover.rs`
- Modify: `alpaca-code-rs/src/filetree.rs` (delete `mod hover { … }`, add
  `use crate::treehover::HoverTree;`), `alpaca-code-rs/src/main.rs` (add
  `mod treehover;`)
- Test: in-module (pure `toggle_hit`); press-capture itself is gated by the
  T8 probe (widget-layer).

**Interfaces:**
- Consumes: gtk4/glib (S1 deps); the `mod hover` body currently at
  `alpaca-code-rs/src/filetree.rs:170-341` (moved verbatim).
- Produces:
  - `crate::treehover::HoverTree` — same subtype as today's `hover::HoverTree`
    (glib::SimpleObject subclass painting the hover band, scroll-adj dedup),
    PLUS `pub fn press_xy(&self) -> (f64, f64)`.
  - `pub fn toggle_hit(px: f64, ca_x: i32, tog_w: i32) -> bool` — free fn,
    the x-geometry law the changes view routes clicks with (python
    gitview.py `_toggle_hit`).

- [ ] **Step 1: Write the failing test**

Create `alpaca-code-rs/src/treehover.rs` with ONLY the header comment + test
module; nothing else exists yet (RED = unresolved `toggle_hit`):

```rust
//! Hover-tree: gtk::TreeView subclass — hover-band painting + scroll-adj sync
//! + press-position capture. Moves here from filetree.rs (S2) so the git
//! changes view (gitview.rs) can reuse it without importing filetree internals.

#[cfg(test)]
mod tests {
    use super::toggle_hit;

    #[test]
    fn toggle_hit_geometry() {
        // python gitview.py _toggle_hit: px<0 = no press → never a toggle;
        // x0 = cell_area.x + 16 (chevron slot: 12px art + 2px pad, measured);
        // inclusive lower, exclusive upper.
        assert!(!toggle_hit(-5.0, 0, 24), "sentinel press: not a toggle hit");
        assert!(toggle_hit(20.0, 4, 24), "x0 = 20 → 20..44");
        assert!(toggle_hit(19.0, 4, 24), "lower bound inclusive");
        assert!(!toggle_hit(44.0, 4, 24), "upper bound exclusive");
        assert!(toggle_hit(43.9, 4, 24), "just inside is a hit");
        assert!(!toggle_hit(10.0, 4, 24), "chevron-side click is the row, not the toggle");
    }
}
```

- [ ] **Step 2: Run — verify RED**

Run: `cargo test --manifest-path alpaca-code-rs/Cargo.toml treehover 2>&1 | tail -10`
Expected: FAIL — `toggle_hit` not found (file compiles no farther; the module
must at least exist for the test to be attempted).

- [ ] **Step 3: Implement — move + press capture + toggle_hit**

Move `mod hover { … }` (filetree.rs:170-341) out of filetree.rs into
treehover.rs at FILE LEVEL (drop the nested `mod hover { }` wrapper; keep
everything else verbatim including its `use` lines, the snapshot override,
`hover_band`, `refresh_hover`, the `vadjustment` notify hook and `last_adj`
dedup — plus `use std::cell::Cell;`, which the new field needs). Keep
`#![allow(deprecated)]` at the top of treehover.rs (carried). Then add —
exactly this diff, to the EXISTING `Imp` struct (keep `hover_xy` /
`hover_row` / `last_adj` verbatim):

```rust
pub struct Imp {
    hover_xy: Cell<(f64, f64)>,
    hover_row: RefCell<Option<TreeRowReference>>,
    last_adj: RefCell<Option<Adjustment>>,
    press_xy: Cell<(f64, f64)>,   // NEW: python's view._press_xy sentinel
}
```

(The existing fields keep their types — open filetree.rs for the exact
lines when moving. `press_xy` defaults to `(-1.0, -1.0)`.)

In `Imp::Default::default()` / init (wherever the existing fields are zeroed —
filetree.rs constructs them in `Default for Imp`): `press_xy: Cell::new((-1.0, -1.0))`.

In `constructed`, where the existing press scaffold sits
(`press.set_propagation_phase(Capture); add_controller`) — CONNECT the
handler (+ keep the phase):

```rust
let press = gtk::GestureClick::new();
press.set_propagation_phase(gtk::PropagationPhase::Capture);
let press2 = this.clone();          // `this` = the HoverTree (existing pattern)
press.connect_pressed(move |_g, _n, x, y| {
    press2.imp().press_xy.set((x, y));
});
o.add_controller(press);
```

(The `(-1.0, -1.0)` sentinel lives in `Default for Imp` only; nothing resets
it — python's `_press_xy` never resets between presses either.)

And the public accessors + free fn at file bottom:

```rust
impl HoverTree {
    /// Pressed position captured at GestureClick press (capture phase — runs
    /// before the treeview's own handling). (-1.0, -1.0) = no press.
    pub fn press_xy(&self) -> (f64, f64) {
        self.imp().press_xy.get()
    }
}

/// Changes-view toggle routing (python gitview.py `_toggle_hit`): press_x
/// inside the toggle CELL AREA, past the chevron slot (+16 = 12px art + 2px
/// pad, measured), within the toggle's natural width (falls back to 24 at
/// the caller when the lazy measure fails). px<0 = no press captured.
pub fn toggle_hit(px: f64, ca_x: i32, tog_w: i32) -> bool {
    if px < 0.0 {
        return false;
    }
    let x0 = ca_x + 16;
    let x1 = (x0 + tog_w) as f64;
    (x0 as f64) <= px && px < x1
}
```

Because `press_xy`/`toggle_hit` are unused until T3 lands, carry the
badge.rs-pattern allowance ON THE TWO ITEMS (`#[allow(dead_code)]` + a
"T3 consumer" comment) — deleted at T3. (No `#![allow(dead_code)]` at file
level: it would mask the moved hover module's own drift.)

New file shape (top-level items, no nested mod):

```rust
#![allow(deprecated)]
// (existing hover-module content at top level: use lines, Imp, HoverTree)
// (NEW at the end: press_xy accessor + free fn toggle_hit + tests)
```

Then in `filetree.rs`: delete the whole `mod hover { … }` block and its
`use self::hover::HoverTree;` if present, import from the new module:

```rust
use crate::treehover::HoverTree;
```

In `src/main.rs` add: `mod treehover;`.

- [ ] **Step 4: Run — verify GREEN**

Run: `cargo test --manifest-path alpaca-code-rs/Cargo.toml 2>&1 | tail -8`
Expected: PASS — toggle_hit green + all S1 suite green (filetree scan tests +
module tests) — filetree.rs compiles against the moved module unchanged.

- [ ] **Step 5: task-done + note**

Run: `scripts/task-done docs/superpowers/plans/2026-10-07-rust-port-stage2.md 2 -- cargo test --manifest-path alpaca-code-rs/Cargo.toml`
Ledger note AFTER the task-done line: `_toggle_hit`'s real x-geometry is
widget-layer — the press-capture → `toggle_hit` → `get_cell_area` flow is
exercised by the T8 probe (recorded as the Ruling: unit test pins the ARITHMETIC,
live probe pins the WIRING).

---

### Task 3: `gitview.rs` — CHANGES view (tree + checkboxes + commit bar)

The store is REBUILT per refresh (python gitview.py:1-4, the filetree
invariant: value-only updates safe under expanded rows, structural mutation
collapses them — surgical patching is off the table here). Port of
`alpaca_code/gitview.py` (360 L, re-verified live above); `treehover::HoverTree`
supplies press capture (T2). Python's `on_open` / `before_commit` / `on_status`
callbacks become outputs; `before_commit` (flush dirty buffers) is an app-side
chain — T7 — routed through the window's flush, keeping the editor git-free.

**Files:**
- Create: `alpaca-code-rs/src/gitview.rs`
- Modify: `alpaca-code-rs/src/main.rs` (add `mod gitview;`),
  `alpaca-code-rs/src/badges.rs` (delete the three `#[allow(dead_code)]`
  annotations on `letter_pixbuf` / `CARD_BG` / `tile_bg` — the consumer lives
  here)
- Test: in-module (pure helpers); widget layer gated by cargo build + T8 probe.

**Interfaces:**
- Consumes: `crate::gitstatus::{GitStatusKind, FlightOut, Row, Rows}` (types)
  and `crate::gitstatus::{changes, ahead, commit_then_push, group_tree,
  checked_dir_state}` (calls — T1 signatures); `crate::treehover::{HoverTree,
  toggle_hit}` (T2); `crate::badges::{pixbuf_for, folder_pixbuf,
  chevron_pixbuf, blank_pixbuf, letter_pixbuf}`.
- Produces (T6/T7 consume):
  - `pub enum GitViewMsg { SetRoot(PathBuf), Refresh { keep: bool },
    Apply { raw: Option<Rows>, ahead: usize, keep: bool },
    Filter(String), RowToggled(gtk::TreePath), Activated(gtk::TreePath),
    RowExpanded(gtk::TreePath), RowCollapsed(gtk::TreePath),
    CommitClicked, StartCommit { root: PathBuf, paths: Vec<String>,
    msg: String } }` (derive Debug)
  - `pub enum GitViewOutput { Open { rel: String, letter: String },
    Commit { root: PathBuf, paths: Vec<String>, msg: String },
    Status(GitStatusKind, String), Busy(bool) }` (derive Debug, Clone)
  - `pub enum GitViewCommand { Phase, Landed(FlightOut) }` (derive Debug)
  - `pub struct ChangesView` — `#[relm4::component(pub)]`, Root = vertical
    `gtk::Box` (python's `class ChangesView(Gtk.Box)`, gitview.py:23)

- [ ] **Step 1: RED — failing tests for the pure helpers**

Append the module skeleton + test module to `main.rs`'s mod list first
(`mod gitview;`), then `src/gitview.rs` test module FIRST (unresolved names
= RED):

```rust
//! CHANGES view: git-change tree (group_tree rows) with checkboxes and a
//! commit bar — python gitview.py port. Pure helpers testable displayless;
//! widget-layer is gated by the T8 probe (S1 precedent).

use crate::badges;
use crate::gitstatus::{self, FlightOut, GitStatusKind, Row, Rows};
use crate::treehover::toggle_hit;
use gtk4::prelude::*;
use relm4::{ComponentParts, ComponentSender, Component, RelmWidgetExt};
use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;

/// python LETTER_COLOR (gitview.py:17-21) — M amber, A/U green, D red,
/// R blue (+T typechange), C conflict red (I1).
pub fn letter_color(letter: &str) -> &'static str {
    match letter {
        "M" => "#f2c94c",
        "A" | "U" => "#22c55e",
        "D" => "#ef4444",
        "R" | "T" => "#2f80ed",
        "C" => "#ef4444",
        _ => "#f2c94c", // LETTER_DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows_of(v: &[(&str, &str)]) -> Rows {
        v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn letter_color_table() {
        assert_eq!(letter_color("M"), "#f2c94c");
        assert_eq!(letter_color("A"), "#22c55e");
        assert_eq!(letter_color("D"), "#ef4444");
        assert_eq!(letter_color("R"), "#2f80ed");
        assert_eq!(letter_color("T"), "#2f80ed");
        assert_eq!(letter_color("C"), "#ef4444");
        assert_eq!(letter_color("Q"), "#f2c94c", "unknown letter → default");
    }

    #[test]
    fn filtered_matches_basename_or_rel() {
        let rows = rows_of(&[("z.md", "M"), ("a/b/c.md", "A")]); // unsorted on purpose
        assert_eq!(filtered(&rows, ""), rows, "empty needle → identity");
        assert_eq!(filtered(&rows, "c.md")[0].0, "a/b/c.md", "basename match");
        assert_eq!(filtered(&rows, "A/b")[0].0, "a/b/c.md", "rel match, case-insensitive");
        let both = filtered(&rows, "md");
        assert_eq!(both.len(), 2, "both names carry md");
        assert!(filtered(&rows, "zzz").is_empty());
    }

    #[test]
    fn toggled_checked_laws() {
        let vis = BTreeSet::from([
            "a/c.txt".to_string(), "a/d/m.md".to_string(), "e.md".to_string(),
        ]);
        let ck = BTreeSet::from(["a/c.txt".to_string(), "a/d/m.md".to_string()]);
        // file: flip
        let c = toggled_checked("f", "a/d/m.md", &ck, &vis).unwrap();
        assert!(!c.contains("a/d/m.md") && c.contains("a/c.txt"));
        // dir, none of its (visible) descendants selected → select all under it
        // (prefix = rel + "/", python gitview.py _on_toggled)
        let ck2 = BTreeSet::from(["a/c.txt".to_string()]);
        let c = toggled_checked("d", "a/d", &ck2, &vis).unwrap();
        assert!(c.contains("a/d/m.md"), "none selected → add all under dir");
        // dir all selected → clear under it
        let full = BTreeSet::from(["a/c.txt".to_string(), "a/d/m.md".to_string(),
                                   "e.md".to_string()]);
        let c = toggled_checked("d", "a", &full, &vis).unwrap();
        assert!(c.is_empty(), "every descendant selected → clear them");
        // masthead (visible-only law, I2)
        let c = toggled_checked("s", "", &full, &vis).unwrap();
        assert!(c.is_empty());
        let c = toggled_checked("s", "", &BTreeSet::new(), &vis).unwrap();
        assert_eq!(c, vis);
        // dir with no visible descendants → |= ∅ no-op (Some, unchanged —
        // python's else-branch `checked |= set()`)
        let c = toggled_checked("d", "zzz", &ck, &vis).unwrap();
        assert_eq!(c, ck);
        // unknown kind → None (never flips a "No changes" row)
        assert!(toggled_checked("e", "x", &ck, &vis).is_none());
    }
}
```

- [ ] **Step 2: Run — verify RED**

Run: `cargo test --manifest-path alpaca-code-rs/Cargo.toml gitview 2>&1 | tail -10`
Expected: FAIL — `letter_color` / `filtered` / `toggled_checked` unresolved
(the file has no component yet; tests first is fine — they cannot compile until
the names exist).

---

- [ ] **Step 3: Implement — pure helpers + component**

**Helpers below the tests** (the RED block pins them):

```rust
/// python _filtered (gitview.py) — empty needle → same Vec (identity)
pub fn filtered(rows: &Rows, needle: &str) -> Rows {
    let n = needle.trim().to_ascii_lowercase();
    if n.is_empty() {
        return rows.clone();
    }
    rows.iter()
        .filter(|(rel, _)| {
            let base = rel.rsplit('/').next().unwrap_or(rel).to_lowercase();
            base.contains(&n) || rel.to_lowercase().contains(&n)
        })
        .cloned()
        .collect()
}

/// python _on_toggled's set law, decision-extracted: f flips; d/s use the
/// visible-descendant law (all-under ⇒ clear, else add-all). Dir with no
/// visible descendants → |= ∅ (Some, unchanged). Unknown kind → None.
pub fn toggled_checked(
    kind: &str,
    rel: &str,
    checked: &BTreeSet<String>,
    visible: &BTreeSet<String>,
) -> Option<BTreeSet<String>> {
    let mut out = checked.clone();
    match kind {
        "f" => {
            if !out.remove(rel) {
                out.insert(rel.to_string());
            }
        }
        "d" | "s" => {
            let prefix = match kind {
                "d" => format!("{rel}/"),
                _ => String::new(),
            };
            let files: Vec<&String> = visible
                .iter()
                .filter(|r| r.starts_with(&prefix))
                .collect();
            let all_in = !files.is_empty() && files.iter().all(|f| out.contains(*f));
            if all_in {
                for f in files {
                    out.remove(f);
                }
            } else {
                for f in files {
                    out.insert(f.clone());
                }
            }
        }
        _ => return None,
    }
    Some(out)
}
```

**Component.** Model fields on `ChangesView`:

```rust
struct Updates { root: Option<PathBuf>, files: Vec<String>, checked: BTreeSet<String>,
    visible: BTreeSet<String>, needle: String, closed: HashSet<String>,
    rows: Rows, shown: Rows, ahead: usize, busy: bool, flight: bool,
    tog_w: std::cell::Cell<Option<i32>> }
```

Plus widgets: `store: gtk4::TreeStore`, `view: HoverTree`, `col:
gtk4::TreeViewColumn`, `tog: gtk4::CellRendererToggle`, `msg:
gtk4::Entry`, `btn: gtk4::Button`.

`init` (python `__init__` 23-160, filetree.rs precedent: `view! { #[root]
gtk::Box { set_orientation: gtk::Orientation::Vertical, set_css_classes:
&["alpaca-card"], set_overflow: gtk::Overflow::Hidden } }`):

1. Store, 12 columns exactly as python appends rows (fill loop below is the
   map — column i = python row index i):
   `TreeStore::new(&[STRING, STRING, STRING, STRING, PIXBUF, PIXBUF, PIXBUF,
   BOOL, BOOL, BOOL, BOOL, BOOL])`.
2. `view = HoverTree::default()`; `set_model(Some(&store))`,
   `headers_visible(false)`, `activate_on_single_click(true)`,
   `#[allow(deprecated)] set_show_expanders(false)`,
   `tooltip_column(1)`, `level_indentation(16)`.
3. **One merged column** (measured law: single `TreeViewColumn`, packed
   chev→tog→letter→badge→name, `set_expand(true)` on the COLUMN — else
   letter chips ride mid-panel when the window is wide):

```rust
let col = gtk4::TreeViewColumn::new();
// 4 — chevron (dirs) / 12px blank (files); xalign bottom-pin 2/3
let chev = gtk4::CellRendererPixbuf::new();
col.pack_start(&chev, false);
col.add_attribute(&chev, "pixbuf", 4);
#[allow(deprecated)] // CellRendererExt helpers deprecated on this gtk4-rs
{ chev.set_padding(2, 2); chev.set_property("yalign", (2.0f64 / 3.0f64)); }
// 7/8/11 — checkbox: NOT activatable (clicks routed by press geometry,
// _toggle_hit); active←checked, inconsistent←inconsist, visible←toggle-vis
let tog = gtk4::CellRendererToggle::new();
#[allow(deprecated)] tog.set_activatable(false);
col.pack_start(&tog, false);
col.add_attribute(&tog, "active", 7);
col.add_attribute(&tog, "inconsistent", 8);
col.add_attribute(&tog, "visible", 11);
// 6/10 — letter chip: chip-vis (=col10) is bound to `not is_dir` — a
// letterless FILE row keeps its slot (blank pixbuf); dirs hide it
let let_ = gtk4::CellRendererPixbuf::new();
col.pack_start(&let_, false);
col.add_attribute(&let_, "pixbuf", 6);
col.add_attribute(&let_, "visible", 10);
#[allow(deprecated)] { let_.set_padding(2, 2); let_.set_property("yalign", 1.0f64); }
// 5/9 — file badge
let bad = gtk4::CellRendererPixbuf::new();
col.pack_start(&bad, false);
col.add_attribute(&bad, "pixbuf", 5);
col.add_attribute(&bad, "visible", 9);
#[allow(deprecated)] { bad.set_padding(2, 2); bad.set_property("yalign", 1.0f64); }
// 0 — name text, packed LAST with expand=True (pin the strip left)
let name = gtk4::CellRendererText::new();
#[allow(deprecated)]
{ name.set_padding(2, 2); name.set_property("ellipsize", gtk4::pango::EllipsizeMode::Middle); }
col.pack_start(&name, true);
col.add_attribute(&name, "text", 0);
col.set_expand(true); // ellipsized cell nat == min; slack must ride the col
view.append_column(&col);
```

4. Scroll: `ScrolledWindow::builder().child(&view).vexpand(true).build()`,
   root.append. Commit bar below (python 162-186): `gtk::Box` vertical,
   spacing 6, margins 12/12/6/6, css `alpaca-commitbar`; `msg = Entry`
   placeholder "Commit message…" `hexpand` size(-1, 30) css `alpaca-msg`;
   `btn = Button` label "Commit and Push" css `alpaca-barbtn` `hexpand`.

Signals (standard relm4 shape: closures send to `sender` clone; filetree.rs
wires exactly these five): `view.connect_row_activated → Activated(path)`,
`connect_row_expanded → RowExpanded(path)` (the expand-state walker runs
HERE — the activated dir branch only calls expand/collapse_row and the
signal does the bookkeeping, python-exact), `connect_row_collapsed →
RowCollapsed(path)`, and `store.connect_row_inserted / connect_row_deleted
→ the HoverTree's refresh_hover()` (filetree.rs does the same — hover
re-reads after store mutation).

`update()` — one match, arms mirror python methods:

| Input | Python origin | Behavior |
|---|---|---|
| `SetRoot(root)` | `set_root` | `self.root = Some(root)`; `closed.clear()`; `refresh(false)` — needle NOT touched here (the panel clears its entry, routing `Filter("")`; python-124 exact) |
| `Refresh{keep}` | `refresh` | sync git calls on UI thread (Global constraint): `raw = root.as_deref().and_then(gitstatus::changes)`; `ahead` = only when `raw.is_some()`: `root.as_deref().map(gitstatus::ahead).unwrap_or(0)`, else 0; → `apply(raw, ahead, keep)` |
| `Apply{raw, ahead, keep}` | `apply` | below |
| `Filter(needle)` | `filter` | needle STORED AS-GIVEN (the helper strips+lowercases per use); `shown = filtered(&self.rows, &needle)`; `fill(&shown)` — no rescan, `_checked` untouched (view-only lens, python comment "_checked untouched; _files stays global") |
| `RowToggled(path)` | `_on_toggled` | below |
| `Activated(path)` | `_on_activated` | below |
| `RowExpanded(path)` / `RowCollapsed(path)` | `_on_expand_toggle` | see walker below |
| `CommitClicked` | `commit_clicked` | below |
| `StartCommit{root, paths, msg}` | `_start`+thread | guard `flight` → return; `flight = true`; `sender.spawn_command(move \|out\| { let out_f = gitstatus::commit_then_push(&root, &paths, &msg, Some(&phase_fn)); out.emit(GitViewCommand::Landed(out_f)); })` where `phase_fn` pushes `GitViewCommand::Phase` — build phase_fn only when `!paths.is_empty()` (python passes `self._phase if paths else None`) |

`apply(raw, ahead, keep)` (python apply, 219-236): `let Some(raw) = raw
else { return }` (None = clean no-op — no repo / timeout); cache `ahead`;
`rows = raw`; `files: HashSet<String>` = group_tree f-rows' rels (stored
BEFORE fill — sync_row needs it); `checked = if keep { checked ∩ files }
else { files.clone() }` (fresh view selects ALL — python
"_checked = &-intersect if keep_selection else set(files)"); `shown =
filtered(&rows, &needle)`; `fill(&shown)`.

`fill(shown)` (python `_fill` 239-263): `store.clear()`; `self.shown =
shown`; `self.visible` = f rels of `group_tree(&shown)`; then rows:

- empty shown → ONE row `["No changes", "", "e", "", blank, blank, blank,
  false, false, false, false, false]` (cols 7-11 ALL false), then
  `sync()` and return.
- else masthead `["Select all", "", "s", "", blank, blank, blank, false,
  false, false, false, true]` (only toggle-vis col11 True).
- then `let mut iters: HashMap<String, gtk4::TreeIter>` walking
  `group_tree(&shown)` — for each (kind, name, rel, letter, depth):
  `is_dir = kind == "d"`; badge = dir ? folder_pixbuf : pixbuf_for(name)
  — None → blank pixbuf (python `badge or blank`); lpix = letter
  nonempty ? letter_pixbuf(letter, letter_color(letter)) : blank; chev =
  dir ? chevron_pixbuf(false) : blank; `parent = iters.get(dir-part) if
  rel has '/'`; `store.append(parent_iter, [name, rel, kind, letter,
  chev, badge, lpix, false, false, TRUE, !is_dir, TRUE])` — EXACT python
  tail (comment: "badge-vis (files+dirs), chip-vis (file rows: a
  letterless file keeps its slot)"); dirs → `iters.insert(rel, iter)`.
- `sync()`.
- expand pass: for (rel, it) in iters — `!closed.contains(rel) →
  #[allow(deprecated)] view.expand_row(&store.path(&it), false)` (expand
  state lives in `closed`, never read back from the view — filetree
  lesson).

`sync()` (python `_sync` 266-279): `st = if !self.shown.is_empty() {
gitstatus::checked_dir_state(&group_tree(&self.shown), &(checked ∩
visible)) } else { empty map }`; walk: `#[allow(deprecated)]
store.iter_first()` → per top row: `sync_row(iter, st)`, capture next
BEFORE recursion, recurse `iter_children`; after walk → `buttons()`.

`sync_row(it, st)` (python `_sync_row` 283-296): read cols 2/1;
`"f"` → col7 = checked.contains(rel); `"d"` → `(allc, somec) =
st.get(rel, (false, false))`, col7 = allc, col8 = !allc && somec;
`"s"` → `allc = !visible.is_empty() && visible.iter().all(|r|
checked.contains(r))`, `somec = checked.iter().any(|r|
visible.contains(*r))`, col7 = allc, col8 = !allc && somec.
Value-only writes (safe under expanded rows — filetree law).

`buttons()` (python `_buttons` 301-304): `btn.set_sensitive(!busy &&
(!checked.is_empty() || ahead > 0))`; `msg.set_sensitive(!busy)`.

`RowToggled(path)` (python `_on_toggled` 306-324): read kind/rel from
the store; `if let Some(next) = toggled_checked(kind, &rel, &checked,
&visible) { self.checked = next; sync(); }` — masthead/dir/file all
through the one law.

**Expand walker** (python `_on_expand_toggle` 327-339) — wired to the
RowExpanded/RowCollapsed inputs, NOT to Activated: rel = col1, empty →
return; expanded → `closed.remove(rel)`, collapsed → `closed.insert`;
`#[allow(deprecated)] store.set_value(iter, 4, chevron_pixbuf(expanded))`;
when EXPANDED only: catch-up walk `iter_children` — child kind d &&
child rel ∉ closed → `view.expand_row(child_path, false)`. (Activated's
dir branch calls expand/collapse_row; the SIGNAL does the bookkeeping —
python-exact.)

`Activated(path)` (python `_on_activated` 342-355):
1. toggle-hit: `px = view.press_xy().0`; if px ≥ 0 AND cell area is
   Some: lazy tog_w once (`#[allow(deprecated)]
   tog.preferred_width(&view).1`, fallback 24); `ca = #[allow(deprecated)]
   view.cell_area(&path, Some(&col))`; `if treehover::toggle_hit(px,
   ca.x, tog_w) → sender.input(RowToggled); return` (T2's free fn owns
   the x0 = ca.x + 16 arithmetic; non-checkbox presses fall through).
2. dir row (kind d, rel nonempty): `if closed.contains(rel) →
   view.expand_row(path, false)` — children are pre-loaded (non-lazy
   store) — `else → view.collapse_row(path)`; no closed bookkeeping
   here (the RowExpanded/RowCollapsed signal owns it).
3. file row: `sender.output(GitViewOutput::Open { rel, letter })`.
`CommitClicked` (python 315-331, port exactly):

```rust
if self.busy || self.root.is_none() { return; }   // guard (python-exact)
let paths: Vec<String> = self.checked.iter().cloned().collect(); // BTreeSet = sorted
if paths.is_empty() && self.ahead <= 0 {
    sender.output(Status(GitStatusKind::Err, "No files selected".into()));
    return;
}
let msg = self.msg.text().trim().to_string();
if !paths.is_empty() && msg.is_empty() {
    sender.output(Status(GitStatusKind::Err, "Empty commit message".into()));
    return;
}
// root SNAPSHOT now (review I4: set_root mid-flight must not reroute it)
let root = self.root.clone().unwrap();
self.busy = true;     // EAGER — buttons dead before the editor flush loop returns
self.buttons();
sender.output(Status(GitStatusKind::Busy, if paths.is_empty() { "Pushing…" } else { "Committing…" }.into()));
sender.output(Busy(true));
sender.output(Commit { root, paths, msg }); // → editor SaveOpen → Flushed → StartCommit
```

`update_cmd` (`ComponentSender<ChangesView>`):

```rust
match message {
    GitViewCommand::Phase => sender.output(Status(GitStatusKind::Busy, "Pushing…".into())),
    GitViewCommand::Landed(out) => {
        self.busy = false; self.flight = false; self.buttons(); self.msg.set_sensitive(true);
        if out.cok { self.msg.set_text(""); self.refresh(true); } // user ruling: message dies with the commit
        if !out.cok { sender.output(Status(GitStatusKind::Err, out.ctext));
        } else if !out.pok { sender.output(Status(GitStatusKind::Err, out.ptext)); // red line explains the count on next refresh
        } else { sender.output(Status(GitStatusKind::Ok, "Pushed ✓".into())); }
    }
}
```

`set_busy` — python's `_set_busy` reduces to `_buttons()` + msg
sensitivity; both above.

Deprecation discipline: file-top `#![allow(deprecated)]` in gitview.rs
(tree cell + TreeView expand APIs are deprecated wholesale on this
gtk4-rs) — skip the inline `#[allow(deprecated)]` attributes in the
snippets above.

`badges.rs`: delete the three `#[allow(dead_code)]` lines (letter_pixbuf /
CARD_BG / tile_bg) — consumer lands here.

- [ ] **Step 4: GREEN**

Run: `cargo test --manifest-path alpaca-code-rs/Cargo.toml 2>&1 | tail -5`
Expected: PASS — gitview helpers + Task 1/2 + full S1 suite untouched.

- [ ] **Step 5: gate + record**

Run: `cargo build --manifest-path alpaca-code-rs/Cargo.toml 2>&1 | tail -3`
Expected: clean.
Then: `scripts/task-done docs/superpowers/plans/2026-10-07-rust-port-stage2.md 3 -- cargo test --manifest-path alpaca-code-rs/Cargo.toml`
(no commit — standing waiver). Ledger note: widget-layer gate = T8 probe
(pressed-checkbox click path, toggle state under refresh, commit bar
interactions); unit tests pin the three pure helpers only.

---

### Task 4: `branchmenu.rs` — branch pill + Switch/New popover

Port of `alpaca_code/branchmenu.py` (234 L, re-verified above). Hover
timers become GENERATION COUNTERS (timer law — python's
`GLib.source_remove` on an already-fired id is a silent no-op, rust
`SourceId::remove` PANICS). Rust changes one python latent bug: `root` is
a real `SetRoot` input (python wires it nowhere — pill flights would run
on `None` — RULING recorded at execution).

**Files:**
- Create: `alpaca-code-rs/src/branchmenu.rs`
- Modify: `alpaca-code-rs/src/main.rs` (`mod branchmenu;`)
- Test: in-module — none pure (see Ruling below)

**Interfaces:**
- Consumes: `crate::gitstatus::{branch_of, branches, switch, create_switch}`
  (T1 — all `(Option<String> | (bool, str))` signatures per T1); T1
  `GitStatusKind`; `badges::widget_icon` (S1 raster path, blur accepted).
- Produces (T6 consumes):
  - `pub enum BranchMenuMsg { SetRoot(Option<PathBuf>),
    Update(Option<String>), Gate(bool), Pick(String), Create(String),
    SubOpen(u64), SubClose(u64) }` (derive Debug)
  - `pub enum BranchMenuOutput { Status(GitStatusKind, String) }`
  - `pub enum BranchCommand { Landed { ok: bool, text: String,
    okmsg: String } }` (derive Debug)
  - `pub struct BranchMenu` — `#[relm4::component(pub)]`, Root =
    `gtk::Button`.

**Ruling (recorded now, repeated at execution): NOTDD-for-widget-task.** No
pure logic exists in this file — every behavior is widget-layer, and the
git ops are already pinned by T1's tests. Gate = `cargo build` + the T8
probe driving a REAL `Pick("probe-branch")` through gitpanel and asserting
the branch flipped. Ledger the ruling verbatim at task start.

Steps:

- [ ] **Step 1: skeleton + component (test-first does not apply — see Ruling
  above; this is the plan's single testless task)**

Component init port (python `__init__` 17-114):

```rust
#[relm4::component(pub)]
impl Component for BranchMenu {
    type Input = BranchMenuMsg;
    type Output = BranchMenuOutput;
    type Init = ();
    type CommandOutput = BranchCommand;

    fn init(_i: Self::Init, sender: ComponentSender<Self>, root: &Self::Root) {
        let widgets = view_output!();
        view! { [root] -> gtk::Button { set_has_frame: false,
            set_css_classes: &["alpaca-branchbtn"] } }
        // strip: branch.svg icon + name label (python 29-37)
        let strip = gtk::Box::builder().spacing(3).build();
        if let Some(ic) = badges::widget_icon("branch.svg", 16) { strip.append(&ic); }
        let label = gtk::Label::builder()
            .ellipsize(gtk::pango::EllipsizeMode::Middle).build();
        strip.append(&label);
        root.set_child(Some(&strip));
        root.set_visible(false);
```

(the view!/builder mix above is illustrative — follow the filetree.rs
init precedent: `view!` may carry only `#[root]`, widgets built manually
when the view! grammar fights; keep ONE style per file.)

Widgets: `menupage` Box vertical; `newpage` Box vertical margins
8/8/6/6 (start/end/top/bottom); `entry` `gtk::Entry` placeholder
"Branch Name" css `alpaca-msg` size(-1, 30) — `entry.connect_activate
→ sender.input(Create(entry text))`; `switch_row` Button has_frame
false css `alpaca-branchitem`, child strip2 spacing 8 = branch.svg
icon + `Label "Switch Branch"` xalign 0.0 hexpand + chevron.svg icon
(python comment: xalign 0 — labels center by default, floated the
text right); `new_row` Button same shape with plus.svg + "New
Branch"; `stack` vhomogeneous(false) hhomogeneous(false)
add_named(menupage, "menu") add_named(newpage, "new") visible_child =
menupage; `branch_box` Box vertical; `scroll` ScrolledWindow
max_content_height 240 propagate_natural_height true hscrollbar NEVER
child branch_box; `sub` Popover child scroll position RIGHT +
`set_autohide(false)` (python comment: the popup grab fires synthetic
leave/enter → hover ping-pong flicker) + `set_parent(switch_row)`;
`pop` Popover child stack position TOP + `set_parent(root)` +
`connect_closed → Closed`; motion controller on switch_row (enter →
SubEnterRow, leave → SubLeaveRow) + motion controller on scroll
(enter → SubEnterList, leave → SubLeaveList).

State: `root: Option<PathBuf>`, `gate: bool = true`, `flight: bool`,
`open_gen: Cell<u64>`, `close_gen: Cell<u64>`, `open_armed: Cell<bool>`.

`update()` match — python mapping (timers via generation counters; a
timer closure clones `sender`, captures its gen, sends the input):

| Input | Python | Behavior |
|---|---|---|
| `SetRoot(r)` | (latent bug fix) | `self.root = r` |
| `Update(b)` | `update` 117-121 | `root.set_visible(b.is_some())`; `Some(n) && !n.is_empty() → label.set_text(n)` |
| `Gate(g)` | (wiring) | `self.gate = g` |
| `Pick(name)` | `_pick` | `start(switch, name, "Switching…", "Switched ✓")` |
| `Create(name)` | `_create` | text taken UNTRIMMED (python 216-218: entry.get_text(); switch/create_switch trim internally) → `start(create_switch, name, "Creating…", "Branch created ✓")` |
| `SubOpen(g)` | `_open_sub` | `g != open_gen → return`; open_armed=false; `fill_branches()`; `sub.popup()` |
| `SubClose(g)` | `_close_sub` | `g != close_gen → return`; `sub.popdown()` |

`start(op, name, phase, okmsg)` (python `_start` 220-229):

```rust
if self.flight || !self.gate { return; }
let root = self.root.clone();                 // snapshot (review I4 same as gitview)
self.flight = true;
self.pop.popdown();                           // closed signal tidies the submenu
sender.output(Status(GitStatusKind::Busy, phase.to_string()));
sender.spawn_command(move |out| {
    let (ok, text) = op(&root, &name);        // op = gitstatus::switch / create_switch
    out.emit(BranchCommand::Landed { ok, text, okmsg: okmsg.to_string() });
});
```

`update_cmd`: `Landed` → `flight = false`;
`sender.output(Status(if ok { GitStatusKind::Ok } else {
GitStatusKind::Err }, if ok { okmsg } else { text }))` (python `_land`
231-234).

Popover interactions (python 124-152, direct on widgets in handlers):

- button `clicked` → `open_menu()`: `fill_branches()`;
  `stack.set_visible_child(&menupage)`; `root.add_css_class("alpaca-open")`;
  `pop.popup()`.
- `pop.on_closed` → remove `alpaca-open`; `open_gen += 1; close_gen +=
  1; open_armed = false` (python's double `_cancel`); `sub.popdown()`.
- `_to_new_entry` (new_row clicked): `stack.set_visible_child(newpage)`;
  `entry.set_text("")` FIRST (grab select-all law — every open's grab
  must land on an empty field); ONE `entry.grab_focus()` directly,
  NEVER via idle (python 136-147 comments: re-arms forever + re-selects
  after each keystroke).
- hover in/out (controller closures clone sender): `SubEnterRow` →
  `close_gen += 1` (cancel close); `if open_armed || sub.visible() {
  return }`; `open_gen += 1; open_armed = true; let g = open_gen.get()`;
  arm `glib::timeout_add(Duration::from_millis(80))` →
  `sender.input(SubOpen(g))` → ONCE via `ControlFlow::Break`.
  `SubLeaveRow` → `open_gen += 1; open_armed = false;` sched_close:
  `close_gen += 1; arm 250ms → SubClose(g)`.
  `SubEnterList` → `close_gen += 1` (cancel close). `SubLeaveList` →
  sched_close.

`fill_branches` (python `_fill_branches` 188-210, SYNC on UI thread —
python parity, the popup handler blocks on two git subprocess calls,
accepted; same class as refresh-sync):

- `cur = self.root.as_deref().and_then(gitstatus::branch_of)`;
- clear `branch_box` children (first_child + next_sibling walk, remove);
- `names = root.and_then(branches).unwrap_or_default()`; empty → Label
  "No branches yet" xalign 0.5 margins top4/bottom4/start10/end10 css
  `alpaca-hint` (unborn HEAD); append; return;
- else per name: Button has_frame false css `alpaca-branchitem` +
  `alpaca-on` when `Some(name) == cur`; child Label
  `"✓ ".to_string() + name` (current) or name, xalign 0.0 hexpand;
  click → `sender.input(Pick(name))` (per-button closure captures its
  name).

Popover styling law (Global constraint): popovers stay BARE (Breeze
paints `popover > contents` — double border otherwise); only item
classes. And popover-child labels carry their own explicit colors
(pierce law): `alpaca-branchitem` is the surface, no `X label`
descendant selectors — the css already exists.

- [ ] **Step 2: build + task-done**

Run: `cargo build --manifest-path alpaca-code-rs/Cargo.toml 2>&1 | tail -3` —
Expected: clean (widget code cannot fail under cargo test; build is the
compile gate).
Then: `cargo test --manifest-path alpaca-code-rs/Cargo.toml 2>&1 | tail -3` —
Expected: PASS (unchanged suite proves nothing regressed).
Then: `scripts/task-done docs/superpowers/plans/2026-10-07-rust-port-stage2.md 4 -- cargo test --manifest-path alpaca-code-rs/Cargo.toml`
(no commit — waiver). Ledger: Ruling NOTDD-for-widget-task + T8 probe
owns the real Pick flow (branch must flip on the scratch repo).

---

### Task 5: Editor diff pages + the flush handoff (`editor.rs`)

**Files:**
- Modify: `alpaca-code-rs/src/editor.rs` only

**Test:** NONE — **Ruling: NOTDD-for-widget-task** (T4's shape, ledgered at
execution): the page lives entirely in the widget tree (GtkSource buffers,
tags, scrolled pairs, tab strip) — no pure logic can be constructed
displayless. Gate = `cargo build` clean + the T8 probe (CHANGES row click →
side-by-side tinted diff page; disk edit → live renewal; dirty buffer +
Commit → flush handoff lands in StartCommit — one commit, not two).

**Interfaces:**
- Consumes: `gitstatus::{diff_for, parse_unified, build_sides, Sides}` (T1,
  exact signatures in T1's Produces block).
- Produces (T7 consumes):
  - `EditorMsg::OpenDiff { root: PathBuf, rel: String, letter: String }`
  - `EditorMsg::RefreshDiffs { root: PathBuf, letters: Vec<(String, String)> }`
    (rel→fresh-letter rows; App folds them from `Rows`)
  - `EditorMsg::SaveOpen(Vec<PathBuf>)` — DROPS its `#[allow(dead_code)]`
    "S2 stub": real now (abs paths to flush before a commit)
  - `EditorOutput::Flushed` (new variant — emitted after EVERY SaveOpen,
    even with zero flushed pages)
  - `pub enum EditorCommand { DiffData { rel: String, letter: String,
    text: String, binary: bool }, FreshDiffs(Vec<(String, Sides, String)>) }`
    (derive Debug) + `type CommandOutput = EditorCommand;` (replaces
    `type CommandOutput = ();`)

**Design** (python-verified against `alpaca_code/editor.py` 208-380,
`alpaca_code/window.py` 205-240 — verbatim reads):

1. PageState gains `diff_of: Option<String>` (None default), `diff_letter:
   String` ("" default), `diff: Option<DiffWidgets>`; `struct DiffWidgets {
   sw_l: gtk::ScrolledWindow, sw_r: gtk::ScrolledWindow,
   linked_l: Rc<RefCell<Option<gtk::Adjustment>>>,
   linked_r: Rc<RefCell<Option<gtk::Adjustment>>> }` — the linked dedupe is
   `*rc.borrow() == Some(adj.clone())` (`gtk::Adjustment` implements
   PartialEq — filetree.rs:243 precedent, comment there says it). Diff pages
   REUSE the existing PageState widget fields harmlessly: buf = LEFT buffer,
   view = left view, sw = sw_l, tabname/badge_slot as built; `path` stays
   None so save/reload/fs paths guard out (save_active already returns on
   path None — comment "diff page (S2): Ctrl+S is a no-op" already present).
2. `fn page_of_key(&self, key: &str) -> Option<usize>` — matches
   `pg.diff_of.as_deref() == Some(key)`; the exact-path `page_of` is
   untouched (file tabs).
3. `refresh_restamps` first loop line gains `if pg.diff_of.is_some() {
   continue; }` — python `_refresh_tab_state` skips diff pages (no dot/badge
   restamp on a diff head; chip preserved).
4. Constants: `const DIFF_DEL_BG: &str = "#25181c"; const DIFF_ADD_BG: &str
   = "#15261d"; const DIFF_HDR_FG: &str = "#5a6375";` (editor.py 209-211).
5. `fn diff_side(&self, lines: &[String], idxs: &BTreeSet<usize>,
   hdr: &BTreeSet<usize>, hexcol: &str, lang: Option<&sourceview::Language>)
   -> (sourceview::View, sourceview::Buffer)` — python `_diff_side` exact:
   fresh `sourceview::Buffer`, text = lines.join("\n"); attach `self.scheme`
   (same as open_file's buffer setup); `lang` = carried through from the
   caller's guess (set_language when Some). Two tags on the buffer —
   `bg_tag.set_background_rgba(Some(&rgba))` / `set_foreground_rgba` (typed
   setters on GtkTextTag; if the binding lacks one, `set_property(
   "background-rgba"/"foreground-rgba", &rgba)`); rgba = `gdk::RGBA::parse(
   hexcol)` (CSS "#…" parse). Apply loop `for &i in idxs.iter()` — skip
   `i >= buf.line_count() as usize || i == usize::MAX` (python's `i < 0`
   clamp — parse never stores negatives; untracked full-del rows clamp out);
   `let a = buf.iter_at_line(i as i32);` apply bg over `[a, a-forward_to_
   line_end]` (copy the iter, don't move); same loop for `hdr` with the fg
   tag (DIFF_HDR_FG). `buf.set_modified(false)` AFTER the tags — python
   comment "diff tabs never dirty". View: show_line_numbers(false) python
   comment "padded sides falsify numbers", set_editable(false),
   wrap_mode(None), `set_pixels_above_lines(2)`, `set_left_margin(12)`,
   css `alpaca-mono`. Returns both view and buffer (the left buffer becomes
   the PageState buf).
6. OpenDiff arm (python `window._open_changes_diff`, verbatim above):
   `let (rel, letter); let root = root;` root snapshot into the flight
   closure — no guard needed beyond snapshot (python guards its own root
   before dispatch; App passes self.root). `sender.spawn_command` closure:
   `let (text, binary) = gitstatus::diff_for(&root_str, &rel,
   letter == "U").unwrap_or_default()` (None = git failed/timeout → empty
   text → flows into the same "No diff" gate python's empty side hits);
   `CommandOutput::DiffData { rel, letter, text, binary }`.
7. update_cmd DiffData: binary → `self.error(&format!("Binary file:
   {rel}"), "No diff view for binary changes.")` and return; else
   `let sides = gitstatus::build_sides(&gitstatus::parse_unified(&text))`;
   `sides.old.is_empty()` → `self.error(&format!("No diff: {rel}"),
   "git produced no diff hunks for this path.")` and return (python
   `len(sides[0]) == 0` — sides[0] is the OLD side); else
   `self.open_diff(root, &rel, &sides, &letter, sender)`.
8. RefreshDiffs arm (python `_on_git_changed`, verbatim above): collect
   `pages.iter().filter_map(|pg| pg.diff_of.as_deref()? [strip "diff:"
   prefix].zip …)` — per open diff page: rel = &diff_of[5..]; fresh = the
   letters vec's entry for rel (build a small `HashMap<String, String>` from
   the input first) — NO fresh key → skip (resolved tabs stay as-is);
   collect `(rel, fresh_letter)` pairs. `sender.spawn_command`: per pair
   `let (text, binary) = diff_for(..., fresh == "U")`; binary → SKIP the
   pair (python: diff tab stays stale rather than die); sides = build
   sides; push `(rel, sides, fresh)`. CommandOutput::FreshDiffs(Vec<...>).
   update_cmd: `for (rel, sides, letter) in fresh { self.refresh_diff(&rel,
   &sides, &letter, sender) }`. NOTE: python runs this fetch SYNC on the UI
   thread; rust moves it into a flight — an improvement only in
   responsiveness, behavior identical when it lands.
9. `fn open_diff(&mut self, root: &Path, rel: &str, sides: &Sides,
   letter: &str, sender: &ComponentSender<Editor>)` (python 249-303
   verbatim): key = `format!("diff:{rel}")`; `page_of_key(&key)` is Some →
   `#[allow(deprecated)] self.nb.set_current_page(idx)` and return. Else:
   lang = `self.lm.guess_language(basename-of(rel), None)` (basename =
   rel's last '/'+1 segment — python `os.path.basename`); build (vleft,
   bleft) = diff_side(&sides.old, &sides.del, &sides.hdr, DIFF_DEL_BG,
   lang); (vright, bright) = diff_side(&sides.new, &sides.add, &sides.hdr,
   DIFF_ADD_BG, lang). sw_l/sw_r ScrolledWindow hexpand+vexpand child
   views; `sep` Box css `alpaca-diffsep`; `pair` Box HORIZONTAL append
   sw_l, sep, sw_r. `page` Box VERTICAL: crumb Label text =
   rel.split('/').collect::<Vec<_>>().join("  ›  ") xalign 0.0, ellipsize
   MIDDLE, css `alpaca-breadcrumb`; then pair. `slot` Box spacing 4 valign
   Center — `if !letter.is_empty() { chip Label letter css
   [alpaca-diffchip<lower>] }`; `name` Label basename(rel) css
   `alpaca-tabname` ellipsize MIDDLE set_size_request(72, -1); `close`
   Button built the construction as open_file's (editor.rs ~425:
   `badges::widget_icon("x-dim.svg", 16)` paintable Image else
   icon_name "window-close-symbolic"; css `alpaca-close`); head Box
   spacing 4 css `alpaca-tab` valign Center append slot, name, close — NO
   dirty dot (python comment: diff tabs never dirty — the dot is an orphan
   Label, constructed for paint_dot parity but never appended to the head).
   `#[allow(deprecated)] nb.append_page(&page, Some(&head))` then
   `#[allow(deprecated)] nb.set_current_page(Some(idx))` — append does not
   switch (CLAUDE.md law). Close "clicked": `#[allow(deprecated)]
   nb.page_num(&page)` at CLICK time (python-exact) → ClosePage(idx,
   false). Build `diff_widgets = DiffWidgets { sw_l, sw_r, linked_l:
   Rc::new(...None...), linked_r: Rc::new(...None...) }`; wire both
   directions (fn below). `pages.push(PageState { path: None, buf: bleft,
   view: vleft, sw: sw_l, dot: orphan label, tabname, badge_slot: slot,
   fs: None, conflict: false, load_mtime: None, diff_of: Some(key),
   diff_letter: letter.to_string(), diff: Some(diff_widgets) })` —
   field ORDER follows the existing struct. `refresh_restamps`; output
   EditorOutput::StateChanged.
10. `fn refresh_diff(&mut self, rel: &str, sides: &Sides, letter: &str,
    sender: &ComponentSender<Editor>)` (python 320-360 verbatim):
    page_of_key(Some) else return. Fracs FIRST: per sw (l then r)
    `adj = sw.vadjustment(); span = (adj.upper() - adj.page_size()).max(0.0);
    frac = if span > 0.0 { (adj.value() / span).min(1.0) } else { 0.0 }`.
    lang per basename(rel). Rebuild BOTH sides fresh via diff_side; set_child
    on both sw; `_wire(&v_l, &v_r, ...)` + `_wire(v_r, v_l)` — wire takes
    two VIEWS (dst view captured at wire time; set_child made them fresh a
    line before). Per side one `glib::idle_add_local_once` scroll_frac:
    fetch vadj AT RUN TIME, span as above, `set_value(min(frac * span,
    span))` — comment "unallocated adj reads 0-span → top (acceptable for
    brief churn)" python kept. Chip rebuild when `letter != pg.diff_letter`:
    empty slot (remove children walk first_child/next_sibling — python
    338-344), then chip Label letter css when letter nonempty; update
    `pg.diff_letter`. `refresh_restamps` NOT called here (python doesn't) —
    output StateChanged last.
11. Cross-link fns (python `_diff_link` + `_diff_resync` + `_wire_diff_
    scroll`, verbatim above, all comments carried):
    - `fn diff_link(v: &sourceview::View, dst: &sourceview::View,
      linked: &Rc<RefCell<Option<Adjustment>>>) -> bool`: `let va =
      v.vadjustment(); if *linked.borrow() == Some(va.clone()) { return
      false } // already on the live adj`; `*linked.borrow_mut() =
      Some(va.clone())`; `va.connect_value_changed(clone!(#[weak] dst, move
      |a| { dst.vadjustment().set_value(a.value()) }))` — comment "equal-
      value set_value fires no changed signal → no loop". true.
    - `fn wire_diff(view: &sourceview::View, dst: &sourceview::View,
      linked: &Rc<RefCell<Option<Adjustment>>>)`: diff_link once; then
      `view.connect_notify_local(Some("allocation"), …)` — closure re-runs
      diff_link; when it returns true ALSO one `glib::idle_add_local_once`
      retry ("cover a post-emit swap in the same frame" — python `_diff_
      resync`). The measured law in the doc comment stays verbatim: "the
      ScrolledWindow swaps the view's placeholder adjustment for its own at
      the page's first allocation, WITHOUT any property notify (measured on
      this build: notify::vadjustment never fires on view or sw) — retry the
      link on every allocation change, deduped by adj identity (this build's
      `Adjustment` PartialEq — filetree.rs:243)". Closures clone the
      `Rc<RefCell<…>>` and #[weak] both views — pages may close.
12. SaveOpen arm — REAL now (drop the S2-stub allow): for `idx in 0..
    pages.len()`: page path is Some AND `paths.contains(path)` AND
    `buf.is_modified()` → `let _ = self.write_page(idx);` (write errors
    SWALLOWED — python leaves the OSError unguarded on its save_open, and
    python's callback chain needs no ack at all; rust's write error must not
    kill the commit chain — **Ruling: SaveOpen write errors swallowed;
    Flushed always fires — ledgered at execution**). Then ALWAYS
    `sender.output(EditorOutput::Flushed)` — even with zero pages flushed
    (the ack drives T7's pending-commit chain; a missing ack would strand
    the eager-busy buttons).

**Steps:**
- [ ] **Step 1: Implement** — per design above (testless ruling cited here
      and in the ledger).
- [ ] **Step 2: Build** — `cargo build --manifest-path
      alpaca-code-rs/Cargo.toml` → clean `Finished`.
- [ ] **Step 3: Suite** — `cargo test --manifest-path
      alpaca-code-rs/Cargo.toml` → all pass (S1 + T1-T4 untouched).
- [ ] **Step 4: task-done** — ledger line notes the NOTDD ruling + that
      widget behavior is gated by the T8 probe.

---

### Task 6: GitPanel card — mode strip, shared search, status row, child wiring
(`gitpanel.rs` + filetree trim + app swap)

**Files:**
- Create: `alpaca-code-rs/src/gitpanel.rs`
- Modify: `alpaca-code-rs/src/main.rs` (add `mod gitpanel;` next to the
  S1/mod list — T1/T2/T3/T4 appended theirs at their own tasks)
- Modify: `alpaca-code-rs/src/filetree.rs` (chrome trim + Count output)
- Modify: `alpaca-code-rs/src/app.rs:63,200-216,223-236,366-394` (browser →
  gitpanel swap; the is-active hook is T7's)

**Test:** NO new unit tests — **Ruling: NOTDD-for-widget-task** (T4/T5's
shape; the panel is a widget collage over already-tested pure calls). Gate
= full `cargo test` green (the filetree trim must not regress S1 tests),
`cargo build` clean, T8 probe drives the flights.

**Interfaces:**
- Consumes: `gitstatus::{branch_of, changes, ahead, Rows, GitStatusKind}` (T1);
  `FileTree::{FileTree, FileTreeMsg, FileTreeOutput}` (existing — trimmed in
  this task); `GitView` (T3) `GitViewMsg::{SetRoot, Refresh{keep},
  Apply{raw, ahead, keep}, Filter, StartCommit{root, paths, msg}}` +
  `GitViewOutput::{Status(GitStatusKind, String), Busy(bool),
  Commit{root, paths, msg}, Open{rel, letter}}`; `BranchMenu` (T4)
  `BranchMenuMsg::{SetRoot, Update, Gate}` + `BranchMenuOutput::Status(GitStatusKind, String)`.
- Produces (T7 consumes): component `GitPanel` with
  `pub enum GitPanelMsg { SetRoot(PathBuf), ModeTree, ModeChanges,
  EntryChanged(String), ChildStatus(GitStatusKind, String),
  ChildBusy(bool), Count(usize), ProbeTick, PulseFired(u64), RefreshGit,
  StartCommit { root: PathBuf, paths: Vec<String>, msg: String } }` and
  `pub enum GitPanelOutput { OpenFile(PathBuf), OpenDiff { rel: String,
  letter: String }, Commit { root: PathBuf, paths: Vec<String>,
  msg: String }, GitChanged(Rows) }` + `pub enum GitPanelCommand {
  ProbeLanded { root: String, raw: Option<Rows>, ahead: usize } }` (derive
  Debug) with `type CommandOutput = GitPanelCommand;`.

**Design** (python-verified against `alpaca_code/filetree.py` 64-360 —
verbatim reads; python "FileBrowser" is this whole card):

1. Root widget: `view! { #[root] gtk::Box { set_orientation: Vertical;
   set_css_classes: &["alpaca-card"]; set_overflow:
   gtk::Overflow::Hidden } }` — comment "clip children to the card's
   rounded corners (python set_overflow)". The card css MOVES here from
   FileTree (block 11).
2. Child components launched in init BEFORE the widgets are assembled
   (relm4 idiom: `Controller::builder().launch(()).detach()`), their
   receivers routed with one `connect_receiver` closure each:
   - `ft = FileTree` — `FileTreeOutput::OpenFile(p) →
     sender.output(GitPanelOutput::OpenFile(p))`;
     `FileTreeOutput::Count(n) → sender.input(GitPanelMsg::Count(n))`.
   - `changes = GitView` — `Status(kind, text) →
     sender.input(GitPanelMsg::ChildStatus(kind, text))`;
     `Busy(b) → sender.input(ChildBusy(b))`;
     `Commit{root, paths, msg} → sender.output(GitPanelOutput::Commit
     {root, paths, msg})`; `Open{rel, letter} → sender.output
     (GitPanelOutput::OpenDiff{rel, letter})`.
   - `branch = BranchMenu` — `Status(kind, text) →
     sender.input(ChildStatus(kind, text))`.
   - The three `Controller`s are KEPT in GitPanel state (App-stores-
     browser precedent, app.rs:63) — T8's probe drives child inputs
     through them.
   - `#[derive(Debug, Clone)] pub enum ProbeChild { Tree(FileTreeMsg),
     Changes(GitViewMsg), Branch(BranchMenuMsg) }`; `GitPanelMsg::
     ProbeChild(ProbeChild)` forwards verbatim via the stored controller
     — **probe-only driver for T8 (production never sends it — comment +
     ledger ruling)**. Child outputs still route through the panel:
     probe-driven flights behave exactly like real ones.
3. Card chrome (python 89-126 + 170-195 values verbatim), top to bottom
   in the root:
   - `head` Box HORIZONTAL spacing 4, margins start 14 / end 10 / top 8 /
     bottom 6; child `Label "File Browser"` xalign 0.0, hexpand, css
     `alpaca-panel-title`.
   - `entry` SearchEntry placeholder "Search files…", css
     `alpaca-search`, size_request(-1, 30) — python comment: "30px floor:
     the entry renders ~36 (Breeze searchentry min, swept to the floor in
     main.py CSS) — the request only keeps it honest below that" —
     hexpand false, vexpand false, margins start 12 / end 12 / bottom 6,
     halign Fill; `connect_search_changed →
     sender.input(EntryChanged(text.into()))`.
   - `btns` strip Box HORIZONTAL spacing 10, margin_top 6, hexpand, css
     `alpaca-modetabs`; `ws_btn = Button "WORKSPACE"`, `ch_btn = Button
     "CHANGES"` — both has_frame(false), css `["alpaca-tabbtn"]` +
     `["alpaca-on"]` on ws_btn ONLY; `ch_btn.set_visible(false)` —
     python comment "outside-repo policy (§2); _refresh_status decides";
     clicks → `sender.input(ModeTree / ModeChanges)`.
   - `stack` Stack transition_type NONE, vexpand true;
     `add_named(Some(ft.widget()), "tree"); add_named(Some(
     changes.widget()), "changes");` — python comment: "mode host (§1):
     zero-transition swap between the tree and the changes view. vexpand
     on the Stack — GtkBox gives non-expanding children only their
     minimum, and the card must fill below the tab row"; explicit
     `set_visible_child_name("tree")` after both adds.
   - `bar` status row (python 177-195): Box HORIZONTAL spacing 6, margins
     start 14 / end 12 / top 0 / bottom 0, css `alpaca-statusbar`;
     children IN PYTHON ORDER: `bar.append(branch.widget())` (the
     BranchMenu button — python appends `self.branchbtn` FIRST);
     `dot` Box 8×8 css `alpaca-status-dot` valign Center, visible false;
     `spin` Spinner 10×10 css `alpaca-status-spin` valign Center, visible
     false; `git_label` Label "" ellipsize MIDDLE, visible false; `spacer`
     Box hexpand (python comment "spacer pushes file count right");
     `count_label` Label "".
4. GitPanel state fields: `root: Option<PathBuf>`, `mode: &'static str`
   ("tree"), `git_busy: bool` (a commit/push flight owns the row —
   python `_git_busy` comment moves here), `probe_busy: bool`,
   `changes_busy: bool` (set from ChildBusy), `pulse_gen: u64` (the
   status pulse's generation counter — python `_pulse_id`;
   SourceId::remove is FORBIDDEN — Global Constraint), `head_mon:
   Option<gio::FileMonitor>`; widget refs `entry, dot, spin, git_label,
   count_label, ws_btn, ch_btn, stack`; child controllers `ft, changes,
   branch`.
5. Probe timer armed ONCE in init (python `_git_tick` recurrence;
   docstring essence moves here): "every 2s the porcelain status runs in
   a worker thread and lands on the UI thread — covers agent edits
   anywhere in the tree (dir monitors only see dirs we've opened).
   Skipped while a flight owns the row or a probe/commit is busy."
   `glib::timeout_add(Duration::from_millis(2000), … sender.input
   (ProbeTick); glib::ControlFlow::Continue)` — timer law (never
   timeout_add_seconds).
6. update arms — SetRoot follows python's `set_root` (198-215) step for
   step:
   - `SetRoot(root)`: HEAD-mon cancel FIRST
     (`if let Some(m) = self.head_mon.take() { m.cancel() }` — python
     cancels its monitors at set_root top); `self.root = Some(root.
     clone())`; `entry.set_text("")` (fires EntryChanged("") queued —
     python RELIES on that same fire, comment "dead before set_text
     fires a synchronous search-changed" moves here); 
     `self.ft.emit(FileTreeMsg::SetRoot(root.clone()))` (populate +
     count — count arrives back as Count); `self.set_mode("tree",
     sender)` (python calls `_set_mode("tree")` explicitly — no-op when
     already tree); `self.changes.emit(GitViewMsg::SetRoot(root.clone
     ()))`; `self.branch.emit(BranchMenuMsg::SetRoot(Some(root.clone
     ())))` — **Ruling: T6 wires BranchMenu.SetRoot — python's `root`
     attr is never fed (latent bug, branchmenu.py); T4's fix lands
     here — ledger.** HEAD monitor: `let head = root.join(".git").join
     ("HEAD"); if head.is_file() { let mon = gio::File::for_path(&head).
     monitor_file(gio::FileMonitorFlags::empty(), None); mon.connect_
     changed(→ sender.input(RefreshGit)); self.head_mon = Some(mon); }`
     (python `_head_mon`'s changed → `refresh_branch` = `_refresh_
     status`; Rust's single RefreshGit input covers focus + monitor —
     python both arms do only `_refresh_status`); `self.refresh_
     status(sender)`.
   - `EntryChanged(text)`: tree mode →
     `self.ft.emit(FileTreeMsg::Search(text))`; changes mode →
     `self.changes.emit(GitViewMsg::Refresh{keep: true})` then
     `.emit(GitViewMsg::Filter(text))`.
   - `ModeTree` / `ModeChanges` → `self.set_mode("tree"/"changes",
     sender)`.
   - `ChildStatus(kind, text)` → `self.show_git_status(kind, text,
     sender)`.
   - `ChildBusy(b)` → `self.changes_busy = b`.
   - `Count(n)` → `count_label.set_text(&format!("{n} files"))`.
   - `ProbeTick`: gate `let Some(root) = self.root.clone() if !self.
     probe_busy && !self.git_busy && !self.changes_busy` (python `_git_
     tick` gate incl. `not self.changes._busy`); root snapshot;
     probe_busy = true; `sender.spawn_command`: `let raw =
     gitstatus::changes(&root_str); let ahead = if raw.is_some()
     { gitstatus::ahead(&root_str) } else { 0 };` → `GitPanelCommand::
     ProbeLanded { root: root_str, raw, ahead }` (python comment "a
     workspace switch mid-flight drops it" moves here).
   - `GitPanelCommand::ProbeLanded`: `self.probe_busy = false` FIRST;
     guard `root != self.root || self.git_busy || self.changes_busy` →
     return (python `_git_landed` 346-349 exact, same three terms; root
     compares as String vs Option<String> — map first); then
     `paint_status(gitstatus::branch_of(&root_str),
     raw.as_ref().map(|r| r.len()), sender)`; `if raw.is_some() {
     sender.output(GitPanelOutput::GitChanged(raw)) }`; `if self.mode
     == "changes" { self.changes.emit(GitViewMsg::Apply{raw, ahead,
     keep: true}) }` (python 351-355 order).
   - `PulseFired(g)`: `if g == self.pulse_gen { self.refresh_status
     (sender) }` — python `_pulse` ("End of the ok pulse: stop owning
     the row, re-sync to the real state").
   - `RefreshGit` → `self.refresh_status(sender)` (window focus + HEAD
     monitor, python both = `_refresh_status`).
   - `StartCommit{root, paths, msg}` → `self.changes.emit(GitViewMsg::
     StartCommit{root, paths, msg})` (T7's flush chain lands here).
   - `ProbeChild(pc)` → match pc: Tree(msg) → `self.ft.emit(msg)`;
     Changes(msg) → `self.changes.emit(msg)`; Branch(msg) →
     `self.branch.emit(msg)` (probe-only, block 2).
7. `fn show_git_status(&mut self, kind, text, sender)` (python 221-247
   verbatim; docstring essence moves here — busy → spinner + phase text;
   ok → green dot pulse, the row re-syncs itself after 2s; err → red dot
   + the error's first line, full git output as tooltip; "The busy guard
   keeps mid-flight refreshes (focus hook, HEAD monitor, mode switches)
   from clobbering the spinner"):
   `self.pulse_gen += 1;` (python source_remove + zero — cancel BY
   GENERATION: "a fresh state outranks a stale pulse" comment moves
   here; SourceId::remove forbidden law noted); `let g = self.pulse_
   gen; let snd = sender.clone();`
   `self.git_busy = kind == GitStatusKind::Busy;` Busy arm: spin
   visible + `spin.start()`, dot hidden, git_label text, tooltip
   Some("") — return. Else: `spin.stop()`, spin hidden, dot visible,
   git_label visible; Ok arm: dot css `["alpaca-status-dot", "ok"]`,
   git_label text, tooltip Some(""), arm the pulse — `glib::timeout_add
   (Duration::from_millis(2000), move || { snd.input(PulseFired(g));
   glib::ControlFlow::Break })` with `g` CLOSURE-BAKED from this call
   (the gen IS the token); Err arm: dot css `["alpaca-status-dot",
   "err"]`, text = `text.lines().next().unwrap_or("Git error")`,
   tooltip = text nonempty ? Some(text) : None. After every arm:
   `self.branch.emit(BranchMenuMsg::Gate(!self.git_busy))` — python's
   `allow = lambda: not self._git_busy` re-evaluates per click; the
   rust port pushes the value on every status change.
8. `fn refresh_status(&mut self, sender)` (python `_refresh_status`
   319-329 verbatim; SYNC git on the UI thread — python parity, python
   docstring "one git call, paints via _paint_status and feeds the
   diff-tab hook + open changes view" moves here as the comment):
   `let Some(root) = self.root.as_ref().map(|p| p.to_string_lossy().
   into_owned()) else { return };` — python computes branch=None when
   root is None and paints nothing new; SKIP is this build's cleaner
   empty-card equivalent (a None-root card shows the bare tree).
   Hmm — deviation: python `branch = branch_of(self.root) if self.root
   else None; _paint_status(branch, len raw or None)`. To stay
   python-exact WITHOUT skipping: `let branch = self.root.as_ref().map(
   |p| gitstatus::branch_of(&p.to_string_lossy()) as Option<String>)
   .flatten(); let raw = match branch { Some(_) => gitstatus::changes
   (&root_str), None => None }; self.paint_status(branch.as_deref(),
   raw.as_ref().map(|r| r.len()), sender);` (a None root paints — pill
   hidden etc. — matching python). Use THIS, not the skip.
   `if raw.is_some() { sender.output(GitPanelOutput::GitChanged(raw.
   clone())) }`; `if self.mode == "changes" { self.changes.emit(Apply
   { raw, ahead: gitstatus::ahead(&root_str), keep: true }) }`.
9. `fn paint_status(&self, branch: Option<&str>, dirty: Option<usize>,
   sender)` (python `_paint_status` 288-317 verbatim; comments move):
   `if self.git_busy { return } // commit/push flight owns the row
   (pulse re-syncs)`; `is_git = branch.is_some()`;
   `self.branch.emit(BranchMenuMsg::Update(branch.map(|b| b.into())))`
   — branchmenu input Update(Option<String>) already covers
   visibility+text; dot/spin/git_label `set_visible(is_git)` THEN
   `spin.set_visible(false)` (python comment "idle state: the dot,
   never the spinner"); `ch_btn.set_visible(is_git)`; `if !is_git &&
   self.mode == "changes" { self.set_mode("tree", sender) }` — python
   comment "repo vanished (HEAD deleted) while reading it"; else-if
   `!is_git { return }`; `git_label.set_tooltip_text(Some(""))` —
   python comment "a past err's tooltip must not outlive the row's
   re-sync"; then dirty: None → text "" + dot hidden; 0 → "No changes"
   + dot css `["alpaca-status-dot", "ok"]` + visible; else → `format!
   ("{dirty} changed")` + dot `["alpaca-status-dot", "warn"]` +
   visible. (Python 305-307 comment moves: count = porcelain rows —
   same number the changes view shows; one git call feeds both.)
10. `fn set_mode(&mut self, mode: &'static str, sender)` (python
    `_set_mode` 257-282 verbatim): `if mode == self.mode { return }`
    (python comment "Entering CHANGES re-syncs the list (§2 refresh
    trigger: view becomes active); the selected tab's css class holds
    it bright, hover brightens the idle one."); store mode;
    `stack.set_visible_child_name(mode)`; css swap (`["alpaca-tabbtn"]
    + ["alpaca-on"]` on the own, only tabbtn on the other); entry
    placeholder `"Search files…" if tree else "Filter changes…"`;
    tree arm — python comments move VERBATIM: "the shared entry text
    survives the tab switch (and its view may be displaced by the last
    search): a needle re-filters the tree, an empty box restores the
    displaced sheet instead of leaving the last flat results stuck
    with no way back" — rust routes `self.ft.emit(FileTreeMsg::Search
    (entry text))` in BOTH sub-cases (the tree's own Search("")
    populate+restore = python's two branches; one input, same result).
    changes arm: `changes.emit(Refresh{keep: true})` — python comment
    "fresh data (re-applies the view's own needle)"; then
    `changes.emit(Filter(entry text))` — comment "entry text outranks
    a stale needle (tree-mode search wrote it)".
11. **filetree.rs trim** (chrome leaves for the panel; tree stays alone):
    - view! root Box: DELETE `set_css_classes: &["alpaca-card"]` +
      `set_overflow` (they move to gitpanel — the tree becomes a plain
      vertical wrapper for its ScrolledWindow as the panel's stack
      child; python's stack child is the ScrolledWindow directly; the
      plain wrapper changes no layout).
    - init: DELETE head block (412-426), entry block incl. builder +
      margins (428-438), status-bar block (503-513), entry connect
      (516-519); `entry`/`count_label` out of the struct (387-388) and
      out of the model construction (547-548).
    - `FileTreeOutput` gains `Count(usize)`; `fn count(&self, sender: &
      ComponentSender<Self>)` emits `sender.output(FileTreeOutput::
      Count(n))` instead of setting a label; the two call sites —
      `set_root` (672) and `dir_changed` (814) — gain the sender arg
      from their update arms.
    - `set_root` (660-673) drops its `self.entry.set_text("")` (670)
      with a note "entry cleared by the panel's SetRoot now".
12. **app.rs swap**: field `browser` → `gitpanel: Controller<GitPanel>` at
    struct (63), construction block (200-216: the receiver match handles
    `OpenFile → AppMsg::OpenFile(p)` ONLY; `GitPanelOutput::OpenDiff |
    Commit | GitChanged` → IGNORED for now with comment "T7 wires these
    — arms arrive with AppMsg's OpenDiff/Commit/GitChanged (Task 7)"),
    model init (223-236). `hpane.set_end_child(Some(gitpanel.widget()))`
    keeps position 1161. `set_workspace_now` line 379:
    `self.gitpanel.emit(GitPanelMsg::SetRoot(path.to_path_buf()))`; the
    comment "tree first, then panes" stays, "notify::is-active →
    refresh_git (S2)" becomes true in T7.

**Steps:**
- [ ] **Step 1: filetree.rs trim first** (compiles standalone; `cargo
      test` + `cargo build` green before the wiring is written).
- [ ] **Step 2: gitpanel.rs + `mod gitpanel;` + app.rs swap** per design;
      `cargo test` full green (no S1 regressions), `cargo build` clean.
- [ ] **Step 3: task-done** — ledger line (NOTDD ruling + ProbeChild
      ruling + BranchMenu.SetRoot ruling).

---

### Task 7: App wiring — the commit flush chain + diff-tab renewal + focus hook
(`app.rs`)

**Files:**
- Modify: `alpaca-code-rs/src/app.rs` only (update arms + a field + one
  notify hook + one header import line if `GitPanelOutput` needs naming)

**Test:** NO new unit tests — **Ruling: NOTDD-for-widget-task** (T4/T5/T6's
shape; this task is pure message routing between already-probed
components). Gate = full `cargo test` green, `cargo build` clean, T8 probe
drives the chain end-to-end.

**Interfaces:**
- Consumes: `gitstatus::{Rows}` (T1); `GitPanel::{GitPanelMsg::{RefreshGit,
  StartCommit{root, paths, msg}}, GitPanelOutput::{OpenDiff{rel, letter},
  Commit{root, paths, msg}, GitChanged(Rows)}}` (T6);
  `Editor::{EditorMsg::{OpenDiff{root, rel, letter}, RefreshDiffs{root,
  letters}, SaveOpen(Vec<PathBuf>)}, EditorOutput::Flushed}` (T5);
  existing `AppMsg::{OpenFile, EditorState, …}` arms.
- Produces: the complete S2 app — nothing later consumes this task (T8
  only drives it).

**Design** (python-verified against `alpaca_code/window.py` 128-142 +
205-240, verbatim read):

1. `AppMsg` gains (derive Debug — Rows is Debug):
   - `OpenDiff { rel: String, letter: String }`
   - `Commit { root: PathBuf, paths: Vec<String>, msg: String }`
   - `Flushed`
   - `GitChanged(Rows)`
   - `WindowActive`
2. `App` gains `pending_commit: Option<(PathBuf, Vec<String>, String)>`
   (init `None`); comment: "T7 — the flush chain: Commit → SaveOpen →
   Flushed → StartCommit".
3. Update arms, python chain for each:
   - `OpenDiff{rel, letter}` → python `_open_changes_diff` runs ON the
     window with its own root guard: rust — `if self.root.is_none() {
     return }` (python root guard, the SAME shape);
     `self.editor.emit(EditorMsg::OpenDiff { root: PathBuf::from
     (self.root.as_deref().unwrap().to_owned()), rel, letter })` (App's
     root is `Option<String>` — build the PathBuf at the arm, no fs read).
   - `Commit{root, paths, msg}` → python `gitview.commit_clicked` calls
     `before_commit(paths)` SYNCHRONOUSLY (window.py:128-142 —
     `editor.save_open([os.path.join(root, r) for r in rels])` on the
     same mainloop turn). Rust's relm4 inputs are QUEUE-processed, not
     call-stack: the turn's equivalent is the pending-store + ack chain —
     `self.pending_commit = Some((root.clone(), paths.clone(),
     msg.clone()))`;
     `self.editor.emit(EditorMsg::SaveOpen(paths.iter().map(|rel| root
     .join(rel)).collect()))` (abs joined per rel — python list
     comprehension shape; joining PathBuf paths, no fs reads of dirs).
   - `Flushed` → the editor's Flushed STRICTLY follows its SaveOpen in
     the sequential input queue (same component, FIFO) → sound chain:
     `if let Some((root, paths, msg)) = self.pending_commit.take() {
     self.gitpanel.emit(GitPanelMsg::StartCommit { root, paths, msg }); }`
     — eager-busy closes the duplicate-click window (T5's SaveOpen always
     emits Flushed, zero-flushed included — the chain cannot strand).
   - `GitChanged(rows)` → `self.editor.emit(EditorMsg::RefreshDiffs {
     root: self.root-as-PathBuf-or-skip, letters: rows.iter().map(|(rel,
     letter)| (rel.clone(), letter.clone())).collect() })` — python `_on_
     git_changed` fires unconditionally on rows; rust: skip when self
     .root is None (no diffs can exist anyway — diff pages die with
     set_workspace? NOTE: rust restore on a new root rebuilds pages; a
     stale diff tab across workspaces is impossible — editor's Restore
     path rebuilds from open_tabs which excludes diff pages (T5
     snapshot's path filter). Python diff pages SURVIVE a workspace
     switch (window only re-roots the editor's trees — measured behavior:
     python window._set_workspace_now calls editor.restore which sets each
     page's root-relative paths — diff tabs keep stale content until Git
     Changed refreshes them; and _on_git_changed's letters map won't
     contain their rel — `fresh is None → continue` leaves them as-is).
     RUST RULE: the editor's restore (S1) rebuilds ONLY path pages — a
     diff page's tab strip survives? Rust restore swaps text on
     matched-path pages; a diff page (path None) is left untouched by
     restore → stays open with stale sides — UNTIL GitChanged skips it
     (letters miss the rel). SAME python behavior (stale-but-alive).
     Consistent — keep both.
   - `WindowActive` → `self.gitpanel.emit(GitPanelMsg::RefreshGit)` —
     python `win.connect("notify::is-active", lambda: tree.refresh_git
     ())` (filetree.py:285-287 `refresh_git`).
4. The notify hook in App::init (after widgets, before the startup
   workspace block): `root.connect_notify_local(Some("is-active"), move
   |_, _| { in.input(AppMsg::WindowActive) })` — python fires on EVERY
   notify (both False→True and the rare True→False edge) — keep exactly
   that (each fire costs one sync refresh — python parity, comment).
5. T6's gitpanel receiver match: fill the three IGNORED arms now:
   `GitPanelOutput::OpenDiff{rel, letter} → app_in.input(AppMsg::
   OpenDiff{rel, letter})`; `GitPanelOutput::Commit{root, paths, msg} →
   app_in.input(AppMsg::Commit{root, paths, msg})`;
   `GitPanelOutput::GitChanged(rows) → app_in.input(AppMsg::GitChanged
   (rows))`.

**Steps:**
- [ ] **Step 1: Implement** the five arms + field + hook + receiver fill.
- [ ] **Step 2:** `cargo test` full green; `cargo build` clean.
- [ ] **Step 3: task-done** — ledger line (NOTDD ruling; the chain is
      T8's probe).

---

### Task 8: Probe + docs + whole-branch review + ledger close

**Files:**
- Modify: `alpaca-code-rs/src/app.rs` (probe-only step arms + `probe_s2`
  driver)
- Create: `/tmp/probe_s2.sh` (throwaway wrapper — never in the repo)
- Modify: `CLAUDE.md` (Layout rows — the ONE python-side file this port
  may touch, per the S1 precedent)
- Ledger: `.superpowers/sdd/2026-10-07-rust-port-stage2/progress.md`

**Interfaces:**
- Consumes: everything Tasks 1-7 built; `std::env::var`; the git binary
  in fixtures (`git init -b main`, `--bare` remote clone).
- Produces: a green S2 close-out. `AppMsg::ProbeS2(PathBuf)` +
  `AppMsg::ProbeS2Step` (probe-only; production never sets
  `ALPACA_PROBE_S2`)+ App fields `probe_ws: Option<(PathBuf, usize)>`
  (probe-only state, default None).

**Probe design** (T8's probe REPLACES the deferral note in T3/T4's
"Ruling" lines; it gates BOTH widget-heavy tasks with one desktop pass):

1. `App::update` arms:
   - `ProbeS2(dir)` → `self.probe_ws = Some((dir, 0)); self.probe_step
     (0, sender)` — schedule the first step.
   - `ProbeS2Step` → `let Some((ws, step)) = &self.probe_ws else {
     return };` drive per step (below), then `*step += 1;` and schedule
     the next: `let ms = STEP_MS[step]; let snd = sender.clone();
     glib::timeout_add(Duration::from_millis(ms), move || {
     snd.input(AppMsg::ProbeS2Step); glib::ControlFlow::Break });` —
     generation-free (each step timer BREAKs — one chain, no re-arm
     hazard).
   - Both arms carry the comment "PROBE-ONLY (ALPACA_PROBE_S2) — never
     runs in production (env unset)".
2. `App::init` tail (after the startup workspace block):
   `if let Ok(dir) = std::env::var("ALPACA_PROBE_S2") { sender.input
   (AppMsg::ProbeS2(PathBuf::from(dir))); }`
3. Step table (STEP_MS = gaps `[500, 500, 350, 100, 2750]` relative to
   each step's own fire; cumulative offsets listed per line; each step =
   real component inputs ONLY):
   0. +300ms: `gitpanel.emit(SetRoot(wsA))`
   1. +800ms: `gitpanel.emit(ModeChanges)` (refresh lands with data —
      changes view's own Refresh is sync)
   2. +1300ms: select-all via the REAL row-toggle law:
      `gitpanel.emit(GitPanelMsg::ProbeChild(ProbeChild::Changes
      (GitViewMsg::RowToggled(gtk::TreePath::from_str("0").unwrap()))))
      ` — row 0 is
      the masthead (T3's fill order); toggled_checked("s") + visible
      does the rest. Probe assertion (stderr print): all rows checked.
   3. +1650ms: `GitPanelMsg::ProbeChild(ProbeChild::Changes(GitViewMsg::CommitClicked))` → the
      FULL production chain fires (validate → eager busy → Commit out →
      App:Commit → SaveOpen → Flushed → panel StartCommit → flight →
      Landed → statuses → pulse).
   4. +1750ms: MID-FLIGHT injection (the Review Focus #1 case):
      `sender.input(AppMsg::SetWorkspace(wsB))` — wsB is its own repo
      (clean tree). The flight's root was SNAPSHOT into StartCommit's
      payload (T3) — the commit still lands on wsA; wsB must receive
      NOTHING. If the queued queue beat the flight, wsA still ends with
      exactly one probe commit — assert both repos (either interleaving).
   5. +4500ms: `sender.spawn_command` — assertion command; each line
      `println!("PROBE assert: <name>: PASS/FAIL")`:
      - `git -C wsA rev-list --count HEAD` == 1 AND the commit message
        is the probe's (verify with rev-list --format)
      - `git -C wsB rev-list --count HEAD` == 0
      - the bare remote's refs (ls-remote) show the pushed branch — the
        rev-list count through the remote == 1 (push succeeded or the
        branch was already there)
      - state.json: last_project == wsB (the REAL SetWorkspace's own
        write; the schema keeps every other key intact)
      - zero-flush: the probe NEVER opened a file — SaveOpen flushes
        zero pages yet the chain completed (Flushed-ack-always, T5's
        ledger ruling — wsA's commit IS the proof)
      - the app is still alive (this code IS alive — trivial)
      final line `println!("PROBE S2 DONE")`.
4. `/tmp/probe_s2.sh` (wrapper): builds the fixtures (wsA repo with
   `git init -b main`, one committed file, then a modified file + an
   untracked file pre-staged; `git clone --bare` → remote; `remote add
   origin` in wsA; wsB a second `git init` repo), writes the probe
   state.json (`last_project: null`), runs the binary with
   `ALPACA_PROBE_S2=/tmp/probe_wsA` under a fresh stderr log, waits ~10s,
   kills the EXACT pid (never pkill -f — memory: self-match kills own
   shell), then re-runs the git assertions itself and greps the log for
   the PROBE PASS/DONE lines; prints an overall PASS/FAIL.

**Docs — CLAUDE.md** (rust Layout rows, added under the S1 module list;
keep one line each, existing comment style):

- `gitstatus.rs` — pure git porcelain/diff parser + subprocess callers
  (`branch_of/branches/switch/create_switch/changes/parse_*/diff_for/
  commit/ahead/commit_then_push/group_tree`, git_run timeouts 5-120s).
- `treehover.rs` — HoverTree + press capture + toggle hit-test moved out
  of filetree.rs (press_xy, toggle_hit).
- `gitview.rs` — CHANGES view: tree + checkbox column + Select-all row +
  commit bar; CommitClicked validation guards + flight (Phase/Landed).
- `branchmenu.rs` — branch status-bar pill + Switch/New-Branch hover
  popover (80/250ms hysteresis timers, generation-cancelled).
- `gitpanel.rs` — WORKSPACE/CHANGES mode card: shared search entry +
  status row (branch pill, dot, spinner, count) + 2s live probe; hosts
  the file tree, changes view, branch pill.
- editor.rs diff pages: `OpenDiff` side-by-side tinted mono views
  (cross-linked scroll via allocation-notify re-link — the measured
  placeholder-swap law), `SaveOpen` flush chain, `Flushed` ack.
- app.rs: Commit → `pending_commit` → SaveOpen → Flushed → StartCommit;
  `notify::is-active` → gitpanel RefreshGit.

(Also add the two missing PYTHON module rows the S2 python work added
earlier tonight: `gitstatus.py` — git call helpers feeding all git
views; `gitview.py` — python CHANGES view. The rust rows are new from
Tasks 1-6, so the doc reflects tonight's git features on both sides.)

**Steps:**
- [ ] **Step 1:** full `cargo test` green, `cargo build` clean.
- [ ] **Step 2:** CLAUDE.md doc rows (commit step skipped — waiver).
- [ ] **Step 3:** write `/tmp/probe_s2.sh` + the probe arms; `cargo
      build`; run the wrapper on the throwaway repos; read its PASS
      lines; iterate until green (systematic-debugging if a step drops).
- [ ] **Step 4:** desktop visual pass: launch on ANOTHER throwaway
      scratch repo, spectacle screenshots asserting CHANGES-mode
      visuals (checkbox column, chips, masthead, commit bar) and a
      non-git dir asserting the pill/tab hiding; kill by exact pid.
- [ ] **Step 5:** review-package + fresh code-reviewer dispatch (the
      executing-plans final-review step; the reviewer weighs the
      ledger's Ruling lines).
- [ ] **Step 6:** re-grade → ONE fix pass for Critical/Important (each
      RED→GREEN + green suite); minors → ledger `Final: minor
      (deferred)`.
- [ ] **Step 7:** ledger close + `finishing-a-development-branch`;
      final message carries Rulings I made + Deferred minors (the
      handoff's five state-contract minors re-appear here).