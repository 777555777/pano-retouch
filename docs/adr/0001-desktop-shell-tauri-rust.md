# ADR-0001: Desktop shell: Tauri 2 with Rust core and Svelte UI

- Status: Proposed (to be validated by spikes S1, S5)
- Date: 2026-10-10

## Context

- Desktop app for very large images; performance is central.
- The maintainer is fluent in web technologies (TypeScript, Svelte), not in a native language. Code is
  largely written by AI agents but must be reviewed and maintained by the maintainer.
- Platforms: macOS, Windows, Linux.

## Decision

- **Tauri 2** as the app shell.
- **Rust** for the entire compute-heavy core (image I/O, tiling, operations, AI, export), organised as a
  Cargo workspace with UI-independent crates.
- **Svelte 5 + TypeScript** for the UI; **WebGPU** for the viewer and brush preview.
- Data between core and UI: commands/events as JSON, image data strictly binary.

## Alternatives considered

| Alternative | Why not (for now) |
| --- | --- |
| Electron + Node/C++ add-ons | Large binaries, higher RAM use, native add-ons more complicated than Rust in Tauri. |
| Fully native UI in Rust (egui, Iced, Slint) + `wgpu` | Best rendering performance, but UI development far from the maintainer's skills. **Fallback** if S1 fails. |
| Qt/C++ | Powerful, but C++ is hard to review; licensing questions. |
| Go / Zig / C | Weaker ecosystem for GUI + image processing + ONNX (Go), young ecosystem (Zig), no memory safety (C). |
| Pure web app | Browser memory and file-system limits for 1–2 GB images; a desktop app is explicitly wanted. |

## Consequences

- + Web know-how stays useful; Rust provides native performance and safety.
- + Core crates are independent of Tauri → testable, CLI possible later.
- − The webview ↔ Rust boundary is a bottleneck for pixel data → tile/LOD design is mandatory.
- − WebGPU support depends on the system webview (Linux: WebKitGTK uncertain).
- − Rust code is mostly written by agents → high test coverage and clear module boundaries are required.
