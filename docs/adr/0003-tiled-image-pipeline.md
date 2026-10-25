# ADR-0003: Tiled image pipeline and working format

- Status: Proposed – recommended by [spike S1 and S1b](../spikes/s1-tile-viewer.md) on macOS and Windows; ready to be accepted by the maintainer
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

## Display (result of spike S1)

- **Virtual texturing directly on the equirect pyramid** – no cube map. The viewer shader picks the
  level per pixel from its angular footprint, looks the tile up in a **page table** (storage buffer) and
  samples a **`texture_2d_array` tile cache**; missing tiles fall back to coarser resident levels and a
  2048×1024 base texture (full mips) that is always resident. New tiles fade in over 150 ms.
- **Poles:** up to 8 taps along u where the equirect is horizontally compressed – the nadir is then as
  sharp as with a cube map. The visible-tile set must cover the whole u footprint there.
- **Tiles:** 512×512 + 1 px gutter (x wraps, y clamps) → 514² slots; levels down to ~3000 px width,
  coarser views use the base texture.
- **Levels** are ⌈w/2⌉ × ⌈h/2⌉; odd sizes are reduced **area-weighted** (fractional weights) so pixel
  centres stay at (i + 0.5)/n on every level – a plain 2×2 average misaligned the levels by up to ~5
  full-resolution pixels at the nadir (found in spike S2).
- **Wire format:** u8 for SDR sources (incl. 16-bit, display only), f16 for HDR. f16 doubles the bytes
  but transport is not a bottleneck (5–7 GB/s measured).
- **Transport:** behind one interface with a per-platform backend – **WebView2 shared buffers on
  Windows** (1.2–1.9 GB/s measured; the custom URI scheme and `ipc::Response` reach only ~150 MB/s
  there), **custom URI scheme on macOS** (5–7 GB/s), loopback WebSocket with a session token as fallback
  (e.g. Linux). Tiles are built in one pass directly in the wire format.
- Rejected: cube-map LOD – needs a second tile representation (memory or on-demand resampling) that every
  edit would have to keep in sync; at a 5K canvas it loaded noticeably slower.

## Open points

- Disk-cache format for 16-bit sources: LZ4 barely compresses u16 (≈ 3 GB per 283 MP image) – evaluate
  zstd / predictor, or a separate display pyramid (E1.3).
- Colour management: working space = source profile, conversion only for display.
- **Windows decode times:** EXR (2.4–2.6 s) and 16-bit TIFF (1.6 s) push the first view over budget on
  the Windows reference system – faster EXR decoding and a progressive base are E1.3 work.

## Consequences

- + Manageable memory use, fast previews, incremental updates.
- − Higher complexity (cache, tile borders for neighbourhood filters → overlap/padding needed).
