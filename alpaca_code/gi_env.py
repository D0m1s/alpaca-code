"""Resolve gir version strings. Call before `from gi.repository import …` in every module."""
import gi

_CACHE: dict[str, str] = {}

def require(ns: str, versions: tuple[str, ...]) -> str:
    if ns not in _CACHE:
        for v in versions:
            try:
                gi.require_version(ns, v)
                _CACHE[ns] = v
                break
            except ValueError:
                pass
        else:
            raise ImportError(f"no gir for {ns} tried {versions}")
    return _CACHE[ns]