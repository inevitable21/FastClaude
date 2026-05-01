# FastClaude Plan 3 — Polished Windows v1.0 Shipping Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship FastClaude as a public Windows-only v1.0: .msi installer via GitHub Releases with auto-updater, first-run onboarding, labeled wt tabs, friendly PATH errors, and CI.

**Architecture:** Adds a thin onboarding layer + updater plugin around the existing Tauri 2 / React app. macOS / Linux stubs become user-visible "not yet supported" errors. Distribution is GitHub Releases with Tauri's signed-update mechanism.

**Tech Stack:** Tauri 2 (Rust + React + TypeScript), `tauri-plugin-updater`, GitHub Actions, `windows-latest` runner, `wt.exe` Windows Terminal CLI.

**Spec:** [`docs/superpowers/specs/2026-05-01-fastclaude-plan-3-shipping.md`](../specs/2026-05-01-fastclaude-plan-3-shipping.md)

---

## File structure (created or modified)

| Path | Created/Modified | Purpose |
|---|---|---|
| `LICENSE` | C | MIT license |
| `README.md` | C | What it is, install, first run, hotkey, build-from-source |
| `src-tauri/icons/*` | M | Replace placeholder icon |
| `src-tauri/Cargo.toml` | M | Add `tauri-plugin-updater` |
| `src-tauri/tauri.conf.json` | M | Updater plugin config + pubkey |
| `src-tauri/src/error.rs` | M | New variants `ClaudeNotOnPath`, `PlatformUnsupported` |
| `src-tauri/src/config.rs` | M | `load` returns `(Config, was_created: bool)` |
| `src-tauri/src/spawner/mod.rs` | M | New `PathLookup` trait |
| `src-tauri/src/spawner/windows.rs` | M | Extract `build_wt_argv`, add `--title`, PATH preflight |
| `src-tauri/src/spawner/macos.rs` | M | Return `PlatformUnsupported` |
| `src-tauri/src/spawner/linux.rs` | M | Return `PlatformUnsupported` |
| `src-tauri/src/window_focus/macos.rs` | M | Return `PlatformUnsupported` |
| `src-tauri/src/window_focus/linux.rs` | M | Return `PlatformUnsupported` |
| `src-tauri/src/commands.rs` | M | `get_first_run`, `clear_first_run`, `check_for_update`, `install_update` |
| `src-tauri/src/main.rs` | M | Wire `was_created` into AppState; updater plugin |
| `src/types.ts` | M | New types: `UpdateInfo` |
| `src/lib/ipc.ts` | M | Wrappers for new commands |
| `src/components/Onboarding.tsx` | C | First-run setup screen |
| `src/components/UpdateBanner.tsx` | C | "Update available — restart to install" banner |
| `src/components/Settings.tsx` | M | "Check for updates" button |
| `src/App.tsx` | M | First-run routing + UpdateBanner |
| `.github/workflows/ci.yml` | C | Build + test on every push/PR |
| `.github/workflows/release.yml` | C | Tag-triggered .msi build, sign, upload to release |

---

## Task 1: LICENSE

**Files:**
- Create: `LICENSE`

- [ ] **Step 1: Write the MIT license**

```
MIT License

Copyright (c) 2026 Tal Bozorgi

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

- [ ] **Step 2: Commit**

```bash
git add LICENSE
git commit -m "docs: add MIT license"
```

---

## Task 2: App icon

**Files:**
- Modify: `src-tauri/icons/icon.png` (and the auto-generated siblings)

- [ ] **Step 1: Confirm icon source with maintainer**

Ask the maintainer for a 1024×1024 PNG icon. If unavailable, use a placeholder: a solid dark background with a white lightning bolt, generated via any tool. Save to `src-tauri/icons/source.png`.

- [ ] **Step 2: Generate all icon sizes**

```bash
cd src-tauri && cargo tauri icon icons/source.png
```

Expected: replaces `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.icns`, `icon.ico` in `src-tauri/icons/`.

- [ ] **Step 3: Verify by building**

```bash
cd src-tauri && cargo build
```

Expected: build succeeds; new icon embedded.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/icons/
git commit -m "feat(branding): replace placeholder app icon"
```

---

## Task 3: Extract `build_wt_argv` pure function

**Files:**
- Modify: `src-tauri/src/spawner/windows.rs`

This is a pure refactor enabling Task 4's test.

- [ ] **Step 1: Read the current spawner**

```bash
cat src-tauri/src/spawner/windows.rs
```

Locate the section that constructs the argv passed to `wt.exe`. It currently lives inline inside the `Spawner::spawn` impl.

- [ ] **Step 2: Extract into a module-level function**

Add to `src-tauri/src/spawner/windows.rs` near the top of the file:

```rust
/// Build the argv passed to wt.exe for a given spawn request.
///
/// Pure function so we can unit-test the title flag without spawning a
/// real terminal.
pub(crate) fn build_wt_argv(req: &SpawnRequest) -> Vec<String> {
    let mut argv = vec![
        "-d".to_string(),
        req.project_dir.clone(),
        "cmd.exe".to_string(),
        "/K".to_string(),
        format!("claude --model {}", req.model),
    ];
    if let Some(prompt) = &req.prompt {
        argv.last_mut().unwrap().push_str(&format!(" -p {}", shell_escape::escape(prompt.into())));
    }
    argv
}
```

(If the existing inline argv differs in flag order or shell escaping, mirror the existing behavior exactly — do not change it.)

- [ ] **Step 3: Replace the inline argv with a call to the new function**

In `Spawner::spawn`, replace the `let argv = vec![...]` block with `let argv = build_wt_argv(req);`.

- [ ] **Step 4: Add a regression test**

Append to the `#[cfg(test)] mod tests` block:

```rust
#[test]
fn build_wt_argv_preserves_existing_shape() {
    let req = SpawnRequest {
        project_dir: "C:\\proj".into(),
        model: "claude-opus-4-7".into(),
        prompt: None,
        terminal_program: "wt".into(),
    };
    let argv = build_wt_argv(&req);
    assert_eq!(argv[0], "-d");
    assert_eq!(argv[1], "C:\\proj");
    assert!(argv.iter().any(|a| a.contains("claude --model claude-opus-4-7")));
}
```

- [ ] **Step 5: Run tests**

```bash
cd src-tauri && cargo test build_wt_argv
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/spawner/windows.rs
git commit -m "refactor(spawner/windows): extract build_wt_argv into pure function"
```

---

## Task 4: Labeled wt tabs

**Files:**
- Modify: `src-tauri/src/spawner/windows.rs`

- [ ] **Step 1: Write the failing test**

Append to the `tests` module:

```rust
#[test]
fn build_wt_argv_includes_title_with_project_basename() {
    let req = SpawnRequest {
        project_dir: "C:\\GitProjects\\FastClaude".into(),
        model: "claude-opus-4-7".into(),
        prompt: None,
        terminal_program: "wt".into(),
    };
    let argv = build_wt_argv(&req);
    let title_idx = argv.iter().position(|a| a == "--title").expect("--title present");
    assert_eq!(argv[title_idx + 1], "FastClaude: FastClaude");
}

#[test]
fn build_wt_argv_title_uses_basename_for_unix_style_paths() {
    let req = SpawnRequest {
        project_dir: "/home/u/cool-project".into(),
        model: "claude-opus-4-7".into(),
        prompt: None,
        terminal_program: "wt".into(),
    };
    let argv = build_wt_argv(&req);
    let title_idx = argv.iter().position(|a| a == "--title").unwrap();
    assert_eq!(argv[title_idx + 1], "FastClaude: cool-project");
}
```

- [ ] **Step 2: Run tests, expect FAIL**

```bash
cd src-tauri && cargo test build_wt_argv
```

Expected: the two new tests FAIL (no `--title`).

- [ ] **Step 3: Add `--title` to `build_wt_argv`**

Modify `build_wt_argv`:

```rust
pub(crate) fn build_wt_argv(req: &SpawnRequest) -> Vec<String> {
    let project_name = std::path::Path::new(&req.project_dir)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("session");
    let mut argv = vec![
        "--title".to_string(),
        format!("FastClaude: {project_name}"),
        "-d".to_string(),
        req.project_dir.clone(),
        "cmd.exe".to_string(),
        "/K".to_string(),
        format!("claude --model {}", req.model),
    ];
    if let Some(prompt) = &req.prompt {
        argv.last_mut().unwrap().push_str(&format!(" -p {}", shell_escape::escape(prompt.into())));
    }
    argv
}
```

(Keep the body of any non-wt path unchanged. The `--title` flag is only relevant when wt is the terminal; cmd.exe ignores unknown args, so this doesn't break the cmd fallback.)

- [ ] **Step 4: Run tests, expect PASS**

```bash
cd src-tauri && cargo test build_wt_argv
```

Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/spawner/windows.rs
git commit -m "feat(spawner/windows): label wt tabs as 'FastClaude: <project>'"
```

---

## Task 5: `PathLookup` trait + `ClaudeNotOnPath` error variant

**Files:**
- Modify: `src-tauri/src/error.rs`
- Modify: `src-tauri/src/spawner/mod.rs`

- [ ] **Step 1: Read existing error.rs to see the variant style**

```bash
cat src-tauri/src/error.rs
```

- [ ] **Step 2: Add error variant**

In `src-tauri/src/error.rs`, add a variant alongside the existing ones:

```rust
#[error("`claude` CLI not found on PATH. Install Claude Code from https://docs.claude.com/en/docs/claude-code/setup, then restart FastClaude.")]
ClaudeNotOnPath,

#[error("FastClaude doesn't yet support {0} — contributions welcome at https://github.com/<owner>/FastClaude")]
PlatformUnsupported(&'static str),
```

(Match the existing `thiserror` style. If the repo uses a different error pattern, mirror it.)

- [ ] **Step 3: Define `PathLookup` trait**

In `src-tauri/src/spawner/mod.rs`, add at module level:

```rust
use std::path::PathBuf;

/// Resolves an executable name on PATH. Behind a trait so the Windows
/// spawner's PATH preflight can be unit-tested with a fake.
pub trait PathLookup: Send + Sync {
    fn find(&self, exe: &str) -> Option<PathBuf>;
}

pub struct EnvPathLookup;

impl PathLookup for EnvPathLookup {
    fn find(&self, exe: &str) -> Option<PathBuf> {
        which::which(exe).ok()
    }
}
```

- [ ] **Step 4: Add the `which` dependency**

```bash
cd src-tauri && cargo add which
```

- [ ] **Step 5: Compile**

```bash
cd src-tauri && cargo build
```

Expected: compiles; `which` available.

- [ ] **Step 6: Add a basic test**

Append to `src-tauri/src/spawner/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    struct FakeLookup(Option<PathBuf>);
    impl PathLookup for FakeLookup {
        fn find(&self, _exe: &str) -> Option<PathBuf> { self.0.clone() }
    }

    #[test]
    fn path_lookup_returns_some_when_found() {
        let l = FakeLookup(Some(PathBuf::from("C:\\bin\\claude.exe")));
        assert!(l.find("claude").is_some());
    }

    #[test]
    fn path_lookup_returns_none_when_missing() {
        let l = FakeLookup(None);
        assert!(l.find("claude").is_none());
    }
}
```

- [ ] **Step 7: Run tests**

```bash
cd src-tauri && cargo test
```

Expected: all pass (existing 23 + 2 new).

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/error.rs src-tauri/src/spawner/mod.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat(spawner): add PathLookup trait and typed errors for missing claude / unsupported platforms"
```

---

## Task 6: PATH preflight in Windows spawner

**Files:**
- Modify: `src-tauri/src/spawner/windows.rs`

- [ ] **Step 1: Wire the trait into `WindowsSpawner`**

Change the struct + constructor:

```rust
pub struct WindowsSpawner {
    path_lookup: Box<dyn PathLookup>,
}

impl WindowsSpawner {
    pub fn new() -> Self {
        Self { path_lookup: Box::new(EnvPathLookup) }
    }

    #[cfg(test)]
    pub fn with_lookup(lookup: Box<dyn PathLookup>) -> Self {
        Self { path_lookup: lookup }
    }
}

impl Default for WindowsSpawner {
    fn default() -> Self { Self::new() }
}
```

(Update `default_spawner()` to call `WindowsSpawner::new()` if it doesn't already.)

- [ ] **Step 2: Add the preflight at the top of `spawn`**

In `Spawner::spawn`:

```rust
fn spawn(&self, req: &SpawnRequest) -> AppResult<SpawnResult> {
    if self.path_lookup.find("claude").is_none() {
        return Err(AppError::ClaudeNotOnPath);
    }
    // ...existing body...
}
```

- [ ] **Step 3: Add the failing test first**

Append to the tests module:

```rust
#[test]
fn spawn_returns_claude_not_on_path_when_missing() {
    struct Missing;
    impl crate::spawner::PathLookup for Missing {
        fn find(&self, _: &str) -> Option<std::path::PathBuf> { None }
    }
    let spawner = WindowsSpawner::with_lookup(Box::new(Missing));
    let req = SpawnRequest {
        project_dir: "C:\\proj".into(),
        model: "claude-opus-4-7".into(),
        prompt: None,
        terminal_program: "wt".into(),
    };
    let err = spawner.spawn(&req).unwrap_err();
    assert!(matches!(err, AppError::ClaudeNotOnPath));
}
```

- [ ] **Step 4: Run tests**

```bash
cd src-tauri && cargo test spawn_returns_claude_not_on_path
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/spawner/windows.rs
git commit -m "feat(spawner/windows): preflight PATH for claude, return typed error"
```

---

## Task 7: macOS / Linux stubs return `PlatformUnsupported`

**Files:**
- Modify: `src-tauri/src/spawner/macos.rs`
- Modify: `src-tauri/src/spawner/linux.rs`
- Modify: `src-tauri/src/window_focus/macos.rs`
- Modify: `src-tauri/src/window_focus/linux.rs`

- [ ] **Step 1: macOS spawner stub**

Replace `src-tauri/src/spawner/macos.rs` body:

```rust
use crate::error::{AppError, AppResult};
use super::{SpawnRequest, SpawnResult, Spawner};

pub struct MacosSpawner;

impl MacosSpawner {
    pub fn new() -> Self { Self }
}

impl Default for MacosSpawner {
    fn default() -> Self { Self::new() }
}

impl Spawner for MacosSpawner {
    fn spawn(&self, _req: &SpawnRequest) -> AppResult<SpawnResult> {
        Err(AppError::PlatformUnsupported("macOS"))
    }
}
```

- [ ] **Step 2: Linux spawner stub**

Same pattern in `src-tauri/src/spawner/linux.rs` with `"Linux"`.

- [ ] **Step 3: macOS / Linux focus stubs**

In `src-tauri/src/window_focus/macos.rs` and `linux.rs`, the `focus` method returns `Err(AppError::PlatformUnsupported("macOS" | "Linux"))`.

- [ ] **Step 4: Build for the host (Windows) to verify nothing references the stubs**

```bash
cd src-tauri && cargo build
```

Expected: clean.

- [ ] **Step 5: Build with `--target x86_64-unknown-linux-gnu` IF rust target is installed (optional sanity check)**

If not, skip — CI will catch it cross-compile-style.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/spawner/macos.rs src-tauri/src/spawner/linux.rs src-tauri/src/window_focus/macos.rs src-tauri/src/window_focus/linux.rs
git commit -m "feat(platform): macOS and Linux return PlatformUnsupported with helpful message"
```

---

## Task 8: `config::load` returns `(Config, was_created: bool)`

**Files:**
- Modify: `src-tauri/src/config.rs`
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: Update tests first**

In `src-tauri/src/config.rs` `tests` module, replace `load_creates_default_when_missing` and add a sibling:

```rust
#[test]
fn load_signals_first_run_when_creating_default() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.json");
    let (cfg, was_created) = load(&path).unwrap();
    assert_eq!(cfg.default_model, "claude-opus-4-7");
    assert!(was_created);
    assert!(path.exists());
}

#[test]
fn load_signals_not_first_run_when_file_exists() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.json");
    let cfg = Config::default();
    save(&path, &cfg).unwrap();
    let (_loaded, was_created) = load(&path).unwrap();
    assert!(!was_created);
}
```

Update `save_then_load_round_trips` and `load_corrupt_json_returns_error` and `load_ignores_legacy_pricing_field` to destructure `(cfg, _)` from the load call.

- [ ] **Step 2: Run tests, expect FAIL**

```bash
cd src-tauri && cargo test config::
```

Expected: fail (signature mismatch).

- [ ] **Step 3: Update `load`**

```rust
pub fn load(path: &PathBuf) -> AppResult<(Config, bool)> {
    if !path.exists() {
        let cfg = Config::default();
        save(path, &cfg)?;
        return Ok((cfg, true));
    }
    let bytes = std::fs::read(path)?;
    let cfg: Config = serde_json::from_slice(&bytes)?;
    Ok((cfg, false))
}
```

- [ ] **Step 4: Update `main.rs` caller**

```rust
let (cfg, was_created) = config::load(&cfg_path).expect("load config");
```

Stash `was_created` for use in Task 9.

- [ ] **Step 5: Run tests, expect PASS**

```bash
cd src-tauri && cargo test
```

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/config.rs src-tauri/src/main.rs
git commit -m "feat(config): load signals whether default config was just created"
```

---

## Task 9: First-run IPC commands

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `src/types.ts`
- Modify: `src/lib/ipc.ts`

- [ ] **Step 1: Add field to `AppState`**

In `commands.rs`:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
// ...
pub struct AppState {
    pub registry: Arc<Registry>,
    pub spawner: Box<dyn Spawner>,
    pub focus: Box<dyn WindowFocus>,
    pub config: Arc<Mutex<Config>>,
    pub config_path: PathBuf,
    pub is_first_run: AtomicBool,
}
```

- [ ] **Step 2: Add the two commands**

```rust
#[tauri::command]
pub fn get_first_run(state: State<'_, AppState>) -> bool {
    state.is_first_run.load(Ordering::SeqCst)
}

#[tauri::command]
pub fn clear_first_run(state: State<'_, AppState>) -> () {
    state.is_first_run.store(false, Ordering::SeqCst);
}
```

- [ ] **Step 3: Wire into main.rs**

```rust
let state = AppState {
    // ... existing ...
    is_first_run: AtomicBool::new(was_created),
};
// ...
.invoke_handler(tauri::generate_handler![
    // existing commands,
    commands::get_first_run,
    commands::clear_first_run,
])
```

- [ ] **Step 4: Add IPC wrappers**

In `src/lib/ipc.ts`:

```typescript
export async function getFirstRun(): Promise<boolean> {
  return invoke<boolean>("get_first_run");
}

export async function clearFirstRun(): Promise<void> {
  return invoke<void>("clear_first_run");
}
```

- [ ] **Step 5: Build**

```bash
cd /c/GitProjects/FastClaude && npm run build && cd src-tauri && cargo build
```

Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/main.rs src/lib/ipc.ts
git commit -m "feat(ipc): get_first_run and clear_first_run commands"
```

---

## Task 10: `Onboarding.tsx` + `App.tsx` routing

**Files:**
- Create: `src/components/Onboarding.tsx`
- Modify: `src/App.tsx`

- [ ] **Step 1: Write the component**

`src/components/Onboarding.tsx`:

```tsx
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useToast } from "@/hooks/use-toast";
import { getConfig, setConfig, clearFirstRun } from "@/lib/ipc";
import type { AppConfig } from "@/types";

export function Onboarding({ onDone }: { onDone: () => void }) {
  const { toast } = useToast();
  const [draft, setDraft] = useState<AppConfig | null>(null);

  useEffect(() => {
    getConfig().then(setDraft).catch(() => {});
  }, []);

  if (!draft) return <div className="p-8">Loading...</div>;

  async function getStarted() {
    if (!draft) return;
    try {
      await setConfig(draft);
      await clearFirstRun();
      onDone();
    } catch (e: unknown) {
      const msg = typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e);
      toast({ title: "Couldn't save", description: msg, variant: "destructive" });
    }
  }

  function field(label: string, value: string, onChange: (v: string) => void, hint?: string) {
    return (
      <label className="block">
        <div className="text-xs font-medium mb-1">{label}</div>
        <Input value={value} onChange={(e) => onChange(e.target.value)} />
        {hint && <div className="text-xs text-muted-foreground mt-1">{hint}</div>}
      </label>
    );
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-background text-foreground">
      <div className="max-w-md w-full p-6 space-y-4 border border-border rounded-lg">
        <div>
          <div className="text-2xl font-bold">Welcome to FastClaude</div>
          <div className="text-sm text-muted-foreground mt-1">
            Three quick choices and you're set.
          </div>
        </div>
        {field(
          "Terminal program",
          draft.terminal_program,
          (v) => setDraft({ ...draft, terminal_program: v }),
          "'auto' picks Windows Terminal if installed, else cmd.exe"
        )}
        {field(
          "Default model",
          draft.default_model,
          (v) => setDraft({ ...draft, default_model: v }),
          "e.g. claude-opus-4-7, claude-sonnet-4-6, claude-haiku-4-5"
        )}
        {field(
          "Global hotkey",
          draft.hotkey,
          (v) => setDraft({ ...draft, hotkey: v }),
          "Pressed from anywhere to open the launch dialog"
        )}
        <div className="pt-2">
          <Button onClick={getStarted} className="w-full">Get started</Button>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Wire into App.tsx**

Replace `src/App.tsx`:

```tsx
import { useEffect, useState } from "react";
import { Dashboard } from "@/components/Dashboard";
import { Settings } from "@/components/Settings";
import { Onboarding } from "@/components/Onboarding";
import { Toaster } from "@/components/ui/toaster";
import { onHotkeyFired, getFirstRun } from "@/lib/ipc";

type View = "dashboard" | "settings" | "onboarding";

export default function App() {
  const [view, setView] = useState<View | null>(null);
  const [launchOpen, setLaunchOpen] = useState(false);

  useEffect(() => {
    getFirstRun()
      .then((isFirst) => setView(isFirst ? "onboarding" : "dashboard"))
      .catch(() => setView("dashboard"));
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    onHotkeyFired(() => {
      setView("dashboard");
      setLaunchOpen(true);
    }).then((fn) => { unlisten = fn; });
    return () => unlisten?.();
  }, []);

  if (view === null) return null;

  return (
    <div className="min-h-screen flex flex-col bg-background text-foreground">
      <div className="flex-1 flex flex-col">
        {view === "onboarding" ? (
          <Onboarding onDone={() => setView("dashboard")} />
        ) : view === "dashboard" ? (
          <Dashboard
            onOpenSettings={() => setView("settings")}
            launchOpen={launchOpen}
            setLaunchOpen={setLaunchOpen}
          />
        ) : (
          <Settings onBack={() => setView("dashboard")} />
        )}
      </div>
      <Toaster />
    </div>
  );
}
```

- [ ] **Step 3: Build**

```bash
cd /c/GitProjects/FastClaude && npm run build
```

Expected: clean.

- [ ] **Step 4: Manual verification**

1. Wipe `%APPDATA%\com.fastclaude.app\config.json`
2. `npm run tauri dev`
3. Onboarding screen should appear
4. Fill, click "Get started", Dashboard appears
5. Restart app — Dashboard appears directly (no onboarding)

- [ ] **Step 5: Commit**

```bash
git add src/components/Onboarding.tsx src/App.tsx
git commit -m "feat(frontend): first-run onboarding screen"
```

---

## Task 11: Add `tauri-plugin-updater` and signing keypair

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `package.json`

- [ ] **Step 1: Generate the signing keypair**

```bash
cd src-tauri && cargo install tauri-cli --version "^2"  # if not already installed
cargo tauri signer generate -w ~/.tauri/fastclaude.key
```

Output: a public key (printed to stdout) and a private key file at `~/.tauri/fastclaude.key`. Note both. The private key MUST NOT be committed.

- [ ] **Step 2: Store the private key as a GitHub secret**

In the GitHub repo settings → Secrets and variables → Actions:
- `TAURI_SIGNING_PRIVATE_KEY` = contents of `~/.tauri/fastclaude.key`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` = the password you chose during generation (empty string is allowed)

(If the repo doesn't exist yet on GitHub, defer this step until it does. Note the values somewhere safe in the meantime.)

- [ ] **Step 3: Add the Rust plugin**

```bash
cd src-tauri && cargo add tauri-plugin-updater
```

- [ ] **Step 4: Add the JS plugin**

```bash
cd /c/GitProjects/FastClaude && npm install @tauri-apps/plugin-updater
```

- [ ] **Step 5: Configure the updater in `tauri.conf.json`**

Add a `plugins.updater` block:

```json
{
  "plugins": {
    "updater": {
      "active": true,
      "endpoints": [
        "https://github.com/<owner>/FastClaude/releases/latest/download/latest.json"
      ],
      "pubkey": "<paste the public key from step 1>"
    }
  }
}
```

Replace `<owner>` with the actual GitHub owner string and `<pubkey>` with the public key from step 1. **If `<owner>` is not yet known, this entire task blocks until it is — do not proceed with placeholder values.**

- [ ] **Step 6: Initialize the plugin in `main.rs`**

In `tauri::Builder::default()` chain:

```rust
.plugin(tauri_plugin_updater::Builder::new().build())
```

- [ ] **Step 7: Add capability**

In `src-tauri/capabilities/default.json` (or whichever capability file is in use), append `"updater:default"` to the permissions list.

- [ ] **Step 8: Build**

```bash
cd src-tauri && cargo build
```

Expected: clean.

- [ ] **Step 9: Commit (excluding the private key)**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json src-tauri/capabilities/default.json package.json package-lock.json
git commit -m "feat(updater): add tauri-plugin-updater configured for GitHub Releases"
```

---

## Task 12: Backend `check_for_update` and `install_update`

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `src/types.ts`
- Modify: `src/lib/ipc.ts`

- [ ] **Step 1: Add commands**

In `commands.rs`:

```rust
use tauri_plugin_updater::UpdaterExt;

#[derive(serde::Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub notes: Option<String>,
}

#[tauri::command]
pub async fn check_for_update(app: tauri::AppHandle) -> AppResult<Option<UpdateInfo>> {
    match app.updater().map_err(|e| AppError::Other(e.to_string()))?.check().await {
        Ok(Some(update)) => Ok(Some(UpdateInfo {
            version: update.version.clone(),
            notes: update.body.clone(),
        })),
        Ok(None) => Ok(None),
        Err(e) => Err(AppError::Other(e.to_string())),
    }
}

#[tauri::command]
pub async fn install_update(app: tauri::AppHandle) -> AppResult<()> {
    let updater = app.updater().map_err(|e| AppError::Other(e.to_string()))?;
    let update = updater.check().await.map_err(|e| AppError::Other(e.to_string()))?;
    if let Some(update) = update {
        update.download_and_install(|_, _| {}, || {})
            .await
            .map_err(|e| AppError::Other(e.to_string()))?;
        app.restart();
    }
    Ok(())
}
```

- [ ] **Step 2: Register commands**

In `main.rs`, add to `invoke_handler`:

```rust
commands::check_for_update,
commands::install_update,
```

- [ ] **Step 3: Add types**

In `src/types.ts`:

```typescript
export interface UpdateInfo {
  version: string;
  notes: string | null;
}
```

- [ ] **Step 4: Add IPC wrappers**

In `src/lib/ipc.ts`:

```typescript
import type { /* existing */, UpdateInfo } from "@/types";

export async function checkForUpdate(): Promise<UpdateInfo | null> {
  return invoke<UpdateInfo | null>("check_for_update");
}

export async function installUpdate(): Promise<void> {
  return invoke<void>("install_update");
}
```

- [ ] **Step 5: Build**

```bash
cd src-tauri && cargo build && cd .. && npm run build
```

Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/main.rs src/types.ts src/lib/ipc.ts
git commit -m "feat(updater): check_for_update and install_update IPC commands"
```

---

## Task 13: `UpdateBanner.tsx` + Settings "Check for updates" button

**Files:**
- Create: `src/components/UpdateBanner.tsx`
- Modify: `src/App.tsx`
- Modify: `src/components/Settings.tsx`

- [ ] **Step 1: Write the banner**

`src/components/UpdateBanner.tsx`:

```tsx
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { useToast } from "@/hooks/use-toast";
import { checkForUpdate, installUpdate } from "@/lib/ipc";
import type { UpdateInfo } from "@/types";

export function UpdateBanner() {
  const { toast } = useToast();
  const [update, setUpdate] = useState<UpdateInfo | null>(null);

  useEffect(() => {
    const t = setTimeout(() => {
      checkForUpdate().then(setUpdate).catch(() => {});
    }, 5000);
    return () => clearTimeout(t);
  }, []);

  if (!update) return null;

  async function install() {
    try {
      await installUpdate();
    } catch (e: unknown) {
      const msg = typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e);
      toast({ title: "Update failed", description: msg, variant: "destructive" });
    }
  }

  return (
    <div className="flex items-center gap-3 px-4 py-2 bg-blue-50 text-blue-900 border-b border-blue-200 text-sm">
      <div className="flex-1">FastClaude {update.version} is available.</div>
      <Button size="sm" onClick={install}>Restart to install</Button>
    </div>
  );
}
```

- [ ] **Step 2: Mount the banner in App.tsx**

Add `<UpdateBanner />` just below the top-level `<div>`, above the view switcher:

```tsx
import { UpdateBanner } from "@/components/UpdateBanner";
// ...
<div className="min-h-screen flex flex-col bg-background text-foreground">
  <UpdateBanner />
  <div className="flex-1 flex flex-col">
    {/* ...existing... */}
```

- [ ] **Step 3: Add the Settings button**

In `Settings.tsx`, add to the form section:

```tsx
import { checkForUpdate } from "@/lib/ipc";
// ... inside component, add:
async function checkUpdates() {
  try {
    const u = await checkForUpdate();
    if (u) {
      toast({ title: `FastClaude ${u.version} available`, description: "Restart to install — see banner." });
    } else {
      toast({ title: "You're up to date" });
    }
  } catch (e: unknown) {
    const msg = typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e);
    toast({ title: "Update check failed", description: msg, variant: "destructive" });
  }
}

// in the render, near the Save row:
<Button variant="ghost" onClick={checkUpdates}>Check for updates</Button>
```

- [ ] **Step 4: Build**

```bash
cd /c/GitProjects/FastClaude && npm run build
```

- [ ] **Step 5: Commit**

```bash
git add src/components/UpdateBanner.tsx src/App.tsx src/components/Settings.tsx
git commit -m "feat(updater): UpdateBanner and Settings 'Check for updates' button"
```

---

## Task 14: Write the README

**Files:**
- Create / Modify: `README.md`

- [ ] **Step 1: Write the README**

```markdown
# FastClaude

A fast launcher for [Claude Code](https://docs.claude.com/en/docs/claude-code/setup) sessions on Windows. Pop open a project, hit a hotkey, get a Claude session in a labeled terminal tab.

![screenshot](docs/screenshot.png)

## Install (Windows)

1. Download the latest `.msi` from [Releases](https://github.com/<owner>/FastClaude/releases/latest).
2. Run the installer.
3. SmartScreen will warn — click **More info → Run anyway**. (FastClaude is unsigned for v1.0; signing certificate may come later.)

You also need the [`claude` CLI](https://docs.claude.com/en/docs/claude-code/setup) on your PATH. FastClaude will tell you if it isn't.

## First run

The app walks you through three choices: terminal program (default `auto`), default model (e.g. `claude-opus-4-7`), and a global hotkey (default `Ctrl+Shift+C`). You can change all three later in Settings.

## Usage

- Click **+ Launch new session** or press your hotkey.
- Pick a project folder and a model.
- Each session opens in a Windows Terminal tab labeled `FastClaude: <project>`.
- The dashboard shows running/idle status and output token counts, updated every few seconds.
- Click **Focus** to bring the session's terminal forward; **Kill** to end it.

## Auto-updates

The app checks for updates ~5s after launch. When one's available, a banner offers to restart and install.

## Build from source

```bash
git clone https://github.com/<owner>/FastClaude
cd FastClaude
npm install
npm run tauri dev
```

Requires Node 20+, Rust stable, Tauri 2 prerequisites for Windows ([docs](https://tauri.app/v2/guides/prerequisites/)).

## Status

- **Windows:** supported
- **macOS / Linux:** not yet supported. Stub backends will tell you so. PRs welcome.

## License

MIT
```

(Replace `<owner>` with the actual GitHub owner.)

- [ ] **Step 2: Take a screenshot of the dashboard with one running session, save to `docs/screenshot.png`**

If skipped for v1.0, remove the screenshot line.

- [ ] **Step 3: Commit**

```bash
git add README.md docs/screenshot.png
git commit -m "docs: README for v1.0"
```

---

## Task 15: CI workflow

**Files:**
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: Write the workflow**

```yaml
name: CI

on:
  push:
    branches: [develop, main]
  pull_request:

jobs:
  build-and-test:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: npm
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: src-tauri
      - run: npm ci
      - run: npm run build
      - name: cargo test
        working-directory: src-tauri
        run: cargo test
      - name: tauri build
        run: npm run tauri build
```

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: build, test, and bundle on every push/PR"
```

(Will only run once pushed to GitHub. Verify by pushing and checking the Actions tab.)

---

## Task 16: Release workflow

**Files:**
- Create: `.github/workflows/release.yml`

- [ ] **Step 1: Write the workflow**

```yaml
name: Release

on:
  push:
    tags: ['v*']

jobs:
  release:
    runs-on: windows-latest
    permissions:
      contents: write
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: npm
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: src-tauri
      - run: npm ci
      - name: Build signed installer
        env:
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
        run: npm run tauri build
      - name: Generate latest.json
        shell: pwsh
        run: |
          $version = '${{ github.ref_name }}'.TrimStart('v')
          $msi = Get-ChildItem 'src-tauri/target/release/bundle/msi/*.msi' | Select-Object -First 1
          $sig = Get-Content "$($msi.FullName).sig" -Raw
          $url = "https://github.com/${{ github.repository }}/releases/download/${{ github.ref_name }}/$($msi.Name)"
          $json = @{
            version   = $version
            notes     = "See release notes on GitHub."
            pub_date  = (Get-Date -Format "yyyy-MM-ddTHH:mm:ssZ")
            platforms = @{
              "windows-x86_64" = @{
                signature = $sig
                url       = $url
              }
            }
          } | ConvertTo-Json -Depth 4
          $json | Out-File -FilePath latest.json -Encoding utf8
      - name: Create draft release
        uses: softprops/action-gh-release@v2
        with:
          draft: true
          generate_release_notes: true
          files: |
            src-tauri/target/release/bundle/msi/*.msi
            src-tauri/target/release/bundle/msi/*.msi.sig
            latest.json
```

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: release workflow builds signed .msi and uploads to GitHub Release"
```

(Verify by tagging a throwaway `v0.0.1-test` once everything's pushed.)

---

## Task 17: Manual smoke test (release dry-run)

This is a hand checklist run on a Windows machine after Tasks 1–16 are merged and pushed.

- [ ] **Step 1: Fresh install**

1. Wipe `%APPDATA%\com.fastclaude.app\` 
2. `npm run tauri dev`
3. Onboarding appears, completes, Dashboard shows.

- [ ] **Step 2: Labeled wt tab**

Launch a session in a project folder. Bring wt to foreground. The tab title shows `FastClaude: <folder name>`.

- [ ] **Step 3: PATH error toast**

Temporarily make `claude` unavailable on PATH:

```powershell
$env:PATH = ($env:PATH -split ';' | Where-Object { -not (Test-Path "$_\claude.exe") }) -join ';'
```

(Or rename the binary. Restart FastClaude so it inherits the modified PATH.)

Click Launch → toast appears: "claude CLI not found on PATH. Install Claude Code from https://...".

- [ ] **Step 4: Release dry-run**

1. `git tag v0.0.1-test && git push --tags`
2. Watch `.github/workflows/release.yml` build and upload `.msi` + `latest.json` to a draft release.
3. Open the draft release, verify the `.msi` and `.msi.sig` and `latest.json` are attached.

- [ ] **Step 5: Install + update cycle**

1. Publish the v0.0.1-test draft release.
2. Install the .msi on a clean Windows VM (or wipe `%APPDATA%\com.fastclaude.app\` and `Program Files\FastClaude\`). SmartScreen warns once → click "Run anyway". App runs.
3. Bump `tauri.conf.json` `version` to `0.0.2`, commit, tag `v0.0.2-test`, push.
4. Wait for release workflow to publish the draft, then publish it.
5. Restart the installed v0.0.1-test app — banner appears within ~5s saying "FastClaude 0.0.2 is available". Click Restart to install. App relaunches as 0.0.2.

- [ ] **Step 6: Icon shows everywhere**

Verify FastClaude icon appears in: installer header, taskbar, Settings page header (if shown), .msi file properties.

- [ ] **Step 7: Document any failures**

Open issues for any failures. If any are blocking, fix and re-run from Step 1.

---

## What's NOT in this plan

- macOS / Linux runtime — stubs return PlatformUnsupported (Task 7); real implementations are post-v1.0.
- UI Automation tab switching — labeled tabs (Task 4) cover the user-pain piece; UIA can come later.
- Code-signing certificate — SmartScreen warning is documented in README. Acquiring a cert and wiring it through `signtool` in release.yml is post-v1.0.
- Telemetry, crash reporting, analytics.
- Marketing site, demo gif, Discord.

---

## Spec coverage check

- [x] App icon — Task 2
- [x] LICENSE + README — Tasks 1, 14
- [x] Labeled wt tabs (option B) — Tasks 3, 4
- [x] PATH preflight + typed error — Tasks 5, 6
- [x] First-run onboarding — Tasks 8, 9, 10
- [x] Auto-updater (plugin + commands + UI) — Tasks 11, 12, 13
- [x] CI workflow — Task 15
- [x] Release workflow — Task 16
- [x] macOS / Linux deferral with user-visible message — Task 7
- [x] Manual smoke (release dry-run) — Task 17

## Open decisions surfaced during implementation

- **Repository owner** (Task 11 step 5, Task 14): the GitHub owner string for the updater endpoint and the README. Implementer must ask the maintainer before completing Task 11.
- **App icon source** (Task 2 step 1): user-provided artwork or placeholder. Implementer must ask before generating.
