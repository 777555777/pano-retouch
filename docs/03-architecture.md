# 03 – Technical Architecture

Derives the technical solution from the [vision](01-vision.md) and the
[functional specification](02-functional-spec.md). Status: **proposal**, to be validated by the tech
spikes in milestone M0 ([roadmap](04-roadmap.md)). Decisions are recorded as [ADRs](adr/).

## 1. Technical drivers

What shapes the architecture:

| Driver | Consequence |
| --- | --- |
| **Very large images**: target range **100–300 MP** (own rigs, Insta360 X6 ≈ 120 MP), i.e. up to approx. 24K×12K. 300 MP in RGB 16-bit ≈ **1.8 GB**, in RGBA f32 ≈ **4.8 GB** per image (+33 % for the pyramid). | The image is **never fully in RAM as f32**. Tiling, image pyramid, out-of-core processing with a disk cache on a fast SSD, memory management in the native core. |
| **HDRI workflow**: 32-bit float, linear, values far above 1.0 (sun). | Pipeline float-capable from day 1; display with exposure/tone mapping; no clipping anywhere in processing. |
| **GPU limits**: WebGPU only guarantees 8192 px texture edges, many GPUs 16384. | Display via tiles/cube faces with level of detail, never as one texture. |
| **Interaction must feel instant** (brush, viewer at 60 fps). | Two stages: instant GPU preview in the frontend + asynchronous full-resolution computation in the core. |
| **Spherical geometry** (poles, seam). | All operations in spherical coordinates; rasterisation per pixel via its direction vector. No special cases for seam/poles. |
| **Local AI inpainting.** | ONNX Runtime in the native core, GPU-accelerated (CoreML/DirectML/CUDA), models outside the binary. |
| **Maintainer knows web technologies**, the core should be native. | UI in TypeScript/Svelte, compute core in Rust, connected through Tauri. |
| **Platforms**: macOS, Windows, Linux. | Cross-platform stack; check WebGPU availability per webview (risk on Linux). |

## 2. Proposed stack

| Layer | Technology | Rationale |
| --- | --- | --- |
| App shell | **Tauri 2** | Small binaries, Rust backend, web UI; fits the maintainer's web skills. → [ADR-0001](adr/0001-desktop-shell-tauri-rust.md) |
| UI | **Svelte 5 + TypeScript** (Vite) | Known from Pano Publisher, lean, reactive. |
| Viewer/rendering | **WebGPU** (evaluate WebGL2 fallback) in the webview | Sphere rendering, brush preview, overlays on the GPU. |
| Compute core | **Rust** (workspace with several crates) | Performance, memory safety, good image/parallel libraries (`rayon`). |
| Image I/O | `image`, `tiff`, `zune-jpeg` / `jpeg-encoder` or mozjpeg, `exr` | Pure Rust preferred; check JPEG encoder quality in the spike. |
| Colour management | `lcms2` (Rust bindings for Little CMS) | Handle ICC profiles correctly. |
| Vector graphics | `resvg` | Rasterise SVG logos for patch assets at any target resolution (R-13). |
| Metadata | XMP/EXIF library (evaluate candidates in the spike, e.g. `xmp_toolkit`, `little_exif`, `kamadak-exif`) | Lossless EXIF/XMP/GPano round trip is mandatory. |
| AI inference | `ort` (ONNX Runtime for Rust) | Cross-platform, GPU providers (CoreML, DirectML, CUDA). |
| Inpainting model | e.g. LaMa / Big-LaMa, MI-GAN (as ONNX) | Proven for object removal; check licences. → [ADR-0004](adr/0004-local-ai-inpainting-onnx.md) |
| Optional GPU compute in the core | `wgpu` | If the CPU pipeline for brushes/resampling is too slow (later). |
| Tests | `cargo test`, `proptest`, Vitest, Playwright (UI) | See test strategy. |

## 3. System overview

```
┌──────────────────────────── Tauri app ────────────────────────────────┐
│                                                                       │
│  ┌──────────── Frontend (webview, Svelte/TS) ────────────┐            │
│  │  UI: filmstrip, tools, panels, batch, review          │            │
│  │  Viewer (WebGPU): sphere rendering from LOD tiles     │            │
│  │  Brush preview: strokes rendered instantly on GPU     │            │
│  │  State: project/edit state (mirrored from the core)   │            │
│  └───────────────┬──────────────────────────▲────────────┘            │
│      Commands    │ (JSON: operations,       │ Binary tiles (custom    │
│      & events    │  strokes, jobs)          │ protocol / ipc Response)│
│  ┌───────────────▼──────────────────────────┴────────────┐            │
│  │  Backend (Rust)                                       │            │
│  │  app-api: Tauri commands, events, tile protocol       │            │
│  └───────────────┬───────────────────────────────────────┘            │
└──────────────────┼────────────────────────────────────────────────────┘
                   │ (pure Rust crates, no Tauri dependency)
   ┌───────────────┼───────────────┬──────────────┬──────────────┐
   ▼               ▼               ▼              ▼              ▼
 pano-geom      pano-image      pano-edit      pano-ai        pano-project
 projections    tiles, pyramid  edit stack,    ONNX inference, project/sidecar
 sphere maths   I/O, metadata,  operations,    reprojection    format, presets,
 rotations      colour, cache   masks, render  for inpainting  batch jobs, export
```

**Ground rule:** the core crates know neither Tauri nor the UI. They are testable on their own and
enable a CLI (B-07) later at no extra cost.

## 4. Key concepts

### 4.1 Coordinates & geometry (`pano-geom`)

- Canonical representation of a direction: **unit vector** `Vec3` (for maths) or yaw/pitch (for UI/persistence).
- Mappings: equirect pixel ↔ direction, direction ↔ cube face/UV, direction ↔ gnomonic view (camera with
  view direction + FOV).
- Rotations as **quaternions** (alignment, clone source, patch rotation).
- Angular distance between directions = basis for brushes, caps, feathering.
- Must be tested extensively (round-trip property tests, seam, poles).

### 4.2 Image pipeline & tiling (`pano-image`)

- On open, the image is split into **tiles** (e.g. 512×512) and a **pyramid** (factor-2 reductions) is
  built. Odd sizes are reduced area-weighted so every level stays aligned with level 0 (spike S2). Tiles live in a RAM cache (LRU, configurable budget) and are paged out to disk when needed.
- Working format: stored in source bit depth (u8/u16/f16/f32); computation per tile in `f32`; preview to
  the frontend in `f16` (HDR-capable) or u8 for thumbnails
  ([ADR-0003](adr/0003-tiled-image-pipeline.md)).
- **Fast first impression:** produce a coarse preview before the image is fully decoded – e.g. JPEG
  decoding with DCT scaling (1/8 resolution, very fast), embedded previews, use tiled/mip-mapped
  TIFF/EXR directly. The full pyramid is built in the background and cached for re-opening.
- **Dirty tracking:** every operation reports which tiles it affects (via its spherical bounding cap).
  Only those are recomputed.
- Source for rendering & export is always: `original tile → edit stack operations in order`, with a
  cache for intermediate results.

### 4.3 Viewer & tile transport

- The frontend requests visible tiles at a suitable LOD (depending on view direction and FOV).
- Transport as **binary data** (Tauri custom URI scheme or `ipc::Response`), never as JSON/Base64.
  Proposed format: 8-bit RGBA or f16 for previews; previews need not have full bit depth.
- Rendering: a full-screen shader computes the direction per screen pixel and samples from a tile atlas
  (virtual texturing on the equirect pyramid, extra taps along u near the poles) – decided in spike S1,
  see [ADR-0003](adr/0003-tiled-image-pipeline.md) and the [spike report](spikes/s1-tile-viewer.md).
- **Loading strategy (role model krpano, counter-example Google Maps):**
  - There is always *something* to see: the coarsest pyramid level of the whole image stays in GPU memory.
  - Priority queue: visible tiles at target resolution first, then intermediate levels, then
    neighbours in the direction of movement (prefetch).
  - Outdated requests are dropped when the view changes quickly.
  - Fade from coarse to sharp instead of switching hard.
  - Goal: a pixelated area is visible for a brief moment at most (see budgets), never for seconds.
- **Navigation vs. tool:** see [08-ux-concept.md](08-ux-concept.md#6-viewport-interaction) (space-drag or
  middle mouse = rotate, wheel = zoom, right mouse = marking menu).

### 4.4 Brushes, masks & strokes

- A stroke is captured in the frontend as a sequence of directions + radius (in degrees).
- **Instant feedback:** the frontend renders the stroke directly on the GPU as an overlay (masks) or as
  a preview result on the LOD tile (clone).
- **Commit:** the stroke is sent to the core as an operation; the core rasterises it at full
  resolution: for each pixel of the affected tiles determine its direction → angular distance to the
  stroke line → opacity. A circle on the sphere is thus automatically "widened" correctly at the poles,
  and the seam needs no special case.
- **Clone stamp:** source = rotation `R`, target pixel direction `d` → source `R·d`. Works anywhere on the
  sphere without distortion.
- **Hand-over preview → result** (confirmed in [spike S2](spikes/s2-brush.md)): preview and commit use the
  same distance formula; every committed stroke gets an edit version, every GPU tile slot knows the
  version it holds, and the shader previews a stroke only where the shown data is older. Changed visible
  tiles are replaced in place – no flicker. Clone sources are sampled bicubic (Catmull-Rom).

### 4.5 Nadir/zenith patch

- The patch image is placed on a spherical cap as an **azimuthal projection** (centre, angular radius,
  rotation, feather). Per target pixel: direction → position in the patch image → sampling
  (bilinear/bicubic, with the mip level matching the target resolution).
- Parameters in degrees → identical on images of any resolution (batch).

### 4.6 AI inpainting (`pano-ai`)

Inpainting models work on flat, undistorted images. Hence, invisible to the user:

1. Determine the bounding cap of the mask (+ context margin).
2. Render a **gnomonic view** of this region (e.g. for the nadir: looking down, suitable FOV), at a
   resolution depending on model & image size (e.g. 512–2048 px).
3. Project the mask the same way; run the model (ONNX, GPU provider if available).
4. **Reproject** the result onto the equirectangular tiles, only inside the mask, with feathering – the
   original stays bit-exact outside the mask.
5. Store the result as a raster artefact of the operation (non-deterministic, see functional spec).

Spike S3: models work at 512 px, so a **full-resolution texture pass** follows (PatchMatch matching low
frequencies, copying original pixels from the surroundings); HDR is tone-mapped (exposure from the
context, Reinhard + gamma) and mapped back exactly. See [spike S3](spikes/s3-inpaint.md).

### 4.7 Project & sidecar format (`pano-project`)

Decided ([ADR-0002](adr/0002-non-destructive-edit-stack.md)):

```
my-folder/
  IMG_0001.jpg
  IMG_0002.jpg
  .pano-retouch/
    project.json            # version, settings, presets, export settings
    assets/                 # patch assets (logos, ground images)
    edits/IMG_0001.json     # edit stack (operations, strokes, parameters, layer groups)
    artifacts/IMG_0001/     # raster results (inpainting), compressed
```

Separately, the **cache** (pyramids, thumbnails, intermediate results) lives **centrally** in the app's
cache directory on the internal SSD (e.g. `~/Library/Caches/pano-retouch/` on macOS), with a size
limit and LRU cleanup. Reason: at 300 MP this quickly amounts to several GB per image, and image
folders often live on slow external drives/NAS. The cache can be deleted at any time. Images and
exports may live anywhere (internal/external SSD, NAS); the cache always stays local.

- JSON with `schemaVersion` and a migration path. Readable, diffable, agent-friendly.
- The cache is reproducible and may be deleted; `artifacts/` must not.
- Saving is automatic and debounced in the background; only edit data and new artefacts are written,
  never the full image (that is only rendered on export).

### 4.8 Batch & jobs

- Job queue in the core (Tokio/thread pool), parallelism limited by RAM budget (large images!).
- Jobs send progress events to the frontend; cancellable; errors per image.
- Inpainting jobs are serialised on the GPU, I/O and patches run in parallel on the CPU.
- AI job states (queued, running with progress, done, cancelled, failed) are surfaced in the UI as
  described in [08-ux-concept.md](08-ux-concept.md#7-states).

### 4.9 Export

- Full-resolution composition tile by tile (streaming) to save RAM.
- TIFF > 4 GB as **BigTIFF**; EXR with selectable compression (lossless: ZIP/PIZ; lossy: DWAA/DWAB) and half/float.
- Copy the original's metadata; adjust: software tag, dimensions, heading (if changed), keep GPano fields consistent.
- Configurable JPEG quality; selectable chroma subsampling (4:4:4 for high quality).

### 4.10 Colour management

The test data contains Adobe RGB, ProPhoto RGB, sRGB and Display P3 (see [06-test-data.md](06-test-data.md)).
Principle: **users don't need to understand colour spaces – the software does the right thing.**

- **Work in the source colour space:** pixels are not converted; the embedded ICC profile stays part of
  the image. Images without a profile are treated as sRGB (EXR: per `chromaticities`, otherwise linear Rec.709).
- **Display:** source profile → monitor profile conversion only in the viewer (shader/LUT). Otherwise
  Adobe RGB images look wrongly saturated on P3 displays.
- **Operations** that blend (feather, heal, shadow lighten) compute linearised in the source colour space.
- **Inpainting:** input to the model as 8-bit sRGB (converted), result back into the source colour space.
- **Export:** the source profile is kept. Web/Street View presets convert to sRGB.
- Implemented with `lcms2`; query the monitor profile from the OS (check in the spike how reliably this
  works in the webview).

### 4.11 Decoding large files (findings from the test data)

- **Striped TIFF** (LZW/Deflate with predictor): strips are independent → **decode in parallel** and
  re-sort directly into tiles and pyramid.
- **JPEG without restart markers** (e.g. 283 MP from Affinity, Insta360): decoding is sequential.
  First view via DCT-scaled decoding, full resolution in the background; cache afterwards. JPEGs with
  restart markers (e.g. from Photoshop) can be decoded in parallel.
- **EXR** (scanlines, PIZ, half): blocks are independent → decodable in parallel.
- **Alpha channel** (associated, from PTGui) preserved throughout; alpha = 0 marks holes.

### 4.12 Updates

- Opt-in check on start against GitHub Releases (the only automatic network access).
- Implemented with the **Tauri updater plugin**: update packages are signed with the project's own free
  key pair and verified before installation. This is independent of paid Apple/Windows code signing and
  thus robust even though the app ships unsigned.
- Check in the spike/release epic: Gatekeeper/SmartScreen behaviour when updating unsigned apps.

## 5. Non-functional requirements

### Performance budgets

Reference image: **300 MP** (24K×12K). Values are starting points and will be calibrated after spike S1.

| | macOS | Windows |
| --- | --- | --- |
| **Reference system** (budgets apply here) | MacBook Pro M4 Max, 48 GB RAM, internal SSD | Intel i7-12700K, NVIDIA RTX 5080, 48 GB RAM, NVMe SSD |
| **Minimum system** (proposal; panorama editing is demanding) | Apple Silicon M1 Pro or newer, 32 GB RAM | Current 6+ core CPU, dedicated GPU with ≥ 8 GB VRAM, 32 GB RAM |

On the minimum system things may be slower but must be stable. The UI stays smooth; only background
computations take longer.

| Scenario | Budget |
| --- | --- |
| Open until first (coarse) view | < 1 s (JPEG); < 2 s for large TIFF/EXR. Reference macOS `sips`: 0.6 s / 2.7 s at 283 MP |
| Open until sharp view of the current viewport | < 3 s (re-opening from cache: < 1 s) |
| Viewer navigation incl. zoom to 1:1 | 60 fps (120 fps on ProMotion desirable), never blocking; blurry → sharp while loading is fine |
| Switching images in the filmstrip | preview instantly (< 100 ms) |
| Brush feedback (preview) | < 16 ms per frame |
| Committing a stroke at full resolution | < 300 ms, asynchronous, UI never blocks |
| Inpainting a nadir region | < 10 s with GPU, < 60 s CPU |
| Export 300 MP JPEG / 16-bit TIFF | < 10 s / < 15 s |
| Peak RAM while editing a 300 MP image | < 8 GB (configurable budget, the rest via disk cache) |
| Thumbnails for a folder of 100 images | usable after < 10 s, the rest in the background |

### Other

- **Offline-first**, no telemetry; network only for explicit actions (e.g. model download after consent).
- **Robustness:** a crash must not lose edits (persist the edit stack immediately/periodically).
- **Accessibility:** keyboard shortcuts for all core actions, visible focus, reduced motion respected.
- **Internationalisation:** UI strings via i18n keys; English (default) + German.
- **Licence hygiene:** all dependencies and models with compatible licences.

## 6. Test strategy

| Level | What | How |
| --- | --- | --- |
| Geometry | projections, rotations, angular distances | unit + property tests (`proptest`): round trips, seam, poles |
| Image operations | patch, clone, mask rasterisation | **golden image tests** with synthetic test panoramas (grid, gradients, checkerboard at the poles) and tolerance comparison |
| I/O & metadata | reading/writing all formats | round-trip tests: pixel diff, metadata diff (e.g. against `exiftool` output) |
| Performance | budgets from section 5 | benchmarks (`criterion`) in CI, flag regressions |
| Frontend | components, state | Vitest |
| Navigation | feel, smoothness, latency | dedicated test protocol (mouse, trackpad, FOV range, 300 MP), sign-off by the maintainer |
| End-to-end | open → retouch → export | Playwright/WebDriver against the Tauri app (later) |

Test data: small synthetic panoramas in the repo; real sample panoramas (large, own/public) outside the
repo or via a download script (see [06-test-data.md](06-test-data.md)).

## 7. Technical risks

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Tile transport Rust → webview too slow | stuttering viewer | spike M0; binary protocol, small LOD tiles, prefetching; fallback: native rendering with `wgpu` |
| WebGPU unavailable in the Linux webview (WebKitGTK) | limited Linux support | WebGL2 fallback for the viewer; Linux as "best effort" |
| Inpainting quality for large masks/high resolution | visibly blurry patches | multi-stage (coarse + detail), tiling, touch-up with clone; evaluate models |
| Incomplete metadata round trip in Rust libraries | loss of GPano/GPS | test early; if needed copy the XMP packet raw and patch only specific fields |
| 300 MP exceeds RAM with naive processing | swapping, stutter, crashes | out-of-core design from the start; check memory budgets in tests |
| Inpainting on HDR (models expect 8-bit sRGB) | colour casts, clipping | tone mapping before/inverse mapping after inference, detail transfer; spike S3 with an HDR test image |
| Memory use in batch + large images | crashes | RAM budget, limited parallelism, streaming export |
| Model licences/size | legal issues, large installer | models separate, download after consent; document the licence per model |

## 8. Repository structure (proposal)

```
pano-retouch/
  AGENTS.md / CLAUDE.md
  docs/                     # these documents, ADRs
  mockups/                  # UX prototypes (reference only)
  apps/desktop/             # Tauri app
    src/                    # Svelte frontend
    src-tauri/              # Rust: Tauri glue (app-api)
  crates/
    pano-geom/
    pano-image/
    pano-edit/
    pano-ai/
    pano-project/
    pano-cli/               # later: headless batch, basis for an MCP server
  testdata/                 # small synthetic test images
  tools/                    # scripts (generate test data, convert models …)
  Cargo.toml                # Rust workspace
  package.json              # pnpm workspace
```
