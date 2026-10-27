# ADR-0005: Metadata round trip and colour conversion

- Status: Proposed – recommendation from [spike S4](../spikes/s4-metadata.md) (macOS; Windows build check open)
- Date: 2026-10-27

## Context

- Lossless metadata is a product goal (vision G4, functional spec F-05): EXIF, XMP incl. GPano, GPS,
  IPTC and the ICC profile must survive an export; only deliberately changed fields may differ.
- Sources are PTGui/Lightroom/Affinity/Insta360 outputs in JPEG, TIFF (up to 1.9 GB, BigTIFF beyond
  4 GB) and EXR; the Street View preset turns a TIFF into an sRGB JPEG.
- The architecture named `lcms2` for colour management and left the metadata library open
  (`xmp_toolkit`, `little_exif`, `kamadak-exif`).

## Decision (proposal)

1. **Own raw-payload metadata layer** in the I/O crate instead of a typed tag library:
   - EXIF as directories of `(tag, type, count, raw bytes)` with Exif/GPS/Interop children and the IFD1
     thumbnail as a blob; unknown tags survive by construction.
   - XMP packet, ICC bytes, Photoshop image resources, IPTC and all other JPEG APPn/COM segments are
     carried verbatim.
   - Writers for JPEG (segments spliced into the encoder output), TIFF/BigTIFF (tags written through the
     `tiff` encoder) and EXR (all header attributes); cross-format mapping for TIFF → JPEG (IPTC into
     the Photoshop resource 0x0404, TIFF layout tags dropped).
2. **XMP edits via `xmp_toolkit`** (Adobe XMP Toolkit; MIT/Apache wrapper, BSD-3 SDK).
3. **Export changes only:**
   - the software tag;
   - the dimensions (EXIF `PixelX/YDimension`; XMP `tiff:`/`exif:` sizes and GPano `FullPano*` /
     `CroppedArea*`, all scaled with the image);
   - `GPano:PoseHeadingDegrees` when the user changes the heading.
4. **Editor documents are dropped on export:** Photoshop `ImageSourceData` (37724) and Affinity
   (51168/51169) describe the pre-edit image. Stale thumbnails: see open points.
5. **Colour conversion with `moxcms` in floating-point mode** (pure Rust, BSD-3/Apache), used for
   export presets that convert to sRGB. `lcms2` (Little CMS, MIT) at double precision serves as the
   reference in tests.
6. **Round-trip tests** in CI: `exiftool` diff on synthetic files with full metadata, plus TIFF pixel
   equality.

## Alternatives considered

- **`little_exif` for EXIF:** worked on the test JPEGs, but rewrites the whole file and only supports
  writing JPEG. Its typed tag model doesn't help with "copy everything".
- **`kamadak-exif`:** reader (+ experimental writer); good for *displaying* EXIF, not for writing.
- **`img-parts`:** JPEG segment container only, no TIFF. Our splice is a few dozen lines.
- **XMP text patch:** keeps the packet byte-identical, but broke on a valid ExifTool-style packet.
  Too fragile for arbitrary sources.
- **`lcms2` for conversions:** exact with `NO_OPTIMIZE` but 6–40× slower than `moxcms`. Its default
  16-bit path deviates by up to 12 code values on ProPhoto sources. It is also a C dependency.

## Consequences

- ~600 lines of own, well-tested metadata code, the same approach PhotoCraft takes. No dependency on
  a typed EXIF library.
- One C++ build dependency (`xmp_toolkit`) – Windows/Linux builds must be verified (Windows check
  pending from the spike).
- Export code must keep GPano consistent with every size change (presets, crops later).
- Architecture §2 (stack), §4.9 (export) and §4.10 (colour management) are updated accordingly.

## Open points

- **Thumbnails:** the EXIF IFD1 thumbnail and the Photoshop thumbnail resource (0x040C) show the pre-edit
  image. Regenerate them from the pyramid, or drop them? (Product decision, E1.6.)
- **Date fields on export:** update `ModifyDate` / `xmp:MetadataDate`, or keep them? (Product decision.)
- **sRGB profile for web presets:** Little CMS's built-in v4 profile, or a compact v2 sRGB profile for
  maximum viewer compatibility?
- **ExtendedXMP** (JPEG XMP > 64 KB) and **MakerNote** offsets: not present in the test data; handle in E1.6
  (keep MakerNote blocks at their offset / copy the EXIF block unchanged when no EXIF field changes).
- Display-side colour management (monitor profile in the webview) – viewer epic.
