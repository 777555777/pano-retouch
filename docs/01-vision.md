# 01 – Product Vision

## Vision statement

> **For** photographers of 360° panoramas – from ambitious individuals to creators of real-estate
> and Street View tours – **who** need to clean their panoramas of tripods, nadir artefacts and
> distracting objects before publishing, **pano-retouch is** an open-source desktop editor **that**
> enables retouching directly on the sphere – distortion-free, AI-assisted and for many images at
> once. **Unlike** general-purpose image editors (Photoshop, GIMP, Affinity), where you have to
> export cube faces, edit them and stitch them back together, **pano-retouch** understands 360°
> geometry natively and turns retouching into a task of seconds instead of minutes per image.

## Problem

1. **Equirectangular images are extremely distorted at the poles.** That is exactly where the
   tripod almost always is (nadir). Classic tools (clone stamp, brush, inpainting) work on the flat,
   distorted projection and produce visible errors.
2. **Today's workaround is cumbersome:** convert to cube faces/rectilinear → retouch in Photoshop →
   convert back → hope no seams appear. This slows things down, especially for tours with 20–200
   panoramas.
3. **Image quality and metadata get lost:** many tools reduce bit depth, strip XMP/GPano metadata
   (mandatory for Street View and tour software) or recompress.
4. **Existing specialised tools** are commercial, platform-bound, outdated or tied to one camera
   ecosystem. There is no good, open, cross-platform tool focused on retouching.

## Target audience & personas

| Persona | Description | Core need |
| --- | --- | --- |
| **The maintainer (primary)** | Shoots 360° panoramas with own camera rigs (100–300 MP), came to 360° photography via HDRIs, publishes on Street View (Pano Publisher), web developer. Today mostly edits single images, used to edit series (real estate 5–25, Street View 15–100). Works with a mouse. | Speed up own workflow; a tool that fits exactly – without stutter. |
| **HDRI photographer** | Creates single HDR panoramas (EXR, 16/32-bit float) as environment maps for 3D software (Blender, Unreal, V-Ray …). Few images, maximum quality. | Remove tripod/photographer without damaging dynamic range, bit depth or light sources (sun!); lossless EXR export. |
| **Tour creator (real estate/business)** | Shoots 10–60 rooms per property, must deliver fast. | Remove the tripod in all images quickly and cleanly, own logo as nadir patch, batch. |
| **Street View mapper** | Uploads sequences (sometimes hundreds of images). | Batch nadir patch/blur, metadata (GPS, GPano) preserved. |
| **Enthusiast** | Occasional panoramas with Insta360/Theta/DSLR rig. | Easy start, better results than general-purpose tools. |

## Goals

| # | Goal | Measured by |
| --- | --- | --- |
| G1 | **Retouching on the sphere** without visible projection errors and without manual reprojection. | Results show no seams/distortion in the viewer and in tour software. |
| G2 | **Fast:** tripod removal of a typical panorama in < 30 s of working time. | Self-test measurement. |
| G3 | **Scalable:** apply the same retouch/patch to a whole tour and correct individual images. | 50 panoramas with nadir patch in < 5 min incl. review. |
| G4 | **Lossless:** bit depth, colour profile and metadata (EXIF, XMP, GPano, GPS) are preserved. | Round-trip tests: empty metadata diff, no unintended recompression. |
| G5 | **Smooth even with very large images:** 100–300 MP (up to approx. 24K × 12K), 16-bit and 32-bit float. Noticeably smoother than Photoshop & co. with the same files. | Performance budgets in [03-architecture.md](03-architecture.md#performance-budgets). |
| G6 | **Open & local:** open source, works fully offline, no cloud requirement. | No network access without explicit user action. |

## Non-goals (deliberately excluded)

- ❌ **Creating virtual tours** (hotspots, linking, floor plans) – dedicated software exists for that.
- ❌ **Stitching** single images into panoramas – done by PTGui/Hugin/camera software.
- ❌ **RAW development & extensive colour grading** – done by Lightroom & co.
- ❌ **Video editing** (360° videos).
- ⏸ **Free compositing / general image editor:** montages of arbitrary images are not a goal.
  Text and SVG logos are out **for now**, but conceivable later (e.g. copyright text, vector logo at
  the nadir) – don't build the architecture against it.
  *Scope note:* retouch layers (every edit is a layer with mask/opacity) and image layers from further
  shots **of the same location** (e.g. a second pano with the tripod moved) are explicitly **in scope**.
- ❌ **Fitting a separately shot ground photo** (perspective alignment) – not for now; interesting later.
- ❌ **Cloud service / account system.**

## Secondary goals (possible, not the focus)

- Simple **alignment**: level the horizon, set the viewing direction (heading).
- Simple **global corrections**: exposure, white balance, contrast – for small fixes without a round
  trip to Lightroom.
- **Output variants**: e.g. little planet / preview image export.

## UX principles

Derived from the maintainer's role models and counter-examples:

| Role model / counter-example | What we adopt or avoid |
| --- | --- |
| **Figma** | Reacts instantly, everything feels "direct". Excellent snapping. Clean UI – the workspace is the focus, options appear only when needed. |
| **Photoshop** | Strong retouching tools (clone stamp, patch tool, content-aware fill) and familiar shortcuts – adopt. Sluggishness with large images and the lack of a sphere mode (the 3D mode was removed; today only plugin workarounds via cube projection) – exactly what we solve. |
| **Maya marking menus / GTA weapon wheel** | Radial quick menu at the cursor for tool selection: fast, without cluttering the UI. |
| **krpano, LizardQ** | Tile-based viewers that show even gigapixel images smoothly. Our benchmark for the viewer. |
| **Google Maps 360°** (counter-example) | Staring at pixelated areas for seconds until tiles load – must not happen here. |
| **PTGui** | Domain depth for panoramas; detail yes, but behind a clean interface. |

Consequences:
- **The sphere view is the workspace** and fills the screen. The flat view is a switchable overview and mini-map.
- **Minimal, contextual UI:** few permanently visible elements, tool options only for the active tool.
- **Mouse first** (pen later), **Photoshop-like shortcuts**, radial quick menu.
- **Snapping** where it helps (nadir/zenith/horizon, angle grid for rotation, alignment to other layers).
- **Base layout:** toolbar left (collapsible), mini-map top right, layers below, filmstrip at the
  bottom (collapsible). The image takes by far the largest part.
- **Expert/zen mode** (role model: Maya's "expert mode"): all panels off, sphere view only; everything
  remains reachable via marking menu, shortcuts and command palette.
- **Courage for new interaction concepts** where they beat the standard – validated with mockups.
- **Dark mode only.** UI language English (default), German via settings.

Details: [08-ux-concept.md](08-ux-concept.md).

## Product principles

1. **The sphere is the truth.** All edits are described in spherical coordinates (directions,
   angles), not pixels. That makes them distortion-free, resolution-independent and transferable to
   other images (→ batch).
2. **Non-destructive.** Originals are never overwritten. Edits are an editable stack of operations;
   export creates new files.
3. **Performance is UX.** Large images must never stutter. Interaction always feels instant (preview),
   full resolution is computed in the background; the UI never blocks. Performance budgets are
   acceptance criteria, not wishes.
4. **Retouching first.** Every feature must prove that it improves retouching workflows. When in
   doubt: leave it out.
5. **Local & private.** No upload, no telemetry. AI models run locally.
6. **Fits existing workflows.** Folder in, folder out, metadata intact.
7. **Automatable.** Everything the UI can do can be described as data (operations) and can therefore
   later be driven via CLI/MCP by scripts and AI agents.

## Success criteria for version 1.0

- The maintainer uses pano-retouch for their own Street View workflow instead of Photoshop/other tools.
- A complete real-estate tour data set (≥ 30 panoramas) can be retouched without other software.
- Runs on macOS (priority 1) and Windows (aiming for parity); Linux optional/best effort.
- A 300 MP panorama can be viewed and retouched without noticeable stutter.
- External users can build the project and contribute (docs, CI, tests).
