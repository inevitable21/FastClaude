# FastClaude — Futuristic UI Redesign

**Status:** Draft for review
**Date:** 2026-05-01
**Scope:** Visual redesign of the FastClaude desktop app. No backend, IPC, or session-lifecycle changes.

## Goal

Replace the current default shadcn-style UI with a distinctive "Warm Aurora" look — a dark, frosted-glass interface lit by drifting Claude-orange aurora gradients. Dark theme is the headliner; a clean light theme exists as a fallback for bright environments.

## Decisions (locked)

| | |
|---|---|
| Visual direction | Glass / Aurora |
| Palette | Claude warm-coral family (#D97757 / #C46141 / #F4B58A) |
| Intensity | Warm Aurora — orange as soft glow, dark canvas dominant |
| Theme support | Dark by default, basic light theme fallback |
| Scope | All surfaces — Dashboard, Launch dialog, Onboarding, Settings, History, Update banner, Toaster |
| Typography | Geist (sans) + Geist Mono — mono used for paths, model names, command preview |
| Motion | Subtle — aurora drift (32s loop), running-dot pulse (2.4s), row fade-in stagger on first mount |
| Iconography | Selective — toolbar buttons icon-only (lucide-react), action buttons keep text labels |
| Implementation strategy | Token + targeted component pass (no new shared primitives) |

## Tokens

### Dark theme

| Variable | Value | Usage |
|---|---|---|
| `--background` | `#0D0807` | App background (under aurora) |
| `--card` | `#1A0F0C` | Solid panel reference (rarely used directly; most panels are glass) |
| `--border` | `rgba(217,119,87,.18)` | Default border |
| `--border-strong` | `rgba(217,119,87,.32)` | Hover / dialog border |
| `--foreground` | `#FAEFE6` | Body text |
| `--muted-foreground` | `#C9B8AB` | Secondary text |
| `--primary` | `#C46141` | Solid token used by `bg-primary`, `text-primary`, focus styles (gradient is layered separately — see below) |
| `--primary-foreground` | `#1A0F0C` | Text on primary buttons |
| `--primary-from` | `#E8825E` | Top of Launch button gradient |
| `--primary-to` | `#C46141` | Bottom of Launch button gradient |
| `--accent` | `#F4B58A` | Badge text, focus ring, brand mark glow |
| `--aurora-1` | `rgba(217,119,87,.40)` | Top-left aurora glow |
| `--aurora-2` | `rgba(180,90,60,.32)` | Bottom-right aurora glow |
| `--aurora-3` | `rgba(244,181,138,.18)` | Right-mid aurora highlight |
| `--ring` | `#F4B58A` | Focus ring |
| `--destructive` | `#F87171` | Kill / error |

Status colors (not theme-mapped — same in light and dark):
- `--status-running`: `#88F0C0`
- `--status-idle`: `#FBBF24`
- `--status-stopped`: `#6B7280`

### Light theme

No aurora layer; no `backdrop-filter`. Solid panels.

| Variable | Value |
|---|---|
| `--background` | `#FDF8F4` |
| `--card` | `#FFFFFF` |
| `--border` | `#F0E2D6` |
| `--foreground` | `#1A0F0C` |
| `--muted-foreground` | `#8A766A` |
| `--primary` | `#C46141` |
| `--accent` | `#D97757` |

### Why HEX (not HSL)

The current `index.css` uses shadcn-style HSL channel triplets. We rewrite as HEX because the warm-coral hues drift in HSL and round to imprecise values. Where opacity modifiers (`bg-primary/50`) are needed, the HEX values still work — Tailwind v3 + modern browsers compute alpha via `color-mix`/relative-color syntax under the hood. Tokens that already encode alpha (`--border`, `--aurora-*`) use `rgba()` directly and aren't intended for the `/N` modifier.

### Primary button gradient

`--primary` stays a single solid color so all `bg-primary`/`text-primary` Tailwind classes keep working. The Launch button's gradient is layered separately via the `primary` variant in `src/components/ui/button.tsx`:

```css
background: linear-gradient(180deg, var(--primary-from), var(--primary-to));
box-shadow: 0 8px 24px rgba(217,119,87,.32), inset 0 1px 0 rgba(255,255,255,.18);
```

Light theme overrides set `--primary-from` and `--primary-to` to the same value (`#C46141`) so the button is solid in light theme.

## Aurora background

Architecture:

- New component `<AuroraBackground />` mounted once at the top of `App.tsx`, before any view.
- A single `position: fixed; inset: 0; z-index: -1; pointer-events: none` div with three layered radial gradients (the three `--aurora-*` colors).
- Drift animation: 32-second `ease-in-out alternate infinite` loop animating the gradient anchor points via custom-property keyframes (`@property` declared for Firefox; falls back gracefully where `@property` is unsupported).
- Hidden in light theme via `:not(.dark) .aurora-layer { display: none; }`.
- `@media (prefers-reduced-motion: reduce)` disables the drift, leaving the gradient static.

Performance: a single composited layer; idle CPU/GPU cost is negligible. No JS animation loop.

## Motion

Three behaviors. Each respects `prefers-reduced-motion`.

1. **Aurora drift** — 32s `ease-in-out alternate infinite`, animates only the gradient anchor positions on the aurora layer.
2. **Running-dot pulse** — 2.4s `ease-out infinite` on `box-shadow`, applied only to `.dot.running`. Idle and stopped dots are static.
3. **Row mount stagger** — when `SessionRow` enters the DOM, `animation: rowIn .42s cubic-bezier(.2,.7,.2,1) both`, with a 70ms `animation-delay` per index. Fires only on first mount; the periodic 5-second `listSessions()` refresh in `Dashboard.tsx` does not re-key existing rows, so they don't re-animate.

No hover transforms. No button-press scale. The effect is calm.

## Typography

- Sans: **Geist** (400, 500, 600, 700)
- Mono: **Geist Mono** (400, 500)
- Loaded from Google Fonts via `<link>` in `index.html` (with `preconnect`).
- Mono is applied to: project paths (`.pdir`), model name badges, command preview block, input fields inside `LaunchDialog`, kbd chips.
- Sans is applied everywhere else (titles, labels, summary lines, button text).
- Tailwind `fontFamily.sans` and `fontFamily.mono` updated to point at Geist with system fallbacks.

## Icons

`lucide-react` is already in `package.json`. We import a small set:

| Icon | Where |
|---|---|
| `Plus` | "+ Launch new session" button |
| `History` | Dashboard topbar (icon-only) |
| `Settings` | Dashboard topbar (icon-only) |
| `X` | Update banner dismiss, dialog close |
| `Check` | Success toast |
| `AlertCircle` | Destructive toast / error message |
| `ArrowLeft` | Settings / History back button |

All other action buttons keep text labels.

## Component specs

### Dashboard top bar

- Translucent black-on-aurora — `bg-background/55 backdrop-blur-xl saturate-140`.
- Brand: gradient-mark + "FastClaude". The 22×22px `<div>` brand mark uses `linear-gradient(135deg,#E8825E,#C46141)` with a soft coral glow shadow. Replaces the raster `public/icon.png` reference; the file remains as the app icon for the OS, but the in-app chrome no longer reads it.
- Action group: `+ Launch new session` (primary, with leading `+` icon), `History` (icon-only, tooltip), `Settings` (icon-only, tooltip).

### SessionRow

- Frosted panel: `bg-foreground/[0.04]` + `border` (default border token) + `backdrop-blur-md`.
- Status dot: 8×8 circle, `running` pulses, `idle` is solid amber, `stopped` is solid grey. Pulse uses box-shadow ripple, not size change.
- Right side: `tokens` (mono), `elapsed` (mono), model badge, **Focus** (ghost), **Kill** (coral-outlined danger button).
- Hover: border lifts to `--border-strong`; no transform.

### EmptyState

- Centered stack:
  - Circular tint icon (Plus from lucide, accent color, 56px container with accent-tinted bg + border).
  - "No running sessions" — h3.
  - Hotkey hint with hotkey rendered as a `<kbd>` chip (mono, panel bg, hairline border).
  - Primary Launch button.

### LaunchDialog

Critical constraint: **keyboard handling logic is byte-identical to current**. Recent commits (565a660, 0921fda, 8071cb2, 86be486) chained fixes for arrow-key and Enter handling. We touch only className strings.

- `DialogContent` becomes a deep glass panel: `bg-card/85 backdrop-blur-2xl saturate-140 border border-[--border-strong] shadow-2xl rounded-xl`.
- `DialogOverlay` becomes `bg-black/50 backdrop-blur-sm`.
- Inputs use mono font, `bg-black/30`, accent focus ring.
- Recents list highlight (current code uses `bg-primary text-primary-foreground` for `recentIndex === i`) changes to a coral gradient bar with a 2px accent left border. The `bg-primary` class is removed from this code path; replaced with `bg-gradient-to-r from-[rgba(217,119,87,.25)] to-[rgba(217,119,87,.10)] border-l-2 border-accent`.
- Command preview block keeps mono font, gets a darker translucent bg with a subtle coral-tinted border.

### UpdateBanner

- Slim bar above the topbar: `bg-gradient-to-r from-[rgba(217,119,87,.20)] to-[rgba(217,119,87,.08)] border-b border-[--border-strong]`.
- Glowing accent dot, message, "Restart & install" ghost button, "Dismiss" icon-only button.

### Onboarding

- Same topbar shell but without action buttons (only brand mark + title).
- Three steps stacked vertically as glass cards:
  - Terminal program (`Select`)
  - Default model (`Select`)
  - Hotkey (capture input)
- 3-dot stepper at top of card area; accent fills the active dot, hairline borders on inactive.
- Primary "Continue" / "Finish" button bottom-right of each step.

### Settings

- Topbar: brand + back arrow (lucide `ArrowLeft`) + title.
- Body: labeled glass-card sections, in this order:
  1. Terminal program
  2. Default model + effort + permission mode + extra args
  3. Hotkey
  4. **Theme** (new) — single toggle button "Dark" / "Light"; persists to `localStorage.fastclaude-theme`; applies/removes `dark` class on `<html>`.
  5. Updates — current "Check for updates" button + last-checked timestamp.
  6. About — version, repo link.

### History

- Same shell as Dashboard.
- List rows use the same panel as `SessionRow` but with: stopped dot, end timestamp instead of elapsed, "Re-launch" ghost button replacing Focus + Kill.

### Toaster

- Bottom-right glass toasts: `bg-card/92 backdrop-blur-xl border border-[--border-strong] shadow-2xl rounded-xl`.
- Status icon (lucide `Check` / `AlertCircle`) tinted accent or destructive.
- Plumbing unchanged — only `toast.tsx` base classes are edited.

## Theme switching

- `index.html` has `class="dark"` on `<html>` so first paint is dark.
- `src/main.tsx` runs a small synchronous block before `createRoot`:
  ```ts
  const stored = localStorage.getItem("fastclaude-theme");
  if (stored === "light") document.documentElement.classList.remove("dark");
  ```
  No flash because this runs before React renders.
- Settings toggle button writes the new value and toggles the class. No re-mount.

## File-level change list

**New (1)**
- `src/components/AuroraBackground.tsx`

**Reskins, no logic changes (most)**
- `src/index.css` — token rewrite, aurora variables, reduced-motion block
- `tailwind.config.js` — Geist registered as `fontFamily.sans/mono`; `pulse` and `drift` keyframes
- `index.html` — Geist `<link>`, `class="dark"` on `<html>`
- `src/main.tsx` — synchronous theme bootstrap
- `src/components/ui/button.tsx` — variant classes
- `src/components/ui/dialog.tsx` — content / overlay base classes
- `src/components/ui/input.tsx` — base classes
- `src/components/ui/select.tsx` — trigger + content base classes
- `src/components/ui/toast.tsx` — toast base classes
- `src/components/Dashboard.tsx` — topbar, brand mark swap, icon buttons
- `src/components/SessionRow.tsx` — panel, dot states, kill button style
- `src/components/EmptyState.tsx` — circular icon, kbd chip
- `src/components/LaunchDialog.tsx` — visual restyle only; keyboard logic untouched
- `src/components/UpdateBanner.tsx` — gradient bar
- `src/components/Onboarding.tsx` — glass cards, stepper
- `src/components/Settings.tsx` — glass sections + new Theme section
- `src/components/History.tsx` — match Dashboard shell, Re-launch button
- `src/App.tsx` — mount `<AuroraBackground />`

**Untouched**
- `src/lib/ipc.ts`, `src/types.ts`, `src/lib/models.ts`, `src/lib/launch-options.ts`, `src/hooks/use-toast.ts`
- All Tauri / Rust backend
- The `public/icon.png` file (still used as the app icon by the OS; only its reference inside `Dashboard.tsx` is removed)

**Deps:** none added. `lucide-react` already present.

## Risks & mitigations

- **`LaunchDialog` keyboard regression.** Recent history shows this code is fragile. Mitigation: in the redesign pass, edit only className strings inside the JSX. Do not touch `useEffect` bodies, `stateRef`, `recentRefs`, `inputRef`, the document-level keydown handler, or any prop on the Radix `<DialogContent>` other than `className` and the existing `onOpenAutoFocus`. After the pass, manually exercise: arrow up/down through recents, Enter on highlighted recent, Esc to clear highlight, Esc again to close, fast-typed `↓Enter`.
- **Theme flash on first paint.** Mitigated by synchronous `main.tsx` bootstrap before React render.
- **`backdrop-filter` on Tauri's WebView2.** Supported in Edge/Chromium baseline; Tauri 2 ships modern WebView2. No fallback needed.
- **Reduced motion.** Handled centrally via `@media (prefers-reduced-motion: reduce)` in `index.css`.
- **Light theme readability.** Light theme drops aurora and glass entirely; all panels become solid white on cream so the warm-coral primary still has high contrast.

## Out of scope

- IPC / Tauri command surface
- Session lifecycle, terminal launching, model selection logic
- The Rust backend
- Hotkey capture mechanics in Settings (visual only — capture flow untouched)
- The OS-level `public/icon.png` raster (kept as-is)
- Any new components beyond `AuroraBackground`

## Verification checklist (for the implementation phase)

- Dark theme: aurora drifts, dots pulse, rows fade in on first mount.
- Light theme: no aurora, no glass, panels solid, primary still readable on white.
- Theme toggle in Settings persists across reloads with no flash.
- `LaunchDialog`: arrow keys, Enter, Esc, fast `↓Enter`, recent click — all behave identically to before the redesign.
- Update banner appears, "Restart & install" wired to existing IPC, dismiss hides it.
- Toasts on session launched + on focus/kill error both render correctly.
- Onboarding three steps complete and write the same config keys as before.
- `prefers-reduced-motion: reduce`: aurora is static, dots don't pulse, rows don't stagger.
