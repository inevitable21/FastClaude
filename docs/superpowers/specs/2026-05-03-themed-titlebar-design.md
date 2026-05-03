# Themed Title Bar — Design

**Date:** 2026-05-03
**Status:** Design approved, awaiting implementation plan

## Goal

Replace the default OS title bar (where minimize, maximize, and close live) with a themed bar that matches the FastClaude warm-aurora palette and absorbs the existing Dashboard top row, so there is exactly one bar across the top of the window on every view.

## Non-goals

- Cross-platform support (macOS, Linux). The themed bar is Windows-only for now; macOS will need platform-aware control layout (left-side traffic lights) later.
- Windows 11 snap-layouts hover popup on the maximize button. Lost in exchange for a uniform, simpler implementation. `Win+arrow` keyboard snaps still work, as does drag-to-edge snap.
- Remembering window size/position across launches.
- Custom window shadow / border-radius beyond what Tauri provides.

## Decisions made during brainstorming

1. **Merge into one bar** rather than stacking a slim title-strip above the existing Dashboard header. One row, every view.
2. **Themed coral buttons** for the window controls (rounded-md, 30×30, matching the existing History/Settings icon buttons), not native-faithful Win11 rectangles.
3. **Adaptive title** — the bar reads "FastClaude" on Dashboard and Onboarding, "Settings" on Settings, "History" on History. The brand is the same logo image; only the text label changes.
4. **No snap-layouts popup.** The maximize button is a simple toggle. This keeps the implementation uniform across platforms and avoids a Windows-only Rust path.

## Architecture

Hide the OS chrome by setting `decorations: false` in `tauri.conf.json`, then render a single global `<TitleBar />` React component at the top of `App.tsx`, above all view content. The Dashboard's existing header row (logo + Launch + History + Settings) is removed; those buttons move into a `rightActions` slot on the title bar. Settings and History likewise drop their current back-button headers; the back arrow lives in the same `rightActions` slot for those views.

```
┌───────────────────────────────────────────────────────────────────┐
│ [logo] <adaptive title>  ───drag region───  [rightActions] [─][☐][✕] │  ← TitleBar (44px)
├───────────────────────────────────────────────────────────────────┤
│ [UpdateBanner — when not onboarding]                              │
├───────────────────────────────────────────────────────────────────┤
│ <view content>                                                    │
└───────────────────────────────────────────────────────────────────┘
```

## Component: `TitleBar`

Path: `src/components/TitleBar.tsx`. Approx. 120 lines.

**Props**

```ts
type TitleBarProps = {
  view: "dashboard" | "settings" | "history" | "onboarding";
  rightActions?: ReactNode;
};
```

**Structure (left → right)**

| Region | Width | Notes |
|---|---|---|
| Logo | 22px | `/icon.png` with the same `shadow-[0_0_12px_rgba(217,119,87,.4)]` as today |
| Title text | auto | `font-semibold tracking-tight`. Adapts: `FastClaude` (dashboard, onboarding), `Settings`, `History` |
| Drag region | flex-1 | `<div data-tauri-drag-region>`; double-click toggles maximize, drag-to-top maximizes, drag-to-side half-snaps (Windows native). The logo and title text also carry `data-tauri-drag-region` so the bar stays draggable when the flex spacer compresses to zero at small widths. |
| `rightActions` slot | auto | Dashboard: Launch / History / Settings cluster. Settings/History: single back-arrow icon button. Onboarding: nothing. |
| Window controls | 3 × 30px + gaps | Themed coral icon buttons |

**Window-control styling**

- Each button: 30×30, `rounded-md`, `border border-border`, `bg-foreground/[0.04]`, `text-foreground`, focus ring matches existing icon-button pattern.
- Hover (min, max): `bg-[rgba(217,119,87,0.18)]`, `border-color: var(--border-strong)`.
- Hover (close): warm-red wash — `bg-[rgba(180,90,60,0.30)]`, `border-color: rgba(248,113,113,.5)`, `color: #FCA5A5` on dark; on light theme, the wash uses `var(--destructive)` at ~25% alpha.
- Maximize icon swaps between a single square (window not maximized) and two overlapping squares (window maximized).

**Behavior**

- Minimize → `getCurrentWindow().minimize()`
- Maximize → `getCurrentWindow().toggleMaximize()`
- Close → `getCurrentWindow().close()`
- `isMaximized` state is local component state, initialized via `getCurrentWindow().isMaximized()` and refreshed on `tauri://resize` (via `getCurrentWindow().onResized(...)`). Listener is cleaned up on unmount.
- Bar is `sticky top-0 z-30` so it stays above the AuroraBackground (z-0) and below the Toaster (z-50) and dialogs.
- `app.windows[0].title` in `tauri.conf.json` stays `"FastClaude"` so the OS taskbar label stays correct even though the in-app title changes per view.

## Tauri configuration

**`src-tauri/tauri.conf.json` — `app.windows[0]`**

```jsonc
{
  "title": "FastClaude",
  "width": 800,
  "height": 600,
  "decorations": false,
  "resizable": true,
  "minWidth": 520,
  "minHeight": 360,
  "shadow": true,
  "transparent": false
}
```

`decorations: false` removes both the OS title bar and the standard resize border on Windows. Tauri 2 still allows edge resize when `resizable: true` via an invisible 4px hit-region around the window edge — no extra implementation needed.

**`src-tauri/capabilities/default.json`** — add the window-control permissions:

```
core:window:allow-minimize
core:window:allow-toggle-maximize
core:window:allow-close
core:window:allow-start-dragging
core:window:allow-is-maximized
```

`allow-start-dragging` is what makes `data-tauri-drag-region` work; without it the drag attribute is silently ignored.

**No new Rust code.** All window control happens through built-in Tauri commands; `commands.rs`, `main.rs`, etc. are untouched by this change.

## App integration

**`src/App.tsx`**

```tsx
return (
  <>
    <AuroraBackground />
    <div className="min-h-screen flex flex-col text-foreground relative z-10">
      <TitleBar
        view={view}
        rightActions={
          view === "dashboard" ? (
            <DashboardActions
              onOpenSettings={() => setView("settings")}
              onOpenHistory={() => setView("history")}
              onLaunch={() => setLaunchOpen(true)}
            />
          ) : view === "settings" || view === "history" ? (
            <BackButton onClick={() => setView("dashboard")} />
          ) : null
        }
      />
      {view !== "onboarding" && <UpdateBanner />}
      <div className="flex-1 flex flex-col">{/* views */}</div>
      <Toaster />
    </div>
  </>
);
```

`DashboardActions` is a small extracted component holding what is currently the Dashboard top-row button cluster. `BackButton` is a single icon button styled like the existing History/Settings icons but with a left-arrow.

**`src/components/Dashboard.tsx`** — delete the entire current header `<div>` (currently lines 52–83). The Launch dialog `setLaunchOpen` callback is now triggered from `DashboardActions` via the prop chain. Component starts straight at the content area `<div className="p-4 min-h-[60vh]">`.

**`src/components/Settings.tsx` / `src/components/History.tsx`** — remove existing back-button rows. Page content renders directly under the global title bar.

**`src/components/Onboarding.tsx`** — unchanged in body. TitleBar above it shows logo + "FastClaude" + drag + window controls; window controls remain visible during onboarding (the user can always close the window).

## Edge cases

| Case | Behavior |
|---|---|
| Double-click drag region | Toggles maximize (Tauri built-in) |
| Drag drag-region to top of screen | Maximizes (Windows native) |
| Drag drag-region to a side | Half-snaps (Windows native) |
| Window already maximized at startup | `isMaximized()` checked on mount; restore icon shown |
| User maximizes via `Win+Up` | `tauri://resize` fires; icon updates |
| Window resized below `minWidth` | Tauri clamps at 520px. The drag region (flex-1) is the first thing to shrink as the bar gets narrower; if the bar runs out of space, it shrinks to zero before any button compresses, which is acceptable since drag still works on the title text and logo |
| Light/dark theme switch | All colors are CSS variables; bar updates automatically |
| UpdateBanner visible | Renders below TitleBar, above content (preserves current ordering) |
| Toast or dialog opens | TitleBar at z-30, Toaster at z-50, dialogs higher. Modals dim the bar; window controls remain reachable |

## Acceptance checklist (manual test plan)

This is window chrome — no automated tests. Acceptance is manual on Windows 11:

1. Window opens borderless with themed bar visible across the top.
2. Dragging the empty middle of the bar moves the window.
3. Double-clicking the empty middle maximizes the window; double-click again restores. Maximize icon swaps both times.
4. Minimize, maximize, and close buttons each perform the correct action.
5. Window can be resized by dragging any edge.
6. `Win+Up`, `Win+Down`, `Win+Left`, `Win+Right` keyboard snaps fire `resize` events; the maximize icon updates.
7. Light/dark theme toggle shows correct bar colors, including the close-hover wash.
8. On Settings, the bar reads "Settings", shows a back arrow that returns to Dashboard, and Settings content has no duplicate header.
9. On History, same as Settings but with title "History".
10. On Onboarding, the bar reads "FastClaude", has no action slot, and the onboarding flow still works.
11. The bar stays at the top while view content scrolls underneath.
12. The OS taskbar label still reads "FastClaude" regardless of which view is open.
