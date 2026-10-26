# ADR-0002: Non-destructive edit stack in spherical coordinates

- Status: Accepted (storage location confirmed by the maintainer, see decision log #11)
- Date: 2026-10-10

## Context

- Originals must never be modified (functional spec, rule 1).
- Edits should be transferable to other images (batch, presets) and resolution-independent.
- Equirectangular pixel coordinates are problematic at the poles and the seam.

## Decision

- Every panorama has an **edit stack**: an ordered list of operations, shown in the UI as layers
  (optionally grouped).
- All geometric parameters (brush strokes, caps, patch position, clone source) are stored in **spherical
  coordinates** (yaw/pitch in degrees, radii as angles, rotations as quaternions).
- Non-reproducible results (AI inpainting) are stored as a **raster artefact** on the operation.
- Persistence as versioned JSON (`schemaVersion`) in a `.pano-retouch/` folder next to the images;
  caches separate (central, local) and deletable. Saving is automatic.

## Alternatives considered

- **Destructive editing with an undo buffer:** simpler, but no batch/presets, no later adjustment.
- **Pixel-based masks (bitmaps):** precise, but resolution-dependent, large, not transferable.
  → Rasterised masks only as a cache.
- **SQLite as project format:** robust, but less diffable/readable; maybe later for caches.
- **Central catalogue (like Lightroom) / visible sidecar per image:** rejected by the maintainer in favour
  of the hidden folder that moves with the image folder.

## Consequences

- + Batch and presets come almost for free.
- + No special cases for seam/poles in the data model.
- − Rendering must evaluate the stack efficiently (tile caches for intermediate results).
- − Schema migrations must be considered from the start.

## Spike S2 (2026-10-25)

Confirmed: strokes as directions + angular radius, rasterised per pixel by angular distance, work at the
seam and the poles and at 300 MP (typical stroke committed in 40–60 ms). The hand-over from the GPU
preview to the committed tiles uses **edit versions** per stroke and per tile slot. See
[spike report](../spikes/s2-brush.md).
