# Spike S4 – Lossless metadata round trip (EXIF / XMP / GPano / GPS / ICC)

- Issue: [#4](https://github.com/777555777/pano-retouch/issues/4)
- Date: 2026-10-27
- Code: branch `spike/s4-metadata`, folder `spikes/s4-metadata/` (throw-away)
- Machine: MacBook Pro M4 Max (all numbers below unless noted); Windows build check: see "Windows check"
- Result: **yes** – with a small own metadata layer on top of the image crates; ADR proposal
  [ADR-0005](../adr/0005-metadata-and-colour-conversion.md)

## Question

Can a Rust core read and re-write EXIF, XMP incl. GPano, GPS and ICC for JPEG, TIFF and BigTIFF
(plus EXR header attributes) **without losing anything**, changing only chosen fields (software tag,
dimensions, heading)? Success criteria from the issue:

| Criterion | Result |
| --- | --- |
| `exiftool -a -G1 -s` diff empty except intentionally changed fields | ✅ on all 3 maintainer sets (9 files, 12 cases) and the synthetic suite |
| JPEG, TIFF, BigTIFF; EXR header attributes preserved | ✅ (BigTIFF write + read back; EXR all attributes) |
| ICC profile byte-identical; sRGB conversion path demonstrated | ✅ byte-identical everywhere; `moxcms` and `lcms2` measured against a double-precision reference |
| Recommendation + licence check | ✅ see below and ADR-0005 |

## Short answer

- **No single crate does it all**, but nothing exotic is needed either. Metadata are copied as **raw
  payloads** (EXIF directories field by field as bytes, XMP packet, ICC bytes, Photoshop resources,
  IPTC, unknown JPEG segments) and only the few fields we change are touched. That is ~600 lines of
  Rust in the spike and the same approach PhotoCraft uses (see "Other Rust editors").
- **XMP edits** go through **`xmp_toolkit`** (Adobe's XMP Toolkit): a text patch is byte-preserving but
  broke on the first packet in a different (but valid) serialisation.
- **Colour conversion** for web/Street View presets: **`moxcms`** (pure Rust) in floating-point mode –
  at most 1 code value (8-bit) off a double-precision reference, 1.7–6.4 GP/s. `lcms2` stays the
  reference in tests.
- Findings that need product decisions: **editor documents** stored in TIFFs (Affinity: 535 MB) must be
  dropped on export; **EXIF thumbnails** go stale after edits.

## How it works (spike)

```
source file ──► read metadata as raw payloads ─────────────┐
     │          (EXIF dirs, XMP, ICC, PS resources, IPTC,   │  edit only chosen fields:
     │           other JPEG APPn)                           │  Software, PixelX/YDimension,
     ▼                                                      ▼  GPano sizes, PoseHeadingDegrees
decode pixels ──► (resize, sRGB) ──► encode pixels ──► write payloads into the new container
```

- **EXIF model** (`ifd.rs`): every field is `(tag, type, count, raw bytes)` in the source byte order;
  Exif / GPS / Interop pointers become child directories, the IFD1 thumbnail is a blob. Serialising
  relocates all values; unknown tags survive.
- **JPEG:** the encoder's output gets its APPn segments replaced by ours (JFIF, EXIF, XMP, multi-segment
  ICC, Photoshop APP13, Adobe APP14, COM …); quantisation/Huffman tables and DRI stay the encoder's.
- **TIFF/BigTIFF:** pixels via the `tiff` crate; all non-structural IFD0 tags copied raw (XMP 700,
  ICC 34675, IPTC 33723, Photoshop 34377, unknown tags), Exif/GPS/Interop written as sub-directories;
  compression, predictor and rows per strip as in the source.
- **Cross-format (TIFF → JPEG, the Street View preset):** TIFF layout tags are dropped, IPTC moves from
  tag 33723 into the Photoshop 8BIM resource 0x0404 of APP13.
- **EXR:** `exr` crate reads all attributes and writes them back.
- **Check:** `s4-diff.sh` compares `exiftool -a -G1 -s -n -u` of original and result and the bytes of
  ICC, XMP and thumbnail; TIFF pixels are compared after decoding.

## Results

### Maintainer test sets (private – field names only)

| Case | Changed fields in the exiftool diff (all intended or structural) | ICC | Pixels |
| --- | --- | --- | --- |
| C: Insta360 JPEG (Display P3, **with GPS**) → JPEG | Software | identical | – |
| C: same, half size + heading, XMP via toolkit | Software, ExifImageWidth/Height, PoseHeadingDegrees, XMPToolkit | identical | – |
| B: PTGui/Lightroom JPEG (IFD1 thumbnail, APP14, restart markers, 110 Camera Raw XMP fields) | Software, ThumbnailOffset (thumbnail bytes identical) | identical | – |
| A: PTGui JPEG 283 MP (Adobe RGB, full GPano) | Software | identical | – |
| B: TIFF 16-bit LZW | Software, StripByteCounts | identical | identical |
| A: TIFF 8-bit (PTGui) | Software, StripByteCounts | identical | – |
| A: TIFF edited in Affinity (Deflate, 1 row/strip) | Software, strips, SampleFormat (added by the encoder), **Affinity tags 51168/51169 dropped** | identical | identical |
| A: TIFF 16-bit ProPhoto → **BigTIFF** (+ heading) | Software, StripByteCounts, PoseHeadingDegrees | identical | – |
| A: BigTIFF → classic TIFF, compared with the original | Software, StripByteCounts, PoseHeadingDegrees | identical | identical |
| A: TIFF 16-bit ProPhoto → **sRGB JPEG, half size** | ICC (sRGB), TIFF layout tags, Software, GPano sizes ×½ | replaced | – |
| B: TIFF 16-bit → JPEG (same profile) | TIFF layout tags, Software | identical | – |
| A: EXR half, PIZ (283 MP) → EXR | `software` (intended), `type` + `chunkCount` (written by the `exr` crate) | – | – |

EXIF, GPS (set C), IPTC, Photoshop resources, all XMP namespaces (GPano, Camera Raw, xmpMM, dc,
photoshop …) and the ICC profile arrive unchanged in every case.

### Synthetic suite (publishable, `run-synthetic.sh`)

Synthetic 4096×2048 JPEG (Display P3) and 16-bit RGBA TIFF (Adobe RGB) with full metadata written by
exiftool: EXIF incl. GPS (made-up coordinates), IFD1 thumbnail, PTGui-style GPano incl.
`PoseHeadingDegrees`, IPTC keywords. Diff lines per case (`<` original, `>` result):

| Case | Diff |
| --- | --- |
| 1 JPEG → JPEG, XMP via `xmp_toolkit` | Software; ThumbnailOffset; XMPToolkit `Image::ExifTool 13.55` → `XMP Core 6.0.0` |
| 2 JPEG → JPEG, heading 90° | Software; ThumbnailOffset; PoseHeadingDegrees 42.0 → 90.0 |
| 3 JPEG half size | Software; ThumbnailOffset; GPano FullPano/CroppedArea sizes 4096×2048 → 2048×1024 |
| 4 JPEG, EXIF via `little_exif` | Software; ThumbnailOffset; ExifImageWidth/Height **added** (spike sets them unconditionally) |
| 5 TIFF 16-bit → TIFF | Software (pixels identical) |
| 6+7 TIFF → BigTIFF → TIFF, heading 12.5° | Software; PoseHeadingDegrees 42.0 → 12.5 (pixels identical) |
| 8 TIFF Adobe RGB 16-bit → sRGB JPEG half size | ICC replaced; TIFF layout tags; Software; GPano sizes ×½ — EXIF, GPS, IPTC, XMP kept |

### Speed (metadata vs. pixels)

Reading, editing and serialising the metadata takes **< 1 ms** per file; the time is all in the pixel
codecs (single-threaded encoders in the spike):

| File | Read + decode | Encode + write |
| --- | --- | --- |
| JPEG 71 MP | 0.26 s | 0.43 s |
| JPEG 283 MP | 0.93 s | 1.89 s |
| TIFF 16-bit LZW 85 MP | 2.5 s | 6.7 s |
| TIFF 8-bit LZW 283 MP | 3.0 s | 11.5 s |
| TIFF 8-bit Deflate, 1 row/strip, 283 MP | 3.1 s | **50 s** |
| TIFF 16-bit LZW 283 MP → BigTIFF | 8.9 s | 20.7 s |

→ For E1.6 (export) the TIFF writer needs parallel strip compression (`tiff` compresses strip by
strip on one thread) – a known pattern, not a metadata problem.

### Colour conversion to sRGB (web / Street View presets)

16-bit RGB(A), relative colorimetric, all cores. Error = deviation from a double-precision Little CMS
reference without optimisations (1 M sampled pixels), in 8-bit code values:

| Engine / mode | ProPhoto 283 MP | Adobe RGB 283 MP | Display P3 71 MP |
| --- | --- | --- | --- |
| `lcms2` default (16-bit optimised) | 884 MP/s, max **12**, 3.0 % off | 1135 MP/s, max 2, 1.4 % off | 1327 MP/s, max **7**, 1.2 % off |
| `lcms2` `NO_OPTIMIZE` | 157 MP/s, max 0 | 154 MP/s, max 0 | 143 MP/s, max 0 |
| `moxcms` default (fixed point Q2.13) | 3351 MP/s, max 6, **32 % off** | 7061 MP/s, max 1, 0.3 % off | 5871 MP/s, max 1, 0.05 % off |
| **`moxcms` floating point** | 1759 MP/s, max **1**, 0.25 % off | 6388 MP/s, max 1, 0.3 % off | 5167 MP/s, max 1, 0.04 % off |

Default settings of both libraries trade accuracy for speed in ways that matter for 16-bit sources;
`moxcms` in floating-point mode is both accurate and fast.

## Other Rust editors – what they use for metadata

| Project | Metadata approach |
| --- | --- |
| [PhotoCraft](https://github.com/storytold/photocraft) | Own code, raw payloads: `Metadata { exif: raw TIFF bytes, xmp: String, … }`, ICC separate; JPEG APPn scan/write by hand, EXIF in TIFF via its own IFD reader on top of `tiff` 0.11; trims Photoshop "layered document" XMP when a packet doesn't fit. Colour management: own `photocraft-cms` crate on **`moxcms`**. Its TIFF EXIF writer leaves out the Interop directory. |
| [RapidRAW](https://github.com/CyberTimon/RapidRAW) (Tauri) | `kamadak-exif` to read, `little_exif` to write EXIF tag by tag into exported JPEGs (rebuilds EXIF from RAW values – fine for RAW development, not a lossless copy). |
| [Oculante](https://github.com/woelper/oculante) | `kamadak-exif` to read, `img-parts` to copy the EXIF blob into saved files; `lcms2` only via `jxl-oxide`. |
| [Graphite](https://github.com/GraphiteEditor/Graphite) | `image` crate only – no metadata round trip. |
| [Fotema](https://github.com/blissd/fotema) (GNOME) | `kamadak-exif`, read-only. |

Nobody in the Rust ecosystem ships a "copy everything, change three fields" library; the projects that
care (PhotoCraft) do what this spike does.

## Candidates

| Crate | Licence | Verdict |
| --- | --- | --- |
| Own raw IFD/segment layer (spike `ifd.rs`, `meta.rs`) | ours | **Use.** Lossless by construction, works for JPEG, TIFF, BigTIFF and cross-format, integrates with the `tiff` encoder. |
| `xmp_toolkit` 1.12 (Adobe XMP Toolkit, C++) | MIT/Apache-2.0, SDK BSD-3-Clause | **Use for XMP edits.** Robust against all XMP serialisations; rewrites the packet (values preserved, `XMPToolkit` changes). +9 s clean build on the Mac; builds with MSVC on Windows. |
| XMP text patch (spike) | ours | Byte-preserving but fragile: failed on ExifTool-style packets (single quotes, element form) until fixed. Not for the product. |
| `little_exif` 0.6 | MIT/Apache-2.0 | Works on JPEG (all fields kept here), but re-parses and rewrites the whole file, writing "only works for JPGs", typed tag API. Not needed. |
| `kamadak-exif` 0.6 | BSD-2-Clause | Reader only (+ experimental writer); useful for **displaying** EXIF in the info panel. |
| `img-parts` 0.4 | MIT/Apache-2.0 | JPEG/PNG/WebP segment handling; our splice is ~60 lines, no TIFF support. Not needed. |
| `moxcms` 0.9 | BSD-3-Clause/Apache-2.0 | **Use for colour conversion** (float mode). Pure Rust, also used by `image` and PhotoCraft. |
| `lcms2` 6.2 (Little CMS 2) | MIT | **Test reference** (double precision, `NO_OPTIMIZE`); fallback if `moxcms` lacks a profile type. |
| `tiff` 0.11, `exr` 1.74, `zune-jpeg`, `jpeg-encoder` | MIT; BSD-3; MIT/Apache/Zlib; MIT/Apache + IJG | Pixel codecs, already planned. |
| `exiftool` (test tool only) | Artistic/GPL | Test oracle in CI, not shipped. |

All licences are compatible with AGPL-3.0.

## Things learned

1. **Raw payloads beat typed tag models.** Anything that parses into known tags and writes them back
   loses what it doesn't know. Copying bytes and patching three fields is simpler and lossless.
2. **TIFF files can carry whole editor documents.** Affinity stores its document in tags 51168/51169
   (535 MB in set A; Photoshop uses 37724). They describe the pre-edit image → drop on export.
3. **EXIF thumbnails go stale.** Set B carries a 10 KB IFD1 thumbnail; after retouching it shows the
   old image. Same for the Photoshop thumbnail resource (0x040C).
4. **BigTIFF metadata is poorly supported by tools.** exiftool names BigTIFF sub-directories after the
   pointer tag and decodes GPS with the wrong table; verify BigTIFF output by converting back to classic
   TIFF (as done here) and with libtiff (`tiffinfo` reads it).
5. **Default CMS settings are not good enough for 16-bit.** Both libraries' fast paths deviate by up to
   6–12 code values on ProPhoto sources.
6. **Test data corrections:** set C (Insta360) **contains GPS** (06-test-data said "no GPS"); the set A
   EXR's chromaticities are **Adobe RGB** primaries (not Rec.709).

## Windows check

Done by the maintainer on the Windows benchmark machine, in a Developer PowerShell for VS:

- `cargo build --release` succeeds with MSVC, including `xmp_toolkit` (C++) and `lcms2-sys` (C).
- `cargo test --release`: 3/3 passed (both XMP engines, including the Adobe XMP Toolkit at runtime).
- `icc` on the set C JPEG (Display P3, 71 MP) gives the same deviations as on the Mac: `lcms2` default
  8-bit max 7, `moxcms` (fixed point, the spike default) 8-bit max 1. Speed 837 MP/s (`lcms2`) and
  760 MP/s (`moxcms` fixed point).

## Not covered / open

- **ExtendedXMP** (packets > 64 KB in JPEG): none in the test data; `xmp_toolkit`'s file API handles
  it, the segment writer must split/merge it.
- **MakerNotes**: relocating EXIF can break maker-specific offsets. Stitched panoramas in the test data
  have none; camera JPEGs (single-shot 360° cameras) may → keep MakerNote blocks at their offset or
  copy the EXIF block unchanged when no EXIF field changes.
- Big-endian TIFF, CMYK, Radiance HDR (no metadata beyond a header) and DNG were not tested.
- **Display colour management** (monitor profile in the webview, architecture §4.10) is a separate
  topic for the viewer epic.

## Recommendation (→ ADR-0005)

1. A `pano-meta` module in the I/O crate: raw payload model (EXIF dirs, XMP, ICC, Photoshop resources,
   IPTC, other JPEG segments), writers for JPEG, TIFF/BigTIFF and EXR, cross-format mapping.
2. XMP edits through `xmp_toolkit`; GPano fields kept consistent with the exported size; heading via
   `GPano:PoseHeadingDegrees`.
3. Export changes only: Software, dimensions (EXIF PixelX/YDimension, XMP `tiff:`/`exif:` sizes, GPano
   sizes), heading when the user changes it; drops editor documents; regenerates or drops thumbnails
   (decision pending).
4. sRGB conversion with `moxcms` (float); `lcms2` double precision as reference in tests.
5. Round-trip tests with exiftool diff in CI on the synthetic suite (no private data needed).

## How to reproduce

```bash
git switch spike/s4-metadata
cd spikes/s4-metadata && cargo build --release
./run-synthetic.sh /tmp/s4                         # synthetic suite (exiftool + sips)
./target/release/s4-metadata rt in.tif out.jpg --half --srgb mox
./s4-diff.sh in.tif out.jpg
./target/release/s4-metadata icc in.tif            # lcms2 vs moxcms vs reference
```
