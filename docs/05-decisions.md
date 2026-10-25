# 05 – Decision Log

Product decisions made during the concept phase (October 2026), in the order they were made, with the
document they were carried into. Architecture decisions live in the [ADRs](adr/).

**New questions** that come up during implementation are tracked as GitHub issues with the label
`question`. Once answered, the decision is added here and to the affected document.

## Round 1 – Fundamentals

| # | Topic | Decision | Carried into |
| --- | --- | --- | --- |
| 1 | Name | `pano-retouch` for now. | – |
| 2 | Licence | AGPL-3.0, unless AI model licences speak against it (checked in spike S3). | README, ADR-0004 |
| 3 | Nadir patches | Own logos/graphics. Fitting a ground photo = later. | Vision, functional spec R-08 |
| 4 | Platforms | macOS first, Windows aiming for parity, Linux optional. | Vision |
| 5 | Resolutions | Handle 100–300 MP well; performance has very high priority. | Vision G5, architecture, ADR-0003 |
| 6 | Pano Publisher | No integration for now; shared libraries conceivable later. | – |
| 7 | Viewer rendering | Measured in spike S1 and compared for usability. | Roadmap S1 |
| 8 | Working format | Store source bit depth, compute in f32, display in f16. | ADR-0003 |
| 9 | Inpainting model | Start with LaMa, exchangeable; compared in spike S3. | ADR-0004 |
| 10 | Model distribution | Optional download on first start, on demand or manual import; classic fallback without a model. | ADR-0004 |
| 11 | Edit storage | JSON files in a hidden `.pano-retouch/` folder next to the images; cache central and local. | Architecture 4.7, ADR-0002 |
| 12 | CLI / MCP | Not now, but prepare the architecture. | Vision principle 7, functional spec B-07/B-08 |
| – | Layers | Retouch layers + image layers from shots of the same location. Text/SVG conceivable later. | Vision, functional spec H-02–H-05, R-13/14 |
| – | HDRI | HDRI photographers as a persona; EXR/32-bit float is a must. | Vision, functional spec UC-6, F-04, V-07 |

## Round 2 – UX, workflow, hardware, project

| # | Topic | Decision | Carried into |
| --- | --- | --- | --- |
| A1 | UX role models | Figma (instant response, snapping, clean), Photoshop tools, Maya marking menus, krpano/LizardQ viewers; counter-example: Google Maps tile loading. | Vision "UX principles", architecture 4.3 |
| A2 | Workspace | Sphere view; flat view switchable + mini-map. | Vision, functional spec V-08 |
| A3 | Input | Mouse first; pen later. | Functional spec U-06 |
| A4 | Shortcuts | Photoshop-like. | Functional spec U-01 |
| A5 | HDR display | Exposure slider + tone mapping is enough. | Functional spec V-07 |
| A6 | Appearance | Dark mode only. | Vision |
| B1 | Lightroom | Folder-based workflow, no "external editor" round trip. | Functional spec Won't |
| B3 | Image management | Folder + simple status. | Functional spec F-12 |
| B4 | Storage | Mostly internal SSD; export target freely selectable (external, NAS). | Functional spec F-08 |
| B5 | Default export | Same format as source, folder `export/`, suffix `_retouched`; configurable later. | Functional spec F-11 |
| C1 | Retouching cases | Very often: tripod/photographer shadows, rig parts, lens flares/sensor dust. Often: stitching errors, pixelating. Photoshop tools used: clone stamp, patch tool, content-aware fill. | Functional spec R-06, R-07, R-11, R-12 |
| C2 | Second shot | Depends on setup: offset ground shot, shot straight down, or a deliberate hole. | Functional spec F-09, R-08 |
| C3 | Privacy | Manual pixelating is enough; no automatic detection. | Functional spec R-07, Won't |
| D1 | Series | Rig orientation varies per image → nadir circles still work; asymmetric masks adjusted per image. | Functional spec batch note, B-09 |
| D2 | Series size | Today mostly single images; previously real estate 5–25, Street View 15–100. | Vision persona |
| E1 | Hardware | Reference: M4 Max 48 GB / i7-12700K + RTX 5080 48 GB. Minimum requirements may be high. | Architecture performance budgets |
| E2 | Test data | Provided by the maintainer → [06-test-data.md](06-test-data.md). | 06-test-data |
| F1 | Language | UI English + German; documents English only. | AGENTS.md, functional spec U-05 |
| F2 | Signing | Start unsigned. | Roadmap E2.6 |
| F3 | Public | Repository public from the start. | – |
| G2 | Review | Via tests, benchmarks, screenshots/demos rather than line-by-line code review. | AGENTS.md |
| G3 | Timeline | No fixed target; iterative. | – |

## Round 3 – Interaction and retouching details

| # | Topic | Decision | Carried into |
| --- | --- | --- | --- |
| H1 | Navigation | Space/middle mouse = rotate, wheel = zoom, right mouse = marking menu; adjust after first real tests. | Functional spec U-07, UX concept |
| H2 | Marking menu | Hold right mouse button, release in a direction (like Maya). Start: tools + nadir/zenith. | Functional spec U-02, UX concept |
| H3 | Saving | Auto-save; undo history per session, layers persistent. Persistent history as a later option. | Functional spec H-06, H-07 |
| H4 | Multiple images | One active image + filmstrip; content from other images via copy/paste. | Functional spec Won't |
| H5 | Layout | Fixed and minimal, panels collapsible. | Functional spec U-04 |
| I1 | Shadows | A dedicated "shadow lighten" tool makes sense; inpainting + clone remain as fallbacks. | Functional spec R-15 |
| I2 | Stitching errors | Primarily fixed in PTGui; leftovers with the existing tools, extend if needed. | – |
| I3 | Holes | Start with AI inpainting; sky replacement later. | Functional spec R-16 |
| I4 | Priority | Single image before batch. | Roadmap |
| J1 | Colour spaces | Test data contains several colour spaces → the software handles colour management automatically (keep source profile, display correctly, web export to sRGB). | Architecture 4.10, functional spec F-14 |
| J2 | Export presets | Full resolution, Street View, 16K, 24K, 32K – each with megapixels shown. | Functional spec F-13 |
| K1 | GitHub | Specs as Markdown in the repo, stories as GitHub issues. | Roadmap |
| K2 | Translation | German during the concept phase, then full translation to English. | Done |
| K3 | Updates | Opt-in check → Tauri updater with its own package signature. | Architecture 4.12 |

## Round 4 – Export, language, test data, main screen

| # | Topic | Decision | Carried into |
| --- | --- | --- | --- |
| L1 | JPEG + holes | Hint before export; fill colour black, configurable. | Functional spec F-15 |
| L2 | Street View | Previously approx. 13000×6500; API limit 75 MB (verify during implementation). | Functional spec F-13 |
| L3 | UI language | English by default, German via settings. | Functional spec U-05 |
| L4 | Test data | Sets A–C stay local; public test images are created separately when needed. | 06-test-data |
| L5 | Main screen | Tools left (collapsible), mini-map top right, layers below, filmstrip bottom (collapsible); maximum image area; open to unconventional concepts; expert/zen mode like Maya. | Vision, UX concept |

## Round 5 – Mockup reviews (v1–v3) and scope

| # | Topic | Decision | Carried into |
| --- | --- | --- | --- |
| W1 | Scope per version | Accepted as proposed, except AI object selection. | 07-tools-and-layers |
| W2 | AI object selection | Not for 1.0 – classic tools are enough. | 07-tools-and-layers |
| W3 | Adjustment layers | Plan for them (1.x) if the effort is reasonable. | 07-tools-and-layers, functional spec H-10 |
| W4 | Layer order | Strict like Photoshop (top covers bottom), also for adjustment layers. | 07-tools-and-layers, UX concept |
| W5 | Zen mode | Tool options stay visible, exit hint shown – sufficient. | UX concept |
| – | Mockup feedback | Domain colours, keycaps, scrub fields, fixed-size tool options, edge-collapse with click-only handles, history panel, groups, inline opacity, AI job states. | [08-ux-concept.md](08-ux-concept.md), `mockups/ux-v*/review/` |

## Round 6 – Spike S1/S1b results (2026-10-25)

| # | Topic | Decision | Carried into |
| --- | --- | --- | --- |
| X1 | Viewer display approach | Virtual texturing on the equirect pyramid with pole taps; no cube map. | [ADR-0003](adr/0003-tiled-image-pipeline.md) (Accepted) |
| X2 | Tile transport | One interface, per-platform backend: WebView2 shared buffers on Windows, custom URI scheme on macOS, loopback WebSocket as fallback. | ADR-0003, [spike report](spikes/s1-tile-viewer.md) |
| X3 | Tile format on the wire | u8 for SDR sources, f16 for HDR; tiles built in one pass in the wire format. | ADR-0003 |
| X4 | Open performance work | Windows decode times (EXR, 16-bit TIFF), disk-cache size for 16-bit sources → E1.3. | spike report, ADR-0003 open points |
