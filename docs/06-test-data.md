# 06 – Test Data & Benchmarks

Real test images are the basis for the tech spikes (M0) and later for golden image tests and
performance benchmarks.

## 1. Maintainer's local test data (`testdata-local/`, not in Git)

These sets stay **local only** (they show private locations). Analysed on 2026-10-11. Only set C
contains GPS data (found in spike S4) – never publish its values. Agents working on the maintainer's machine can use them via `PANO_TESTDATA`
(see section 4); in CI and for other contributors they are not available.

### Set A – Sony A7R4 + Sigma 14 mm (`Pano_A7R4-Sigma-14mm/`) – "the big one"

Snowy garden. **23798 × 11899 px = 283 MP.** Nadir: hole (transparent) with tripod legs, rig parts and
a green spirit level at the edge; cleanly filled with snow in the edited version.

| File | Format | Findings |
| --- | --- | --- |
| `Edit/_DSC5395_8bit Panorama.tif` | TIFF, **8-bit** RGBA (associated alpha), LZW + predictor, strips of 105 rows, 450 MB | PTGui 12.27, **Adobe RGB (1998)**, alpha = hole at the nadir |
| `Edit/_DSC5395_16bit Panorama.tif` | identical to the 8-bit file: **also only 8-bit** | see note below |
| `Edit/_DSC5395 Panorama edited.tif` | TIFF, 8-bit RGBA, Deflate, 1 row per strip, 960 MB | edited with **Affinity 3.0.2**, Adobe RGB; carries the 535 MB Affinity document in private tags 51168/51169 |
| `Edit/_DSC5395 Panorama edited.jpg` | JPEG baseline, 4:4:4, **no restart markers**, 67 MB | Adobe RGB, full GPano fields |
| `Edit/_DSC5395 Panorama_hdr.exr` | EXR, RGBA **half**, PIZ, scanlines, 828 MB | chromaticities = **Adobe RGB** primaries, D65 white (linear) |
| `Edit/_DSC5395 Panorama_hdr.hdr` | Radiance RGBE 32-bit, 1.1 GB | PTGui |
| `Edit-2/_DSC5395 Panorama 16bit.tif` | TIFF, **true 16-bit** RGBA, LZW, strips of 52 rows, **1.7 GB**, 23800×11900 | **ProPhoto RGB**, re-stitched from 16-bit sources |
| `Edit-2/_DSC5395 Panorama 8bit.tif` | TIFF, 8-bit RGBA, LZW, 387 MB, 23800×11900 | ProPhoto RGB; 8-bit counterpart (8-bit + ProPhoto tends to banding – good test for correct display) |
| `Edit/Stitching/` | 42 source TIFFs (8-bit, 61 MP, Adobe RGB) + PTGui project | not relevant (stitching) |
| `RAW/` | 60 × ARW + Lightroom XMP | not relevant (RAW development is a non-goal) |

### Set B – Sony A7M2 + Samyang 12 mm fisheye (`Pano_A7M2-Samyang-Fisheye-12mm/`)

Summer field path with the **sun and a starburst** and a **large, sharp tripod and photographer shadow
on asphalt**. **13000 × 6500 px = 85 MP.** Nadir: jagged hole (fisheye image circles) with rig parts.

| File | Format | Findings |
| --- | --- | --- |
| `Edit/BC_07864_Pano_41.tif` | TIFF, **16-bit** RGBA, LZW, 547 MB | PTGui 12.11, **sRGB** |
| `Edit/BC_07864_Pano_41 edited.jpg` | JPEG 4:4:4, **with restart markers**, 60 MB | sRGB, GPano |
| `Edit/BC_07864_Pano_41 edited.psd` | Photoshop, 948 MB | reference only (PSD is not supported) |
| `Edit/BC_07864_Pano_41.hdr` | Radiance RGBE, 338 MB | HDR with sun |

### Set C – Insta360 X5 (`Insta360X5/`)

Snowy field path with blue barriers. **11904 × 5952 px = 71 MP.**

| File | Format | Findings |
| --- | --- | --- |
| `IMG_…_B2.jpg` | JPEG 4:4:4, no restart markers, 24 MB | **Display P3**, GPano only `ProjectionType` |
| `IMG_…_A.dng` (2×) | DNG "linear raw" 16-bit, uncompressed, 405 MB | raw data → non-goal; not supported |

## 2. Findings for product and technology

1. **Four colour spaces** (Adobe RGB, ProPhoto RGB, sRGB, Display P3) in just three sets → colour
   management via embedded ICC profiles is mandatory from version 0.1 (functional spec F-14).
2. **All PTGui outputs have an alpha channel** marking holes at the nadir → must be preserved and can be
   used directly as a "fill holes" mask (F-09).
3. **No test image is tiled.** TIFFs are striped (decodable in parallel), EXR uses scanlines. Large
   JPEGs **without restart markers** can only be decoded sequentially → on first open the decode time is
   the lower bound; the cache makes every later open fast.
4. **macOS reference** (`sips`, 2400 px preview): 283 MP JPEG in **0.6 s**, 283 MP TIFF in **2.7 s**.
   Our "first view < 1 s" budget is realistic for JPEG, ambitious for large TIFFs.
5. **Typical retouching cases are well covered:** hole + rig parts at the nadir (A, B), large shadow on
   textured ground (B, ideal for "shadow lighten"), sun/HDR (B), before/after pairs for golden/comparison
   tests (A, B).
6. Memory for set A at full resolution: RGBA 8-bit ≈ 1.1 GB, RGBA 16-bit/half ≈ 2.3 GB, RGBA f32 ≈ 4.5 GB.

**Workflow note (not part of the software):** a "16-bit" panorama is only 16-bit if the whole chain is:
export 16-bit TIFFs from Lightroom and choose 16-bit output in PTGui.

## 3. Still missing (capture when convenient)

| ID | What | How |
| --- | --- | --- |
| T3 | **32-bit EXR** (float, not half) with sun | Export set B from PTGui as EXR (float if available). |
| T5 | **Series** of 10–20 panoramas of one place (rig orientation may vary) | e.g. a walk with the Insta360 X5 or the rig; JPEG output. |
| T6 | **Drone panorama** with missing zenith | drone app output or PTGui with a hole. |
| T7 | **Second shot at the same location** | (a) rig rotated by ~90°, (b) offset ground shot without tripod. |
| T8 | **Defect images** | lens flares, sensor dust (e.g. f/16 against the sky), people/licence plates, stitching errors. |
| P1 | **Public test set** | a few panoramas from non-private locations that may be published (CI, issues, PRs). |

## 4. Rules for a public repository

- `testdata-local/` and review screenshots (`mockups/**/review/*.png`) are excluded via `.gitignore`.
- Test code finds large test images via the environment variable **`PANO_TESTDATA`**; tests that need
  them are skipped when it is not set. Later, a public test set (P1) is provided as a download (e.g.
  GitHub release asset) for CI.
- **Sets A–C never leave the maintainer's machine.** No screenshots of them in public issues/PRs –
  use public or synthetic images there.
- The repository itself only contains small, **synthetically generated** test images (grid,
  checkerboard at the poles, gradients) created by a script.

## 5. Benchmark against Photoshop / Affinity

Measure once with set A (JPEG + TIFF) and set B (16-bit TIFF) (stopwatch or screen recording is enough):

| Scenario | Photoshop | Affinity 3 | pano-retouch |
| --- | --- | --- | --- |
| Open file until the image is usable | | | |
| Zoom/pan at 100 % – smooth? (subjective 1–5) | | | |
| Clone stamp stroke: visible lag? (subjective 1–5) | | | |
| Content-aware fill on the tripod area: duration | | | |
| Save as JPEG / TIFF: duration | | | |
| Retouch the nadir incl. reprojection back and forth: total time | | | |
