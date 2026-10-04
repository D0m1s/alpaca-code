"""Detect how to run a project: npm scripts.dev → scripts.start, or dotnet run. Pure — no gi imports."""
import glob, json, os

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