# AGENTS.md – Working rules for AI agents

## Read before every task

1. [docs/01-vision.md](docs/01-vision.md) – especially **non-goals** and **product principles**.
2. The sections of [docs/02-functional-spec.md](docs/02-functional-spec.md) and
   [docs/03-architecture.md](docs/03-architecture.md) relevant to the task.
3. All ADRs in [docs/adr/](docs/adr/) touching the affected modules.
4. For UI tasks: [docs/08-ux-concept.md](docs/08-ux-concept.md) (patterns, rules, states) and the
   reference prototype `mockups/ux-v3/` (look at the behaviour – do not copy its code).
5. The GitHub issue you are working on, including linked issues and its milestone.

## Ground rules

- **Scope:** implement only what the issue asks for. Never add features from the non-goals (tour
  creation, stitching, RAW, video). Scope per version: [docs/07-tools-and-layers.md](docs/07-tools-and-layers.md).
- **Spherical coordinates:** always persist geometric parameters as angles/directions, never pixels
  (ADR-0002). Projections only via `pano-geom`, never re-implemented ad hoc.
- **Originals are sacred:** no code may write to or overwrite source files.
- **Core without UI:** crates under `crates/` must not depend on Tauri or the frontend.
- **No pixel data as JSON/Base64** across the IPC boundary.
- **No network access** without explicit user action; no telemetry.
- **New dependencies** only with a justification in the PR; the licence must be compatible with AGPL-3.0.
- **Architecture deviations** from the ADRs: never silently – create a new ADR (status `Proposed`) and
  point it out in the PR.
- **Private test data:** the maintainer's images in `testdata-local/` (via `PANO_TESTDATA`) must never be
  committed, uploaded or shown in public issues/PRs. Use synthetic or public images there
  ([docs/06-test-data.md](docs/06-test-data.md)).

## Quality & evidence

The maintainer reviews mainly through **tests, benchmarks, screenshots and demos**, less through
individual lines of Rust or shader code. Every change must therefore prove its correctness:

- Every core change comes with tests (unit, property tests for geometry, golden images for image
  operations; see the test strategy in the architecture document).
- Performance-relevant changes: add/run a benchmark and state **before/after numbers in the PR**; the
  budgets in the architecture document are acceptance criteria.
- UI changes: **screenshot or short screen recording** in the PR.
- PR description in plain language: what was done, why, how it was verified, what the risks are.
- Code, comments, commits, issues and docs in English.

## Workflow

1. Work from a GitHub issue. If acceptance criteria are missing or unclear, add a proposal to the issue
   and get it confirmed before implementing.
2. One branch per issue (`<issue-number>-short-name`), small reviewable PRs, `Closes #<n>` in the PR.
3. Questions that come up during implementation: ask in the issue (label `question`) instead of making
   assumptions. Answered product decisions are added to [docs/05-decisions.md](docs/05-decisions.md).
4. Keep docs in sync: if behaviour or architecture changes, update the affected document in the same PR.

## Commands

_Not available yet – added with the project scaffold (E1.1): build, test, lint, dev start._
