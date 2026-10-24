# Probes (display chains)

The app's display-level verification runs as env-gated probe chains inside `app.rs` (step tables + probe fns). Probes are NEVER run with env unset — plain spawns forward to the running single-instance primary and lose the env.

## Environment registry (all gated in app.rs)

| Env | Arg | Chain |
|---|---|---|
| `ALPACA_PROBE_S2=<dir>` | wsA dir | workspace chain: open/switch/diff pages/commit flow; `wsB` = name-replaced sibling of the same parent dir |
| `ALPACA_PROBE_S2_HOLD=N` | — | parks the S2 chain after step N−1 (use 5 = diff page open) for capture |
| `ALPACA_PROBE_S3=<dir>` | wsA dir | hpane width-step chain + tab-strip geometry |
| `ALPACA_PROBE_S4=<dir>` | wsA dir | multi-workspace: opens, switch-survival, cap, close/tombstone/swing/re-tag; rig files: `a.txt`/`b.txt` per dir, `wsB/b2.txt` for the re-tag proof |
| `ALPACA_PROBE_WB_SAVE=<dir>` | wsA dir | workbench pair, side 1: build a session, save, grep written files, end with real `root.close()` |
| `ALPACA_PROBE_WB_RESTORE=<dir>` | wsA dir | side 2: boots on the SAME `XDG_CONFIG_HOME`, asserts restored paneds/scroll/tabs/open-set; `a.txt` needs ~300 lines so the scroll span > 0; row 1 = "sub" dir |
| `ALPACA_PROBE_LN=<dir>` | rig dir | launcher chain (boot page, card order/tiles, real card clicks, rename/`+` identity — 15 steps, commit never driven in the `+` flow) |
| `ALPACA_PROBE_LN_HOLD=N` | — | parks the LN chain after step N−1 = popover+hover capture |

`ALPACA_PROBE_LN` is a rig dir (its own content); S2/S3/S4/WB are wsA dirs (wsB — and wsC for the S4 chain — derived by name replacement).

## Headless rig

```sh
gtk4-broadwayd :N            # fresh number per run (browser capture port = N+8080)
dbus-run-session -- env GDK_BACKEND=broadway BROADWAY_DISPLAY=:N \
    XDG_CONFIG_HOME=<rig-dir-isolated> ALPACA_PROBE_...=<arg> ./target/debug/alpaca-code-rs
```

- `XDG_CONFIG_HOME=<rig dir>` is MANDATORY — probe sessions write `open_projects`/`last_project` and would leak into the user's next startup strip.
- Own DBus session (`dbus-run-session`) → own single-instance scope.
- Between runs: delete `/run/user/1000/broadway*.socket`; kill daemons with `pkill -x gtk4-broadwayd` (the binary NAME is `gtk4-broadwayd`; `pkill -x broadwayd` matches NOTHING and orphans daemons).
- Capture: chromium headless `--screenshot` against the broadway http port (display+8080). `spectacle -a` silently no-ops on this box (rc=0, no file) and KWin's ScreenShot2 is portal-gated. google-chrome-stable is the chromium binary here.
- S3's wrapper arms a 60ms root `queue_draw` damage keeper: an occluded/late-mapped Wayland surface gets no frame callbacks and its allocations freeze after first paint. It prints `PROBE step N`/tick traces and the probe script kills the process at `PROBE S3 DONE` — the timer is never disarmed.
- After a KWin restart, surfaces get no frame callbacks until touched (GTK allocations freeze at startup paint) — live geometry probes need the running display, never a restarted one.
- A second spawn without display isolation forwards to the running primary (env lost, no probe lines) — if the user's app is alive, always use the broadway rig.
- Assert probe state numerically, never eyeball pixels; re-derive window geometry from the KWin Scripting dump each capture (a stale spectacle frame silently scans the wrong window).

## Fixture recipes

S2 diff fixture (full reset before every run — a partial reset is a booby trap: worktree restore landing on a HEAD already holding the new content reads CLEAN and the run gets no diff page):

```sh
# /tmp/s2probe/: a_base.txt (50-line base), a_new.txt (long alternating-width lines, lines 5-44)
cd /tmp/s2probe && rm -rf wsA/.git wsA_origin && git init -q --bare wsA_origin \
 && git init -q wsA && git -C wsA config user.email probe@local \
 && git -C wsA config user.name probe && git -C wsA remote add origin /tmp/s2probe/wsA_origin \
 && cp a_base.txt wsA/a.txt && git -C wsA add a.txt && git -C wsA commit -qm baseonly \
 && (git -C wsA branch -m master main) && git -C wsA push -q -u origin main \
 && cp a_new.txt wsA/a.txt
```

Each probe run commits wsA's change. `ProbeDiffScroll` self-renews (appends a line when a.txt looks wsA — production is env-guarded). wsB holds ZERO commits; wsA needs origin/main==HEAD on branch main.

## Discipline

- An ablation means nothing unless the probe REPRODUCES first.
- Widget pairing must be probed live before trust — the respawn-band bug shipped past 21 green unit tests because the bookkeeping is widget-layer. Kill a pane child externally and count resurrections via `pgrep -P <app pid>`: that is the pane-exit path's only live gate.
- Grep every probe/capture run's log for `parser error` — a dead CSS rule prints one at startup and hides forever otherwise.
- Capture rigs: broadway http port = display+8080 (e.g. `:11` → 8091). A HOLD's outer `sleep` (WAIT) must exceed the chain's ms-sum up to the park — a short sleep shoots MID-CHAIN and reads as a different state.
- Fullscreening the probe window collapses the headerbar to a 12×0 allocation on this build — verify header stuff on a WINDOWED replica; header child allocs read 0 even when painted (locate by pixel pattern).