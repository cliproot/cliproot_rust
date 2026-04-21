# Cliproot End-to-End Demo — Research with Full Provenance

A 15-minute walkthrough that exercises every hook and MCP tool shipped through Phase F of `karpathy_method_5.md`. The demo stages a small research session about OAuth / PKCE, lets the hooks capture and compile it, then uses the new `cliproot wiki query` and `cliproot wiki lint` commands to show the loop closing.

Phases in play: **A** (plugin), **B** (skills), **C** (flush → daily digest), **D** (session-start + compile → wiki), **F** (wiki-lint + query). Phase E is not yet shipped and is out of scope.

---

## 0. One-time setup

```bash
# From a shell where `cliproot` is on PATH
claude plugin marketplace add cliproot/cliproot-rust
claude plugin install cliproot@cliproot-rust --scope user

# In the directory you want to demo in (new or existing project)
mkdir -p ~/demo/pkce-research && cd ~/demo/pkce-research
git init -q
cliproot init           # creates .cliproot/ and default config
cliproot config set knowledge.level wiki   # turn on flush + compile + session-start
```

Sanity check:

```bash
cliproot config get knowledge.level       # → wiki
cliproot config get knowledge.models.flush   # → claude-haiku-4-5-20251001
cliproot config get knowledge.models.compile # → claude-haiku-4-5-20251001
cliproot config get knowledge.models.lint    # → claude-haiku-4-5-20251001
cliproot config get knowledge.models.query   # → claude-haiku-4-5-20251001
```

The Pro-plan Haiku default matters — Sonnet would rate-limit during the demo.

---

## 1. Day 1 session — capture and synthesize

Start a new Claude Code session in the demo directory. The `SessionStart` hook runs (phase D) but since there's no wiki yet it injects only an empty-index header.

Prompt Claude (copy-paste):

> Help me understand how PKCE protects OAuth public clients. Look at the RFC summary and the Auth0 blog post. Cite specific passages as you go.

Claude will use WebFetch / WebSearch. After the first tool call, `PostToolUse → cliproot hook capture` starts writing JSONL to `.cliproot/agent-log/<session>.jsonl` (phase A, already in place).

Now demonstrate **explicit clipping** — ask Claude to highlight what mattered. A good follow-up prompt:

> For each of those two sources, call `cliproot_clip_create` on the single sentence that most cleanly explains how PKCE defeats the code-interception attack. Then `cliproot_clip_derive` a one-paragraph synthesis citing both clips with `transformation_type = "combine"`.

Expected tool calls:
- `cliproot_clip_create` × 2 — one per source, with `source_type = "external-quoted"`
- `cliproot_clip_derive` × 1 — synthesis clip with `from = [clip1_hash, clip2_hash]`

Show the resulting provenance:

```bash
cliproot clip list                        # the 3 new clips show up
cliproot clip trace <synthesis-clip-hash> # shows derivation lineage back to both sources
```

---

## 2. End the session — watch the hooks fire

When you close the Claude Code session (or type `/quit`), three hooks run in order:

1. **`hook consolidate`** (Stop) — prints any still-unhighlighted sources as a block message. If you missed clipping something, it shows up here.
2. **`hook flush`** (Stop, level ≥ digest) — detached-spawns `cliproot hook flush`. A few seconds later `.cliproot/knowledge/daily/2026-04-18.md` appears with the daily digest. Records a `Derive` activity.
3. **`hook flush`** chains into **`cliproot wiki compile`** (level ≥ wiki, hour ≥ `compile_after_hour`) — which uses Haiku to synthesize wiki articles from the digest + index.

Wait ~30 s, then:

```bash
ls .cliproot/knowledge/daily/             # 2026-04-18.md — daily digest
ls .cliproot/knowledge/concepts/          # e.g. pkce-flow.md
ls .cliproot/knowledge/connections/       # cross-cutting articles
cat .cliproot/knowledge/index.md          # master catalog
cat .cliproot/knowledge/log.md            # hook trace: FLUSH_OK, COMPILE_OK, etc.
```

Note the frontmatter on a generated article — it has a stable UUID and a `contentHash` that will survive recompiles (D5):

```bash
head -10 .cliproot/knowledge/concepts/pkce-flow.md
```

---

## 3. Day 2 session — SessionStart injection (phase D payoff)

Open a fresh Claude Code session in the same directory. The `hook session-start` now runs against a populated wiki and injects:

- `index.md` headings (article count, titles, types)
- The most recent daily digest's H2 headings

Nothing you did yesterday is lost — Claude enters the session already knowing what the wiki covers, without burning any tokens.

Verify the injection by asking:

> What does my wiki already know about PKCE?

Claude will paraphrase the injected context.

---

## 4. `cliproot wiki query` — two-phase retrieval (NEW in Phase F)

From the CLI:

```bash
cliproot wiki query "how does our OAuth flow handle PKCE code-interception attacks?"
```

What happens under the hood:
1. **Phase 1** — Haiku extracts 3–8 keywords as a JSON array (`["pkce", "oauth", "code interception"]`). Cheap (<$0.001).
2. **Deterministic selection** — `index::select_articles_for_compile` picks candidate articles by substring match against keywords; under 50 articles, it loads all.
3. **Phase 2** — Haiku answers using the selected article bodies; the system prompt requires `[cliproot:sha256-…]` citations over `[[wikilinks]]`.
4. A `Research` Activity is stored with `used_source_refs` pointing to every cited clip.

Expected output shape:

```
PKCE binds the authorization code to the original requester via a
`code_verifier` / `code_challenge` pair [cliproot:sha256-abc…]. Without
the verifier, an attacker who intercepts the code cannot redeem it
[cliproot:sha256-def…].

─────
consulted: concepts/pkce-flow.md, connections/oauth-code-flow.md
citations: 2 clip(s)
```

Same query with file-back:

```bash
cliproot wiki query "how does PKCE handle code-interception?" --file-back
ls .cliproot/knowledge/qa/          # how-does-pkce-handle-code-interception.md
```

Rerun the same question — the UUID on the generated Q&A article is preserved and the body updates in place (D5 again).

From inside Claude Code, exercise the MCP path:

> Use `cliproot_wiki_query` to ask: "what's the relationship between PKCE and the authorization code flow?"

The MCP tool shells out to the CLI and returns the JSON outcome.

---

## 5. `cliproot wiki lint` — provenance hygiene (NEW in Phase F)

```bash
cliproot wiki lint
```

Checks, in order:

1. Broken `[[wikilinks]]` — informational
2. **Broken `[cliproot:sha256-…]` citations** — load-bearing; any finding → exit 1
3. Orphan pages (no inbound references)
4. Orphan sources (daily digest not yet compiled)
5. Stale articles (body drifted from frontmatter `contentHash`)
6. Sparse articles (< 200 words)
7. Missing backlinks (one-way edges)
8. Uncovered claims — paragraph-level `cliproot doc coverage` pass

Run the LLM-backed contradiction pass on top:

```bash
cliproot wiki lint --contradictions --report
# → .cliproot/knowledge/reports/wiki-lint-2026-04-18.md
```

Prove the load-bearing invariant — manually break a citation, rerun, watch exit code flip:

```bash
sed -i 's/sha256-[a-zA-Z0-9_-]\{40,\}/sha256-DEADBEEF/' \
    .cliproot/knowledge/concepts/pkce-flow.md
cliproot wiki lint; echo "exit=$?"    # exit=1, with a #2 finding
# Undo:
cliproot wiki compile                   # rewrites the article from index
```

From inside Claude Code:

> Run `cliproot_wiki_lint` with `contradictions = true` and summarize findings.

---

## 6. Trace a clip end-to-end

Pick any clip hash from §1 and follow the lineage the hooks recorded:

```bash
CLIP=$(cliproot clip list --limit 1 --format json | jq -r '.[0].clipHash')

cliproot clip get    "$CLIP"    # full clip metadata
cliproot clip trace  "$CLIP"    # derivation DAG upward to sources
cliproot bundle export "$CLIP"  # full CRP bundle (all activities + edges)
```

You should see:
- The clip itself
- The `Derive` activity from the day-1 synthesis
- The `Derive` activity from `cliproot wiki compile` — linked via `CitedIn` to the wiki article
- The `Research` activity from `cliproot wiki query` — linked via `used` to the cited clips

**This is the point of the demo.** A research question Claude answered today draws a straight, verifiable line back to the two sentences you highlighted yesterday.

---

## 7. Teardown

```bash
# Keep the repo, just drop the level if you want hooks quiet:
cliproot config set knowledge.level curator

# Or nuke everything:
rm -rf .cliproot
```

---

## Troubleshooting

| Symptom | Likely cause |
|---|---|
| Hooks never fire | Plugin not installed user-scope, or `cliproot` not on PATH — run `bin/install-cliproot.sh` |
| `hook flush` logs `BUDGET_EXCEEDED` | Daily token/cost cap hit; `cliproot config set knowledge.max_bg_tokens_per_day 100000` |
| `wiki compile` silently skips | Before `compile_after_hour` (default 18:00) — run `cliproot wiki compile` manually |
| `wiki query` returns `Skipped: empty wiki index` | No compile has run yet — do §2 first |
| `wiki query` returns `Skipped: level … does not allow query` | `knowledge.level` is `curator`; set to `wiki` |
| `wiki lint` fails only #2 | Broken citations — a compile after a clip was purged; rerun `cliproot wiki compile` or investigate with `cliproot clip verify <hash>` |

---

## What this demonstrates

- **Capture (Phase A/B)** — hooks record tool usage without explicit agent effort; `cliproot_clip_create` / `cliproot_clip_derive` mark what was load-bearing.
- **Flush (Phase C)** — the day's work is summarized on Haiku once, under a strict daily budget, as a daily digest artifact.
- **Compile (Phase D)** — the digest is promoted into durable, UUID-stable wiki articles with full citation edges back to the originating clips.
- **SessionStart (Phase D)** — the next session begins already aware of what the wiki covers, for zero tokens.
- **Query + Wiki Lint (Phase F)** — answers are grounded in the wiki with `[cliproot:sha256-…]` citations; a single command audits the whole corpus for structural and provenance invariants.

Everything above was exercised without the agent ever fabricating a citation. Claims trace to clips; clips trace to URLs.
