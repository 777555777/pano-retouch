# ADR-0004: Local AI inpainting via ONNX Runtime

- Status: Proposed (model choice open, spike S3)
- Date: 2026-10-10

## Context

- Tripod removal is the most important use case; AI inpainting gives the best results here for the least effort.
- Principles: local, offline, no cloud requirement, open source.
- Models work on flat images of limited resolution, mostly 8-bit sRGB.

## Decision (proposal)

- Inference in the Rust core via **ONNX Runtime** (`ort`) with GPU providers (CoreML, DirectML, CUDA) and
  CPU fallback.
- Models are **exchangeable** via a small model manifest (name, version, input format, resolution,
  licence, checksum, download URL).
- The region is **reprojected gnomonically** before inference and reprojected back afterwards; the
  original outside the mask stays unchanged.
- Models are **not shipped in the installer**. They are downloaded (with size and licence notice) –
  either **optionally on first start** (onboarding step "download AI models now?", since unsigned builds as
  a plain DMG/ZIP have no real install step), **on first use** with one click, or via manual import. Reasons:
  - The installer stays small; models can be updated independently of app releases.
  - Models are not part of our distribution → the project licence (AGPL-3.0) and model licences affect
    each other less. Still: only include models whose licences allow free use, including commercial use
    by users.
- **The app stays fully usable without a model:** classic inpainting (PatchMatch-like) as a built-in
  fallback; clone stamp and patches work anyway. The UI offers the model download where it is needed
  ("Better results with AI model – download 200 MB?").
- **Starting candidate: LaMa** (good on ground and textures, i.e. the tripod case; fast; verify the licence
  before inclusion). Further candidates for the spike: Big-LaMa, MI-GAN. Document each model's licence in
  the manifest.

## Alternatives considered

- **Cloud API** (optional): contradicts "local & private"; later at most as an explicit opt-in.
- **Classic inpainting** (PatchMatch/Telea): no model needed, but much weaker for large areas.
  → Useful as the **fallback** without a model.
- **Diffusion models:** potentially better quality, but large, slow, hard to ship cross-platform.
  Evaluate later.

## Consequences

- + Good quality, offline, exchangeable models.
- − ONNX Runtime increases the app size (native library per platform).
- − 16-bit/HDR requires tone mapping before and detail/colour restoration after inference.
