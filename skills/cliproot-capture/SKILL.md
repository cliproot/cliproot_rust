---
name: cliproot-capture
description: >
  Lightweight provenance capture. Clips sources and derives syntheses
  during research. Use when doing research or writing cited documents.
compatibility: Requires cliproot MCP server configured.
metadata:
  author: cliproot
  version: "1.0"
---

## Principles

1. Clip important sources — don't fabricate citations.
2. Record syntheses as derivations with parent clip references.
3. Use `cliproot_project_use` to scope clips to the right project before capturing.

## Workflow

1. **Scope**: `cliproot_project_use` to set the active project (create with `cliproot_project_create` if needed).
2. **Capture**: When encountering a source worth citing, `cliproot_clip` the relevant passage. Set `source_type` appropriately.
3. **Synthesize**: When combining or summarizing clips, `cliproot_derive` with parent clip IDs and accurate `transformation_type`.
4. **Cite**: Use `cliproot_annotate` for inline citations and `cliproot_cite` for bibliography in final output.
