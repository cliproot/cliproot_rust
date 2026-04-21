# /cliproot:query

Answer a natural-language question from the compiled wiki with source citations.

## When to use

- Before starting new research — check whether the topic has been covered before
- To pull together context on a past design decision or incident
- To resolve "did we decide X?" questions without re-reading daily digests
- Whenever you would otherwise `grep` through `.cliproot/knowledge/`

## What it does

Runs `cliproot wiki query "<question>"` (or the `cliproot_wiki_query` MCP tool). Two phases:

1. **Retrieve** — extract 3–8 keywords from the question (cheap Haiku call) and pick the most relevant articles from `index.md`.
2. **Answer** — Haiku drafts an answer using the selected article bodies, preferring inline `[cliproot:sha256-…]` citations over `[[wikilinks]]`.

Every call records a `Research` activity so the answer is auditable via `cliproot clip trace`.

## Output

The answer text, followed by the list of consulted articles and cited clips. With `--file-back`, the answer is also persisted as `.cliproot/knowledge/qa/<slug>.md` with a stable UUID — reruns update the body in place.
