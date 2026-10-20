# 08 – UX Concept

Status: **Agreed** (outcome of mockup rounds v1–v3, 2026-10-21).
Reference prototype: [`mockups/ux-v3/`](../mockups/README.md). The prototype shows behaviour and
structure. It is **not a code template**: the real UI is rebuilt in Svelte. Visual polish (detailed
typography, final icons, the character of individual dialogs) follows in the real app.

This document defines **patterns and rules**. New UI elements must follow them or propose a change here.

## 1. Principles

1. **The sphere view is the stage.** It fills the whole window. Everything else floats above it as
   collapsible panels.
2. **Instant response.** Every input gives visible feedback within one frame. Long-running work happens
   in the background with a visible status (section 7).
3. **Little visible UI, everything reachable.** Rarely used things live in menus. Everything is also
   reachable via marking menu, command palette and shortcuts.
4. **Consistency over originality.** Familiar patterns from Photoshop (shortcuts, layers), Figma
   (scrub fields, command palette) and Maya (marking menu, domain colours) are adopted.
5. **Mouse first, keyboard equal.** Pen and trackpad gestures come later.

## 2. Layout

```
┌──────────────┐   ┌────────────── Tool options (fixed) ─────────────┐   ┌─────────────┐
│ Header (fixed│   │ Chip │ ≤3 controls        │ Actions │ ⋯         │   │ Navigator   │
└──────────────┘   │ Hints & shortcuts (2nd row)                     │   ├─────────────┤
┌──┐               └─────────────────────────────────────────────────┘   │ Layers |    │
│T │                                                                     │ History     │
│o │                         Sphere view (full window)                  ◂│ (full       │
│o │▸                                                                    │  height)    │
│l │                                                                     │             │
└──┘                       ┌────────── Filmstrip ─────────┐              │             │
                           └──────────────▴───────────────┘              └─────────────┘
```

| Area | Rules |
| --- | --- |
| **Header** (top left) | Fixed width (~350 px). Contents: app menu (logo with chevron and the save-status dot), file name (truncated with "…"), metadata chips (MP, bit depth, colour space; details in the tooltip), undo, redo, before/after. No export button (export lives in the menu and on `⌘E`). |
| **Tool options** (top centre) | Fixed size (~660 × 74 px), see section 4.1. |
| **Toolbar** (left) | Vertical, grouped by domain with dividers. |
| **Right panels** | ~304 px wide, **full height**. Navigator on top, layers/history as tabs below. |
| **Filmstrip** (bottom centre) | Fixed-size thumbnails. |
| **Collapsing** | One pattern for toolbar, right panels and filmstrip: the panel slides out of view and a **handle** stays at the edge. **One click** on the handle toggles it. No hover reveal (rejected in review because the handle jumps away). |
| **Zen mode** (`Tab`) | Hides header, toolbar, panels and filmstrip. Tool options stay visible. A persistent hint shows `Tab` to exit, right mouse for tools and `⌘K`. |
| **Small windows** (< ~1380 px) | Tool options move below the header. Panels can be collapsed. The image area always has priority. |

## 3. Visual language

- **Dark mode only.** Panels are translucent with a blurred backdrop, 12 px corner radius, subtle borders.
- **Colours as tokens** (proposal, polish later): background `#0e0f11`, panel `#18191d` at 88 % opacity,
  text `#e9e9ec`, secondary text `#a2a4ad`, tertiary text `#71737c`. Secondary text must reach at least
  WCAG AA (4.5 : 1) on panels.
- **Domain colours** (after Maya). Every tool and action belongs to exactly one domain:

  | Domain | Colour | Examples |
  | --- | --- | --- |
  | View | blue `#59c8ff` | navigate, zenith/horizon/nadir, projections, saved views |
  | Mask/selection | pink `#ff5c8a` | mask brush, eraser, lasso, cap |
  | Retouch | amber `#f5b83d` | Generative Fill, spot healing, clone, shadow, blur |
  | Nadir/output | green `#5fd38d` | nadir patch, fill holes, "saved" status |

  Rules: the colour only appears as an **accent** – in the duotone icon, the tool chip, the marker of the
  selected layer and the hover outline in the marking menu. Never as the fill of large elements.
  **At most 4–5 domains.**
- **Icons:** duotone. Strokes in text colour, one accent part in the domain colour. 1.7 px stroke at 24 px.
- **Shortcuts** are always shown as **keycaps**, platform-aware (`⌘ ⌥ ⇧` on macOS, `Ctrl Alt Shift`
  on Windows/Linux).
- **Primary buttons** are light (white), secondary ones transparent. Destructive actions are red/pink and
  only appear in menus.

## 4. Components

### 4.1 Tool options

- **Fixed size, two rows.** Row 1: tool chip (icon + name in the domain colour, flashes briefly on
  change), **at most 3 controls**, actions on the right (e.g. "Apply Fill"), `⋯` at the far right for
  further settings. Row 2: hints and shortcuts for the active tool.
- Anything that doesn't fit goes into the `⋯` popover. The bar never grows.
- Multi-step tools (e.g. clone stamp: first pick a source, then paint) show the current step in row 2.

### 4.2 Scrub fields (numeric values)

Dragging the label changes the value (`⇧` ×10, `⌥` fine). Click into the field to type, `↑`/`↓` to step,
`Enter` commits, `Esc` cancels, **double-click on the label resets to the default**. A thin bar shows the
value relative to its range. Units are shown next to the value (°, %).

### 4.3 Marking menu

- **Hold the right mouse button**, move in a direction, release. A quick click without movement keeps the
  menu open; selection is then done by clicking.
- **8 fixed slots** (muscle memory – the assignment does not change with context):

  | Direction | Action | | Direction | Action |
  | --- | --- | --- | --- | --- |
  | ↑ | Zenith | | ↓ | Nadir |
  | ↗ | Spot healing □ | | ↙ | Nadir patch □ |
  | → | Clone stamp □ | | ← | Mask brush □ |
  | ↘ | Apply fill | | ↖ | Horizon |

- **Option box □** (like Maya): aiming at the box next to a tool selects the tool **and** opens its settings.
- **List below the circle** for further actions: navigate, other tools, before/after, projection, saved
  views with their keys.

### 4.4 Command palette (`⌘K`)

Searches all commands, tools, views and open images. Each entry shows its icon (in domain colour) and
shortcut. Every action of the app must be findable here.

### 4.5 Tooltips, menus, dialogs, toasts

- **Tooltips** appear after approx. 450 ms, also on keyboard focus. Tools show name, key and a one-sentence
  description. Metadata (image, filmstrip) appears as a detailed card.
- **Menus/context menus:** icon, label, shortcut. Destructive entries come last and are red.
- **Dialogs:** close × at the top right, `Esc` closes, `Enter` triggers the primary action.
- **Toasts:** short, bottom centre. Destructive actions show "`⌘Z` to undo".

## 5. Panels in detail

### Navigator

- Header: zenith / horizon / nadir as icons, menu "saved views & projection".
- Mini-map: image with the outline of the current field of view and the view direction. Edited areas are
  tinted; hovering a layer highlights its area. Click or drag to jump.
- Footer: yaw / pitch / FOV as scrub fields across the full width, directly editable.
- Saved views on keys `4`–`9` (e.g. "Street View start 90° / −5°").

### Layers

- **Row:** type icon, name (double-click = rename), subtitle, **opacity as a draggable percentage**
  (double-click = 100 %), trash (on hover), eye.
- **Selected layer:** brighter background and a coloured bar on the left in the domain colour.
- **Hover** highlights the layer's area in the sphere view and the navigator.
- **Drag & drop** to reorder. The order applies **strictly like Photoshop**: top covers bottom.
- **Groups:** `⌘G`, rename, expand/collapse, show/hide.
- **Context menu:** rename, duplicate, group, delete.
- **`⌫`:** clears the mask if one is painted, otherwise deletes the selected layer.
- The original is always at the bottom and locked.
- **No explanatory empty-state text.** Hints only when actionable (e.g. "transparent areas at the nadir"
  with a "Fill" button).

### History

List of all steps with icons. A click jumps to that step. Later steps stay available (greyed out) until a
new edit discards them.

### Filmstrip

Fixed-size thumbnails with a single-line file name. Edited images get a badge with a layers icon and the
number of edits. Metadata appears in the tooltip.

### Header & app menu

The app menu contains: file (open folder, export), edit (undo, redo, history), view (zen mode,
before/after, toggle panels), help (command palette, shortcuts), settings (language, cache, …). The
**save-status dot** on the logo shows: green = saved, pulsing amber = saving, red = error (with tooltip and toast).

## 6. Viewport interaction

| Input | Effect |
| --- | --- |
| Left-drag | use the tool; with the navigate tool: rotate the view |
| Space + drag / middle mouse | rotate the view (in any tool) |
| Mouse wheel | zoom (FOV) |
| `⌥` + wheel, `[` `]` | brush size or patch radius |
| Arrow keys | rotate the view in steps (`⇧` = larger, scaled with FOV) |
| Hold right mouse button | marking menu |
| `⇧` on release near the horizon | snap to the horizon |

- **Default tool:** navigate.
- **Momentum after release:** smoothed and capped to avoid jumps. Rotation speed matches the angle per
  pixel exactly (the image "sticks" to the mouse).
- **Brush cursor:** drawn on the sphere and shows the true brush shape, also at the nadir. Subtract (`⌥`)
  is shown dashed. With the clone stamp the source is visible as a crosshair.
- **Navigation is the most important UX topic.** The real app gets a dedicated test protocol (mouse,
  trackpad, FOV range, 300 MP images) with sign-off by the maintainer.

## 7. States

| Situation | Presentation |
| --- | --- |
| **Image loading** | Instantly a coarse preview, then sharpening tiles. A subtle status hint top left, never a blocking loader. |
| **AI job running** | The layer appears immediately with a spinner, "Generating… 45 %", a progress bar and cancel (× / `Esc`). The UI stays usable. |
| **AI job done** | The result fades in (~350 ms) with a short toast. The history entry is created only now. |
| **AI job cancelled** | The layer disappears, the mask is restored. |
| **AI job failed** | The layer row turns red with a short reason and "Retry". The mask is kept. *(not simulated in the mockup yet)* |
| **AI model missing** | Hint right at the action: "Better results with AI model – download 200 MB?". Until then the classic fallback works. |
| **Saving failed** | Save-status dot red, toast with the reason. Edits are not lost (retry in the background). |
| **Export** | Runs in the background with progress. At the end a toast with "Show in Finder". |

## 8. Shortcuts (default mapping)

| Area | Shortcuts |
| --- | --- |
| Tools | `V` navigate · `B` mask · `E` eraser · `J` spot healing · `S` clone · `O` shadow · `R` blur · `N` nadir patch |
| View | `1` nadir · `2` horizon · `3` zenith · `4`–`9` saved views · `L` little planet · `F` flat · `Tab` zen · `Y` (hold) before/after |
| Edit | `Enter` apply fill · `⌫` clear mask / delete layer · `⌥` subtract · `⇧`-click line · `⌘G` group · `⌘Z` / `⇧⌘Z` |
| App | `⌘K` commands · `⌘E` export · `⌘O` open folder · `?` shortcut overview · `Esc` cancel/close |

Custom key mapping comes later (not for 0.1).

## 9. Accessibility & language

- Everything is keyboard-reachable with a visible focus ring. Tooltips also appear on focus.
- The system's "reduce motion" setting is respected (shorten or disable animations).
- UI language English (default), German in settings. All strings go through i18n keys.

## 10. Deliberately open (for the implementation phase)

- Visual polish: type scale, final icon set, character of export and settings dialogs.
- Fine-tuning navigation (sensitivity, momentum, trackpad gestures) using the test protocol.
- Customisable key mapping and marking menu assignment.
- Behaviour with very many layers (search/filter in the layer list).
