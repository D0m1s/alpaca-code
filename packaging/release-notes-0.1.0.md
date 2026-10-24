# alpaca-code 0.1.0

A GTK4 desktop editor that puts the **claude agent** where your code lives — one dark
window with a tabbed code editor, a file browser, and console strip running the real
`claude` TUI beside your terminal and your app's output. Multiple projects live side
by side; whatever you leave running keeps running when you switch.

## Downloads

| artifact | use it when |
|---|---|
| `alpaca-code-0.1.0-linux-x86_64.AppImage` | any x86_64 Linux desktop — the GTK 4.22 stack ships inside, nothing to install, run it directly |
| `alpaca-code-0.1.0-linux-x86_64.tar.gz` | you're on a distro with GTK ≥ 4.22 (Arch, nixos, bleeding edge) and want the bare binary — `./install-linux.sh` wires the launcher entry + app icon into `~/.local` |
| `alpaca-code-0.1.0-macos-arm64.zip` | macOS 14+ on Apple Silicon — unpack and double-click `alpaca-code.app` (first launch: right-click → **Open**, or `xattr -cr alpaca-code.app` once; not notarized) |

No Windows build in this release: the agent/terminal panes run on VTE, which upstream
supports only on POSIX/Darwin (no ConPTY backend, so Windows has nothing to link).
That's a porting session, not a packaging tweak — tracked for a later release.

## Requirements

- The `claude` CLI on PATH (`npm install -g @anthropic-ai/claude-code`) for the agent pane; the editor, file tree and git views work without it.
- Verify your download against `sha256sums.txt`.

## License & notices

- Code is dual-licensed — your choice of [MIT](https://github.com/D0m1s/alpaca-code/blob/v0.1.0/LICENSE-MIT)
  or [Apache-2.0](https://github.com/D0m1s/alpaca-code/blob/v0.1.0/LICENSE-APACHE)
  (SPDX `MIT OR Apache-2.0`); both files ship in the repo and the linux tarball.
- The macOS bundle redistributes the DejaVu fonts; their Bitstream Vera + Arev terms are
  at [`packaging/fonts/LICENSE`](https://github.com/D0m1s/alpaca-code/blob/v0.1.0/packaging/fonts/LICENSE).