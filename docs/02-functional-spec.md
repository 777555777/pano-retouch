# 02 – Functional Specification

Derives the functional requirements from the [vision](01-vision.md): use cases, features, domain
model and terminology.

> Detailed scope of tools, selection and layers per version: [07-tools-and-layers.md](07-tools-and-layers.md).
> Interaction patterns and UI rules: [08-ux-concept.md](08-ux-concept.md).

## 1. Core workflows (use cases)

### UC-1: Remove the tripod from a panorama
1. User opens a panorama (or a folder).
2. The 360° viewer shows the image; user looks down (or uses "Go to nadir").
3. User paints over the tripod with the mask brush (or chooses "auto-mask nadir" with a radius in degrees).
4. User triggers **AI inpainting** → result appears after a few seconds.
5. Small errors are fixed with **clone stamp / healing brush** directly in the sphere view.
6. Export → new file with preserved metadata.

### UC-2: Nadir patch (logo) for a whole tour
1. User opens a folder with 40 panoramas.
2. Creates a **patch template**: own image (logo/disc/ground texture), size in degrees, edge feather, rotation.
3. Applies the template to all (or selected) images.
4. Checks every image in **review mode** (filmstrip + nadir view), adjusts individual ones (e.g. rotation).
5. Batch export.

### UC-3: Remove the tripod in a series (batch retouching)
1. User retouches the first image (mask + inpainting).
2. Saves the mask/operation as a **preset** (the mask is in spherical coordinates, hence transferable).
3. Applies the preset to the series → inpainting runs as a background job.
4. Review mode: flag bad results and fix them manually.

### UC-4: Remove a distracting object / person
1. User navigates to the object in the viewer (anywhere on the sphere, also across the 0°/360° seam).
2. Paint mask → inpainting or clone stamp.

### UC-5: Ground photo as nadir fill *(later – advanced feature, not for 1.0)*
1. User has shot a separate photo of the ground (without tripod).
2. Imports it, aligns it at the nadir by dragging/rotating/scaling (possibly control points).
3. Soft blending at the edges.

### UC-6: Retouch an HDRI for 3D software
1. User opens an HDR panorama (EXR, 32-bit float, e.g. 16K×8K).
2. Viewer shows it with adjustable exposure/tone mapping (display only, data stays linear).
3. Remove tripod and photographer (inpainting/clone) – bright areas must not be clipped, light sources
   outside the mask stay bit-exact.
4. Export as EXR (selectable compression), full dynamic range preserved.

### UC-7: Take over an area from a second shot
1. User shot two panoramas at the same location (e.g. tripod moved, or once without the photographer).
2. Adds the second pano as an **image layer**; aligns it by rotation if needed.
3. Paints a mask to reveal the areas that should come from the second shot.

### UC-8: Small corrections *(secondary)*
- Level the horizon / set the heading (also writes GPano `PoseHeadingDegrees` or similar).
- Slightly adjust exposure/white balance globally.

## 2. Feature catalogue

MoSCoW prioritisation: **M**ust (MVP / 1.0), **S**hould (1.0 if possible), **C**ould (later), **W**on't (deliberately not).

### 2.1 Files & project

| ID | Feature | Prio |
| --- | --- | --- |
| F-01 | Open a single image (drag & drop, dialog) | M |
| F-02 | Open a folder as project, filmstrip/grid with thumbnails | M |
| F-03 | Read/write formats: JPEG 8-bit, TIFF 8/16-bit | M |
| F-04 | Read/write formats: EXR (16/32-bit float), Radiance HDR | M |
| F-05 | Preserve EXIF, XMP, GPano, GPS, ICC profile on export | M |
| F-06 | Detect "is equirectangular 2:1", warn for other images | M |
| F-07 | Non-destructive storage of edits (sidecar) | M |
| F-08 | Export: format, quality, target folder (also external/NAS), file name pattern | M |
| F-09 | Detect panoramas with holes (missing zenith on drones, deliberately skipped nadir) and treat them as areas to fill | S |
| F-10 | Very large files: BigTIFF (> 4 GB), images up to approx. 24K×12K (300 MP) | M |
| F-11 | Default export without settings: same format as source, sub-folder `export/`, suffix `_retouched` (configurable later) | M |
| F-12 | Simple image status in the filmstrip: unedited / edited / exported / flagged, filterable | M |
| F-13 | Export presets showing size in pixels **and megapixels**: full resolution, Street View (current practice 13000×6500, sRGB JPEG; API limit 75 MB according to the maintainer – verify during implementation), 16K, 24K, 32K (only if ≤ source resolution, otherwise with a hint) | M |
| F-14 | Colour management via embedded ICC profiles (at least sRGB, Adobe RGB, Display P3, ProPhoto); correct display on the monitor profile; export keeps the source profile, web/Street View presets convert to sRGB | M |
| F-15 | Read, display and preserve the alpha channel (holes from PTGui) on export (TIFF/EXR). For JPEG: hint "image contains unfilled areas" before export, fill colour black by default (configurable) | M |

### 2.2 View

| ID | Feature | Prio |
| --- | --- | --- |
| V-01 | Interactive 360° viewer (rectilinear), zoom/FOV, smooth at 300 MP | M |
| V-02 | Flat equirectangular view (overview) | M |
| V-03 | Quick jumps: nadir, zenith, horizon | M |
| V-04 | Before/after comparison | S |
| V-05 | Overlays: mask display, grid/horizon line | S |
| V-06 | Little planet/fisheye view for checking the nadir | C |
| V-07 | HDR display: exposure slider/tone mapping in the viewer (display only; real HDR display output not required) | M |
| V-08 | Mini-map (flat overview with view direction/extent, click to navigate) | M |
| V-09 | Progressive loading without long pixelated phases: visible tiles prioritised, neighbours prefetched | M |

### 2.3 Retouching (core)

| ID | Feature | Prio |
| --- | --- | --- |
| R-01 | Mask brush directly in the viewer (size in degrees, hardness, eraser) | M |
| R-02 | Nadir/zenith circular mask by parameters (radius, centre, softness) | M |
| R-03 | **AI inpainting** of the mask (local model) | M |
| R-04 | **Nadir/zenith patch** from an image (logo, disc, texture; PNG with transparency), size/rotation/feather | M |
| R-05 | **Clone stamp** on the sphere (source = rotation relative to the target) | M |
| R-06 | **Healing brush** (structure from source, colour/brightness matched to the surroundings) | M |
| R-07 | Manual pixelate/blur (faces, licence plates) and colour fill | M |
| R-08 | Fit a ground photo as nadir fill (UC-5) – happens regularly in practice (offset ground shot) | C (later) |
| R-09 | Automatic tripod/rig detection (suggest a mask) – especially useful for batch since rig rotation varies per image | C |
| R-10 | Generate and choose between inpainting variants | C |
| R-11 | **Spot healing brush** (no source selection) – for sensor dust, lens flares, small things | M |
| R-12 | **Patch tool** (select an area, drag onto a source area – like Photoshop's patch tool) | S |
| R-13 | SVG logos as patch assets (vector-based, sharp at any size) | S |
| R-14 | Text as a layer (e.g. copyright) | C (later) |
| R-15 | **Shadow lighten:** match brightness/colour of a shadow area to its surroundings while keeping the structure (tripod/photographer shadows) | S |
| R-16 | Sky replacement (gradient or template), mainly for drone zeniths | C (later) |

### 2.4 Layers & history

| ID | Feature | Prio |
| --- | --- | --- |
| H-01 | Undo/redo across all operations | M |
| H-02 | **Retouch layers:** every operation is a layer with mask, opacity, visibility, order and editable parameters | M |
| H-03 | **Image layer from another shot** of the same location (UC-7), aligned by rotation and mask | S |
| H-04 | Edit layer masks afterwards (paint, erase, invert, feather) | M |
| H-05 | Blend modes for layers (normal is enough at first) | C |
| H-06 | Automatic saving of layers (no `Cmd+S` needed); undo history is per session | M |
| H-07 | Keep the undo history across sessions | C |
| H-08 | History panel: all steps of the session, jump to any step | M |
| H-09 | Layer groups (folders): rename, show/hide, expand/collapse; drag & drop ordering with strictly applied order | M |
| H-10 | Adjustment layers (exposure, white balance, curves) locally via mask or globally | S (1.x) |

### 2.5 Batch

| ID | Feature | Prio |
| --- | --- | --- |
| B-01 | Multi-selection in the filmstrip | M |
| B-02 | Presets: save operation(s) and apply to a selection | M |
| B-03 | Background job queue with progress, cancel, error list | M |
| B-04 | Review mode: quickly step through results (focused on the affected region), flag, fix | S |
| B-05 | Copy edits from image A to image B | S |
| B-06 | Batch export | M |
| B-07 | Command line interface (headless batch) | C |
| B-08 | MCP server so AI agents can drive the tool (builds on core/CLI) | C |
| B-09 | Adjust a preset per image during review: rotate around the vertical axis (yaw), change radius | S |

**Note on batch:** rig orientation varies from image to image. This doesn't matter for circular nadir
masks and nadir patches – they are rotationally symmetric around the vertical axis and fit any levelled
panorama. Asymmetric masks (rig arm, shadow) must be rotated/adjusted per image (B-09) or detected
automatically later (R-09).

### 2.6 Interaction

| ID | Feature | Prio |
| --- | --- | --- |
| U-01 | Photoshop-like keyboard shortcuts | M |
| U-02 | Marking menu: hold right mouse button, release in a direction = action (like Maya); option boxes for tool settings | S |
| U-03 | Snapping: nadir/zenith/horizon, angle grid for rotation, patch centre at the pole | S |
| U-04 | Clean, contextual UI with fixed layout; panels collapsible (not freely dockable) | M |
| U-05 | UI languages: English (default), German via settings | M |
| U-06 | Pen/tablet support (pressure) | C (later) |
| U-07 | Navigation: left-drag = tool (navigate tool: rotate view), space + drag or middle mouse = rotate view, wheel = zoom, modifier + wheel = brush size, arrow keys = step | M |
| U-08 | Command palette (`Cmd+K`) with all commands, tools and views | M |
| U-09 | Saved views (yaw/pitch/FOV) with keys `4`–`9` | S |

### 2.7 Secondary features

| ID | Feature | Prio |
| --- | --- | --- |
| S-01 | Level horizon (pitch/roll), set heading | S |
| S-02 | Global corrections: exposure, white balance, contrast, saturation (see H-10) | C |
| S-03 | Export little planet / preview image | C |
| S-04 | Hand-off to Pano Publisher (e.g. "Open in Pano Publisher" / Street View export profile) | C |

### 2.8 Won't

Tour creation, stitching, RAW development, 360° video, free compositing, cloud sync, automatic
face/licence plate detection, Lightroom "edit in external editor" round trip (the workflow is
folder-based), editing GPS/GPano metadata (Pano Publisher does that), RAW/DNG files, PSD files,
several images side by side (one active image + filmstrip is enough; content from other images via
copy/paste).

## 3. Domain model

```
Project ───────────── 1..n ──► Panorama ──── 1 ──► EditStack ─── 0..n ──► Operation (= layer)
  │  rootFolder                  │ sourceFile         │ (ordered)           │ type
  │  exportSettings              │ dimensions         │                     │ params (spherical coords)
  │  presets[]                   │ bitDepth/colorSpace│                     │ mask? ──► Mask
  │                              │ metadata           │                     │ enabled, opacity, group
  │                              │ orientation        │                     │
  ▼                              ▼                                          ▼
Preset (template of 1..n operations)           BatchJob (preset × panoramas → results)
PatchAsset (logo/ground image, reusable)       ExportJob (panorama × ExportSettings → file)
```

### Entities

- **Project** – a working folder with panoramas, project settings, presets, assets.
- **Panorama** – a source image (equirectangular). Immutable; references the original file.
- **Metadata** – EXIF, XMP (incl. GPano), ICC, GPS. Read, displayed (read-only, except deliberately
  changed fields such as heading) and written on export.
- **EditStack** – ordered list of operations per panorama. The result is the original plus all active
  operations applied in order. The UI shows the edit stack as a **layer list**: every operation is a
  layer with mask, opacity and visibility; layers can be grouped.
- **Operation** – one edit. Types (extensible):
  - `Inpaint` (mask, model, seed, result raster as cache/artefact)
  - `Patch` (PatchAsset, centre direction, angular radius, rotation, feather)
  - `CloneStroke` / `HealStroke` (brush strokes, source rotation)
  - `Fill` / `Blur` (mask, parameters)
  - `ImageLayer` (reference to a second panorama of the same location, rotation, mask)
  - `Adjust` (exposure, white balance, curves – locally via mask or globally)
  - `Orientation` (yaw/pitch/roll)
- **Mask** – described as vectors (brush strokes as paths on the sphere, circular caps) and rasterised
  on demand. Hence resolution-independent and transferable.
- **Stroke** – sequence of points (directions on the sphere) + brush parameters (radius in degrees,
  hardness, opacity).
- **Preset** – named template of operations (without image-specific results), applicable to other panoramas.
- **PatchAsset** – reusable image (logo, ground texture) for patches.
- **BatchJob** – application of a preset to n panoramas; has a status per image.
- **ExportSettings** – format, bit depth, quality, target folder, name pattern, metadata rules.

### Key business rules

1. Original files are **never** modified.
2. Export writes to a separate folder by default; overwriting only after explicit confirmation.
3. Metadata is copied 1:1; only deliberately changed fields are adjusted (e.g. heading after alignment,
   software tag, dimensions if resized).
4. Edit parameters are defined in **angles/directions**, never in pixels.
5. Inpainting results are not deterministically reproducible → the computed result is stored as an
   artefact so the project looks the same when reopened.
6. A batch error on one image does not abort the whole job.

## 4. Glossary

| Term | Meaning |
| --- | --- |
| **Equirectangular** | Projection of the sphere onto a 2:1 rectangle; x = longitude (yaw), y = latitude (pitch). |
| **Nadir** | Point straight down (pitch −90°); where the tripod stands. |
| **Zenith** | Point straight up (pitch +90°). |
| **Seam** | Left/right image edges of the equirectangular image, which meet on the sphere. |
| **Rectilinear / gnomonic** | "Normal" perspective like a classic photo; what the viewer shows. |
| **Cubemap** | The sphere as 6 cube faces; useful for rendering and editing without pole distortion. |
| **Spherical cap** | Circular area on the sphere, defined by centre + angular radius. |
| **GPano** | Google XMP namespace for panorama metadata (ProjectionType, PoseHeading, croppings …). |
| **Inpainting** | Automatically filling a masked area with plausible image content. |
| **Feather** | Soft transition at the edge of a mask/patch. |
| **Nadir patch** | Graphic/logo placed over the nadir (or zenith). Not to be confused with the patch tool. |
| **Patch tool** | Retouching tool (as in Photoshop): select an area and replace it with a source area. |
| **Spot healing** | Brush that automatically repairs small defects from the surroundings. |
| **Marking menu** | Radial menu appearing at the cursor; selection by moving the mouse in a direction. |
| **Yaw / pitch / roll** | Rotation around the vertical axis / tilt up-down / tilt sideways. |
