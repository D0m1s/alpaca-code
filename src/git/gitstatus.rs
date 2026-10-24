use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub type Row = (String, String);
pub type Rows = Vec<Row>;

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
    pub old_start: usize,
    pub new_start: usize,
}
#[derive(Clone, Debug)]
pub struct Sides {
    pub old: Vec<String>,
    pub new: Vec<String>,
    pub del: BTreeSet<usize>,
    pub add: BTreeSet<usize>,
    pub hdr: BTreeSet<usize>,
    pub old_no: Vec<u32>,
    pub new_no: Vec<u32>,
}
#[derive(Clone, Debug)]
pub struct FlightOut {
    pub cok: bool,
    pub ctext: String,
    pub pok: bool,
    pub ptext: String,
}

fn has_git(root: &str) -> bool {
    !root.is_empty() && std::path::Path::new(root).join(".git").is_dir()
}

fn clip400(s: &str) -> String {
    s.trim().chars().take(400).collect()
}

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

fn git_run(
    root: &str,
    args: &[&str],
    env_prompt: bool,
    timeout: Duration,
) -> Option<std::process::Output> {
    git_run_env(root, &[], args, env_prompt, timeout)
}

fn git_run_env(
    root: &str,
    envs: &[(&str, String)],
    args: &[&str],
    env_prompt: bool,
    timeout: Duration,
) -> Option<std::process::Output> {
    if root.is_empty() {
        return None;
    }
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if env_prompt {
        cmd.env("GIT_TERMINAL_PROMPT", "0");
    }
    for (k, v) in envs {
        cmd.env(k, v);
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
                return Some(std::process::Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = t1.join();
                    let _ = t2.join();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => {
                let _ = t1.join();
                let _ = t2.join();
                return None;
            }
        }
    }
}

pub fn branch_of(root: &str) -> Option<String> {
    if root.is_empty() {
        return None;
    }
    let f = std::fs::read_to_string(std::path::Path::new(root).join(".git").join("HEAD"));
    let content = f.ok()?.trim().to_string();
    content
        .strip_prefix("ref: refs/heads/")
        .map(|s| s.to_owned())
}

pub fn branches(root: &str) -> Option<Vec<String>> {
    if !has_git(root) {
        return None;
    }
    let r = git_run(
        root,
        &[
            "for-each-ref",
            "refs/heads",
            "--sort=refname",
            "--format=%(refname:short)",
        ],
        false,
        Duration::from_secs(5),
    )?;
    if !r.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&r.stdout);
    Some(
        s.lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_owned())
            .collect(),
    )
}

fn _checkout(root: &str, args: &[&str]) -> (bool, String) {
    let Some(r) = git_run(root, args, true, Duration::from_secs(60)) else {
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

pub fn create_switch(root: &str, name: &str) -> (bool, String) {
    let name = name.trim();
    if name.is_empty() || name.starts_with('-') {
        return (false, "Bad branch name".into());
    }
    _checkout(root, &["checkout", "-b", name])
}

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
    let (a_n, s) = match s.strip_prefix(',') {
        Some(r) => {
            let (n, r) = num(r)?;
            (n, r)
        }
        None => (1, s),
    };
    let s = s.strip_prefix(' ')?;
    let s = s.strip_prefix('+')?;
    let (b_start, s) = num(s)?;
    let (b_n, s) = match s.strip_prefix(',') {
        Some(r) => {
            let (n, r) = num(r)?;
            (n, r)
        }
        None => (1, s),
    };
    s.strip_prefix(" @@")?;
    Some((a_start, a_n, b_start, b_n))
}

pub fn parse_unified(text: &str) -> Vec<Hunk> {
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut in_hunk = false;
    for line in text.split('\n') {
        if let Some((a, _an, b, _bn)) = hunk_head(line) {
            hunks.push(Hunk {
                lines: Vec::new(),
                old_start: a,
                new_start: b,
            });
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

#[allow(clippy::too_many_arguments)]
fn push_change_group(
    old: &mut Vec<String>,
    new: &mut Vec<String>,
    old_no: &mut Vec<u32>,
    new_no: &mut Vec<u32>,
    dels: &[(String, u32)],
    adds: &[(String, u32)],
    dl: &mut BTreeSet<usize>,
    al: &mut BTreeSet<usize>,
) {
    let k = dels.len().min(adds.len());
    for (d, a) in dels.iter().zip(adds.iter()).take(k) {
        old.push(d.0.clone());
        old_no.push(d.1);
        new.push(a.0.clone());
        new_no.push(a.1);
        let i = old.len() - 1;
        dl.insert(i);
        al.insert(i);
    }
    for (t, no) in dels.iter().skip(k) {
        old.push(t.clone());
        old_no.push(*no);
        new.push(String::new());
        new_no.push(0);
        dl.insert(old.len() - 1);
    }
    for (t, no) in adds.iter().skip(k) {
        old.push(String::new());
        old_no.push(0);
        new.push(t.clone());
        new_no.push(*no);
        al.insert(new.len() - 1);
    }
}

pub fn build_sides(hunks: &[Hunk]) -> Sides {
    let (mut old, mut new) = (Vec::new(), Vec::new());
    let (mut dl, mut al, mut hdr) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    let (mut old_no, mut new_no): (Vec<u32>, Vec<u32>) = (Vec::new(), Vec::new());
    for hunk in hunks {
        let (mut o, mut n) = (hunk.old_start as u32, hunk.new_start as u32);
        old.push(HUNK_ROW.to_string());
        new.push(HUNK_ROW.to_string());
        old_no.push(0);
        new_no.push(0);
        hdr.insert(old.len() - 1);
        let (mut dels, mut adds) = (Vec::new(), Vec::new());
        for (kind, txt) in &hunk.lines {
            match kind {
                '=' => {
                    push_change_group(
                        &mut old,
                        &mut new,
                        &mut old_no,
                        &mut new_no,
                        &dels,
                        &adds,
                        &mut dl,
                        &mut al,
                    );
                    (dels, adds) = (Vec::new(), Vec::new());
                    old.push(txt.clone());
                    old_no.push(o);
                    new.push(txt.clone());
                    new_no.push(n);
                    (o, n) = (o + 1, n + 1);
                }
                '<' => {
                    dels.push((txt.clone(), o));
                    o += 1;
                }
                '>' => {
                    adds.push((txt.clone(), n));
                    n += 1;
                }
                _ => {}
            }
        }
        push_change_group(
            &mut old,
            &mut new,
            &mut old_no,
            &mut new_no,
            &dels,
            &adds,
            &mut dl,
            &mut al,
        );
    }
    Sides {
        old,
        new,
        del: dl,
        add: al,
        hdr,
        old_no,
        new_no,
    }
}

pub fn diff_num_field_width(no: &[u32]) -> usize {
    no.iter()
        .filter(|n| **n > 0)
        .map(|n| n.to_string().len())
        .max()
        .unwrap_or(2)
        .max(2)
}

pub fn diff_num_text(no: &[u32]) -> String {
    let w = diff_num_field_width(no);
    no.iter()
        .map(|n| {
            if *n > 0 {
                format!("{n:>w$}")
            } else {
                " ".repeat(w)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn parse_porcelain(stdout: &[u8]) -> Option<Rows> {
    let text = String::from_utf8_lossy(stdout).to_string();
    let entries: Vec<&str> = text.split('\0').filter(|e| !e.is_empty()).collect();
    let mut out: Rows = Vec::new();
    let mut i = 0usize;
    while i < entries.len() {
        let e = entries[i];
        if e.len() < 4 {
            i += 1;
            continue;
        }
        let xy = &e[..2];
        let path = &e[3..];
        if xy.as_bytes()[0] == b'R' {
            out.push((path.to_string(), "R".to_string()));
            if i + 1 < entries.len() {
                out.push((entries[i + 1].to_string(), "D".to_string()));
            }
            i += 2;
            continue;
        }
        let x = xy.as_bytes()[0] as char;
        let y = xy.as_bytes()[1] as char;
        let mut chosen = if x != ' ' {
            x.to_string()
        } else {
            y.to_string()
        };
        if x == 'U' || y == 'U' {
            chosen = "C".to_string();
        }
        if chosen == "?" {
            chosen = "U".to_string();
        }
        out.push((path.to_string(), chosen));
        i += 1;
    }
    out.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    out.dedup();
    Some(out)
}

pub fn changes(root: &str) -> Option<Rows> {
    if !has_git(root) {
        return None;
    }
    let r = git_run(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        false,
        Duration::from_secs(10),
    )?;
    if !r.status.success() {
        return None;
    }
    parse_porcelain(&r.stdout)
}

pub fn git_out(root: &str, args: &[&str], timeout_s: u64) -> Option<Vec<u8>> {
    let r = git_run(root, args, false, Duration::from_secs(timeout_s))?;
    if !r.status.success() && r.status.code() != Some(1) {
        return None;
    }
    Some(r.stdout)
}

const DIFF_CTX: &str = "-U999999999";
pub fn diff_for(root: &str, rel: &str, is_untracked: bool) -> Option<(String, bool)> {
    if root.is_empty() || rel.is_empty() {
        return Some((String::new(), false));
    }
    let mut stdout: Option<Vec<u8>> = None;
    if !is_untracked {
        stdout = git_out(
            root,
            &["diff", "HEAD", "--no-color", DIFF_CTX, "--", rel],
            15,
        );
        if stdout.is_none() {
            stdout = git_out(
                root,
                &["diff", "--cached", "--no-color", DIFF_CTX, "--", rel],
                15,
            );
        }
    }
    if stdout.is_none() {
        stdout = git_out(
            root,
            &[
                "diff",
                "--no-index",
                "--no-color",
                DIFF_CTX,
                "--",
                "/dev/null",
                &format!("{root}/{rel}"),
            ],
            15,
        );
    }
    let text = stdout
        .map(|b| String::from_utf8_lossy(&b).to_string())
        .unwrap_or_default();
    if text.contains("Binary files") || text.contains("GIT binary patch") {
        return Some((String::new(), true));
    }
    Some((text, false))
}

fn staged_removals(root: &str, paths: &[String]) -> BTreeSet<String> {
    let mut args: Vec<&str> = vec![
        "diff",
        "--cached",
        "--name-only",
        "--diff-filter=D",
        "-z",
        "--",
    ];
    args.extend(paths.iter().map(|s| s.as_str()));
    match git_out(root, &args, 15) {
        Some(bytes) => bytes
            .split(|b| *b == 0)
            .map(|s| String::from_utf8_lossy(s).to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        None => BTreeSet::new(),
    }
}

pub fn commit(root: &str, paths: &[String], msg: &str) -> (bool, String) {
    if paths.is_empty() || !has_git(root) {
        return (false, "No files selected".into());
    }
    if msg.trim().is_empty() {
        return (false, "Empty commit message".into());
    }
    let staged_rm = staged_removals(root, paths);
    let present: Vec<&String> = paths
        .iter()
        .filter(|p| {
            std::path::Path::new(root)
                .join(*p)
                .symlink_metadata()
                .is_ok()
        })
        .collect();
    let addable: Vec<&String> = present
        .iter()
        .copied()
        .filter(|p| !staged_rm.contains(*p))
        .collect();
    let mut removals: BTreeSet<String> = paths
        .iter()
        .filter(|p| !present.contains(p))
        .cloned()
        .collect();
    removals.extend(staged_rm);
    if !addable.is_empty() {
        let mut args: Vec<&str> = vec!["add", "--"];
        args.extend(addable.iter().map(|s| s.as_str()));
        let Some(r) = git_run(root, &args, true, Duration::from_secs(30)) else {
            return (false, "git failed: timed out".into());
        };
        if !r.status.success() {
            let text = either(&r, "git add failed");
            return (false, clip400(&text));
        }
    }
    if removals.is_empty() {
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
        let first = out
            .trim()
            .lines()
            .next()
            .unwrap_or("Committed.")
            .to_string();
        return (true, clip400(&first));
    }
    let gitdir = match git_out(root, &["rev-parse", "--git-dir"], 5) {
        Some(b) => String::from_utf8_lossy(b.trim_ascii()).to_string(),
        None => return (false, "git failed: rev-parse".into()),
    };
    let gitdir = std::path::Path::new(root).join(&gitdir);
    let tmpidx = std::env::temp_dir().join(format!(
        "alpaca-commit-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let envs = [("GIT_INDEX_FILE", tmpidx.to_string_lossy().into_owned())];
    let _ = std::fs::copy(gitdir.join("index"), &tmpidx);
    let Some(r) = git_run_env(
        root,
        &envs,
        &["read-tree", "HEAD"],
        true,
        Duration::from_secs(15),
    ) else {
        let _ = std::fs::remove_file(&tmpidx);
        return (false, "git failed: timed out".into());
    };
    if !r.status.success() {
        let text = either(&r, "git read-tree failed");
        let _ = std::fs::remove_file(&tmpidx);
        return (false, clip400(&text));
    }
    let mut args: Vec<&str> = vec!["update-index", "--force-remove", "--"];
    let removal_list: Vec<String> = removals.iter().cloned().collect();
    args.extend(removal_list.iter().map(|s| s.as_str()));
    let Some(r) = git_run_env(root, &envs, &args, true, Duration::from_secs(15)) else {
        let _ = std::fs::remove_file(&tmpidx);
        return (false, "git failed: timed out".into());
    };
    if !r.status.success() {
        let text = either(&r, "git update-index failed");
        let _ = std::fs::remove_file(&tmpidx);
        return (false, clip400(&text));
    }
    if !addable.is_empty() {
        let mut args: Vec<&str> = vec!["update-index", "--add", "--"];
        args.extend(addable.iter().map(|s| s.as_str()));
        let Some(r) = git_run_env(root, &envs, &args, true, Duration::from_secs(15)) else {
            let _ = std::fs::remove_file(&tmpidx);
            return (false, "git failed: timed out".into());
        };
        if !r.status.success() {
            let text = either(&r, "git update-index failed");
            let _ = std::fs::remove_file(&tmpidx);
            return (false, clip400(&text));
        }
    }
    let Some(r) = git_run_env(
        root,
        &envs,
        &["commit", "-m", msg],
        true,
        Duration::from_secs(60),
    ) else {
        let _ = std::fs::remove_file(&tmpidx);
        return (false, "git failed: timed out".into());
    };
    if !r.status.success() {
        let text = either(&r, "git commit failed");
        let _ = std::fs::remove_file(&tmpidx);
        return (false, clip400(&text));
    }
    let out = String::from_utf8_lossy(&r.stdout).to_string();
    let first = out
        .trim()
        .lines()
        .next()
        .unwrap_or("Committed.")
        .to_string();
    let mut args: Vec<&str> = vec!["rm", "--cached", "--ignore-unmatch", "-q", "--"];
    args.extend(removals.iter().map(|s| s.as_str()));
    let _ = git_run(root, &args, true, Duration::from_secs(15));
    let _ = std::fs::remove_file(&tmpidx);
    (true, clip400(&first))
}

pub fn ahead(root: &str) -> usize {
    if !has_git(root) {
        return 0;
    }
    let Some(r) = git_run(
        root,
        &["rev-list", "--count", "@{upstream}..HEAD"],
        false,
        Duration::from_secs(5),
    ) else {
        return 0;
    };
    if !r.status.success() {
        return 0;
    }
    String::from_utf8_lossy(&r.stdout)
        .trim()
        .parse::<usize>()
        .unwrap_or(0)
}

pub fn push(root: &str) -> (bool, String) {
    if !has_git(root) {
        return (false, "Not a git repository".into());
    }
    let Some(r) = git_run(root, &["push"], true, Duration::from_secs(120)) else {
        return (false, "git push failed: timed out".into());
    };
    if r.status.success() {
        return (true, clip400(&either(&r, "Pushed.")));
    }
    let mut err = either(&r, "").trim().to_string();
    if err.contains("no upstream")
        && let Some(branch) = branch_of(root)
    {
        let Some(r2) = git_run(
            root,
            &["push", "-u", "origin", &branch],
            true,
            Duration::from_secs(120),
        ) else {
            return (false, "git push failed: timed out".into());
        };
        if r2.status.success() {
            return (true, clip400(&either(&r2, "Pushed.")));
        }
        err = either(&r2, "git push failed").trim().to_string();
    }
    if err.is_empty() {
        err = "git push failed".into();
    }
    (false, clip400(&err))
}

pub fn pull(root: &str, _name: &str) -> (bool, String) {
    if !has_git(root) {
        return (false, "Not a git repository".into());
    }
    let Some(r) = git_run(root, &["pull"], true, Duration::from_secs(120)) else {
        return (false, "git pull failed: timed out".into());
    };
    if r.status.success() {
        return (true, clip400(&either(&r, "Pulled.")));
    }
    let err = either(&r, "").trim().to_string();
    (
        false,
        clip400(&if err.is_empty() {
            "git pull failed".into()
        } else {
            err
        }),
    )
}

pub fn commit_then_push(
    root: &str,
    paths: &[String],
    msg: &str,
    phase: Option<&(dyn Fn(&str) + Send + Sync)>,
) -> FlightOut {
    let (cok, ctext) = if paths.is_empty() {
        (true, String::new())
    } else {
        commit(root, paths, msg)
    };
    if !cok {
        return FlightOut {
            cok: false,
            ctext,
            pok: false,
            ptext: String::new(),
        };
    }
    if !paths.is_empty()
        && let Some(phase) = phase
    {
        phase("busy");
    }
    let (pok, ptext) = push(root);
    FlightOut {
        cok: true,
        ctext,
        pok,
        ptext,
    }
}

struct Lev {
    dirs: BTreeMap<String, Lev>,
    files: Vec<(String, String)>,
}

impl Lev {
    fn empty() -> Lev {
        Lev {
            dirs: BTreeMap::new(),
            files: Vec::new(),
        }
    }
}

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
        node.files
            .push((parts[parts.len() - 1].to_string(), letter));
    }
    let mut out: Vec<TreeRow> = Vec::new();
    walk(&tree, "", &mut out);
    out
}

fn walk(node: &Lev, prefix: &str, out: &mut Vec<TreeRow>) {
    for (name, sub) in &node.dirs {
        let drel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        out.push(TreeRow {
            kind: "d",
            name: name.clone(),
            rel: drel.clone(),
            letter: String::new(),
        });
        walk(sub, &drel, out);
    }
    for (name, letter) in &node.files {
        let frel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        out.push(TreeRow {
            kind: "f",
            name: name.clone(),
            rel: frel,
            letter: letter.clone(),
        });
    }
}

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
            acc = if acc.is_empty() {
                p.to_string()
            } else {
                format!("{acc}/{p}")
            };
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

    fn git(repo: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .unwrap();
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
        v.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    fn clean(p: &Path) {
        let _ = std::fs::remove_dir_all(p);
    }

    #[test]
    fn branch_of_reads_head() {
        let p = repo("branch");
        w(&p, "f.txt", "x");
        let name = String::from_utf8(
            Command::new("git")
                .arg("-C")
                .arg(&p)
                .args(["symbolic-ref", "--short", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_string();
        assert_eq!(
            branch_of(p.to_str().unwrap()).as_deref(),
            Some(name.as_str())
        );
        git(&p, &["add", "."]);
        git(&p, &["commit", "-q", "-m", "c"]);
        git(&p, &["checkout", "-q", "--detach"]);
        assert_eq!(
            branch_of(p.to_str().unwrap()),
            None,
            "detached HEAD hides the pill"
        );
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
        assert_eq!(
            switch(p.to_str().unwrap(), ""),
            (false, "Bad branch name".into())
        );
        assert_eq!(
            switch(p.to_str().unwrap(), "-x"),
            (false, "Bad branch name".into())
        );
        assert_eq!(
            create_switch(p.to_str().unwrap(), "-b"),
            (false, "Bad branch name".into())
        );
        let (ok, text) = create_switch(p.to_str().unwrap(), "newbie");
        assert!(ok, "checkout -b failed: {text}");
        assert_eq!(branch_of(p.to_str().unwrap()).as_deref(), Some("newbie"));
    }

    #[test]
    fn changes_letters_and_untracked() {
        let p = repo("letters");
        w(&p, "m.txt", "one\n");
        git(&p, &["add", "."]);
        git(&p, &["commit", "-q", "-m", "base"]);
        w(&p, "u1.txt", "x");
        w(&p, "untracked/z.txt", "y");
        git(&p, &["add", "u1.txt"]);
        w(&p, "m.txt", "two\n");
        let got = changes(p.to_str().unwrap()).unwrap();
        let want = rows_of(&[("m.txt", "M"), ("u1.txt", "A"), ("untracked/z.txt", "U")]);
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
        let _ = Command::new("git")
            .arg("-C")
            .arg(&p)
            .args(["merge", "side"])
            .output()
            .unwrap();
        let got = changes(p.to_str().unwrap()).unwrap();
        assert_eq!(got, rows_of(&[("f.txt", "C")]), "UU → C");
    }

    #[test]
    fn parse_porcelain_synthetic() {
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
        assert_eq!(changes("/no/such/root"), None);
        let notrepo = scratch("notrepo");
        assert_eq!(changes(notrepo.to_str().unwrap()), None);
    }

    #[test]
    fn unified_and_sides_exact() {
        let text = "diff --git a/f b/f\n--- a/f\n+++ b/f\n\
                    @@ -1,2 +1,2 @@\n keep\n-gone\n+add\n keep2\n";
        let hunks = parse_unified(text);
        assert_eq!(hunks.len(), 1);
        let s = build_sides(&hunks);
        assert_eq!(s.old.len(), s.new.len(), "equal lengths by construction");
        assert_eq!(s.old[0], HUNK_ROW);
        assert!(s.hdr.contains(&0), "hunk-row index 0 on both sides");
        assert_eq!(s.old[2], "gone");
        assert_eq!(
            s.new[2], "add",
            "a 1:1 change group pairs on one shared row"
        );
        assert_eq!(s.old[3], "keep2");
        assert_eq!(s.new[3], "keep2");
        assert!(
            s.del.contains(&2) && s.add.contains(&2),
            "a paired row tints BOTH sides"
        );
        assert!(!s.del.contains(&3) && !s.add.contains(&3));
        assert_eq!(parse_unified("@@ -5 +9 @@\n a\n").len(), 1);
        assert_eq!(parse_unified("index 123..abc 100644\n").len(), 0);
        assert_eq!(parse_unified("").len(), 0);
    }

    #[test]
    fn sides_numbers_exact() {
        let text = "@@ -2,3 +2,4 @@\n ctx\n-del line\n+ADD A\n+ADD B\n ctx2\n@@ -10,0 +10,3 @@\n+pure add 1\n+pure add 2\n@@ -20,2 +23,0 @@\n-del-only 1\n-del-only 2\n";
        let hunks = parse_unified(text);
        assert_eq!(hunks[0].old_start, 2);
        assert_eq!(hunks[0].new_start, 2);
        assert_eq!(hunks[1].old_start, 10);
        assert_eq!(hunks[2].new_start, 23);
        let sides = build_sides(&hunks);
        assert!(sides.del.contains(&2) && sides.add.contains(&2) && sides.add.contains(&3));
        assert_eq!(sides.old_no, [0, 2, 3, 0, 4, 0, 0, 0, 0, 20, 21]);
        assert_eq!(sides.new_no, [0, 2, 3, 4, 5, 0, 10, 11, 0, 0, 0]);
        assert_eq!(sides.old.len(), sides.old_no.len());
        assert_eq!(sides.new.len(), sides.new_no.len());
    }

    #[test]
    fn diff_num_text_shapes() {
        assert_eq!(diff_num_text(&[0, 2, 3]), "  \n 2\n 3");
        assert_eq!(diff_num_text(&[12, 130, 10]), " 12\n130\n 10");
        assert_eq!(diff_num_text(&[10000]), "10000");
        assert_eq!(diff_num_text(&[10, 100000]), "    10\n100000");
        assert_eq!(diff_num_text(&[]), "");
        assert_eq!(diff_num_text(&[0]), "  ");
        assert_eq!(diff_num_field_width(&[0, 2, 3]), 2);
        assert_eq!(diff_num_field_width(&[12, 130, 10]), 3);
        assert_eq!(diff_num_field_width(&[]), 2);
    }

    #[test]
    fn diff_for_full_context() {
        let p = repo("fullctx");
        w(&p, "f.txt", "a\nb\nc\nd\ne\n");
        git(&p, &["add", "."]);
        git(&p, &["commit", "-m", "five", "-q"]);
        w(&p, "f.txt", "a\nb\nCHANGED\nd\ne\n");
        let (t, _b) = diff_for(p.to_str().unwrap(), "f.txt", false).unwrap();
        assert!(t.contains(" e\n"), "tail line rides the output: {t}");
        assert!(t.contains(" a\n"), "head line rides the output: {t}");
        assert!(t.contains("+CHANGED"));
    }

    #[test]
    fn diff_for_chain() {
        let p = repo("diffchain");
        w(&p, "tracked.txt", "a\n");
        git(&p, &["add", "."]);
        let (t, b) = diff_for(p.to_str().unwrap(), "tracked.txt", false).unwrap();
        assert!(!b && t.contains("+"), "staged file diffs via --cached");
        w(&p, "untracked.txt", "un\n");
        let (t, b) = diff_for(p.to_str().unwrap(), "untracked.txt", true).unwrap();
        assert!(!b && t.contains("+"), "no-index vs /dev/null: all-added");
        assert!(t.contains("untracked.txt"));
        assert_eq!(
            diff_for(p.to_str().unwrap(), "", false).unwrap(),
            ("".into(), false)
        );
    }

    #[test]
    fn diff_for_binary_sniff() {
        let p = repo("bin");
        std::fs::write(p.join("blob.bin"), [0x00u8, 1, 2, 3]).unwrap();
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
        w(&p, "gone.txt", "v\n");
        git(&p, &["add", "."]);
        git(&p, &["commit", "-q", "-m", "second"]);
        git(&p, &["rm", "-q", "--cached", "gone.txt"]);
        std::fs::remove_file(p.join("gone.txt")).unwrap();
        let paths = vec!["gone.txt".to_string()];
        let (ok, text) = commit(p.to_str().unwrap(), &paths, "rm gone");
        assert!(ok, "staged-only removal commits without add: {text}");
        assert_eq!(
            commit(p.to_str().unwrap(), &[], "x"),
            (false, "No files selected".into())
        );
        assert_eq!(
            commit(p.to_str().unwrap(), &["f".into()], " "),
            (false, "Empty commit message".into())
        );
    }

    #[test]
    fn ahead_and_push_bootstrap() {
        let p = repo("ahead");
        let mut w2 = scratch("remote");
        let out = Command::new("git")
            .arg("init")
            .arg("--bare")
            .arg("-q")
            .arg(w2.join("remote.git"))
            .output()
            .unwrap();
        assert!(out.status.success());
        w2 = w2.join("remote.git");
        w(&p, "a.txt", "one\n");
        git(&p, &["add", "."]);
        git(&p, &["commit", "-q", "-m", "c1"]);
        assert_eq!(ahead(p.to_str().unwrap()), 0, "no upstream → 0");
        git(&p, &["remote", "add", "origin", w2.to_str().unwrap()]);
        w(&p, "b.txt", "two\n");
        git(&p, &["add", "."]);
        git(&p, &["commit", "-q", "-m", "c2"]);
        assert_eq!(ahead(p.to_str().unwrap()), 0, "no upstream branch yet → 0");
        let (ok, text) = push(p.to_str().unwrap());
        assert!(ok, "bootstrap push failed: {text}");
        let up = String::from_utf8(
            Command::new("git")
                .arg("-C")
                .arg(&p)
                .args(["rev-parse", "--abbrev-ref", "@{upstream}"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        assert!(up.starts_with("origin/"));
        w(&p, "c.txt", "three\n");
        git(&p, &["add", "."]);
        git(&p, &["commit", "-q", "-m", "c3"]);
        assert_eq!(ahead(p.to_str().unwrap()), 1, "one unpushed commit");
        let (ok, _) = push(p.to_str().unwrap());
        assert!(ok);
        assert_eq!(ahead(p.to_str().unwrap()), 0);
        assert_eq!(
            push("/no/such/root"),
            (false, "Not a git repository".into())
        );
    }

    #[test]
    fn pull_fast_forward_and_errors() {
        let p = repo("pull");
        let mut w2 = scratch("remote4");
        let out = Command::new("git")
            .arg("init")
            .arg("--bare")
            .arg("-q")
            .arg("-b")
            .arg("main")
            .arg(w2.join("remote.git"))
            .output()
            .unwrap();
        assert!(out.status.success());
        w2 = w2.join("remote.git");
        w(&p, "a.txt", "one\n");
        git(&p, &["add", "."]);
        git(&p, &["commit", "-q", "-m", "c1"]);
        git(&p, &["remote", "add", "origin", w2.to_str().unwrap()]);
        git(&p, &["push", "-q", "-u", "origin", "main"]);
        w(&p, "a.txt", "one\ntwo\n");
        git(&p, &["commit", "-qam", "c2"]);
        git(&p, &["push", "-q", "origin", "main"]);
        let side = scratch("pullside");
        let out = Command::new("git")
            .arg("clone")
            .arg("-q")
            .arg(w2.to_str().unwrap())
            .arg(side.to_str().unwrap())
            .output()
            .unwrap();
        assert!(out.status.success());
        let (ok, text) = pull(side.to_str().unwrap(), "");
        assert!(ok, "fast-forward pull failed: {text}");
        assert_eq!(
            std::fs::read_to_string(side.join("a.txt")).unwrap(),
            "one\ntwo\n"
        );
        let lone = repo("pulllone");
        w(&lone, "b.txt", "base\n");
        git(&lone, &["add", "."]);
        git(&lone, &["commit", "-q", "-m", "b1"]);
        let (ok, text) = pull(lone.to_str().unwrap(), "");
        assert!(!ok, "no-tracking pull must fail");
        assert!(
            text.to_lowercase().contains("tracking"),
            "git's no-tracking text surfaced: {text}"
        );
        assert_eq!(
            pull("/no/such/root", ""),
            (false, "Not a git repository".into())
        );
    }

    #[test]
    fn commit_then_push_phase_contract() {
        let p = repo("ctp");
        let mut w2 = scratch("remote2");
        let out = Command::new("git")
            .arg("init")
            .arg("--bare")
            .arg("-q")
            .arg(w2.join("remote.git"))
            .output()
            .unwrap();
        assert!(out.status.success());
        w2 = w2.join("remote.git");
        w(&p, "f.txt", "x\n");
        git(&p, &["add", "."]);
        git(&p, &["commit", "-q", "-m", "c1"]);
        git(&p, &["remote", "add", "origin", w2.to_str().unwrap()]);
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
        let r = commit_then_push(p.to_str().unwrap(), &[], "ignored", Some(&phase));
        assert!(r.cok && r.pok, "push-only: {r:?}");
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 0);
        w(&p, "f.txt", "y\n");
        let r = commit_then_push(
            p.to_str().unwrap(),
            &["f.txt".to_string()],
            "v2",
            Some(&phase),
        );
        assert!(r.cok && r.pok, "full flight failed: {r:?}");
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::Relaxed),
            1,
            "one 'busy' phase call"
        );
        assert_eq!(*kind.lock().unwrap(), "busy");
        let r = commit_then_push(
            p.to_str().unwrap(),
            &["f.txt".to_string()],
            "  ",
            Some(&phase),
        );
        assert!(
            !r.cok && !r.pok && calls.load(std::sync::atomic::Ordering::Relaxed) == 1,
            "commit fail stops the flight: {r:?}"
        );
        assert_eq!(r.ctext, "Empty commit message");
    }

    #[test]
    fn commit_keeps_untracked_kept_staged_removals() {
        let p = repo("rmkeep");
        w(&p, "a.txt", "v1\n");
        w(&p, "ignored.txt", "keep me\n");
        w(&p, "kept.txt", "unignored keeper\n");
        w(&p, "stale.txt", "old\n");
        git(&p, &["add", "."]);
        git(&p, &["commit", "-q", "-m", "base"]);
        git(&p, &["rm", "-q", "--cached", "ignored.txt", "kept.txt"]);
        w(&p, ".gitignore", "ignored.txt\n");
        w(&p, "a.txt", "v2\n");
        w(&p, "other.txt", "staged, unselected\n");
        git(&p, &["add", "other.txt"]);
        let staged_rm = staged_removals(
            p.to_str().unwrap(),
            &["a.txt".into(), "ignored.txt".into(), "kept.txt".into()],
        );
        assert_eq!(
            staged_rm.len(),
            2,
            "both staged removals detected, a.txt not"
        );
        let (ok, text) = commit(
            p.to_str().unwrap(),
            &["a.txt".into(), "ignored.txt".into(), "kept.txt".into()],
            "removals ride the temp index",
        );
        assert!(ok, "commit with kept staged removals failed: {text}");
        let files = Command::new("git")
            .arg("-C")
            .arg(&p)
            .args(["show", "--name-only", "--format="])
            .output()
            .unwrap();
        let names = String::from_utf8_lossy(&files.stdout);
        assert!(
            names.contains("ignored.txt") && names.contains("kept.txt") && names.contains("a.txt"),
            "all three in the commit: {names}"
        );
        let head_a = Command::new("git")
            .arg("-C")
            .arg(&p)
            .args(["show", "HEAD:a.txt"])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&head_a.stdout),
            "v2\n",
            "a's edit rode along"
        );
        let head_ignored = Command::new("git")
            .arg("-C")
            .arg(&p)
            .args(["show", "HEAD:ignored.txt"])
            .output()
            .unwrap();
        assert!(
            !head_ignored.status.success(),
            "ignored.txt must read DELETED at HEAD"
        );
        assert!(p.join("ignored.txt").is_file() && p.join("kept.txt").is_file());
        let ls = Command::new("git")
            .arg("-C")
            .arg(&p)
            .args(["ls-files"])
            .output()
            .unwrap();
        let tracked = String::from_utf8_lossy(&ls.stdout);
        assert!(
            !tracked.contains("ignored.txt") && !tracked.contains("kept.txt"),
            "untrackings must hold: {tracked}"
        );
        let head_other = Command::new("git")
            .arg("-C")
            .arg(&p)
            .args(["show", "HEAD:other.txt"])
            .output()
            .unwrap();
        assert!(
            !head_other.status.success(),
            "unselected staged file must not leak"
        );
        let st = Command::new("git")
            .arg("-C")
            .arg(&p)
            .args(["status", "--porcelain=v1"])
            .output()
            .unwrap();
        let lines = String::from_utf8_lossy(&st.stdout).replace('\0', "\n");
        assert!(
            lines.contains("A  other.txt"),
            "other staged state survives: {lines}"
        );
        assert!(
            !lines.contains("a.txt"),
            "committed a.txt clean, no MM ghost: {lines}"
        );
        git(&p, &["rm", "-q", "--cached", "stale.txt"]);
        let (ok, text) = commit(p.to_str().unwrap(), &["stale.txt".into()], "deletion only");
        assert!(ok, "deletion-only commit failed: {text}");
        let head_stale = Command::new("git")
            .arg("-C")
            .arg(&p)
            .args(["show", "HEAD:stale.txt"])
            .output()
            .unwrap();
        assert!(!head_stale.status.success(), "stale.txt deleted at HEAD");
    }

    #[test]
    fn group_tree_shapes() {
        let rows = rows_of(&[("e.txt", "U"), ("a/b/c.txt", "M"), ("a/d.txt", "A")]);
        let got = group_tree(&rows);
        let kinds: Vec<&str> = got.iter().map(|r| r.kind).collect();
        assert_eq!(
            got.iter()
                .map(|r| (r.rel.as_str(), r.kind))
                .collect::<Vec<_>>(),
            vec![
                ("a", "d"),
                ("a/b", "d"),
                ("a/b/c.txt", "f"),
                ("a/d.txt", "f"),
                ("e.txt", "f"),
            ],
            "parents before children, dirs before files at a level"
        );
        assert_eq!(kinds[0], "d");
        assert_eq!(got.iter().find(|r| r.name == "c.txt").unwrap().letter, "M");
        assert!(got.iter().all(|r| r.kind != "d" || r.letter.is_empty()));
    }

    #[test]
    fn dir_state_all_any() {
        let rows = group_tree(&rows_of(&[
            ("e.txt", "U"),
            ("a/b/c.txt", "M"),
            ("a/d.txt", "A"),
        ]));
        let ck = BTreeSet::from(["a/b/c.txt".to_string()]);
        let st = checked_dir_state(&rows, &ck);
        assert_eq!(st.get("a/b"), Some(&(true, true)));
        assert_eq!(st.get("a"), Some(&(false, true)));
        assert_eq!(st.get("e.txt"), None);
        let ck: BTreeSet<String> = ["a/b/c.txt", "a/d.txt", "e.txt"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let st = checked_dir_state(&rows, &ck);
        assert_eq!(st.get("a/b"), Some(&(true, true)));
        assert_eq!(st.get("a"), Some(&(true, true)));
        let st = checked_dir_state(&rows, &BTreeSet::new());
        assert_eq!(st.get("a"), Some(&(false, false)));
    }
}
