//! Git surface — branch/status/porcelain/diff/commit/push, python
//! `alpaca_code/gitstatus.py` port. Pure (no gi imports): safe to run and test
//! displayless. Every call is `git -C root`; never raises (returns error states).

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub type Row = (String, String);
pub type Rows = Vec<Row>;

/// Status-row vocabulary (python's on_status kinds) — consumed by
/// gitview/branchmenu/gitpanel (S2 landed them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitStatusKind {
    Busy,
    Ok,
    Err,
}

pub const HUNK_ROW: &str = "@@ … @@";

pub struct TreeRow {
    pub kind: &'static str,
    pub name: String,
    pub rel: String,
    pub letter: String,
}
#[derive(Clone, Debug)]
pub struct Hunk {
    pub lines: Vec<(char, String)>,
}
/// editor diff sides — old/new equal-length lines + 0-based tint sets.
#[derive(Clone, Debug)]
pub struct Sides {
    pub old: Vec<String>,
    pub new: Vec<String>,
    pub del: BTreeSet<usize>,
    pub add: BTreeSet<usize>,
    pub hdr: BTreeSet<usize>,
}
/// commit_then_push result: (commit_ok, commit_text, push_ok, push_text);
/// push fields are (false, "") when no push was attempted. ptext — the
/// Landed handler displays it when pok is false.
#[derive(Clone, Debug)]
pub struct FlightOut {
    pub cok: bool,
    pub ctext: String,
    pub pok: bool,
    pub ptext: String,
}

// ------------------------------------------------------------------------------
// subprocess runner — one shared helper, replaces python's six try/except blocks
// ------------------------------------------------------------------------------

fn has_git(root: &str) -> bool {
    !root.is_empty() && std::path::Path::new(root).join(".git").is_dir()
}

fn clip400(s: &str) -> String {
    s.trim().chars().take(400).collect()
}

/// python `(stderr or stdout or fallback)` — raw, unstripped.
fn either(r: &std::process::Output, fallback: &str) -> String {
    let se = String::from_utf8_lossy(&r.stderr).to_string();
    if !se.is_empty() {
        return se;
    }
    let so = String::from_utf8_lossy(&r.stdout).to_string();
    if !so.is_empty() {
        return so;
    }
    fallback.to_string()
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

// ------------------------------------------------------------------------------
// branch surface · gitstatus.py 7-73
// ------------------------------------------------------------------------------

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
    // menu-open sync call — 5s timeout like the status row
    let r = git_run(root, &["for-each-ref", "refs/heads", "--sort=refname",
                             "--format=%(refname:short)"], false,
                    Duration::from_secs(5))?;
    if !r.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&r.stdout);
    Some(s.lines().filter(|l| !l.is_empty()).map(|l| l.to_owned()).collect())
}

fn _checkout(root: &str, args: &[&str]) -> (bool, String) {
    let Some(r) = git_run(root, args, true, Duration::from_secs(60)) else {
        // python embeds the TimeoutExpired exception text; stable text here
        return (false, "git checkout failed: timed out".into());
    };
    if !r.status.success() {
        let text = either(&r, "git checkout failed");
        return (false, clip400(&text));
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
/// name; the leading-dash guard keeps a "-force" request from being parsed as
/// flags. python create_switch also strips whitespace around the name.
pub fn create_switch(root: &str, name: &str) -> (bool, String) {
    let name = name.trim();
    if name.is_empty() || name.starts_with('-') {
        return (false, "Bad branch name".into());
    }
    _checkout(root, &["checkout", "-b", name])
}

// ------------------------------------------------------------------------------
// changes view ops · gitstatus.py 75-156   (status() is DEAD in python — no port)
// ------------------------------------------------------------------------------

/// ^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@ — python re.match, no end
/// anchor: everything after the closing "@@" is ignored.
fn hunk_head(line: &str) -> Option<(usize, usize, usize, usize)> {
    fn num(s: &str) -> Option<(usize, &str)> {
        let nd = s.bytes().take_while(|b| b.is_ascii_digit()).count();
        if nd == 0 {
            return None;
        }
        let n: usize = s[..nd].parse().ok()?;
        Some((n, &s[nd..]))
    }
    let s = line.strip_prefix("@@ -")?;
    let (a_start, s) = num(s)?;
    let (a_n, s) = match s.strip_prefix(',') { Some(r) => { let (n, r) = num(r)?; (n, r) } None => (1, s) };
    let s = s.strip_prefix(' ')?;
    let s = s.strip_prefix('+')?;
    let (b_start, s) = num(s)?;
    let (b_n, s) = match s.strip_prefix(',') { Some(r) => { let (n, r) = num(r)?; (n, r) } None => (1, s) };
    s.strip_prefix(" @@")?;
    Some((a_start, a_n, b_start, b_n))
}

/// Unified diff → hunks; kinds '=' context / '<' removed / '>' added. File
/// headers, mode/summary lines and no-newline markers ('\\') are skipped;
/// unknown lines inside a hunk are ignored — never raises.
pub fn parse_unified(text: &str) -> Vec<Hunk> {
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut in_hunk = false;
    for line in text.split('\n') {
        if let Some(_) = hunk_head(line) {
            hunks.push(Hunk { lines: Vec::new() });
            in_hunk = true;
            continue;
        }
        if !in_hunk {
            continue;
        }
        let Some(h) = hunks.last_mut() else { continue };
        if let Some(rest) = line.strip_prefix('+') {
            h.lines.push(('>', rest.to_string()));
        } else if let Some(rest) = line.strip_prefix('-') {
            h.lines.push(('<', rest.to_string()));
        } else if let Some(rest) = line.strip_prefix(' ') {
            h.lines.push(('=', rest.to_string()));
        }
    }
    hunks
}

/// (old_lines, new_lines, del_idx, add_idx, hdr_idx) — ready-built editor
/// sides (0-based line indexes for tinting). Equal lengths by construction:
/// every event feeds exactly one side and pads the other.
pub fn build_sides(hunks: &[Hunk]) -> Sides {
    let (mut old, mut new) = (Vec::new(), Vec::new());
    let (mut dl, mut al, mut hdr) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    for hunk in hunks {
        old.push(HUNK_ROW.to_string());
        new.push(HUNK_ROW.to_string());
        hdr.insert(old.len() - 1);
        for (kind, txt) in &hunk.lines {
            match kind {
                '=' => {
                    old.push(txt.clone());
                    new.push(txt.clone());
                }
                '<' => {
                    old.push(txt.clone());
                    new.push(String::new());
                    dl.insert(old.len() - 1);
                }
                '>' => {
                    new.push(txt.clone());
                    old.push(String::new());
                    al.insert(new.len() - 1);
                }
                _ => {}
            }
        }
    }
    Sides { old, new, del: dl, add: al, hdr }
}

/// porcelain v1 -z stdout → rows, path-sorted. Pure (split out of `changes`
/// so -z edge cases are unit-testable without a repo).
pub fn parse_porcelain(stdout: &[u8]) -> Option<Rows> {
    let text = String::from_utf8_lossy(stdout).to_string(); // decode replace
    let entries: Vec<&str> = text.split('\0').filter(|e| !e.is_empty()).collect();
    let mut out: Rows = Vec::new();
    let mut i = 0usize;
    while i < entries.len() {
        let e = entries[i];
        if e.len() < 4 {
            // truncated/stray record (submodules emit extras)
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
            // unmerged (UU/AA/DD/AU/UA/DU/UD): every unmerged code has a U —
            // != untracked "?"
            chosen = "C".to_string();
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

pub fn changes(root: &str) -> Option<Rows> {
    if !has_git(root) {
        return None;
    }
    let r = git_run(root, &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
                    false, Duration::from_secs(10))?;
    if !r.status.success() {
        return None;
    }
    parse_porcelain(&r.stdout)
}

// ------------------------------------------------------------------------------
// diff / commit / push · gitstatus.py 158-273
// ------------------------------------------------------------------------------

/// stdout or None; rc 1 counts (no-index/compare returns it on differences).
pub fn git_out(root: &str, args: &[&str], timeout_s: u64) -> Option<Vec<u8>> {
    let r = git_run(root, args, false, Duration::from_secs(timeout_s))?;
    if !r.status.success() && r.status.code() != Some(1) {
        return None;
    }
    Some(r.stdout)
}

/// (diff text, binary) for one path: worktree-vs-HEAD == the --only commit.
/// Untracked, or a HEAD-less repo (`git diff HEAD` fails): --no-index vs
/// /dev/null (all-added). Binary files → ("", True). empty root/rel → the
/// python tuple directly (never None — the Option is the harness shape).
pub fn diff_for(root: &str, rel: &str, is_untracked: bool) -> Option<(String, bool)> {
    if root.is_empty() || rel.is_empty() {
        return Some((String::new(), false));
    }
    let mut stdout: Option<Vec<u8>> = None;
    if !is_untracked {
        stdout = git_out(root, &["diff", "HEAD", "--no-color", "-U3", "--", rel], 15);
        if stdout.is_none() {
            // no HEAD: staged files still need seeing
            stdout = git_out(root, &["diff", "--cached", "--no-color", "-U3", "--", rel], 15);
        }
    }
    if stdout.is_none() {
        stdout = git_out(root, &["diff", "--no-index", "--no-color", "-U3",
                                 "--", "/dev/null", &format!("{root}/{rel}")], 15);
    }
    let text = stdout.map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default();
    if text.contains("Binary files") || text.contains("GIT binary patch") {
        return Some((String::new(), true));
    }
    Some((text, false))
}

pub fn commit(root: &str, paths: &[String], msg: &str) -> (bool, String) {
    if paths.is_empty() || !has_git(root) {
        return (false, "No files selected".into());
    }
    if msg.trim().is_empty() {
        return (false, "Empty commit message".into());
    }
    // `add` is only needed to bring untracked/edited WORKTREE content into the
    // index; a path existing solely as a staged removal (git rm/mv/rename)
    // refuses `git add` (fatal pathspec, rc 128 measured) and needs no add —
    // the pathspec is already staged. Keep the FULL list for --only; add only
    // what exists (lexists keeps a dangling symlink's typechange addable).
    let present: Vec<&String> = paths.iter().filter(|p| {
        std::path::Path::new(root).join(*p).symlink_metadata().is_ok()
    }).collect();
    if !present.is_empty() {
        let mut args: Vec<&str> = vec!["add", "--"];
        args.extend(present.iter().map(|s| s.as_str()));
        let Some(r) = git_run(root, &args, true, Duration::from_secs(30)) else {
            return (false, "git failed: timed out".into());
        };
        if !r.status.success() {
            let text = either(&r, "git add failed");
            return (false, clip400(&text));
        }
    }
    let mut args: Vec<&str> = vec!["commit", "--only", "-m", msg, "--"];
    args.extend(paths.iter().map(|s| s.as_str()));
    let Some(r) = git_run(root, &args, true, Duration::from_secs(60)) else {
        return (false, "git failed: timed out".into());
    };
    if !r.status.success() {
        let text = either(&r, "git commit failed");
        return (false, clip400(&text));
    }
    let out = String::from_utf8_lossy(&r.stdout).to_string();
    let first = out.trim().lines().next().unwrap_or("Committed.").to_string();
    (true, clip400(&first))
}

pub fn ahead(root: &str) -> usize {
    // No upstream / not a repo / timeouts → 0 (push() handles the -u bootstrap).
    if !has_git(root) {
        return 0;
    }
    let Some(r) = git_run(root, &["rev-list", "--count", "@{upstream}..HEAD"], false,
                          Duration::from_secs(5)) else {
        return 0;
    };
    if !r.status.success() {
        return 0;
    }
    // python: int(strip()) with ValueError → 0
    String::from_utf8_lossy(&r.stdout).trim().parse::<usize>().unwrap_or(0)
}

pub fn push(root: &str) -> (bool, String) {
    if !has_git(root) {
        return (false, "Not a git repository".into());
    }
    let Some(r) = git_run(root, &["push"], true, Duration::from_secs(120)) else {
        return (false, "git push failed: timed out".into());
    };
    if r.status.success() {
        // bootstrap push output lives on stderr
        return (true, clip400(&either(&r, "Pushed.")));
    }
    let mut err = either(&r, "").trim().to_string();
    if err.contains("no upstream") {
        // first push of a new branch — retry with -u origin <branch>
        if let Some(branch) = branch_of(root) {
            let Some(r2) = git_run(root, &["push", "-u", "origin", &branch], true,
                                   Duration::from_secs(120)) else {
                return (false, "git push failed: timed out".into());
            };
            if r2.status.success() {
                return (true, clip400(&either(&r2, "Pushed.")));
            }
            err = either(&r2, "git push failed").trim().to_string();
        }
    }
    if err.is_empty() {
        err = "git push failed".into();
    }
    (false, clip400(&err))
}

pub fn commit_then_push(
    root: &str,
    paths: &[String],
    msg: &str,
    phase: Option<&(dyn Fn(&str) + Send + Sync)>,
) -> FlightOut {
    // Empty `paths` = push-only (the auto-detect button rule: a checked
    // nothing with unpushed commits). The phase closure bakes "Pushing…" —
    // one call mid-flight just before the push (worker-thread context; the
    // UI marshals to the mainloop).
    let (cok, ctext) = if paths.is_empty() {
        (true, String::new())
    } else {
        commit(root, paths, msg)
    };
    if !cok {
        return FlightOut { cok: false, ctext, pok: false, ptext: String::new() };
    }
    if !paths.is_empty() {
        if let Some(phase) = phase {
            phase("busy");
        }
    }
    let (pok, ptext) = push(root);
    FlightOut { cok: true, ctext, pok, ptext }
}

// ------------------------------------------------------------------------------
// changes tree grouping · gitstatus.py 274-313
// ------------------------------------------------------------------------------

struct Lev {
    dirs: BTreeMap<String, Lev>,
    files: Vec<(String, String)>,
}

impl Lev {
    fn empty() -> Lev {
        Lev { dirs: BTreeMap::new(), files: Vec::new() }
    }
}

/// changes() rows → every changed path's ancestor dirs become dir rows;
/// parents before children, dirs before files at a level.
pub fn group_tree(rows: &Rows) -> Vec<TreeRow> {
    let mut sorted: Rows = rows.clone();
    sorted.sort();
    let mut tree = Lev::empty();
    for (rel, letter) in sorted {
        let parts: Vec<&str> = rel.split('/').collect();
        let mut node = &mut tree;
        for p in &parts[..parts.len() - 1] {
            node = node.dirs.entry(p.to_string()).or_insert_with(Lev::empty);
        }
        node.files.push((parts[parts.len() - 1].to_string(), letter));
    }
    let mut out: Vec<TreeRow> = Vec::new();
    walk(&tree, "", &mut out);
    out
}

fn walk(node: &Lev, prefix: &str, out: &mut Vec<TreeRow>) {
    for (name, sub) in &node.dirs {
        let drel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        out.push(TreeRow {
            kind: "d",
            name: name.clone(),
            rel: drel.clone(),
            letter: String::new(),
        });
        walk(sub, &drel, out);
    }
    for (name, letter) in &node.files {
        let frel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        out.push(TreeRow {
            kind: "f",
            name: name.clone(),
            rel: frel,
            letter: letter.clone(),
        });
    }
}

/// group_tree rows → dir rel → (all descendant files checked, any checked).
/// all ⊇ any (fully-checked dir → (True, True)); dirs exist only as file
/// ancestors (group_tree), so each accumulates ≥1 file.
pub fn checked_dir_state(
    rows: &[TreeRow],
    checked: &BTreeSet<String>,
) -> BTreeMap<String, (bool, bool)> {
    let mut state: BTreeMap<String, (bool, bool)> = BTreeMap::new();
    for r in rows {
        if r.kind != "f" {
            continue;
        }
        let ok = checked.contains(&r.rel);
        let parts: Vec<&str> = r.rel.split('/').collect();
        let mut acc = String::new();
        for p in &parts[..parts.len() - 1] {
            acc = if acc.is_empty() { p.to_string() } else { format!("{acc}/{p}") };
            let prev = state.get(&acc).copied().unwrap_or((true, false));
            state.insert(acc.clone(), (prev.0 && ok, prev.1 || ok));
        }
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::process::Command;

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

    fn clean(p: &Path) {
        let _ = std::fs::remove_dir_all(p);
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
        // (ruling: detach needs a commit — unborn branches cannot detach)
        git(&p, &["add", "."]);
        git(&p, &["commit", "-q", "-m", "c"]);
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
        // (ruling: m.txt must be TRACKED to read M — a never-added file is ??)
        w(&p, "m.txt", "one\n");
        git(&p, &["add", "."]);
        git(&p, &["commit", "-q", "-m", "base"]);
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
        // Send+Sync captures (production phase runs on the worker thread)
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let kind = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let phase = {
            let calls = calls.clone();
            let kind = kind.clone();
            move |k: &str| {
                calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if let Ok(mut g) = kind.lock() {
                    *g = k.to_string();
                }
            }
        };
        // paths empty = push-only: zero phase calls
        let r = commit_then_push(p.to_str().unwrap(), &[], "ignored", Some(&phase));
        assert!(r.cok && r.pok, "push-only: {r:?}");
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 0);
        w(&p, "f.txt", "y\n");
        let r = commit_then_push(p.to_str().unwrap(), &["f.txt".to_string()], "v2",
                                 Some(&phase));
        assert!(r.cok && r.pok, "full flight failed: {r:?}");
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1, "one 'busy' phase call");
        assert_eq!(*kind.lock().unwrap(), "busy");
        // failing commit: no phase, no push attempt
        let r = commit_then_push(p.to_str().unwrap(), &["f.txt".to_string()], "  ", Some(&phase));
        assert!(!r.cok && !r.pok
                && calls.load(std::sync::atomic::Ordering::Relaxed) == 1,
                "commit fail stops the flight: {r:?}");
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
        assert_eq!(st.get("a/b"), Some(&(true, true)) /* only file checked → all */);
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