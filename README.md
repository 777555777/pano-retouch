# pano-retouch

An open-source desktop editor for **retouching 360° panoramas** (equirectangular): remove tripods, add
nadir/zenith patches, retouch away distracting objects – directly on the sphere, assisted by local AI
inpainting, with batch processing for whole tours, and fast even with 300-megapixel images.

> **Status: concept phase.** There is no application code yet. This repository contains the product
> vision and the functional, technical and UX specifications from which the software will be built
> (largely by AI agents, reviewed by the maintainer). Work is tracked in
> [issues and milestones](../../milestones).
>
> Licence: [AGPL-3.0](LICENSE).

## Where it fits

```
Camera ──► Stitching / alignment ──► Colour / exposure ──► ★ pano-retouch ★ ──► Tour software / Street View
           (PTGui, camera app)        (Lightroom)             retouch, nadir,        (Pano Publisher,
                                                              batch                  3DVista, Pano2VR …)
```

## Documentation

| Document | Contents |
| --- | --- |
| [docs/01-vision.md](docs/01-vision.md) | Vision, problem, personas, goals, non-goals, principles |
| [docs/02-functional-spec.md](docs/02-functional-spec.md) | Use cases, feature catalogue with priorities, domain model, glossary |
| [docs/03-architecture.md](docs/03-architecture.md) | Technical design: stack, components, image pipeline, budgets, risks |
| [docs/04-roadmap.md](docs/04-roadmap.md) | Milestones M0–M6, epics |
| [docs/05-decisions.md](docs/05-decisions.md) | Decision log of the concept phase |
| [docs/06-test-data.md](docs/06-test-data.md) | Test data and benchmarks |
| [docs/07-tools-and-layers.md](docs/07-tools-and-layers.md) | Scope of tools, selection and layers per version |
| [docs/08-ux-concept.md](docs/08-ux-concept.md) | UX concept: layout, interaction patterns, components, states, shortcuts |
| [docs/adr/](docs/adr/) | Architecture decision records |
| [mockups/](mockups/README.md) | Clickable UX prototypes v1–v3 |
| [AGENTS.md](AGENTS.md) | Working rules for AI agents (and humans) contributing |

## Planned stack

Tauri 2 · Rust core (tiling, colour management, ONNX inference) · Svelte 5 + TypeScript · WebGPU.
See [ADR-0001](docs/adr/0001-desktop-shell-tauri-rust.md).
