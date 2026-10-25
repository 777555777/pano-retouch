# Spike S1 – tile viewer (throw-away code)

Prototype for [issue #1](https://github.com/777555777/pano-retouch/issues/1): can Tauri 2 + WebGPU in the
system webview show a 300 MP panorama smoothly when the tiles come from Rust? Results and the
recommendation are in [docs/spikes/s1-tile-viewer.md](../../docs/spikes/s1-tile-viewer.md).

**This is spike code.** It lives only on the `spike/s1-tile-viewer` branch and will not be merged into
the product; the real implementation starts with E1.3/E1.4.

## What is in here

| Part | File | What it does |
| --- | --- | --- |
| Decoders | `src-tauri/src/decode.rs` | JPEG via libjpeg-turbo (`mozjpeg`, incl. 1/8 DCT-scaled preview) or zune-jpeg; striped TIFF decoded in parallel per strip; EXR via `exr` |
| Pyramid & tiles | `src-tauri/src/pyramid.rs` | Levels by 2×2 averaging, 512 px tiles with 1 px gutter (x wraps, y clamps), 2048×1024 base texture with mips, cube-face tiles generated on demand, u8/f16 wire conversion |
| Disk cache | `src-tauri/src/cache.rs` | One LZ4 pack file per level in the OS cache dir (`…/Caches/pano-retouch-spike-s1/`) |
| Session | `src-tauri/src/session.rs` | Open flow: probe → fast base → full decode → pyramid → cache write, progress events |
| Transport | `src-tauri/src/lib.rs` | Custom URI scheme `tile://` and `ipc::Response` (both binary) |
| Viewer | `src/viewer.wgsl`, `src/renderer.ts` | Full-screen WebGPU pass, virtual texturing with page table + tile array, fade-in, pole taps, level tint |
| Scheduler | `src/tiles.ts` | Visible-tile set (same LOD rule as the shader), priority queue, prefetch, stale-request dropping, slot LRU |
| Benchmarks | `src/main.ts`, `src-tauri/src/bin/s1-bench.rs` | In-app benchmarks (open, camera path, transport) and a headless Rust benchmark |

## Spike S1b – Windows transport ([issue #14](https://github.com/777555777/pano-retouch/issues/14))

On Windows, WebView2 delivered only ~150 MB/s through the custom URI scheme and `ipc::Response`. S1b adds:

| Transport | How | Where |
| --- | --- | --- |
| `protocol` | custom URI scheme `tile://` (S1) | all |
| `ipc` | `ipc::Response` (S1) | all |
| `ws` | loopback WebSocket on 127.0.0.1, random port, session token | all |
| `shared` | WebView2 shared buffers: a ring of 48 tile slots + one 32 MB slot is posted to the page once; Rust writes tiles straight into that memory, only a tiny message goes through IPC | Windows only |

Also new: tiles are built in one pass directly in the wire format, mimalloc as allocator, the
transport benchmark includes a "null" payload (pure transport, no tile work) and batches of 8 tiles,
and peak memory is reported on Windows.

## Running

Requirements: Rust (stable), Node 20+, pnpm. Test images are read from `PANO_TESTDATA`
(default: `testdata-local/` in the repo root).

```bash
cd spikes/s1-tile-viewer
pnpm install
pnpm tauri dev
```

Pick an image, press **Open**. Drag = look around, wheel = zoom, `1` = 1:1, `n` = nadir, `0` = horizon,
`d` = level tint, `h` = hide UI. The panel on the right shows fps, tile counts, tile waits and memory.

### Automatic benchmark run

The app can run all benchmarks by itself and quit – useful for comparing machines:

```bash
S1_AUTO="Panorama edited.jpg,Panorama 16bit.tif,Panorama_hdr.exr" pnpm tauri dev 2>&1 | tee s1-run.log
```

| Variable | Meaning |
| --- | --- |
| `S1_AUTO` | comma-separated parts of file names under `PANO_TESTDATA` |
| `S1_AUTO_LAYOUTS` | `equirect` (default), or `equirect,cube` |
| `S1_AUTO_TRANSPORTS` | `protocol,ws,shared` (default; unavailable ones are skipped) |
| `S1_SCALE` | render scale instead of the display's pixel ratio (e.g. `3` ≈ 5K canvas) |
| `S1_TUNE` | UI overrides, e.g. `inflight=24,uploads=16,slots=512` |
| `S1_SNAP_DIR` | write canvas snapshots there (only when set; **they show your images – keep them local**) |

The last log line `[ui] AUTO RESULT {…}` contains all numbers as JSON.

Headless Rust benchmark (decode, pyramid, tile extraction, cache):

```bash
cd src-tauri && cargo run --release --bin s1-bench -- "<path to image>"
```

### Windows

1. Install the [Tauri prerequisites](https://tauri.app/start/prerequisites/) (MSVC build tools; WebView2 is
   part of Windows 11) and Rust via `rustup`.
2. Install NASM (`winget install NASM.NASM`), otherwise libjpeg-turbo builds without SIMD and JPEG
   decoding is several times slower.
3. In PowerShell:

```powershell
cd spikes\s1-tile-viewer
pnpm install
$env:PANO_TESTDATA="D:\path\to\testdata-local"
$env:S1_AUTO="Panorama edited.jpg,Panorama 16bit.tif,Panorama_hdr.exr"
pnpm tauri dev 2>&1 | Tee-Object s1-run-windows.log
```

Send back the `AUTO RESULT` line (it contains no image data).
