"""Detect how to run a project: npm scripts.dev → scripts.start, or dotnet run. Pure — no gi imports."""
import glob, json, os

# Claude's ambient session markers, exported by any claude session to its children.
# When the app itself was launched from inside a claude session, pane children would
# inherit them: the Agent pane's claude then sees the nested marker and disables
# transcript saving (unresumable sessions), besides leaking stale ids/sockets.
# Pane children are top-level, not nested — claude there runs like a fresh invocation.
_CLAUDE_SESSION_MARKERS = (
    "CLAUDECODE", "CLAUDE_PID", "CLAUDE_CODE_CHILD_SESSION", "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_SESSION_ID", "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_MESSAGING_SOCKET", "CLAUDE_CODE_MESSAGING_TOKEN", "CLAUDE_CODE_SSE_PORT",
)

def pane_environ() -> list[str]:
    # VTE spawn_async MERGES envv onto the child's INHERITED environ (measured:
    # /tmp probe, child kept all parent vars) — omitting a key here does NOT remove
    # it from the child. Override instead: claude's nested-marker check is a plain
    # truthiness test on CLAUDE_CODE_CHILD_SESSION, so an empty value disables it.
    env = {k: v for k, v in os.environ.items() if k not in _CLAUDE_SESSION_MARKERS}
    for k in _CLAUDE_SESSION_MARKERS:
        if k in os.environ:
            env[k] = ""
    p = os.path.expanduser("~/.local/bin")
    if os.path.isdir(p) and p not in env.get("PATH", "").split(os.pathsep):
        env["PATH"] = env.get("PATH", "") + os.pathsep + p
    return [f"{k}={v}" for k, v in env.items()]

def detect(root: str) -> dict | None:
    pj = os.path.join(root, "package.json")
    if os.path.isfile(pj):
        try:
            with open(pj, encoding="utf-8") as f:
                scripts = (json.load(f) or {}).get("scripts")
        except (OSError, ValueError, AttributeError):
            scripts = None
        if isinstance(scripts, dict):
            if "dev" in scripts:
                return {"argv": ["npm", "run", "dev"], "label": "npm run dev"}
            if "start" in scripts:
                return {"argv": ["npm", "start"], "label": "npm start"}
        return None
    if glob.glob(os.path.join(root, "*.csproj")):
        return {"argv": ["dotnet", "run"], "label": "dotnet run"}
    nested = sorted(glob.glob(os.path.join(root, "*", "*.csproj")))
    if nested:
        d = os.path.relpath(os.path.dirname(nested[0]), root)
        return {"argv": ["dotnet", "run", "--project", d], "label": f"dotnet run --project {d}"}
    return None