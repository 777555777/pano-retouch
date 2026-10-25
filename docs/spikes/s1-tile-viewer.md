# Spike S1 – Tile viewer: Tauri + WebGPU at 300 MP

- Issue: [#1](https://github.com/777555777/pano-retouch/issues/1)
- Code: branch `spike/s1-tile-viewer`, folder [`spikes/s1-tile-viewer/`](https://github.com/777555777/pano-retouch/tree/spike/s1-tile-viewer/spikes/s1-tile-viewer)
  (throw-away, not merged into the product)
- Date: 2026-10-24
- Status: **macOS: all budgets met. Windows: rendering fine, but tile transport through WebView2 is the bottleneck – follow-up needed**

## Question

Can Tauri 2 + WebGPU in the system webview deliver a smooth 360° viewer for a 300 MP panorama whose
tiles are produced by Rust? Which display approach is better: virtual texturing (tile atlas) or
cube-map faces with LOD?

## Short answer

**Yes – on macOS with a large margin.** With the 283 MP test images (set A) all budgets are met:

| Budget | Target | Measured (M4 Max, worst of JPEG / 8-bit TIFF / 16-bit TIFF / EXR) |
| --- | --- | --- |
| First (coarse) view, JPEG | < 1 s | **0.53–0.65 s** |
| First view, large TIFF / EXR | < 2 s | **0.46 s** (8-bit TIFF), **1.05 s** (EXR), **1.58 s** (16-bit TIFF, 1.86 GB) |
| Sharp view of the viewport | < 3 s | **≤ 1.8 s** (16-bit TIFF at 5K canvas) |
| Re-open from cache, sharp | < 1 s | **0.08–0.40 s** (first view 16–23 ms) |
| Navigation incl. zoom to 1:1 | 60 fps | **60 fps** on all images, incl. a 5K canvas (4800×2904) |
| Pixelated area visible | briefly at most | single tiles wait **≤ 20 ms** (display res.) / **≤ 170 ms** (5K canvas) |
| Peak RAM (Rust core) | < 8 GB | **1.6 GB** (JPEG) … **3.4 GB** (EXR); 16-bit TIFF 3.2 GB |

**Recommendation:** virtual texturing on the **equirect pyramid** (no cube map), 512 px tiles with 1 px
gutter, a page table in a storage buffer and a `texture_2d_array` as tile cache, binary transport via
the **custom URI scheme** (`ipc::Response` is equally fast), **u8 tiles for SDR, f16 for HDR**.
Details below; ADR-0003 is updated accordingly.

## Setup

- MacBook Pro M4 Max (16 cores), 48 GB, macOS 27 (Darwin 27.0), system WKWebView.
- WebGPU adapter: Apple, `maxTextureArrayLayers` 2048, features incl. `shader-f16`, `float32-filterable`,
  `timestamp-query`.
- Images: set A (23 800 × 11 900 ≈ 283 MP): JPEG 67 MB (Adobe RGB, no restart markers), 8-bit TIFF
  405 MB, 16-bit TIFF 1.86 GB (LZW, strips), EXR half/PIZ 828 MB; plus Insta360 X5 JPEG (71 MP) and
  set B 16-bit TIFF (84 MP).
- Canvas 1600×968 (display, dpr 1) and 4800×2904 (render scale 3, ≈ 5K, worst case).
- Each number comes from the automatic benchmark run (`S1_AUTO=…`, see the spike README); the "camera
  path" is: pan 360° at FOV 90° (6 s) → tilt to nadir → zoom to 1:1 at the nadir → tilt to the
  horizon at 1:1 → pan 60° at 1:1 (≈ 1300 px/s) → zoom out to FOV 100°.
- **Caveat:** the files were in the OS file cache (warm). Reading 1.86 GB cold from the SSD adds an
  estimated 0.3–0.6 s to the TIFF numbers. A cold measurement needs `sudo purge` before the run.

## Results

### Opening (in the app, incl. IPC and GPU upload)

| Image | First view | Sharp | From cache: first / sharp |
| --- | --- | --- | --- |
| 283 MP JPEG | 528 ms | 877 ms | 17 / 83 ms |
| 283 MP 16-bit TIFF | 1584 ms | 1625 ms | 23 / 184 ms |
| 283 MP 8-bit TIFF | 457 ms | 521 ms | 16 / 83 ms |
| 283 MP EXR (half) | 1052 ms | 1104 ms | 16 / 104 ms |
| 71 MP JPEG (Insta360) | 210 ms | 310 ms | 17 / 84 ms |
| 84 MP 16-bit TIFF (set B) | 467 ms | 534 ms | 16 / 83 ms |

How the first view becomes fast:

- **JPEG:** libjpeg-turbo with DCT scaling (1/8) decodes a 2975×1488 preview in ~0.5 s (entropy
  decoding still has to read the whole file); the full decode (~0.8 s) runs in parallel.
- **TIFF:** strips are decoded in parallel (one decoder per rayon worker) straight into the output
  rows: 1.2 s for 1.86 GB 16-bit LZW, 0.4 s for 8-bit.
- **EXR:** the `exr` crate decompresses blocks in parallel: 0.65 s.
- Pyramid (2×2 average) 30–55 ms, base texture 3–5 ms.

### Rust core (headless benchmark `s1-bench`, 283 MP)

| | JPEG | 16-bit TIFF | 8-bit TIFF | EXR |
| --- | --- | --- | --- | --- |
| Fast preview | 509 ms (1/8 DCT) | – | – | – |
| Full decode | 763 ms (turbo), 954 ms (zune-jpeg) | 1179 ms | 407 ms | 653 ms |
| Pyramid (4 levels) | 29 ms | 46 ms | 35 ms | 54 ms |
| Tile 514² → u8 / f16 | 0.17 / 0.44 ms | 0.44 / 0.62 ms | 0.20 / 0.46 ms | 0.84 / 0.26 ms |
| Cube tile (on demand) | 4.7 ms | 5.2 ms | 4.7 ms | 4.7 ms |
| Cache write (LZ4) | 348 ms, 753 MB | 476 ms, **2950 MB** | 386 ms, 867 MB | 664 ms, 2241 MB |
| Cache open + base | 4 ms | 4 ms | 4 ms | 6 ms |
| Peak RSS | 1.55 GB | 3.22 GB | 1.68 GB | 3.38 GB |

libjpeg-turbo (`mozjpeg` crate) is ~25 % faster than zune-jpeg on Apple Silicon → keep turbo.

### Navigation (camera path, worst phase)

| Canvas | Layout | fps | longest frame | CPU / frame (p95) | longest wait of a needed tile |
| --- | --- | --- | --- | --- | --- |
| 1600×968 | equirect | 59–60 | 19–46 ms (≤ 1 frame > 25 ms per run) | 4 ms | ≤ 18 ms |
| 1600×968 | cube | 59–60 | 19–40 ms | 3–5 ms | ≤ 19 ms |
| 4800×2904 | equirect | 59–60 | 19–55 ms | 4 ms | 133–167 ms |
| 4800×2904 | cube | 59–60 | 19–62 ms | 3–5 ms | 67–83 ms, but **2.6 s** total time with a missing tile (equirect: 0.5 s) |

- WKWebView caps `requestAnimationFrame` at **60 Hz** (no 120 Hz ProMotion), so 60 fps is the
  ceiling; the JS work per frame stays at 3–5 ms, so there is plenty of headroom.
- GPU timestamps exist in WebKit but are quantised (median values of 0.02 ms for 14 MP are not
  plausible) – only usable as a rough upper bound: p95 ≈ 6–7 ms at 5K with the nadir in view
  (equirect, pole taps), ≈ 2.5 ms for the cube map.
- One-off: in the very first run of the session a single 1.7 s frame occurred (JS work was 6 ms, i.e.
  the webview itself stalled). Not reproducible in 5 further runs; keep an eye on it in E1.4.

### Tile transport

120 level-0 tiles (514×514 px, u8 = 1.06 MB, f16 = 2.11 MB):

| Transport | Format | 1 at a time: latency p50 / p95, throughput | 8 in parallel: throughput |
| --- | --- | --- | --- |
| custom URI scheme | u8 | 1 / 3 ms, 1.1 GB/s | 5.3 GB/s (5000 tiles/s) |
| custom URI scheme | f16 | 1 / 1 ms, 2.3 GB/s | 7.7 GB/s (3600 tiles/s) |
| `ipc::Response` | u8 | 1 / 1 ms, 1.6 GB/s | 5.3 GB/s (5000 tiles/s) |
| `ipc::Response` | f16 | 1 / 1 ms, 2.1 GB/s | 7.2 GB/s (3400 tiles/s) |

Transport is not a bottleneck on macOS. Both mechanisms are equivalent; the URI scheme is preferred
because requests can be aborted (`AbortController`) and server timing comes back as a header. f16 costs
twice the bytes but stays far above what the viewer needs (a 5K screen at 1:1 needs ~60 tiles).

## Windows (i7-12700K, RTX 5080, WebView2 / Edge 155) – first run 2026-10-25

Same code, automatic run with the 283 MP JPEG, 16-bit TIFF and EXR, canvas 1600×1000. WebGPU works
(adapter "nvidia blackwell", `maxTextureArrayLayers` 2048).

| | JPEG | 16-bit TIFF | EXR | Budget |
| --- | --- | --- | --- | --- |
| First view | 970 ms | **3050 ms** | **4658 ms** | < 1 s / < 2 s |
| Sharp | 2422 ms | **3783 ms** | **6843 ms** | < 3 s |
| From cache: first / sharp | 265 / 882 ms | 666 / **1166** ms | 516 / **1750** ms | < 1 s |
| Rust: full decode / pyramid | 1111 / 116 ms | 2169 / 315 ms | 3613 / 492 ms | (macOS: 763/29, 1179/46, 653/54) |
| Navigation | 60 fps (a few frames up to 67 ms in the 1:1 zoom) | 60 fps | 60 fps | 60 fps |
| Longest wait of a needed tile | 1.6 s | 1.9 s | 3.4 s | briefly |

**Tile transport is ~40× slower than on macOS:**

| Transport | u8, 1 at a time | u8, 8 parallel | f16, 8 parallel |
| --- | --- | --- | --- |
| custom URI scheme | 56 MB/s, 11 ms/tile | 144 MB/s, 57 ms/tile | 143 MB/s |
| `ipc::Response` | 92 MB/s, 11 ms/tile | 167 MB/s, 51 ms/tile | 141 MB/s |
| (macOS for comparison) | 1.1–1.6 GB/s, 1 ms | 5.3 GB/s | 7.2–7.7 GB/s |

Interpretation:

- Rendering is not the problem (60 fps, JS work 3–7 ms per frame). The long tile waits at 1:1 (up to
  180 missing tiles) and the slow first view after the decode (base texture of 11–22 MB takes
  120–550 ms to arrive) come from **WebView2's resource/IPC path**, which copies response bodies through
  COM streams. Both Tauri mechanisms use it, so switching between them does not help.
- The Rust side is also 1.5–7× slower (memory-bound steps like the pyramid and the EXR decode are hit
  hardest; Windows Defender scanning large files may add to it). Headless `s1-bench` numbers on Windows
  are still needed to separate CPU/disk from transport.

Follow-up (spike S1b, before E1.4): measure alternative transports on Windows – WebView2 **shared
buffers** (`CreateSharedBuffer` / `PostSharedBufferToScript`, zero-copy, reachable via wry's
`with_webview` and `webview2-com`), a **loopback WebSocket** with a session token, **batching**
several tiles per response, and **compressed tiles** (e.g. LZ4 decoded in the page, or GPU block
compression BC7) to cut bytes. Target: ≥ 1 GB/s or ≤ 5 ms per tile.

## Atlas vs cube map

Both were implemented on the same virtual-texturing core (page table + tile array); the cube variant
generates face tiles on demand from the equirect pyramid.

| | Equirect atlas | Cube map |
| --- | --- | --- |
| Sharpness at the nadir (1:1) | identical to the cube map thanks to **pole taps** (up to 8 samples along u where the equirect is compressed) | uniform sampling, no extra taps needed |
| Extra data / work | none – the tiles are the edit pyramid | a second tile set: either a precomputed cube pyramid (+75 % memory, second resampling = extra blur) or on-demand resampling (~5 ms/tile) |
| Edits (dirty tiles) | one pyramid to invalidate | equirect **and** cube tiles to invalidate and recompute |
| Loading at 5K | 0.5 s total "some tile missing" over the path | 2.6 s (on-demand generation is the bottleneck) |
| GPU cost at the nadir | higher (taps), still small | lower |
| Seam / pole handling | wrap in x (gutter), taps at the poles, tile coverage must include the u footprint | face edges need gutters across faces |

**Decision: equirect atlas.** The cube map's only advantage (uniform sampling at the poles) is
matched by the pole taps, while it adds a second representation that every edit would have to keep in
sync. The cube code is removed after the spike.

## Things learned the hard way (relevant for E1.3/E1.4)

1. **Visible-tile set:** computing it on the CPU with the shader's LOD rule works (3–4 ms per view
   change), but must be **conservative**: every screen sample has to cover its whole grid cell and the
   full u footprint near the poles. Otherwise thin tiles – especially the *partial last column* at the
   seam, which shrinks to a wedge at the nadir – are never requested. Sample spacing must be in CSS px
   (scaled with the pixel ratio), otherwise the CPU cost grows with the display.
2. **Events can overtake responses:** a cache hit emits "ready" before `open_image` returns. The UI
   must buffer status events per session.
3. **Throughput knobs:** 24 requests in flight and 16 uploads per frame keep tile waits below 50 ms at
   1:1; the defaults of 12/6 were too small for a 5K canvas.
4. **Base texture:** a 2048×1024 overview with a full mip chain is always resident – there is never a
   black screen, and it covers all zoomed-out views (levels coarser than ~3000 px are not tiled).
5. **Fade:** 150 ms cross-fade from coarse to sharp per tile (composited in the shader over coarser
   resident levels) makes loading barely noticeable.
6. **Cache size for 16-bit sources:** LZ4 hardly compresses u16 data (2.95 GB for one image). Options
   for E1.3: zstd, delta/predictor before compression, or caching a display pyramid (u8/f16) separately
   from the full-depth working tiles.

## Not covered / open

- **Windows transport** (see the Windows section) – follow-up spike S1b. Peak RSS is not reported on
  Windows yet (use Task Manager); headless `s1-bench` numbers on Windows are missing.
- **Cold file cache** numbers (see caveat above).
- **Comparison with Photoshop/Affinity** – maintainer task [#6](https://github.com/777555777/pano-retouch/issues/6).
- **Colour management**: the viewer shows source values without conversion (ProPhoto/Adobe RGB look
  wrong). Display conversion via a shader LUT is part of E1.3/E1.4 (§4.10).
- **120 Hz** in WKWebView (WebKit preference, possibly reachable via wry) – nice to have.
- **First view for 16-bit TIFF** (1.6 s) could be improved by building the base from strips while they
  are decoded (progressive base) – optional, the budget is met.
- Brush preview / edits are spike S2.

## How to reproduce

See [the spike README](https://github.com/777555777/pano-retouch/tree/spike/s1-tile-viewer/spikes/s1-tile-viewer): `pnpm tauri dev` for
the interactive viewer, `S1_AUTO=… pnpm tauri dev` for the automatic run, `cargo run --release --bin
s1-bench` for the headless Rust numbers.
