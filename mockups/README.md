# Mockups

Clickable UX prototypes for trying out and iterating on interaction concepts. **Not product code** –
everything runs in the browser (WebGL2), image processing is simulated. The agreed result is written
down in [docs/08-ux-concept.md](../docs/08-ux-concept.md); **ux-v3 is the reference**.

## Running

The mockups load downscaled previews from `testdata-local/_mockup/` (not in Git, private). Without
these files a synthetic grid panorama is shown.

```bash
python3 -m http.server 8765 --bind 127.0.0.1
```

Then open <http://localhost:8765/mockups/ux-v3/> (current) or `…/ux-v2/`, `…/ux-v1/`.

To create previews from your own panoramas (macOS, from the repo root), per image `a`, `b`, `c`:

```bash
mkdir -p testdata-local/_mockup && P="/System/Library/ColorSync/Profiles/sRGB Profile.icc" && sips -m "$P" -s format png -Z 8192 path/to/pano.tif --out testdata-local/_mockup/a-8k.png
```

(likewise `-Z 2048` → `a-2k.png` and `-Z 480 -s format jpeg` → `a-thumb.jpg`; see the image list at the top
of the script in `ux-v3/index.html`.)

Review notes from the maintainer are in `ux-v*/review/Review-notes.md` (screenshots stay local).

## ux-v1 – Main screen

| Area | Implementation |
| --- | --- |
| Layout | Tools left (collapsible), navigator + layers right, filmstrip bottom (collapsible), tool options as a floating bar at the top |
| Sphere view | Real WebGL rendering; no tiling/LOD yet (8K preview) |
| Brush | The mask is painted **on the sphere** (size in °); the cursor ring shows the true spherical brush, also at the nadir |
| Generative Fill | ⏎ fills the mask – **simulated** (push-pull from known pixels), no AI model |
| Spot healing (J) | Paints directly into a "Spot Healing" layer, same simulation |
| Clone stamp (S) | Alt-click = source; the source is a sphere rotation (works across nadir/seam) |
| Shadow lighten (O) | Lifts only pixels darker than their surroundings |
| Blur (R) | Blur for privacy |
| Nadir patch (N) | Logo / disc / "blurred floor"; radius, rotation (drag, ⇧ = 15° steps) |
| Holes | Transparent areas as a checkerboard; "Fill" banner in the layer panel |
| Layers | Every edit is a layer: visibility, opacity, delete; hover highlights the region in the image and navigator |
| Navigator | Mini-map with field-of-view outline, click/drag to jump; zenith/horizon/nadir; sphere/planet/flat |
| Marking menu | Hold right mouse button, direction, release (quick click = stays open) |
| Command palette | ⌘K |
| Zen mode | Tab hides all panels |
| Before/after | hold `\` |
| Export | ⌘E – presets with pixels + MP, hint about unfilled areas for JPEG |
| Auto-save | status in the file chip ("Saving…" → "Saved") |

Deliberately simplified (all versions): no real tiling/pyramid, no full resolution (8K preview,
sRGB-converted); retouch results are GPU approximations; layer order is not rendered exactly (fixed order
by type); max. 3 visible clone layers, one nadir patch layer; UI in English only.

## ux-v2 – Revision after review 1

Review notes on v1: [ux-v1/review/Review-notes.md](ux-v1/review/Review-notes.md) (German).

| Area | Change |
| --- | --- |
| General | **Domain colours** (Maya principle): blue = view, pink = mask, amber = retouch, green = nadir. Duotone icons everywhere. Shortcuts as **keycaps**, platform-aware (⌘/⌥ on macOS, Ctrl/Alt elsewhere). Tooltips with shortcuts. `?` shows all shortcuts. Default tool is **navigate**. |
| Header | App menu (logo): file, edit, view, help. Undo/redo and before/after as buttons, export button. Metadata as short chips, details in the tooltip. |
| Tool options | Fixed height. Tool chip with icon in domain colour that flashes on change. **Scrub fields** like Figma/Blender: drag label, `⇧` ×10, `⌥` fine, double-click = default, arrow keys. Too many options → `⋯` popover. |
| Navigator | Views as icons. **Saved views** (bookmark menu, keys 4–9). **Yaw/pitch/FOV editable**. |
| Layers | Delete via hover trash, `⌫` or context menu. **Groups** (⌘G, rename by double-click, collapse). **Drag & drop**, duplicate, rename. **History** tab with jump to any step. |
| Tools | Mask brush: `⌥` = subtract (dashed cursor), `⇧`-click = straight line (great circle). Clone: source with the first click, crosshair and step hint. Blur: Gaussian / pixelate. Shadow: colour matching to the surroundings. Nadir patch: upload own logo, choose colour. |
| Marking menu | No overlap (labels aligned left/right). Domain colours. **Option boxes** □ like Maya. **Horizon** in the circle. **List below** for further actions and saved views. |
| Filmstrip | Uniform size, file name only (single line). "Edited" marker only on edited images. Metadata as tooltip. |
| Zen mode | Tool options stay visible. Persistent hint on how to exit. |
| Navigation | Momentum smoothed and capped. Rotation speed matches the angle per pixel exactly. Before/after on `Y`. |

## ux-v3 – Structural revision (final round, reference)

Review notes on v2: [ux-v2/review/Review-notes.md](ux-v2/review/Review-notes.md) (German); on v3:
[ux-v3/review/Review-notes.md](ux-v3/review/Review-notes.md).

| Area | Change |
| --- | --- |
| Header | Fixed width (352 px), file name truncated. No "Saved" text or export button; save status is a dot on the logo, export lives in the menu. The logo has a border and chevron so it reads as clickable. |
| Collapsing | **One pattern for all panels** (tools, right panels, filmstrip): the panel slides out and a handle stays at the edge. Clicking the handle toggles it (hover reveal removed after review). |
| Tool options | **Fixed size, two rows.** Top: tool chip, max. 3 controls, actions right, `⋯` for the rest. Bottom: hints and shortcuts. The bar no longer changes size between tools. |
| Navigator | View icons, map and yaw/pitch/FOV across the full width. Projection moved to the views menu (rarely needed). |
| Layers / history | Panel uses the full height. Hint texts removed. **Opacity inline in the row**: drag the percentage, double-click = 100 %. Selected layer with a coloured marker. |
| AI status | Generative Fill appears immediately as a layer with spinner, progress and cancel (`Esc`). The result fades in with a toast. Error state not simulated yet (proposal: row turns red with "Retry"). |
| Filmstrip | Badge with layers icon and number of edits instead of "edited". |
| Readability | Brighter secondary text. Toolbar tooltips with a short description and shortcut. |
| Dialogs | Close button (×) at the top right. More spacing in the shortcut overview. |
| Navigation | Arrow keys rotate the view (`⇧` = larger steps, scaled with FOV). |
