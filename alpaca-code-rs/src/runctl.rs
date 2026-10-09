//! Pure helpers: pane environment build + project run detection.
//! Port of alpaca_code/runctl.py — no gi imports, safe to test directly.

use std::path::Path;

/// Claude's ambient session markers, exported by any claude session to its children.
/// When the app itself was launched from inside a claude session, pane children would
/// inherit them: the Agent pane's claude then sees the nested marker and disables
/// transcript saving (unresumable sessions), besides leaking stale ids/sockets.
/// Pane children are top-level, not nested — claude there runs like a fresh invocation.
const CLAUDE_SESSION_MARKERS: [&str; 9] = [
    "CLAUDECODE", "CLAUDE_PID", "CLAUDE_CODE_CHILD_SESSION", "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_SESSION_ID", "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_MESSAGING_SOCKET", "CLAUDE_CODE_MESSAGING_TOKEN", "CLAUDE_CODE_SSE_PORT",
];

/// VTE spawn_async MERGES envv onto the child's INHERITED environ (measured on the
/// Python app: the child kept all parent vars) — omitting a key here does NOT remove
/// it from the child. Override instead: claude's nested-marker check is a plain
/// truthiness test on CLAUDE_CODE_CHILD_SESSION, so an empty value disables it.
pub fn pane_environ() -> Vec<String> {
    let env: Vec<String> = std::env::vars()
        .filter(|(k, _)| !CLAUDE_SESSION_MARKERS.contains(&k.as_str()))
        .map(|(k, v)| format!("{k}={v}"))
        .chain(
            CLAUDE_SESSION_MARKERS
                .iter()
                .filter(|k| std::env::var_os(*k).is_some())
                .map(|k| format!("{k}=")),
        )
        .collect();
    // ~/.local/bin appended when it exists and the current PATH lacks it
    let mut env = env;
    if let Some(home) = std::env::var_os("HOME") {
        let p = Path::new(&home).join(".local/bin");
        if p.is_dir() {
            if let Some(ps) = p.to_str() {
                if let Some(idx) = env.iter().position(|e| e.starts_with("PATH=")) {
                    let cur = &env[idx]["PATH=".len()..];
                    if !cur.split(':').any(|c| c == ps) {
                        env[idx] = format!("PATH={cur}:{ps}");
                    }
                }
            }
        }
    }
    env
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunCmd {
    pub argv: Vec<String>,
    pub label: String,
}

/// Detect how to run a project (runctl.py:29-49): package.json scripts dev→start
/// (present-but-broken package.json stops the decision), else csproj top level,
/// else one-level-nested first hit, else None.
pub fn detect(root: &Path) -> Option<RunCmd> {
    let pj = root.join("package.json");
    if pj.is_file() {
        return match std::fs::read_to_string(&pj)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("scripts").and_then(|s| s.as_object()).cloned())
        {
            Some(scripts) if scripts.contains_key("dev") => {
                Some(RunCmd { argv: vec!["npm".into(), "run".into(), "dev".into()], label: "npm run dev".into() })
            }
            Some(scripts) if scripts.contains_key("start") => {
                Some(RunCmd { argv: vec!["npm".into(), "start".into()], label: "npm start".into() })
            }
            _ => None,
        };
    }
    let top_csproj = |e: &std::fs::DirEntry| {
        e.path().extension() == Some(std::ffi::OsStr::new("csproj"))
    };
    if let Ok(rs) = std::fs::read_dir(root) {
        if rs.filter_map(|e| e.ok()).any(|e| top_csproj(&e)) {
            return Some(RunCmd { argv: vec!["dotnet".into(), "run".into()], label: "dotnet run".into() });
        }
    }
    // one-level nested: */*.csproj sorted by path, first hit
    let mut subdirs: Vec<std::fs::DirEntry> = std::fs::read_dir(root)
        .ok()?
        .filter_map(|e| e.ok())
        .collect();
    subdirs.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    for d in subdirs {
        if std::fs::read_dir(d.path()).ok().is_some_and(|mut rd| {
            rd.any(|e| e.ok().is_some_and(|e| top_csproj(&e)))
        }) {
            let rel = d.file_name().to_string_lossy().into_owned();
            return Some(RunCmd {
                argv: vec!["dotnet".into(), "run".into(), "--project".into(), rel.clone()],
                label: format!("dotnet run --project {rel}"),
            });
        }
    }
    None
}

/// New Project flow (window.py:_act_new_project on_resp): `os.makedirs(target,
/// exist_ok=False)` — intermediates allowed, final dir must NOT exist — then
/// `git init` with a 10s wall. Both failures surface in the app's error dialog
/// ("Couldn't create {target}"), and a nonzero git exit is NOT an error there
/// (python runs without check=True, only the timeout/launch raise).
pub fn create_project(target: &Path) -> Result<(), String> {
    if target.exists() {
        // exist_ok=False — an existing leaf dir is FileExistsError in python
        return Err(format!("File exists: '{}'", target.display()));
    }
    // makedirs creates intermediates; an empty parent means the cwd (relative target)
    let parent = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    std::fs::create_dir(target).map_err(|e| e.to_string())?;
    git_init(target)
}

/// `git init` with a 10s wall (window.py:423 subprocess.run(timeout=10));
/// git's output is captured-and-discarded, exactly like python's.
fn git_init(target: &Path) -> Result<(), String> {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let mut child = Command::new("git")
        .arg("init")
        .arg(target)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            // nonzero exit is not an error (python: no check=True)
            Ok(Some(_)) => return Ok(()),
            Ok(None) => {
                if start.elapsed() >= Duration::from_secs(10) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("git init timed out after 10 seconds".into());
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn dir_with(path: &str, files: &[(&str, &str)]) -> PathBuf {
        let d = std::env::temp_dir().join(format!("alpaca-runctl-{}", path));
        let _ = std::fs::remove_dir_all(&d); // leftovers from a previous run
        std::fs::create_dir_all(&d).unwrap();
        for (name, contents) in files {
            std::fs::write(d.join(name), contents).unwrap();
        }
        d
    }

    #[test]
    fn create_project_makedirs_and_git_init() {
        // window.py:_act_new_project — makedirs(exist_ok=False) + git init
        let base = std::env::temp_dir().join("alpaca-newproj");
        let _ = std::fs::remove_dir_all(&base);
        let parent = base.join("p"); // nonexistent parent chain is created
        let target = parent.join("proj");
        create_project(&target).unwrap();
        assert!(target.is_dir());
        assert!(target.join(".git").is_dir());
        // exist_ok=False: the second run must fail, not silently reuse
        create_project(&target).unwrap_err();
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn detect_matrix() {
        let npm = dir_with("npm", &[(
            "package.json",
            r#"{ "scripts": { "dev": "vite", "start": "node ." } }"#,
        )]);
        assert_eq!(detect(&npm).unwrap().argv, vec!["npm", "run", "dev"]);
        assert_eq!(detect(&npm).unwrap().label, "npm run dev");
        let start = dir_with("start", &[(
            "package.json",
            r#"{ "scripts": { "start": "node server.js" } }"#,
        )]);
        assert_eq!(detect(&start).unwrap().argv, vec!["npm", "start"]);
        let csproj = dir_with("dotnet", &[("a.csproj", "")]);
        assert_eq!(detect(&csproj).unwrap().argv, vec!["dotnet", "run"]);
        let nested = dir_with("nested", &[]);
        let sub = nested.join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("a.csproj"), "").unwrap();
        std::fs::write(sub.join("z.csproj"), "").unwrap();
        let cmd = detect(&nested).unwrap();
        assert_eq!(cmd.argv, vec!["dotnet", "run", "--project", "sub"]);
        assert_eq!(cmd.label, "dotnet run --project sub");
        let bare = dir_with("bare", &[]);
        assert!(detect(&bare).is_none());
    }

    #[test]
    fn package_json_corrupt_or_without_scripts_is_none() {
        // package.json PRESENT means decision stops there — no csproj fallback
        // (runctl.py:31-42: the is_file branch returns in all cases)
        let d = dir_with("broken", &[("package.json", "{ nope")]);
        assert!(detect(&d).is_none());
        let d = dir_with("noscripts", &[("package.json", r#"{ "name": "x" }"#)]);
        assert!(detect(&d).is_none());
        let d = dir_with("nodev", &[("package.json", r#"{ "scripts": { "test": "t" } }"#)]);
        assert!(detect(&d).is_none());
    }

    #[test]
    fn pane_environ_marks_and_path() {
        let env = pane_environ();
        let get = |k: &str| env.iter().find(|e| e.starts_with(&format!("{k}=")));
        // every ambient marker must be overridden to an EMPTY value (never merely
        // omitted — VTE merges envv onto the inherited environ; CLAUDE.md invariant)
        for k in [
            "CLAUDECODE", "CLAUDE_PID", "CLAUDE_CODE_CHILD_SESSION",
            "CLAUDE_CODE_ENTRYPOINT", "CLAUDE_CODE_SESSION_ID",
            "CLAUDE_CODE_SESSION_ATTENDED", "CLAUDE_CODE_MESSAGING_SOCKET",
            "CLAUDE_CODE_MESSAGING_TOKEN", "CLAUDE_CODE_SSE_PORT",
        ] {
            // present in the ambient environ → overridden to EMPTY (VTE merge
            // invariant); absent from it → must NOT appear at all (python:
            // `if k in os.environ`)
            if std::env::var_os(k).is_some() {
                assert_eq!(get(k).unwrap().split_once('=').unwrap().1, "");
            } else {
                assert!(get(k).is_none(), "{k} absent from parent must stay absent");
            }
        }
        // PATH append invariant (python: `p not in PATH.split(os.pathsep)`);
        // present anywhere → PATH untouched; absent → appended after a colon
        let home = std::env::var("HOME").unwrap();
        let local_bin = format!("{home}/.local/bin");
        let p_before = std::env::var("PATH").unwrap_or_default();
        if p_before.split(':').any(|c| c == local_bin) {
            assert_eq!(get("PATH").unwrap(), &format!("PATH={p_before}"));
        } else {
            let after = get("PATH").unwrap().to_owned();
            assert!(after.ends_with(&format!(":{local_bin}")), "PATH must append ~/.local/bin");
        }
    }
}