"""Branch name from .git/HEAD. Pure — no gi imports, no git binary."""
import os

def branch_of(root: str) -> str | None:
    try:
        with open(os.path.join(root, ".git", "HEAD"), encoding="utf-8") as f:
            content = f.read().strip()
    except OSError:
        return None
    if content.startswith("ref: refs/heads/"):
        return content[len("ref: refs/heads/"):]
    return None  # detached HEAD / worktree pointer → hide branch widget