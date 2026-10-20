# ADR-0003: Tiled image pipeline and working format

- Status: Proposed (working format recommended, display approach open until spike S1)
- Date: 2026-10-10

## Context

- Images of 100–300 MP (up to approx. 24K×12K), 8/16-bit as well as 16/32-bit float HDR.
- 300 MP in RGBA f32 ≈ 4.8 GB → keeping everything in f32 in memory is not feasible.
- Memory needs of several GB per image; batch processes many images.
- The viewer needs low resolutions quickly; export needs full resolution.

## Decision (proposal)

- Images are organised in **tiles** (start value 512×512, optimise in the spike) with a **pyramid**.
- **LRU tile cache** with a configurable RAM budget; paging to a disk cache.
- **Storage format** of tiles: source bit depth (u8 / u16 / f16 / f32).
  **Compute format** inside operations: f32 per tile, linear where colour-relevant (blending, heal).
  **Display format** (preview tiles to the frontend): f16, so HDR exposure in the viewer works without
  recomputing in the core; thumbnails u8.
- **Disk cache** for pyramids central on the internal SSD (not in the image folder).
- **Dirty tracking** per operation via its spherical bounding region → only affected tiles are recomputed.
- Export streams tile/row-wise into the encoder where the format allows it.

## Open points

- Tile atlas (virtual texturing) vs. cube-map LOD for display.
- Bandwidth: f16 previews double the data compared to u8 – measure in the spike, maybe u8 for SDR sources.
- Colour management: working space = source profile, conversion only for display.

## Consequences

- + Manageable memory use, fast previews, incremental updates.
- − Higher complexity (cache, tile borders for neighbourhood filters → overlap/padding needed).
