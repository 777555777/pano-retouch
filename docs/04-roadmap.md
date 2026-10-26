# 04 – Roadmap

Milestones are cut so that **every milestone delivers something usable** and the biggest technical
risks are resolved first. Epics are broken down into stories with acceptance criteria before
implementation. Milestones, epics and stories are tracked as **GitHub milestones and issues**; this
document is the overview.

## Overview

| Milestone | Outcome | Core features |
| --- | --- | --- |
| **M0 – Tech spikes** | Architecture validated, ADRs decided | – |
| **M1 – Viewer & project** | View folders of panoramas (incl. 300 MP & EXR), export losslessly | F-01–06, F-08, F-10–12, F-14, F-15, V-01–03, V-07–09, U-04, U-05, U-07, U-08 |
| **M2 – Layers & nadir patch** ⭐ first release (0.1) | Logo/nadir patches as layers on single images, simple batch | R-04, F-07, F-13, H-01, H-06, H-08, H-09, B-01–03, B-06 |
| **M3 – Retouching tools & masks** | Manual retouching on the sphere (clone, heal, spot heal, pixelate), layer masks | R-01, R-02, R-05, R-06, R-07, R-11, H-02, H-04, U-01 |
| **M4 – AI inpainting** ⭐ 0.x | Remove the tripod with AI | R-03, model management |
| **M5 – Batch retouching & review** | Retouch whole tours efficiently | B-04, B-05, presets for inpainting |
| **M6 – Towards 1.0** | Polish, secondary features | H-03, R-12, R-13, R-15, F-09, B-09, U-02, U-03, U-09, V-04, S-01 |

**Single image before batch** (decided): tools for single images (M3, M4) take priority; batch review
follows in M5.

Why M2 before the brushes? The nadir patch is the simplest feature with high value, yet it already
covers the complete chain (sphere operation → edit stack → layers → batch → export with metadata).

---

## M0 – Tech spikes

Test data: see [06-test-data.md](06-test-data.md).

Goal: check the riskiest assumptions from [03-architecture.md](03-architecture.md#7-technical-risks)
with throw-away code. Outcome of each spike: a short report + ADR update.

| Spike | Question | Success criterion |
| --- | --- | --- |
| S1 Tile viewer | Can Tauri + WebGPU deliver a smooth viewer for a **300 MP** image (JPEG, 16-bit TIFF, EXR) whose tiles come from Rust? Virtual texturing vs. cube-map LOD? | Budgets from architecture section 5 on macOS; Windows checked; comparison against Photoshop with the same files |
| S2 Brush round trip | How does stroke preview + asynchronous commit feel? | No noticeable offset/flicker between preview and result |
| S3 Inpainting | Does LaMa/MI-GAN via `ort` with reprojection give good nadir results – also for HDR? Which licences? | Visually good on 5 real test images (1 of them HDR EXR), < 10 s with GPU, licence overview |
| S4 Metadata | Is there a Rust solution for a lossless EXIF/XMP/GPano round trip (JPEG + TIFF)? | `exiftool` diff between original and export empty (except expected fields) |
| S5 Linux webview | WebGPU/WebGL2 in WebKitGTK? | Decision on how Linux is supported |

## M1 – Viewer & project

- **E1.1 Project scaffold:** monorepo, Tauri app, Rust workspace, CI (build + tests on macOS/Windows/Linux), lint/format.
- **E1.2 Geometry library:** `pano-geom` with full test coverage.
- **E1.3 Image I/O & tiling:** read JPEG/TIFF/BigTIFF/EXR, fast first preview, pyramid, tile cache (RAM + central disk cache), thumbnails, colour management, alpha.
- **E1.4 Viewer:** sphere view, flat view, navigation, quick jumps, HDR exposure slider (UX per [08-ux-concept.md](08-ux-concept.md)).
- **E1.5 Folder project & app shell:** open folder, filmstrip, image switching, header, collapsible panels, command palette.
- **E1.6 Export:** lossless export (initially without edits) preserving metadata, export presets.

## M2 – Layers & nadir patch (release 0.1)

- **E2.1 Edit stack & persistence:** operations, undo/redo, sidecar format, auto-save.
- **E2.2 Layer panel & history:** layer list with groups, drag & drop, opacity, history tab.
- **E2.3 Patch operation:** manage patch assets (PNG/SVG), place the patch (size/rotation/feather, interactive in the viewer), colour and blurred-floor modes.
- **E2.4 Presets & batch:** save a patch as preset, apply to a selection, job queue.
- **E2.5 Batch export.**
- **E2.6 Release pipeline:** installable unsigned builds (GitHub Releases), updater signing key, release notes, user README.

## M3 – Retouching tools & masks

- **E3.1 Mask brush** (preview + commit), eraser, straight lines, circular cap mask.
- **E3.2 Clone stamp** on the sphere.
- **E3.3 Layer masks:** edit masks afterwards, selection → layer mask, refine (grow/shrink/feather/invert).
- **E3.4 Healing brush & spot healing** (sensor dust, lens flares).
- **E3.5 Pixelate/blur** (faces, licence plates).
- **E3.6 Shortcuts** (Photoshop-like) and the full navigation scheme.
- **E3.7 Lasso / polygon lasso** on the sphere.

## M4 – AI inpainting

- **E4.1 Inference infrastructure:** `ort`, GPU providers, model management (download after consent, checksum), classic fallback.
- **E4.2 Reprojection & inpainting operation:** LaMa + texture pass and PatchMatch + texture pass
  (spike S3); less repetitive texture pass; invisible boundary (gradient-domain blending, irregular mask
  edge); result as its own layer.
- **E4.3 UX:** job states (progress, cancel, error/retry), discard/regenerate result, switch fill method per region.
- **E4.4 Custom models:** model manifest, import of own ONNX inpainting models with a self-test
  (ADR-0004); diffusion models later.

## M5 – Batch retouching & review

- **E5.1 Presets with masks + inpainting.**
- **E5.2 Review mode.**
- **E5.3 Copy/paste edits.**

## M6 – Towards 1.0

Image layer from a second shot, patch tool, shadow lighten (colour transfer), SVG logos, fill holes
(drone/nadir), marking menu, snapping, saved views, before/after, horizon/heading, docs & website,
contributor guide.

## Later (after 1.0, not planned)

Adjustment layers (H-10, 1.x), AI object selection, text layers (R-14), pen support (U-06), CLI (B-07) and
MCP server (B-08) for automation by scripts/AI agents – the architecture (UI-free core crates,
operations as data) is prepared for this. Ground photo fitting (R-08), sky replacement (R-16).

---

## Template: story

The same template is available as a GitHub issue form.

```markdown
### <ID> <Title>
**As** <persona> **I want** <capability> **so that** <benefit>.

**Acceptance criteria**
- [ ] Given … when … then …
- [ ] …

**Technical notes**
- Affected crates/modules: …
- Relevant ADRs/docs: …

**Definition of done**
- [ ] Tests (unit/golden/E2E as applicable) green
- [ ] Performance budget met (if relevant), numbers in the PR
- [ ] Screenshot/recording in the PR (if UI)
- [ ] Docs/ADR updated
```
