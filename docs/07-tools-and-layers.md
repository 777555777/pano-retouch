# 07 – Tools, Selection & Layers (Scope)

Status: **Agreed** (mockup reviews v1/v2, questions W1–W5 answered on 2026-10-21, see
[05-decisions.md](05-decisions.md)). Goal: define *how much* image editing pano-retouch should offer –
enough to replace Photoshop for panorama retouching, but not a second Photoshop.

## 1. Scope guidelines

1. **Smart first, manual always.** Every "smart" feature (AI fill, shadow lighten, later object
   selection) has a manual fallback (clone stamp, brush mask). When the smart path fails, the user is
   never stuck.
2. **Selection is the heart.** In Photoshop, the quality of the selection determines the quality of the
   retouch. So we invest in good selection/mask tools rather than many special tools – almost every tool
   acts on a selection.
3. **Layers like Photoshop – but only what retouching needs.** Order, masks, groups, opacity: yes.
   Layer styles, smart objects, vector shapes, free painting on empty layers: no.
4. **Everything on the sphere.** Every selection and mask is sphere-based (in degrees), works across
   nadir, zenith and seam, and is transferable to other images.
5. **Every feature must improve a panorama workflow** (principle "retouching first", vision).

## 2. Layer model

### Layer types

| Type | What it does | Examples | When |
| --- | --- | --- | --- |
| **Retouch layer** | creates new pixels inside its mask | Generative Fill, spot healing, clone stamp, healing brush, patch tool | 0.x |
| **Adjustment layer** | changes existing pixels locally (non-destructive) | shadow lighten, blur/pixelate; later exposure, white balance, curves | 0.x / 1.x |
| **Image layer** | brings in image content | nadir patch (logo/colour/floor), second shot of the same location (UC-7); later text, SVG | 0.x / 1.0 / later |
| **Group** | folder for layers | "Tripod", "People", "Logo" | 0.x |

### Properties of every layer

| Property | When |
| --- | --- |
| Visibility, name, delete, duplicate | ✅ 0.x |
| Order via drag & drop, **strict like Photoshop**: top covers bottom, including adjustment layers (W4) | ✅ 0.x (the mockup does not render the order exactly yet) |
| Opacity | ✅ 0.x |
| **Layer mask** – editable afterwards with all selection/mask tools (brush, eraser, feather …) | 1.0 |
| Groups: rename, show/hide, expand/collapse | ✅ 0.x |
| Group opacity and mask | 1.0 |
| Blend modes (multiply, screen …) | later, only on concrete demand |
| Locking, colour labels, filter/search in the layer list | later |
| Layer styles, smart objects, vector shapes, free painting | ❌ not planned |

## 3. Selection and mask system

One concept for everything: there is an **active selection** (red overlay). Tools such as Generative
Fill, blur or adjustments act on the selection. When it becomes a layer, the selection becomes that
layer's **layer mask** and stays editable.

| Function | Description | When |
| --- | --- | --- |
| Mask brush / eraser | size in degrees, hardness; hold `⌥` = subtract | 0.x (in mockup v2+) |
| Straight line | `⇧`-click connects to the last point – on the sphere this is a great circle | 0.x (in mockup v2+) |
| Circular cap | nadir/zenith circle by radius (fastest tripod selection) | 0.x |
| Lasso / polygon lasso | points on the sphere, edges as great circles | 0.x |
| **Refine selection** | grow/shrink (in degrees), feather, smooth, invert | 1.0 |
| Mask view | show the mask as black & white or overlay | 1.0 |
| Save / load selection | selection as a template for other images (batch) | 1.0 (part of presets) |
| AI object selection | click or box on an object → mask (Segment-Anything-style model locally via ONNX, same reprojection pipeline as inpainting; check licence and size) | later (W2) |
| Select by colour / magic wand | select similar pixels (e.g. sky, snow) | later |
| Refine edge (AI matting) | hair, branches against the sky | later |

## 4. Retouching tools

| Tool | Smart / manual | When | Notes |
| --- | --- | --- | --- |
| Generative Fill (AI inpainting) | smart | 0.x | main tool for tripod, rig, holes |
| Spot healing | smart | 0.x | sensor dust, lens flares, small things |
| Clone stamp | manual | 0.x | fallback for everything; source set with the first click (or `⌥`-click), source stays visible |
| Blur: Gaussian / pixelate | manual | 0.x | privacy and refinement |
| Nadir patch: logo (PNG/SVG), colour, "blurred floor" | – | 0.x | own logo upload; later pattern/texture, mirror fill |
| Healing brush | semi | 1.0 | clone + matching brightness/colour |
| Patch tool | semi | 1.0 | drag a selection onto a source area |
| Shadow lighten | smart | 1.0 | the mockup only brightens (looks grey). Proper: **colour transfer** from the surroundings (match mean and contrast per channel) while keeping the structure |
| Adjustments local/global (exposure, WB, curves) | manual | 1.x | W3: plan for it if the effort is reasonable; design the layer model for it from the start |
| Sky replacement | smart | later | mainly drone zeniths |
| Ground photo fitting | semi | later | see functional spec R-08 |

## 5. Scope per version

- **0.1 (first release, roadmap M2):** navigation, layers (without masks), groups, history, nadir patch, export.
- **0.x (M3/M4):** mask brush/eraser/line, circular cap, lasso, Generative Fill, spot healing, clone
  stamp, blur/pixelate, fill holes.
- **1.0:** edit layer masks, refine selection, healing brush, patch tool, shadow lighten (colour
  transfer), group masks, selection templates for batch.
- **1.x:** adjustment layers (exposure, white balance, curves) – local via mask or global.
- **Later:** AI object selection, magic wand, refine edge, blend modes, sky, ground photo, text/SVG.

## 6. Technical assessments from the reviews

**Auto-save with large images.** Not the image is saved, only the edit data: layers, parameters and
masks in spherical coordinates (KB to a few MB), plus AI fill results as pixel patches (usually a few
tens of MB even at 300 MP). Writing happens in the background, debounced (e.g. 1–2 s after the last
change). The full image is only rendered on export. → uncritical.

**History like Photoshop.** Straightforward because every edit is an operation anyway (ADR-0002).
Jumping to a step means undoing/redoing operations, not storing image states. Consequences:
(1) AI results of undone steps must stay in memory/cache until the end of the session → size limit needed.
(2) Many small brush strokes should be merged in the history so it stays readable.
(3) History is per session; persistent history would be H-07 (later).

**Sensitive navigation.** Cause in the mockup: the momentum after release was computed from very short
time intervals and produced spikes. Since v2 it is smoothed and capped, and rotation speed matches the
angle per pixel exactly. In the real app, navigation gets its own test protocol (mouse, trackpad,
different FOVs) because it is the biggest share of the UX.
