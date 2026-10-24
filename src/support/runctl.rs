
use std::path::Path;

const CLAUDE_SESSION_MARKERS: [&str; 9] = [
    "CLAUDECODE", "CLAUDE_PID", "CLAUDE_CODE_CHILD_SESSION", "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_SESSION_ID", "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_MESSAGING_SOCKET", "CLAUDE_CODE_MESSAGING_TOKEN", "CLAUDE_CODE_SSE_PORT",
];

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
        let mut env = env;
    if let Some(home) = std::env::var_os("HOME") {
        let p = Path::new(&home).join(".local/bin");
        if p.is_dir()
            && let Some(ps) = p.to_str()
                && let Some(idx) = env.iter().position(|e| e.starts_with("PATH=")) {
                    let cur = &env[idx]["PATH=".len()..];
                    if !cur.split(':').any(|c| c == ps) {
                        env[idx] = format!("PATH={cur}:{ps}");
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
    let is_csproj = |e: &std::fs::DirEntry| {
        e.path().extension() == Some(std::ffi::OsStr::new("csproj"))
    };
    if let Ok(rs) = std::fs::read_dir(root)
        && rs.filter_map(|e| e.ok()).any(|e| is_csproj(&e)) {
            return Some(RunCmd { argv: vec!["dotnet".into(), "run".into()], label: "dotnet run".into() });
        }
                    let runs = |p: &Path| {
        std::fs::read_to_string(p).ok().is_some_and(|mut s| {
            s.make_ascii_lowercase();
            s.contains("microsoft.net.sdk.web") || s.contains("<outputtype>exe")
        })
    };
    let mut subdirs: Vec<std::fs::DirEntry> = std::fs::read_dir(root)
        .ok()?
        .filter_map(|e| e.ok())
        .collect();
    subdirs.sort_by_key(|a| a.file_name());
    for pick in [true, false] {
        for d in &subdirs {
            if std::fs::read_dir(d.path()).ok().is_some_and(|rd| {
                rd.filter_map(|e| e.ok())
                    .any(|e| is_csproj(&e) && (!pick || runs(&e.path())))
            }) {
                let rel = d.file_name().to_string_lossy().into_owned();
                return Some(RunCmd {
                    argv: vec!["dotnet".into(), "run".into(), "--project".into(), rel.clone()],
                    label: format!("dotnet run --project {rel}"),
                });
            }
        }
    }
    None
}

pub fn create_project(target: &Path) -> Result<(), String> {
    if target.exists() {
                return Err(format!("File exists: '{}'", target.display()));
    }
        let parent = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    std::fs::create_dir(target).map_err(|e| e.to_string())?;
    git_init(target)
}

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
        let _ = std::fs::remove_dir_all(&d); 
        std::fs::create_dir_all(&d).unwrap();
        for (name, contents) in files {
            std::fs::write(d.join(name), contents).unwrap();
        }
        d
    }

    #[test]
    fn create_project_makedirs_and_git_init() {
                let base = std::env::temp_dir().join("alpaca-newproj");
        let _ = std::fs::remove_dir_all(&base);
        let parent = base.join("p"); 
        let target = parent.join("proj");
        create_project(&target).unwrap();
        assert!(target.is_dir());
        assert!(target.join(".git").is_dir());
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
    fn nested_prefers_runnable_host_over_first_lexical() {
                        let d = dir_with("hostpick", &[]);
        let lib = d.join("aaa_lib");
        std::fs::create_dir(&lib).unwrap();
        std::fs::write(
            lib.join("lib.csproj"),
            r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup></PropertyGroup></Project>"#,
        )
        .unwrap();
        let web = d.join("zzz_web");
        std::fs::create_dir(&web).unwrap();
        std::fs::write(
            web.join("web.csproj"),
            r#"<Project Sdk="Microsoft.NET.Sdk.Web"><PropertyGroup></PropertyGroup></Project>"#,
        )
        .unwrap();
        let cmd = detect(&d).unwrap();
        assert_eq!(cmd.argv, vec!["dotnet", "run", "--project", "zzz_web"]);
        assert_eq!(cmd.label, "dotnet run --project zzz_web");
                let d = dir_with("conpick", &[]);
        for (dir, body) in [
            ("aaa_lib", r#"<Project Sdk="Microsoft.NET.Sdk"></Project>"#),
            (
            "mmm_con",
            r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType></PropertyGroup></Project>"#,
            ),
        ] {
            let s = d.join(dir);
            std::fs::create_dir(&s).unwrap();
            std::fs::write(s.join("p.csproj"), body).unwrap();
        }
        let cmd = detect(&d).unwrap();
        assert_eq!(cmd.argv, vec!["dotnet", "run", "--project", "mmm_con"]);
    }

    #[test]
    fn nested_all_libraries_falls_back_to_first_lexical() {
                let d = dir_with("alllib", &[]);
        for dir in ["aaa_lib", "zzz_lib"] {
            let s = d.join(dir);
            std::fs::create_dir(&s).unwrap();
            std::fs::write(
                s.join("p.csproj"),
                r#"<Project Sdk="Microsoft.NET.Sdk"></Project>"#,
            )
            .unwrap();
        }
        assert_eq!(
            detect(&d).unwrap().argv,
            vec!["dotnet", "run", "--project", "aaa_lib"]
        );
    }

    #[test]
    fn package_json_corrupt_or_without_scripts_is_none() {
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
                        for k in [
            "CLAUDECODE", "CLAUDE_PID", "CLAUDE_CODE_CHILD_SESSION",
            "CLAUDE_CODE_ENTRYPOINT", "CLAUDE_CODE_SESSION_ID",
            "CLAUDE_CODE_SESSION_ATTENDED", "CLAUDE_CODE_MESSAGING_SOCKET",
            "CLAUDE_CODE_MESSAGING_TOKEN", "CLAUDE_CODE_SSE_PORT",
        ] {
                                                if std::env::var_os(k).is_some() {
                assert_eq!(get(k).unwrap().split_once('=').unwrap().1, "");
            } else {
                assert!(get(k).is_none(), "{k} absent from parent must stay absent");
            }
        }
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