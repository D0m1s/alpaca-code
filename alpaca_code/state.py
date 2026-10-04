"""state.json persistence: ~/.config/alpaca-code/state.json"""
import json, os

STATE_PATH = os.path.expanduser("~/.config/alpaca-code/state.json")

def default() -> dict:
    return {"last_project": None, "recents": [], "projects": {}}

def load(path: str = STATE_PATH) -> dict:
    try:
        with open(path, encoding="utf-8") as f:
            s = json.load(f)
    except (OSError, ValueError):
        return default()
    if not isinstance(s, dict):
        return default()
    s = {**default(), **s}
    # wrong-typed-but-valid JSON must not brick startup (Reviewer finding) — coerce per key
    if not (isinstance(s["recents"], list) and isinstance(s["projects"], dict)
            and (s["last_project"] is None or isinstance(s["last_project"], str))):
        return default()
    s["recents"] = [p for p in s["recents"] if isinstance(p, str) and p]
    s["projects"] = {k: v for k, v in s["projects"].items() if isinstance(k, str) and isinstance(v, dict)}
    return s

def save(s: dict, path: str = STATE_PATH) -> None:
    os.makedirs(os.path.dirname(path), exist_ok=True)
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(s, f, indent=2)
    os.replace(tmp, path)

def remember(s: dict, project_path: str) -> dict:
    s["last_project"] = project_path
    s["recents"] = ([project_path] + [p for p in s["recents"] if p != project_path])[:10]
    return s

def project_tabs(s: dict, project_path: str) -> dict:
    # per-project shape is hand-editable/corruptible just like top level: read must
    # never raise, or a bad entry bricks every startup that opens the project
    d = s["projects"].get(project_path) or {}
    if not isinstance(d, dict):
        return {"open_tabs": [], "active_tab": 0}
    raw = d.get("open_tabs", [])
    tabs = [t for t in raw if isinstance(t, str)] if isinstance(raw, (list, tuple)) else []
    try:
        active = int(d.get("active_tab", 0))
    except (TypeError, ValueError):
        active = 0
    return {"open_tabs": tabs, "active_tab": active}

def set_tabs(s: dict, project_path: str, open_tabs: list, active: int) -> dict:
    s.setdefault("projects", {})[project_path] = {"open_tabs": list(open_tabs), "active_tab": int(active)}
    return s