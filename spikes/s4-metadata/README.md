# Spike S4 – lossless metadata round trip (throw-away code)

Question (#4): can we read and re-write EXIF, XMP incl. GPano, GPS and ICC for JPEG, TIFF and
BigTIFF (plus EXR header attributes) without losing anything, changing only chosen fields?

Report: [docs/spikes/s4-metadata.md](../../docs/spikes/s4-metadata.md) (on `main`).

## What is in here

| File | Purpose |
| --- | --- |
| `src/ifd.rs` | Minimal TIFF/EXIF directory model: every field kept as raw bytes, Exif/GPS/Interop as child directories, IFD1 thumbnail as a blob |
| `src/meta.rs` | Format-neutral metadata (EXIF dirs, XMP, ICC, Photoshop resources, IPTC, other JPEG segments); JPEG APPn read/write/splice |
| `src/tif.rs` | TIFF/BigTIFF via the `tiff` crate, all non-structural tags copied raw, editor documents dropped |
| `src/xmp.rs` | XMP edits with `xmp_toolkit` (Adobe XMP Toolkit) or a minimal text patch |
| `src/icc.rs` | Source profile → sRGB with `lcms2` or `moxcms`, plus a double-precision reference |
| `s4-diff.sh` | `exiftool -a -G1 -s -n -u` diff, byte comparison of ICC/XMP/thumbnail |
| `make-synthetic.sh`, `run-synthetic.sh` | Synthetic files with full metadata and the round-trip suite (publishable output) |

## Commands (macOS)

```bash
cargo build --release
cargo test --release
./run-synthetic.sh /tmp/s4            # needs exiftool and sips (macOS)
./target/release/s4-metadata rt <in.jpg|tif> <out.jpg|tif> [--half] [--heading 90] [--srgb lcms|mox] [--xmp toolkit|text] [--exif own|little] [--bigtiff|--classic]
./target/release/s4-metadata icc <file>   # lcms2 vs moxcms vs double reference
./target/release/s4-metadata exr <in.exr> <out.exr>
```

## Windows check

Only the C/C++ dependencies are platform-specific (`xmp_toolkit` builds the Adobe XMP Toolkit in
C++, `lcms2` builds Little CMS in C); everything else is plain Rust. In a *Developer PowerShell for
VS* (MSVC toolchain):

```powershell
cd code\pano-retouch\spikes\s4-metadata
git pull
cargo build --release
cargo test --release
.\target\release\s4-metadata.exe icc "..\..\testdata-local\Insta360X5\<file>.jpg"
.\target\release\s4-metadata.exe rt "..\..\testdata-local\Insta360X5\<file>.jpg" "$env:TEMP\s4.jpg" --half --heading 90
```
