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
2. **Check prior work**: Before starting research, call `cliproot_query` to see if the topic has been covered before — reuse beats rediscovery.
3. **Capture**: When encountering a source worth citing, `cliproot_clip` the relevant passage. Set `source_type` appropriately.
3. **Synthesize**: When combining or summarizing clips, `cliproot_derive` with parent clip IDs and accurate `transformation_type`.
4. **Cite**: Use `cliproot_annotate` for inline citations and `cliproot_cite` for bibliography in final output.

## Consolidation — Review Unhighlighted Sources

When the consolidation hook fires (you'll see a block message listing consulted sources):

1. **Review each source** — did it contain a passage that anchored your reasoning?
2. **Highlight what mattered** — for key sources, call `cliproot_clip` with the specific passage that informed your thinking. You don't need to clip the whole document — just the sentence or paragraph that caught your attention.
3. **Record syntheses** — if the hook identifies a file you wrote that drew from multiple sources, consider recording it with `cliproot_derive` to preserve the reasoning chain.
4. **Skip freely** — sources you consulted but didn't find useful don't need highlights. They'll still appear in the provenance graph as consulted-but-not-cited (lower confidence), and semantic enrichment can infer connections later.

If no hooks are available (Windsurf), call `cliproot_consolidate` periodically to check for unhighlighted sources.
