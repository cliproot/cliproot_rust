# /cliproot:wiki-lint

Audit the compiled wiki for structural and provenance invariants.

## When to use

- Before sharing or publishing articles synthesised from the knowledge base
- After a large compile run to spot drift
- Periodically (weekly) to catch orphaned pages and uncovered claims
- Whenever you suspect a citation hash has become stale

## What it does

Runs `cliproot wiki-lint` (or the `cliproot_wiki_lint` MCP tool). Default run covers:

1. Broken `[[wikilinks]]` — informational
2. Broken `[cliproot:sha256-…]` citations — **load-bearing**; any finding fails the run
3. Orphan pages with no inbound references
4. Orphan sources (daily digests not yet rolled into a compile)
5. Stale articles (body hash drifted from frontmatter)
6. Sparse articles (< 200 words)
7. Missing backlinks (one-way edges)
8. Uncovered claims via `cliproot doctor`

Pass `--structural-only` to skip #8, or `--contradictions` to add the LLM-backed pairwise pass (#9, ~5k Haiku tokens).

## Output

A per-check summary. Check #2 findings are hard errors; other check failures print but exit 0 unless `--strict` is set. With `--report`, a timestamped markdown report is also written under `knowledge/reports/`.
