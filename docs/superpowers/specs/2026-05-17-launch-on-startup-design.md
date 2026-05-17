# Launch on startup — design

**Date:** 2026-05-17
**Status:** Approved
**Branch:** develop

## Goal

Let the user opt FastClaude into starting automatically when Windows boots, with a choice of how the window appears (normal, minimized, hidden).

## Non-goals

- macOS / Linux parity work beyond what the cross-platform autostart plugin gives us for free. Windows is the verification target.
- A system tray icon. (Out of scope; "Hidden" mode relies on the global hotkey.)
- Auto-launching after install. The toggle is off by default and the user enables it explicitly.

## User-facing surface

A new **"Startup"** section in Settings, placed between "Hotkey" and "Theme":

| Control | Values | Default | Notes |
|---|---|---|---|
| `Launch on login` | toggle | off | Master switch. |
| `Launch as` | select: `Window` / `Minimized` / `Hidden (hotkey only)` | `Window` | Disabled when the toggle is off. |

Helper text under the select: *"'Hidden' relies on your global hotkey — set one in the Hotkey section first."*

If the user saves with `Hidden` mode and no hotkey configured, save still succeeds but a warning toast appears: *"Hidden mode set, but no global hotkey is configured. You'll only be able to reach the window via Task Manager."*

The section follows the existing draft-and-save pattern — changes only take effect when the user clicks **Save**.

## Architecture

### Dependencies

- Rust: `tauri-plugin-autostart = "2"` added to `src-tauri/Cargo.toml`.
- Capability: `autostart:default` added to `src-tauri/capabilities/default.json`.
- No JS binding (`@tauri-apps/plugin-autostart`) is needed — all enable/disable calls happen in the Rust IPC handler. Frontend only reads/writes the two config fields.

### Config schema

Two new fields on `AppConfig`:

```rust
launch_on_login: bool,           // default: false
launch_mode: LaunchMode,         // default: Window
```

```rust
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum LaunchMode { Window, Minimized, Hidden }
```

Persisted via the existing config write path. Field order in the file is appended after current fields so older configs deserialize cleanly with `serde` defaults.

### Save flow

```
Settings.tsx
  └─ setConfig(IPC)
       └─ Rust set_config(cfg)
            ├─ reconcile_autostart(&cfg)
            │    ├─ if cfg.launch_on_login: AutoLaunchManager.enable()
            │    └─ else: AutoLaunchManager.disable()
            └─ config::save(...)   // only if reconcile succeeded
```

Args registered with the autostart plugin are static (set once at `init()` time) — `enable()` takes no per-call args. So the autostart registry entry always carries just `--launched-by-autostart`. The chosen `launch_mode` is read from config at boot, **not** encoded into the CLI flag. This is simpler and avoids needing to re-register on every mode change.

If `enable()` or `disable()` errors, `write_config` returns the error, the IPC call fails, the toast surfaces the message, and the in-memory draft is preserved — config on disk is not mutated. This avoids the "config says on, registry says off" lying state.

`enable()` when already enabled and `disable()` when already disabled are treated as success (idempotent).

### Boot flow

In `lib.rs` `.setup()`:

1. Parse `std::env::args()` via `parse_launch_args()` → `LaunchArgs { from_autostart: bool }`. The mode itself comes from the loaded config (`cfg.launch_mode`).
2. **No reconciliation against the registry.** Boot is read-only. This respects user edits made via Task Manager's Startup tab.
3. Apply `cfg.launch_mode` to the main window:
   - `Window` → no-op.
   - `Minimized` → `window.minimize()` immediately after window creation.
   - `Hidden` → see "Hidden mode mechanism" below.
4. Path-drift check: if `launch_on_login` is true in config and `AutoLaunchManager.is_enabled()` returns false, log a single warning line to stderr (likely cause: reinstall to a different path invalidated the registry entry). Do not auto-repair. The plugin doesn't expose the registered path/args directly, so this is the strongest check available without reading the registry by hand.

### Hidden mode mechanism

The autostart plugin can't toggle window visibility per-launch. Workaround:

- Flip `tauri.conf.json` window default to `"visible": false`.
- In `.setup()`:
  - If `from_autostart && mode == Hidden`: leave the window hidden.
  - Otherwise (manual launch, or autostart in Window/Minimized mode): call `window.show()` immediately (then `minimize()` if Minimized).

The CLI flag is the sole signal that distinguishes autostart launches from manual ones. It's not a security boundary — if someone runs the exe with the flag, that's fine.

### Dev mode

`reconcile_autostart` is guarded by `#[cfg(not(debug_assertions))]`. In `tauri dev` it's a no-op so developer machines don't get registry pollution. The Settings toggle still appears and the config field still persists, but the registry isn't touched.

## Error handling & edge cases

| Situation | Behavior |
|---|---|
| `enable()` / `disable()` fails | IPC returns error → toast with the error message → config not saved → toggle reverts in UI. |
| Plugin reports already-enabled / already-disabled | Treated as success. |
| `Hidden` saved with no hotkey configured | Save succeeds; warning toast. |
| Stray `--launched-by-autostart` flag on manual run | Honored. Internal flag, not a trust boundary. |
| Unrecognized `launch_mode` in config JSON (corruption) | `serde` default puts it back to `Window` on next read. |
| Registry path stale (reinstall in different dir) | Stderr warning at boot; next Save rewrites it. |
| Dev mode (`debug_assertions`) | `reconcile_autostart` is a no-op. |

## Testing

- **Unit:** Rust test for `parse_launch_args` covering: no `--launched-by-autostart` flag → `from_autostart: false`; flag present → `from_autostart: true`; unrelated args ignored. Plus a config round-trip test that the new fields serde-default cleanly when missing from on-disk JSON.
- **Manual:** registry side is an OS integration; mocking adds no value. Manual checklist:
  1. Toggle on, mode=Window, save, reboot → window appears.
  2. Toggle on, mode=Minimized, save, reboot → window minimized in taskbar.
  3. Toggle on, mode=Hidden, save, reboot → no window; global hotkey opens it.
  4. Toggle off, save, reboot → no FastClaude after boot.
  5. Each save inspected with `reg query HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.

## Files touched

- `src-tauri/Cargo.toml` — add `tauri-plugin-autostart`.
- `src-tauri/capabilities/default.json` — add `autostart:default`.
- `src-tauri/src/lib.rs` — register plugin, parse launch args, apply window mode in `.setup()`.
- `src-tauri/src/config.rs` (or wherever `AppConfig` lives) — add `launch_on_login`, `launch_mode`, plus `reconcile_autostart` helper.
- `src-tauri/tauri.conf.json` — set window `visible: false`.
- `src/types.ts` — add the two fields to the `AppConfig` shape.
- `src/components/Settings.tsx` — add the "Startup" section.
