# Architecture Decision Records

Every significant technical decision is recorded as a short ADR: context, decision, consequences. ADRs
are never deleted; when a decision changes, a new ADR supersedes the old one
(`Status: Superseded by ADR-XXXX`).

Status values: `Proposed` → `Accepted` | `Rejected` | `Superseded`.

| ADR | Title | Status |
| --- | --- | --- |
| [0001](0001-desktop-shell-tauri-rust.md) | Desktop shell: Tauri 2 with Rust core and Svelte UI | Proposed (S1 confirmed on macOS + Windows; S5 open) |
| [0002](0002-non-destructive-edit-stack.md) | Non-destructive edit stack in spherical coordinates | Accepted |
| [0003](0003-tiled-image-pipeline.md) | Tiled image pipeline and working format | Accepted |
| [0004](0004-local-ai-inpainting-onnx.md) | Local AI inpainting via ONNX Runtime | Proposed (spike S3) |

## Template

```markdown
# ADR-XXXX: <Title>
- Status: Proposed
- Date: YYYY-MM-DD

## Context
## Decision
## Alternatives considered
## Consequences
```
