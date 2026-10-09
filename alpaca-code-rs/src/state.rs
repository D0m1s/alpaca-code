//! state.json persistence — port of alpaca_code/state.py.
//! Shared file with the Python app: ~/.config/alpaca-code/state.json.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct State {
    pub last_project: Option<String>,
    pub recents: Vec<String>,
    pub projects: HashMap<String, ProjectTabs>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct ProjectTabs {
    pub open_tabs: Vec<String>,
    pub active_tab: i64,
}

impl<'de> serde::Deserialize<'de> for ProjectTabs {
    // Read path is parse_project_entry (per-element tolerance), never an error.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        serde_json::Value::deserialize(d).map(|v| parse_project_entry(&v))
    }
}

/// ~/.config/alpaca-code/state.json — the SHARED file (spec §2).
pub fn state_path() -> PathBuf {
    if let Some(mut dir) = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from) {
        if dir.is_absolute() {
            dir.push("alpaca-code");
            return dir.join("state.json");
        }
    }
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join(".config/alpaca-code/state.json")
}

/// Load from arbitrary JSON value with state.py's tolerance contract:
/// wrong-typed-but-valid JSON must never brick startup.
fn parse(v: &serde_json::Value) -> State {
    match v {
        serde_json::Value::Object(root) => {
            let get = |k: &str| root.get(k).unwrap_or(&serde_json::Value::Null);
            // wholesale check BEFORE per-entry filtering (state.py:19-24)
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
            State {
                last_project: serde_json::from_value(last.clone()).ok(),
                recents,
                projects,
            }
        }
        _ => State::default(),
    }
}

/// Coerce one per-project entry the way `int()` + list-filter do in python:
/// open_tabs non-str filtered, non-list → empty; active_tab truthy-str/float/bool
/// coerced like `int()`, error → 0.
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
            .map(|f| f as i64) // int(2.7) → 2 (truncate toward 0)
            .unwrap_or(0),
        _ => 0,
    };
    ProjectTabs { open_tabs, active_tab }
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
    fn coercion_rules_mirror_python() {
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
        assert!(s.projects.contains_key(" /a")); // keys not normalized (mirrors python)
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