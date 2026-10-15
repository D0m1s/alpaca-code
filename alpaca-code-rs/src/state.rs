//! state.json persistence — ~/.config/alpaca-code/state.json.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct State {
    pub last_project: Option<String>,
    pub recents: Vec<String>,
    pub projects: HashMap<String, ProjectTabs>,
    /// multi-workspace (2026-10-14): the open tabs at last save, add order =
    /// strip order. Read path is parse() — the derive's Deserialize is never
    /// exercised (load_from goes through serde_json::Value), absent keys on
    /// old files are fine.
    pub open_projects: Vec<String>,
    /// workbench step (2026-10-14): the last saved workbench's uuid + the
    /// window geometry, both additive/lenient (parse reads them per-key; the
    /// derive's Deserialize must default them for old files).
    #[serde(default)]
    pub last_workbench: Option<String>,
    #[serde(default)]
    pub window: Option<WindowGeom>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct WindowGeom {
    pub width: i32,
    pub height: i32,
    pub maximized: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct ProjectTabs {
    pub open_tabs: Vec<String>,
    pub active_tab: i64,
}

/// One workbench file — ~/.config/alpaca-code/workbenches/<name>.json.
/// The app mints `name` (uuid placeholder); `projects` keys are ABS roots.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct WorkbenchFile {
    pub name: String,
    pub projects: HashMap<String, ProjectSession>,
}

/// Per-project session state inside a workbench file. vpane/hpane are plain
/// positions (0/negative means "not saved" → built-in default at restore, no
/// Option: a real GTK paned clamps its own minimums above 0).
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct ProjectSession {
    pub language: Option<String>,
    pub vpane: i32,
    pub hpane: i32,
    pub scroll: Option<ScrollPos>,
    /// filetree's open set — ABS dir paths
    pub expanded: Vec<String>,
    /// gitview's closed set — REL dir paths
    pub changes_closed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct ScrollPos {
    pub rel: String,
    pub frac: f64,
}

impl<'de> serde::Deserialize<'de> for ProjectTabs {
    // Read path is parse_project_entry (per-element tolerance), never an error.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        serde_json::Value::deserialize(d).map(|v| parse_project_entry(&v))
    }
}

/// ~/.config/alpaca-code/state.json — the SHARED file (spec §2).
pub fn state_path() -> PathBuf {
    if let Some(mut dir) = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
        && dir.is_absolute() {
            dir.push("alpaca-code");
            return dir.join("state.json");
        }
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join(".config/alpaca-code/state.json")
}

/// Load from arbitrary JSON value with the tolerance contract:
/// wrong-typed-but-valid JSON must never brick startup.
fn parse(v: &serde_json::Value) -> State {
    match v {
        serde_json::Value::Object(root) => {
            let get = |k: &str| root.get(k).unwrap_or(&serde_json::Value::Null);
            // wholesale check BEFORE per-entry filtering
            let recents = get("recents");
            let projects = get("projects");
            let last = get("last_project");
            let last_ok = last.is_null() || last.is_string();
            if !(recents.is_array() && projects.is_object() && last_ok) {
                return State::default();
            }
            // per-entry filtering
            let recents = recents
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|p| p.as_str().filter(|s| !s.is_empty()))
                .map(str::to_owned)
                .collect();
            let projects: HashMap<String, ProjectTabs> = projects
                .as_object()
                .unwrap()
                .iter()
                .filter(|(_k, v)| v.is_object())
                .map(|(k, v)| (k.clone(), parse_project_entry(v)))
                .collect();
            // lenient per-key: absent on pre-multi-workspace files → empty;
            // non-array → empty; strings only, capped at the strip's 3
            let open_projects = get("open_projects")
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|p| p.as_str().filter(|s| !s.is_empty()))
                        .map(str::to_owned)
                        .take(3)
                        .collect()
                })
                .unwrap_or_default();
            State {
                last_project: serde_json::from_value(last.clone()).ok(),
                recents,
                projects,
                open_projects,
                // workbench-step keys: lenient per-key, absent on old files —
                // must never feed the wholesale default gate above
                last_workbench: get("last_workbench").as_str().map(str::to_owned),
                window: parse_window(get("window")),
            }
        }
        _ => State::default(),
    }
}

/// Coerce one per-project entry with int()/list-filter tolerance:
/// open_tabs non-str filtered, non-list → empty; active_tab truthy-str/float/bool
/// coerced (str parsed, float truncated toward 0, bool 1/0), error → 0.
fn parse_project_entry(v: &serde_json::Value) -> ProjectTabs {
    const EMPTY: ProjectTabs = ProjectTabs { open_tabs: Vec::new(), active_tab: 0 };
    let Some(obj) = v.as_object() else { return EMPTY };
    let raw = obj.get("open_tabs").unwrap_or(&serde_json::Value::Null);
    let open_tabs = raw
        .as_array()
        .map(|a| a.iter().filter_map(|t| t.as_str()).map(str::to_owned).collect())
        .unwrap_or_default();
    let active_tab = match obj.get("active_tab").unwrap_or(&serde_json::Value::Null) {
        serde_json::Value::Null => 0,
        serde_json::Value::Bool(b) => *b as i64,
        serde_json::Value::String(s) => s.trim().parse::<i64>().unwrap_or(0),
        serde_json::Value::Number(n) => n
            .as_f64()
            .map(|f| f as i64) // truncate toward 0: 2.7 → 2
            .unwrap_or(0),
        _ => 0,
    };
    ProjectTabs { open_tabs, active_tab }
}

/// The saved window geometry, or None (missing/wrong shape). Both extents
/// must coerce — a half-geometry is no geometry.
fn parse_window(v: &serde_json::Value) -> Option<WindowGeom> {
    let obj = v.as_object()?;
    let num = |k: &str| {
        match obj.get(k).unwrap_or(&serde_json::Value::Null) {
            serde_json::Value::Number(n) => n.as_f64().map(|f| f as i32),
            serde_json::Value::String(s) => s.trim().parse::<i32>().ok(),
            _ => None,
        }
    };
    let maximized = match obj.get("maximized").unwrap_or(&serde_json::Value::Null) {
        serde_json::Value::Bool(b) => *b,
        _ => false,
    };
    Some(WindowGeom { width: num("width")?, height: num("height")?, maximized })
}

/// One workbench file, tolerant: non-object top-level → None (the caller's
/// "was there ever a file" signal), non-object session entries dropped,
/// everything else coerced like parse_project_entry. Never raises.
pub fn parse_workbench(v: &serde_json::Value) -> Option<WorkbenchFile> {
    let root = v.as_object()?;
    let name = root.get("name").and_then(|n| n.as_str()).unwrap_or("").to_owned();
    let projects = root
        .get("projects")
        .and_then(|p| p.as_object())
        .unwrap_or(&serde_json::Map::new())
        .iter()
        .filter(|(_k, v)| v.is_object())
        .filter_map(|(k, v)| parse_session(v).map(|s| (k.clone(), s)))
        .collect();
    Some(WorkbenchFile { name, projects })
}

fn parse_session(v: &serde_json::Value) -> Option<ProjectSession> {
    let obj = v.as_object()?;
    let num = |k: &str| -> i32 {
        match obj.get(k).unwrap_or(&serde_json::Value::Null) {
            serde_json::Value::Number(n) => n.as_f64().map(|f| f as i32).unwrap_or(0),
            serde_json::Value::String(s) => s.trim().parse::<i32>().unwrap_or(0),
            serde_json::Value::Bool(b) => *b as i32,
            _ => 0,
        }
    };
    let strs = |k: &str| -> Vec<String> {
        obj.get(k)
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|s| s.as_str()).map(str::to_owned).collect())
            .unwrap_or_default()
    };
    let scroll = obj.get("scroll").and_then(parse_scroll);
    Some(ProjectSession {
        language: obj.get("language").and_then(|v| v.as_str()).map(str::to_owned),
        vpane: num("vpane"),
        hpane: num("hpane"),
        scroll,
        expanded: strs("expanded"),
        changes_closed: strs("changes_closed"),
    })
}

/// {rel: str, frac: number} — anything else → None (never an error value).
fn parse_scroll(v: &serde_json::Value) -> Option<ScrollPos> {
    let obj = v.as_object()?;
    let rel = obj.get("rel")?.as_str()?;
    let frac = obj.get("frac")?.as_f64()?;
    Some(ScrollPos { rel: rel.to_owned(), frac })
}

/// ~/.config/alpaca-code/workbenches/ — same XDG branch as state_path().
pub fn workbenches_dir() -> PathBuf {
    if let Some(mut dir) = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
        && dir.is_absolute() {
            dir.push("alpaca-code");
            return dir.join("workbenches");
        }
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join(".config/alpaca-code/workbenches")
}

/// <dir>/<name>.json. The name is app-minted (uuid), but the path is built
/// from it — sanitize to bare hex/dashes so nothing else can ride in.
pub fn workbench_path(name: &str) -> PathBuf {
    let clean: String = name
        .chars()
        .filter(|c| c.is_ascii_hexdigit() || *c == '-')
        .map(|c| c.to_ascii_lowercase())
        .collect();
    workbenches_dir().join(format!("{clean}.json"))
}

/// Load a workbench file from an arbitrary path. Absent/corrupt → None
/// (the "was there ever a file" signal) — the never-brick contract.
pub fn load_workbench_from(path: &Path) -> Option<WorkbenchFile> {
    let text = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    parse_workbench(&v)
}

/// Atomic tmp+rename write for a workbench file (save_to's shape).
pub fn save_workbench_to(wb: &WorkbenchFile, path: &Path) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(wb).unwrap_or_else(|e| {
        unreachable!("WorkbenchFile serializes by construction: {e}")
    });
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

pub fn load_workbench(name: &str) -> Option<WorkbenchFile> {
    load_workbench_from(&workbench_path(name))
}

pub fn load_from(path: &Path) -> State {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<serde_json::Value>(&text)
            .map(|v| parse(&v))
            .unwrap_or_default(),
        Err(_) => State::default(),
    }
}

pub fn save_to(s: &State, path: &Path) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(s).unwrap_or_else(|e| {
        unreachable!("State serializes by construction: {e}")
    });
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

pub fn remember(mut s: State, project_path: &str) -> State {
    s.last_project = Some(project_path.to_owned());
    let mut recents = vec![project_path.to_owned()];
    recents.extend(s.recents.into_iter().filter(|p| p != project_path));
    s.recents = recents.into_iter().take(10).collect(); // newest first, deduped, cap 10
    s
}

/// The per-project tab set (the app flushes it on its debounce + saves; the
/// open-workspace set itself is rebuilt by the app's strip on add/close —
/// app-side state is the single writer).
pub fn project_tabs(s: &State, project_path: &str) -> ProjectTabs {
    s.projects
        .get(project_path)
        .map(|t| ProjectTabs {
            open_tabs: t.open_tabs.clone(),
            active_tab: t.active_tab,
        })
        .unwrap_or_default()
}

pub fn set_tabs(mut s: State, project_path: &str, open_tabs: Vec<String>, active: i64) -> State {
    s.projects.insert(
        project_path.to_owned(),
        ProjectTabs { open_tabs, active_tab: active },
    );
    s
}

pub fn load() -> State {
    load_from(&state_path())
}

pub fn save(s: &State) {
    save_to(s, &state_path())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmp(p: &str) -> std::path::PathBuf {
        let mut d = std::env::temp_dir();
        d.push(format!("alpaca-state-test-{}", p));
        std::fs::remove_file(&d).ok();
        d
    }

    #[test]
    fn coercion_rules_match_contract() {
        // wrong top-level types → whole default (wholesale, before per-entry filter)
        let bad = json!({"recents": "x", "projects": [], "last_project": 3});
        assert_eq!(parse(&bad), State::default());
        // per-entry filtering: recents keeps non-empty strings only, projects keeps dict values
        let ok = json!({
            "last_project": "/a", "recents": ["/a", "", 42, null],
            "projects": {" /a": {"open_tabs": ["r/a", "r/b"], "active_tab": 2}, "/bad": 3 }
        });
        let s = parse(&ok);
        assert_eq!(s.recents, vec!["/a".to_string()]);
        assert!(s.projects.contains_key(" /a")); // keys not normalized
        assert_eq!(s.projects.get("/bad"), None);
        assert_eq!(s.projects[" /a"].active_tab, 2);
        // last_project None is legal
        let none = parse(&json!({"last_project": null, "recents": [], "projects": {}}));
        assert_eq!(none.last_project, None);
    }

    #[test]
    fn remember_and_tabs() {
        let mut s = State::default();
        s = remember(s, "/p");
        assert_eq!(s.recents, vec!["/p".to_string()]);
        assert_eq!(s.last_project.as_deref(), Some("/p"));
        s = remember(s, "/q");
        s = remember(s, "/p");
        assert_eq!(s.recents, vec!["/p", "/q"]); // newest first, deduped, cap 10 below
        for i in 0..15 {
            let _ = i;
            s = remember(s, &format!("/r{i}"));
        }
        assert_eq!(s.recents.len(), 10);
        assert_eq!(s.recents[0], "/r14");
        s = set_tabs(s, "/p", vec!["a.md".into()], 0);
        assert_eq!(project_tabs(&s, "/p").open_tabs, vec!["a.md".to_string()]);
        assert_eq!(project_tabs(&s, "/missing"), ProjectTabs::default()); // absent → empty
    }

    #[test]
    fn tabs_coercion_per_project() {
        // hand-corruptible per-project shape: read must never raise
        let s = parse(&json!({"recents": [], "projects": {
            "/w1": {"open_tabs": ["a", 5], "active_tab": "x"},
            "/w2": {"open_tabs": "nope", "active_tab": 7},
            "/w3": {"open_tabs": null, "active_tab": null},
            "/w4": 3,
        }}));
        assert_eq!(project_tabs(&s, "/w1"), ProjectTabs { open_tabs: vec!["a".into()], active_tab: 0 });
        assert_eq!(project_tabs(&s, "/w2"), ProjectTabs { open_tabs: vec![], active_tab: 7 });
        assert_eq!(project_tabs(&s, "/w3"), ProjectTabs::default());
        assert_eq!(project_tabs(&s, "/w4"), ProjectTabs::default());
        assert_eq!(project_tabs(&s, "/absent"), ProjectTabs::default());
    }

    #[test]
    fn open_projects_toleration_and_cap() {
        // old files lack the key entirely → empty open_projects, nothing
        // else downgraded (must NOT hit the wholesale default)
        let old = parse(&json!({"last_project": "/a", "recents": ["/a"], "projects": {}}));
        assert_eq!(old.open_projects, Vec::<String>::new());
        assert_eq!(old.recents, vec!["/a".to_string()]);
        // non-array → empty; non-string/non-empty filtered; cap 3. The
        // app-side (open_workspace) rebuilds the set from the live strip on
        // every ADD — dedupe and the ≤3 cap ride that rebuild, not a helper.
        let s = parse(&json!({"recents": [], "projects": {}, "open_projects":
                               ["one", 5, null, "", "two", "three", "four"]}));
        assert_eq!(s.open_projects, vec!["one", "two", "three"]);
    }

    #[test]
    fn open_projects_roundtrip() {
        let p = tmp("op.json");
        let mut s = State::default();
        s.open_projects = vec!["/w1".into(), "/w2".into()];
        s.last_project = Some("/w2".into());
        save_to(&s, &p);
        assert_eq!(load_from(&p), s);
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn state_additive_keys() {
        // workbench-step addition (2026-10-14): last_workbench + window are
        // additive — an old file without them must NOT lose anything and must
        // NOT hit the wholesale default gate.
        let old = parse(&json!({"last_project": "/a", "recents": ["/a"], "projects": {}}));
        assert_eq!(old.last_workbench, None);
        assert_eq!(old.window, None);
        assert_eq!(old.recents, vec!["/a".to_string()]);
        // lenient per-key reads: wrong types → None, right types → Some
        let s = parse(&json!({"recents": [], "projects": {},
                              "last_workbench": 5,
                              "window": {"width": "x", "height": 992, "maximized": false}}));
        assert_eq!(s.last_workbench, None);
        assert_eq!(s.window, None);
        let s = parse(&json!({"recents": [], "projects": {},
                              "last_workbench": "abc-123",
                              "window": {"width": 1586, "height": 992.0, "maximized": true}}));
        assert_eq!(s.last_workbench.as_deref(), Some("abc-123"));
        assert_eq!(s.window, Some(WindowGeom { width: 1586, height: 992, maximized: true }));
        // window Null → None (not an error)
        let s = parse(&json!({"recents": [], "projects": {}, "window": null}));
        assert_eq!(s.window, None);
    }

    #[test]
    fn workbench_roundtrip_and_corrupt() {
        let p = tmp("wb.json");
        let mut wb = WorkbenchFile { name: "deadbeef-1234".into(), projects: Default::default() };
        wb.projects.insert(
            "/w".into(),
            ProjectSession {
                language: Some("rust".into()),
                vpane: 592,
                hpane: 1161,
                scroll: Some(ScrollPos { rel: "src/main.rs".into(), frac: 0.37 }),
                expanded: vec!["/w/src".into()],
                changes_closed: vec!["docs".into()],
            },
        );
        save_workbench_to(&wb, &p);
        assert_eq!(load_workbench_from(&p), Some(wb));
        std::fs::write(&p, "not json at all").unwrap();
        assert_eq!(load_workbench_from(&p), None); // corrupt → None, never panic
        std::fs::write(&p, "[]").unwrap();
        assert_eq!(load_workbench_from(&p), None); // non-object top level
        // name sanitization on the path helper: only [0-9a-f-] survive
        let q = workbench_path("a/b?c");
        assert!(q.to_string_lossy().contains("bc"));
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn workbench_entry_coercion() {
        // hand-corruptible per-session shape: read must never raise
        let v = json!({"name": "n", "projects": {
            "/w1": {"language": "rust", "vpane": "592", "hpane": 592.9, "scroll": null,
                    "expanded": ["/w/src", 5, null], "changes_closed": "nope"},
            "/w2": {"vpane": -3, "scroll": {"rel": "a", "frac": "0.5"}},
            "/w3": 3,
        }});
        let wb = parse_workbench(&v).unwrap();
        assert_eq!(wb.name, "n");
        let s1 = wb.projects.get("/w1").unwrap();
        assert_eq!(s1.language.as_deref(), Some("rust"));
        assert_eq!(s1.vpane, 592); // string parsed
        assert_eq!(s1.hpane, 592); // float truncated toward 0 (592.9 → 592)
        assert_eq!(s1.scroll, None);
        assert_eq!(s1.expanded, vec!["/w/src".to_string()]);
        assert_eq!(s1.changes_closed, Vec::<String>::new());
        let s2 = wb.projects.get("/w2").unwrap();
        assert_eq!(s2.vpane, -3);
        assert_eq!(s2.scroll, None); // wrong-typed frac → None, not an error
        assert!(wb.projects.get("/w3").is_none()); // non-object entry dropped
    }

    #[test]
    fn save_load_roundtrip_and_corrupt() {
        let p = tmp("rt.json");
        let s = set_tabs(remember(State::default(), "/w"), "/w", vec!["x".into()], 1);
        save_to(&s, &p);
        assert_eq!(load_from(&p), s);
        std::fs::write(&p, "not json at all").unwrap();
        assert_eq!(load_from(&p), State::default()); // corrupt → default, never panic
        std::fs::write(&p, "[]").unwrap();
        assert_eq!(load_from(&p), State::default()); // non-object top level
        std::fs::remove_file(&p).ok();
    }
}