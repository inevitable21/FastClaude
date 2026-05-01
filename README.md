# FastClaude

A fast launcher for [Claude Code](https://docs.claude.com/en/docs/claude-code/setup) sessions on Windows. Pop open a project, hit a hotkey, get a Claude session in a labeled terminal tab.

## Install (Windows)

1. Download the latest `.msi` from [Releases](https://github.com/inevitable21/FastClaude/releases/latest).
2. Run the installer.
3. SmartScreen will warn — click **More info → Run anyway**. (FastClaude is unsigned for v1.0; signing certificate may come later.)

You also need the [`claude` CLI](https://docs.claude.com/en/docs/claude-code/setup) on your PATH. FastClaude will tell you if it isn't.

## First run

The app walks you through three choices: terminal program (default `auto`), default model (e.g. `claude-opus-4-7`), and a global hotkey (default `Ctrl+Shift+C`). All three are editable later in Settings.

## Usage

- Click **+ Launch new session** or press your hotkey from anywhere.
- Pick a project folder and a model.
- Each session opens in a Windows Terminal tab labeled `FastClaude: <project>`.
- The dashboard shows running/idle status and output token counts, refreshed every few seconds.
- Click **Focus** to bring the session's terminal forward; **Kill** to end it.

## Auto-updates

The app checks for updates ~5 seconds after launch. When one's available, a banner offers to restart and install. You can also check manually from Settings.

## Build from source

```bash
git clone https://github.com/inevitable21/FastClaude
cd FastClaude
npm install
npm run tauri dev
```

Requires Node 20+, Rust stable, and the [Tauri 2 Windows prerequisites](https://tauri.app/v2/guides/prerequisites/).

## Status

- **Windows:** supported
- **macOS / Linux:** not yet supported. Stub backends will tell you so. PRs welcome.

## License

MIT — see [LICENSE](LICENSE).
