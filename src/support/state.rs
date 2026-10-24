
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gtk4::gio::prelude::FileExt; 
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct State {
    pub last_project: Option<String>,
    pub recents: Vec<String>,
    pub projects: HashMap<String, ProjectTabs>,
                    pub open_projects: Vec<String>,
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

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct WorkbenchFile {
    pub name: String,
                    pub display: Option<String>,
    pub projects: HashMap<String, ProjectSession>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct ProjectSession {
    pub language: Option<String>,
    pub vpane: i32,
    pub hpane: i32,
    pub scroll: Option<ScrollPos>,
        pub expanded: Vec<String>,
        pub changes_closed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct ScrollPos {
    pub rel: String,
    pub frac: f64,
}

impl<'de> serde::Deserialize<'de> for ProjectTabs {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        serde_json::Value::deserialize(d).map(|v| parse_project_entry(&v))
    }
}

pub fn state_path() -> PathBuf {
    if let Some(mut dir) = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
        && dir.is_absolute() {
            dir.push("alpaca-code");
            return dir.join("state.json");
        }
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join(".config/alpaca-code/state.json")
}

fn parse(v: &serde_json::Value) -> State {
    match v {
        serde_json::Value::Object(root) => {
            let get = |k: &str| root.get(k).unwrap_or(&serde_json::Value::Null);
                        let recents = get("recents");
            let projects = get("projects");
            let last = get("last_project");
            let last_ok = last.is_null() || last.is_string();
            if !(recents.is_array() && projects.is_object() && last_ok) {
                return State::default();
            }
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
                                                last_workbench: get("last_workbench").as_str().map(str::to_owned),
                window: parse_window(get("window")),
            }
        }
        _ => State::default(),
    }
}

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
            .map(|f| f as i64) 
            .unwrap_or(0),
        _ => 0,
    };
    ProjectTabs { open_tabs, active_tab }
}

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
    let display = root
        .get("display")
        .and_then(|d| d.as_str())
        .map(str::to_owned);
    Some(WorkbenchFile { name, display, projects })
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

fn parse_scroll(v: &serde_json::Value) -> Option<ScrollPos> {
    let obj = v.as_object()?;
    let rel = obj.get("rel")?.as_str()?;
    let frac = obj.get("frac")?.as_f64()?;
    Some(ScrollPos { rel: rel.to_owned(), frac })
}

pub fn workbenches_dir() -> PathBuf {
    if let Some(mut dir) = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
        && dir.is_absolute() {
            dir.push("alpaca-code");
            return dir.join("workbenches");
        }
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join(".config/alpaca-code/workbenches")
}

pub fn workbench_path(name: &str) -> PathBuf {
    let clean: String = name
        .chars()
        .filter(|c| c.is_ascii_hexdigit() || *c == '-')
        .map(|c| c.to_ascii_lowercase())
        .collect();
    workbenches_dir().join(format!("{clean}.json"))
}

pub fn load_workbench_from(path: &Path) -> Option<WorkbenchFile> {
    let text = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    parse_workbench(&v)
}

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

pub fn delete_workbench(name: &str) -> bool {
    delete_workbench_at(&workbench_path(name))
}

pub fn delete_workbench_at(p: &Path) -> bool {
    if !p.exists() {
        return false;
    }
    let f = gtk4::gio::File::for_path(p);
    let trashed = f.trash(None::<&gtk4::gio::Cancellable>).is_ok();
            trashed || std::fs::remove_file(p).is_ok()
}

pub fn set_workbench_display(name: &str, display: &str) -> bool {
    set_workbench_display_at(&workbench_path(name), display)
}

pub fn set_workbench_display_at(p: &Path, display: &str) -> bool {
    let Some(mut wb) = load_workbench_from(p) else {
        return false;
    };
    let old_mtime = std::fs::metadata(p).and_then(|m| m.modified()).ok();
    wb.display = Some(display.to_owned());
    save_workbench_to(&wb, p);
    if let Some(t) = old_mtime {
        if let Ok(f) = std::fs::OpenOptions::new().write(true).open(p) {
            let _ = f.set_times(std::fs::FileTimes::new().set_modified(t));
        }
    }
    true
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
    s.recents = recents.into_iter().take(10).collect(); 
    s
}

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
                let bad = json!({"recents": "x", "projects": [], "last_project": 3});
        assert_eq!(parse(&bad), State::default());
                let ok = json!({
            "last_project": "/a", "recents": ["/a", "", 42, null],
            "projects": {" /a": {"open_tabs": ["r/a", "r/b"], "active_tab": 2}, "/bad": 3 }
        });
        let s = parse(&ok);
        assert_eq!(s.recents, vec!["/a".to_string()]);
        assert!(s.projects.contains_key(" /a")); 
        assert_eq!(s.projects.get("/bad"), None);
        assert_eq!(s.projects[" /a"].active_tab, 2);
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
        assert_eq!(s.recents, vec!["/p", "/q"]); 
        for i in 0..15 {
            let _ = i;
            s = remember(s, &format!("/r{i}"));
        }
        assert_eq!(s.recents.len(), 10);
        assert_eq!(s.recents[0], "/r14");
        s = set_tabs(s, "/p", vec!["a.md".into()], 0);
        assert_eq!(project_tabs(&s, "/p").open_tabs, vec!["a.md".to_string()]);
        assert_eq!(project_tabs(&s, "/missing"), ProjectTabs::default()); 
    }

    #[test]
    fn tabs_coercion_per_project() {
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
                        let old = parse(&json!({"last_project": "/a", "recents": ["/a"], "projects": {}}));
        assert_eq!(old.open_projects, Vec::<String>::new());
        assert_eq!(old.recents, vec!["/a".to_string()]);
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
                                let old = parse(&json!({"last_project": "/a", "recents": ["/a"], "projects": {}}));
        assert_eq!(old.last_workbench, None);
        assert_eq!(old.window, None);
        assert_eq!(old.recents, vec!["/a".to_string()]);
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
                let s = parse(&json!({"recents": [], "projects": {}, "window": null}));
        assert_eq!(s.window, None);
    }

    #[test]
    fn workbench_roundtrip_and_corrupt() {
        let p = tmp("wb.json");
        let mut wb = WorkbenchFile { name: "deadbeef-1234".into(), display: None, projects: Default::default() };
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
        assert_eq!(load_workbench_from(&p), None); 
        std::fs::write(&p, "[]").unwrap();
        assert_eq!(load_workbench_from(&p), None); 
                let q = workbench_path("a/b?c");
        assert!(q.to_string_lossy().contains("bc"));
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn workbench_display_roundtrip_and_parse_fallback() {
        let p = tmp("wbdisp.json");
                        let mut wb = WorkbenchFile {
            name: "deadbeef-1234".into(),
            display: Some("My Workbench".into()),
            projects: Default::default(),
        };
        save_workbench_to(&wb, &p);
        assert_eq!(load_workbench_from(&p), Some(wb.clone()));
                wb.display = None;
        save_workbench_to(&wb, &p);
        assert_eq!(load_workbench_from(&p).unwrap().display, None);
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn set_workbench_display_rewrites_and_keeps_mtime() {
        let dir = std::env::temp_dir().join("alpaca-state-display");
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a1a1bbbb-0001.json");
        save_workbench_to(
            &WorkbenchFile { name: "a1a1bbbb-0001".into(), display: None, projects: Default::default() },
            &p,
        );
        let before = std::fs::metadata(&p).and_then(|m| m.modified()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50)); 
        assert!(set_workbench_display_at(&p, "Nice"));
        assert_eq!(
            load_workbench_from(&p).unwrap().display.as_deref(),
            Some("Nice")
        );
                let after = std::fs::metadata(&p).and_then(|m| m.modified()).unwrap();
        assert_eq!(after, before);
                let miss = dir.join("nope-0001.json");
        assert!(!set_workbench_display_at(&miss, "x"));
        assert!(!miss.exists());
    }

    #[test]
    fn delete_workbench_removes_file() {
        let dir = std::env::temp_dir().join("alpaca-state-delete");
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("b1b1bbbb-0001.json");
        save_workbench_to(
            &WorkbenchFile { name: "b1b1bbbb-0001".into(), display: None, projects: Default::default() },
            &p,
        );
        assert!(delete_workbench_at(&p));
        assert!(!p.exists()); 
        assert!(!delete_workbench_at(&p)); 
    }

    #[test]
    fn workbench_entry_coercion() {
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
        assert_eq!(s1.vpane, 592); 
        assert_eq!(s1.hpane, 592); 
        assert_eq!(s1.scroll, None);
        assert_eq!(s1.expanded, vec!["/w/src".to_string()]);
        assert_eq!(s1.changes_closed, Vec::<String>::new());
        let s2 = wb.projects.get("/w2").unwrap();
        assert_eq!(s2.vpane, -3);
        assert_eq!(s2.scroll, None); 
        assert!(wb.projects.get("/w3").is_none()); 
    }

    #[test]
    fn save_load_roundtrip_and_corrupt() {
        let p = tmp("rt.json");
        let s = set_tabs(remember(State::default(), "/w"), "/w", vec!["x".into()], 1);
        save_to(&s, &p);
        assert_eq!(load_from(&p), s);
        std::fs::write(&p, "not json at all").unwrap();
        assert_eq!(load_from(&p), State::default()); 
        std::fs::write(&p, "[]").unwrap();
        assert_eq!(load_from(&p), State::default()); 
        std::fs::remove_file(&p).ok();
    }
}