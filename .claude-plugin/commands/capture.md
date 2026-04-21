# /cliproot:capture

Start a lightweight provenance capture session for research or cited document writing.

## When to use

- Beginning research on a topic
- Writing documents that need citations
- Gathering sources for later synthesis

## What it does

Activates the cliproot-capture skill principles:
1. Clip important sources (don't fabricate citations)
2. Record syntheses as derivations with parent clip references
3. Scope work to the right project before capturing

## Initial steps

1. Use `cliproot_project_use` to set the active project (create with `cliproot_project_create` if needed)
2. Research normally — the hooks will log your tool usage
3. When you encounter key sources, explicitly `cliproot_clip_create` the relevant passages
4. When combining information, `cliproot_clip_derive` with accurate transformation types
5. Use `cliproot_doc_annotate` and `cliproot_doc_cite` for inline citations and bibliography

## Consolidation

The Stop hook will periodically prompt you to review unhighlighted sources. When this happens:
- Review sources you've consulted
- Clip key passages that anchored your reasoning
- Skip sources you didn't find useful (they remain in the log as consulted-but-not-cited)
