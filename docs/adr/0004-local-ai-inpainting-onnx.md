# ADR-0004: Local AI inpainting via ONNX Runtime

- Status: Proposed – recommendation from [spike S3](../spikes/s3-inpaint.md) (macOS + Windows, maintainer-reviewed); ready to be accepted
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

## Spike S3 (2026-10-26)

- **No method wins on every scene** (maintainer review): LaMa + full-resolution texture pass is best on
  asphalt and good on snow, PatchMatch + texture pass best on uniform snow. → **Offer both:** LaMa +
  texture as the default when the optional model is installed, PatchMatch + texture without a model; the
  user can switch per region. Plain LaMa (512 px model) and plain PatchMatch are too soft at 300 MP.
- **Before shipping:** a less repetitive texture pass and an invisible boundary (gradient-domain blending,
  irregular mask edge) – the perfect circle of the cap stood out in three of four scenes.
- **MI-GAN not offered for now** (smears or hallucinates), re-evaluate on other scenes later.
- **Execution:** `ort` 2.0 / ONNX Runtime 1.31 on the **CPU**: CoreML is slower and needs 9 GB (note: `ort`
  registers CoreML by default unless the CPU policy is set); DirectML fails at run time on LaMa's FFT
  block. Per region LaMa + texture ~1.5–5 s (M4 Max) / ~7.4 s (i7-12700K), + model load 3 s / 7–8 s once.
- **HDR:** exposure from the context median, Reinhard + gamma, exact inverse – fill within a few percent
  of the surroundings, no colour cast.
- **Licences:** LaMa Apache-2.0, MI-GAN MIT; both trained on Places2/Places365 (non-commercial image
  terms, effect on weights unsettled) → models stay an optional download with a notice in the dialog.
