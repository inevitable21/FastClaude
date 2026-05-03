# Default Prompt — Design

**Date:** 2026-05-03
**Status:** Design approved, awaiting implementation plan

## Goal

Let the user save a default prompt in Settings that pre-fills the LaunchDialog's "Starting prompt" field on every launch. The prompt is the first user message claude processes on startup; the field stays editable per-launch.

## Non-goals

- Auto-launch on hotkey (i.e., skipping the dialog entirely). Considered as option B during brainstorming, deferred.
- Multiple prompt presets / a prompt picker. Single default only.
- Per-project default prompts. The default is global.
- Server-side prompt templating, variable interpolation, or any kind of templating syntax. The string is passed verbatim.

## Decisions made during brainstorming

1. **Pre-fill, not bypass.** The default prompt is just a value that fills the existing LaunchDialog field. Submit behavior is unchanged from today.
2. **Multi-line textarea** in both Settings and LaunchDialog. A single-line input would crop usefully-long prompts ("You are a senior reviewer. First, summarize the diff. Then..."). The cost is one tiny new shadcn-style `Textarea` primitive.

## Architecture

Add `default_prompt: String` to `AppConfig` (Rust + TS). Surface it in Settings under the existing "Defaults" `<Section>` as a `Textarea`, alongside the existing `default_effort` / `default_permission_mode` / `default_extra_args` fields. The LaunchDialog already seeds its other launch fields from config in a `useEffect` keyed on `[open]` — extend that block by one line so it also seeds `prompt` from `cfg.default_prompt`. Replace the `<Input>` in the LaunchDialog's "Starting prompt" field with the new `<Textarea>`.

No backend command changes. `build_claude_command` already accepts `Option<&str>` for the prompt and `shell_escape::escape`s it correctly, including embedded newlines.

```
┌─────────────────┐    setConfig    ┌──────────────┐
│ Settings UI     │ ──────────────▶ │ Rust config  │  ← persists default_prompt
│ <Textarea>      │                 │ JSON on disk │
└─────────────────┘                 └──────┬───────┘
                                           │ getConfig
                                           ▼
                                    ┌──────────────┐
                                    │ LaunchDialog │  ← seeds prompt state on open
                                    │ <Textarea>   │
                                    └──────┬───────┘
                                           │ launchSession({prompt})
                                           ▼
                                    ┌──────────────┐
                                    │ Rust spawner │  → claude --model X ... '<prompt>'
                                    └──────────────┘
```

## File structure

| File | Action | Responsibility |
|---|---|---|
| `src-tauri/src/config.rs` | Modify | Add `default_prompt: String` field with `#[serde(default)]`. Update `Default` impl. |
| `src/types.ts` | Modify | Add `default_prompt: string` to `AppConfig`. |
| `src/components/ui/textarea.tsx` | Create | New shadcn-style `Textarea` primitive (~15 lines), styled identically to `Input`. |
| `src/components/Settings.tsx` | Modify | Import `Textarea`, render a "Default prompt" `<Field>` in the Defaults `<Section>`. |
| `src/components/LaunchDialog.tsx` | Modify | Import `Textarea`. Seed `prompt` state from `cfg.default_prompt` in the existing config-loading `useEffect`. Replace the `<Input>` for prompt with `<Textarea>`. |

No changes to: `src-tauri/src/commands.rs`, `src-tauri/src/spawner/`, `src/lib/ipc.ts`, `src/lib/launch-options.ts`.

## Components

### `Textarea` primitive (`src/components/ui/textarea.tsx`)

```tsx
import * as React from "react";
import { cn } from "@/lib/utils";

export interface TextareaProps
  extends React.TextareaHTMLAttributes<HTMLTextAreaElement> {}

const Textarea = React.forwardRef<HTMLTextAreaElement, TextareaProps>(
  ({ className, ...props }, ref) => (
    <textarea
      ref={ref}
      className={cn(
        "input-fill flex w-full rounded-md border border-border px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 ring-offset-background disabled:cursor-not-allowed disabled:opacity-50 resize-y min-h-[88px]",
        className,
      )}
      {...props}
    />
  ),
);
Textarea.displayName = "Textarea";

export { Textarea };
```

- `min-h-[88px]` ≈ 4 rows visible at default font size.
- `resize-y` allows vertical grow only; horizontal resize would break the form layout.
- Reuses the same `input-fill` utility and border/focus tokens as `Input`, so it matches both light and dark themes automatically.

### Settings field

In the existing "Defaults" `<Section>`, add as the last `<Field>` (after the existing "Default extra args" field):

```tsx
<Field label="Default prompt (sent to claude on launch)">
  <Textarea
    value={draft.default_prompt}
    onChange={(e) => setDraft({ ...draft, default_prompt: e.target.value })}
    placeholder="e.g. Review the latest changes and run all tests"
  />
</Field>
```

### LaunchDialog change

Two edits in `src/components/LaunchDialog.tsx`:

1. In the existing `useEffect` keyed on `[open]` (currently around lines 82–96) that calls `getConfig()` and seeds defaults, add one line:

   ```tsx
   .then((c) => {
     setCfg(c);
     setModel(c.default_model);
     setEffort(c.default_effort);
     setPermissionMode(c.default_permission_mode);
     setExtraArgs(c.default_extra_args);
     setPrompt(c.default_prompt);   // new
   })
   ```

2. Replace the "Starting prompt" `<Input>` block (currently around lines 314–322) with a `<Textarea>`:

   ```tsx
   <div>
     <label className="text-[10px] uppercase tracking-[0.10em] text-muted-foreground">Starting prompt (optional)</label>
     <Textarea
       value={prompt}
       onChange={(e) => setPrompt(e.target.value)}
       placeholder="Implement X..."
       className="font-sans"
     />
   </div>
   ```

3. The `setPrompt("")` reset after a successful launch (currently around line 143) **stays as `""`**. After launch we clear the field; the next time the dialog opens, the `useEffect` re-fills it from config. This way the user gets a clean reset between launches but always sees their default on a fresh open.

## Backend config

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    // ...existing fields unchanged...
    #[serde(default)]
    pub default_extra_args: String,
    #[serde(default)]
    pub default_prompt: String,
}
```

`#[serde(default)]` means a `config.json` written by an older build (without the field) deserializes cleanly to `default_prompt: ""`. No migration step is needed.

The existing `impl Default for Config` block (config.rs lines 25–37) gains `default_prompt: String::new(),` alongside the other empty-string defaults.

## Frontend type

In `src/types.ts`:

```ts
export interface AppConfig {
  terminal_program: string;
  default_model: string;
  hotkey: string;
  idle_threshold_seconds: number;
  default_effort: string;
  default_permission_mode: string;
  default_extra_args: string;
  default_prompt: string;   // new
}
```

`LaunchInput` is unchanged — it already has `prompt?: string`.

## Edge cases

| Case | Behavior |
|---|---|
| Default prompt empty string | Textarea renders empty; placeholder shows. Behaves like today's empty extra-args. |
| User clears the textarea before launching | The LaunchDialog submit path currently sends `prompt: prompt \|\| undefined`, so an empty string becomes `undefined` on the wire. The backend `LaunchInput.prompt` is `None`, and `build_claude_command` simply doesn't append a prompt arg. |
| Multi-line default prompt with newlines | `shell_escape::escape` quotes the string with embedded newlines. The chosen terminal program (`cmd`, PowerShell, Windows Terminal, the Mac/Linux equivalents) receives a quoted positional arg whose `argv[]` entry contains literal newlines, which `claude` reads as part of the first user message. |
| Live-preview command in dialog | The "Will run: ..." preview shows the shell-escaped version. Multi-line prompts will look slightly busy in the preview but render correctly. |
| Upgrade from a `config.json` that has no `default_prompt` | `#[serde(default)]` → field deserializes to `""`. No JSON parse error. User sees an empty default until they fill one in. |
| LaunchDialog auto-opens on app startup | Prompt textarea pre-fills with the default. If the user wants a launch without the default, they manually clear the textarea before clicking Launch. Same UX as `default_extra_args` today. |

## Acceptance checklist (manual)

This is UI + config — no automated tests (no test runner is configured). Manual acceptance on Windows:

1. Open Settings → Defaults section now has a "Default prompt" textarea, ~4 rows visible, vertically resizable.
2. Type a default, click Save, close & reopen Settings → value persists.
3. Open LaunchDialog → "Starting prompt (optional)" is now a textarea pre-filled with the default value.
4. Edit the prompt in the dialog and launch → only that launch is affected; Settings still shows the original default.
5. Clear the prompt textarea in the dialog and launch → `claude` starts with no initial message (verify the live "Will run:" preview shows no positional prompt arg).
6. Set a multi-line default ("Line one.\nLine two.") and launch → `claude` receives both lines (verify by reading the first user message in the claude UI / JSONL).
7. Backup the config file, manually delete the `default_prompt` key, restart the app → app starts cleanly, default prompt field shows empty.
8. Light/dark theme: textareas in both Settings and LaunchDialog use the same fill/border/focus tokens as `<Input>` and look correct on both themes.
