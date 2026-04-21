# Cliproot Command Surface — Refactor Plan (v1)

Companion to `cliproot_knowledge_management_considerations_3.md` and `_4.md`.
This document closes out the open questions in v4 §15.1 and provides a concrete,
single-cutover plan for collapsing the 35 top-level CLI commands into a
noun-first grammar.

Design stance (from JW, v3): **group by noun, sooner than later, but plan it
carefully**. No active external audience yet, so breaking changes are free as
long as everything inside `cliproot_rust` moves in lockstep.

---

## 1. Ground Truth

### 1.1 Current CLI surface (35 top-level commands)

Source: `crates/cliproot-cli/src/main.rs`.

| Category | Commands |
|---|---|
| Setup | `init`, `mcp` |
| Hooks | `capture-hook`, `consolidate-hook`, `flush-hook`, `session-start-hook`, `consolidate` (manual) |
| Capture / Lineage | `clip`, `copy`, `derive`, `inspect`, `trace`, `verify`, `list`, `record` |
| Wiki | `compile`, `wiki-lint`, `query` |
| Portability | `ingest`, `export` |
| Documents | `annotate`, `cite`, `doctor` |
| Scopes | `project {create,list,use,delete}`, `artifact {add,list,get,restore,link}`, `pack {create,import,inspect,verify}`, `activity {start,end}`, `session {start,end}`, `remote {add,remove,list}` |
| Remote ops (flat) | `push`, `pull`, `search`, `login`, `logout` |
| Config | `config {get,set}` |

### 1.2 Current MCP surface (30 tools)

Source: `crates/cliproot-mcp/src/service.rs`. Mostly already noun-first
(`cliproot_project_*`, `cliproot_artifact_*`, `cliproot_pack_*`,
`cliproot_activity_*`, `cliproot_session_*`). Flat exceptions:
`cliproot_clip`, `cliproot_derive`, `cliproot_inspect`, `cliproot_trace`,
`cliproot_verify`, `cliproot_list`, `cliproot_search`, `cliproot_export`,
`cliproot_annotate`, `cliproot_cite`, `cliproot_doctor`,
`cliproot_consolidate`, `cliproot_wiki_lint`, `cliproot_query`.

**Name collision uncovered:** MCP `cliproot_search` is a local clip-text
substring search; CLI `cliproot search` is a remote-registry search. Same
verb, different surfaces, different behaviors. Must be resolved in this
refactor.

### 1.3 Downstream consumers (lockstep migration)

Every rename must land atomically with:

- `.claude-plugin/hooks/*.sh` (5 files) — `exec cliproot <hook-cmd> --harness claude-code`
- `.claude-plugin/commands/*.md` (5 files) — docs referencing CLI + MCP names
- `.claude-plugin/plugin.json` — declares `"args": ["mcp"]`
- `integrations/cursor/hooks.json` — hardcoded `cliproot capture-hook --harness cursor` etc.
- `crates/cliproot-cli/src/commands/init/hook_config.rs` — generates `.claude/settings.json` + `.cursor/hooks.json` with hardcoded command strings; **asserted in tests**
- `skills/cliproot-capture/SKILL.md`, `skills/cliproot-session/SKILL.md` — reference MCP tool names (embedded via `include_str!` in `skills.rs`, written to multiple dirs by `agent_config.rs`)
- `skills/cliproot-session/scripts/verify-provenance.sh` — calls `cliproot verify` and `cliproot doctor`
- `README.md` — ~40 inline usage examples

Anything missed in that list breaks tests or silently orphans a consumer.

---

## 2. Answers to v4 §15.1 Open Questions

### 2.1 Grouping shape — noun-first or lifecycle?

**Decision: noun-first.** Reasons:

1. MCP is already noun-first for the grouped tools (`cliproot_project_create`, `cliproot_artifact_add`, etc.). Aligning CLI to MCP halves the mental model.
2. Noun-first makes `--help` discoverable: `cliproot project --help` shows every project operation. Lifecycle groupings (`cliproot create <thing>`) scatter operations across verbs.
3. Existing sub-noun groups (`project`, `artifact`, `pack`, `activity`, `session`, `remote`) are already in place and working — noun-first is an extension of a proven pattern, not a new design.

### 2.2 Alias horizon

**Decision: no aliases.** Single atomic cutover PR.

Tool has no active external audience. Aliases add code, hide the new shape, and defer the real work. The internal consumers (plugin hooks, skills, README, tests) are all in-repo — they migrate in the same PR that does the rename.

### 2.3 Hook subcommand interop

**Decision: hooks move under `cliproot hook <verb>`.** All four hook entry points (`capture-hook`, `consolidate-hook`, `flush-hook`, `session-start-hook`) become `hook {capture,consolidate,flush,session-start}`. The manual-invocation `consolidate` command is a distinct operation (user-facing, not harness-driven) and moves under `session consolidate`.

Plugin hook scripts and `hook_config.rs` generators all update in the same commit; the hook-command-string tests in `hook_config.rs` update with them.

### 2.4 JSON-output contract impact

**Decision: JSON shapes are noun-level contracts, unaffected by renames.** The schemas returned by `cliproot clip … --format json` do not depend on the verb path; they depend on the underlying entity (clip, project, artifact). Skills that consume JSON will change the command they invoke, but not the field names they parse.

One exception: if we fold `inspect` → `clip get` (see §3), we should confirm the JSON output of `clip get` is a strict superset of current `inspect --format json` output. Expected to be true since `inspect` already takes any entity hash/id and dispatches.

### 2.5 MCP surface reshape

**Decision: minimal rename in this PR, optional full alignment deferred.**

- **Required now:** `cliproot_search` → `cliproot_clip_search` (resolves the name collision; CLI `search` → `remote search` in the same PR).
- **Recommended now:** rename the 13 flat MCP tools to noun-first (`cliproot_clip`, `cliproot_derive`, `cliproot_inspect`, `cliproot_trace`, `cliproot_verify`, `cliproot_list`, `cliproot_export`, `cliproot_annotate`, `cliproot_cite`, `cliproot_doctor`, `cliproot_consolidate`, `cliproot_wiki_lint`, `cliproot_query`). Updates skill SKILL.md files in the same PR.
- **Rationale for doing it now:** skills are regenerated from `include_str!`-embedded SKILL.md, so there's exactly one place to edit per skill. Deferring means two coordinated PRs later instead of one now.

MCP tool name ↔ CLI path don't have to be byte-identical, but they should rhyme. `cliproot_clip_new` ↔ `cliproot clip new` is easier to teach than `cliproot_clip` ↔ `cliproot clip new`.

---

## 3. Proposed Grammar

**16 top-level entries: 14 nouns + 2 singletons.**

| Top-level | Subcommands |
|---|---|
| `init` | (singleton — project bootstrap) |
| `mcp` | (singleton — MCP server; external contract) |
| `clip` | `new`, `get`, `list`, `search`, `copy`, `derive`, `trace`, `verify` |
| `artifact` | `add`, `get`, `list`, `restore`, `link` |
| `bundle` | `export`, `import` |
| `pack` | `create`, `import`, `inspect`, `verify` |
| `doc` | `annotate`, `cite`, `coverage` |
| `wiki` | `compile`, `lint`, `query` |
| `project` | `create`, `list`, `use`, `delete` |
| `session` | `start`, `end`, `record`, `consolidate` |
| `activity` | `start`, `end` |
| `remote` | `add`, `delete`, `list`, `push`, `pull`, `search`, `login`, `logout` |
| `hook` | `capture`, `consolidate`, `flush`, `session-start` |
| `config` | `get`, `set` |

### 3.1 Key design choices

- **`clip` verbs:** `new` (formerly top-level `clip`), `get` (formerly `inspect` — dispatches to any entity by hash, but "get" reads more naturally than "inspect"), `list` (formerly top-level `list`), `search` (formerly MCP-only local search; promoted to CLI to resolve the collision), `copy` (formerly top-level `copy`), `derive` (formerly top-level `derive`), `trace`, `verify`.
- **`inspect` → `clip get`:** The existing `inspect` accepts any hash and dispatches based on entity type. Naming it `clip get` is slightly narrower-sounding than reality — alternative is a top-level `cliproot get <hash>`, but that breaks the noun-first principle. Recommend `clip get` since clips are by far the most common lookup target; other entities have their own `get` (`artifact get`, `project <id>`).
- **`doctor` → `doc coverage`:** Today's `doctor` audits provenance coverage of a *document*, not repo health. Rename clarifies what it does.
- **`bundle` as new noun:** Today's flat `export` and `ingest` operate on portable bundles (.cliproot-bundle). Grouping them under `bundle {export,import}` mirrors `pack {create,import}` and makes the pair discoverable together. Verb choice `import` (not `ingest`) for symmetry with `pack import`.
- **`record` → `session record`:** `record` captures command output with provenance. It's semantically session-scoped (it only makes sense during a session) — placing it under `session` reinforces that. Stays a subcommand of `session`, not of `clip`, because it produces clips but also writes activity events.
- **`consolidate` (manual) → `session consolidate`:** Same reasoning — the manual consolidate is a session-scoped operation, distinct from `hook consolidate` which is a harness entry point.
- **`compile` / `wiki-lint` / `query` → `wiki {compile,lint,query}`:** All three operate on the compiled wiki. Groups naturally.
- **`push` / `pull` / `search` / `login` / `logout` → `remote {push,pull,search,login,logout}`:** All five are remote-registry operations. `remote add` / `remote delete` / `remote list` already live under `remote`; moving the flat verbs makes the surface complete. Note: `remove` → `delete` for symmetry with `project delete`.
- **Hook subcommand set:** `hook {capture,consolidate,flush,session-start}` — drops the `-hook` suffix since the `hook` parent already disambiguates. The `--harness <claude-code|cursor|codex>` flag stays on each.

---

## 4. Migration Table (old → new)

### 4.1 CLI

| Current | New |
|---|---|
| `cliproot init` | `cliproot init` *(unchanged)* |
| `cliproot mcp` | `cliproot mcp` *(unchanged)* |
| `cliproot capture-hook` | `cliproot hook capture` |
| `cliproot consolidate-hook` | `cliproot hook consolidate` |
| `cliproot flush-hook` | `cliproot hook flush` |
| `cliproot session-start-hook` | `cliproot hook session-start` |
| `cliproot consolidate` *(manual)* | `cliproot session consolidate` |
| `cliproot clip <url> <quote>` | `cliproot clip new <url> <quote>` |
| `cliproot copy` | `cliproot clip copy` |
| `cliproot derive` | `cliproot clip derive` |
| `cliproot inspect <hash>` | `cliproot clip get <hash>` |
| `cliproot trace` | `cliproot clip trace` |
| `cliproot verify` | `cliproot clip verify` |
| `cliproot list` | `cliproot clip list` |
| `cliproot record` | `cliproot session record` |
| `cliproot compile` | `cliproot wiki compile` |
| `cliproot wiki-lint` | `cliproot wiki lint` |
| `cliproot query` | `cliproot wiki query` |
| `cliproot ingest` | `cliproot bundle import` |
| `cliproot export` | `cliproot bundle export` |
| `cliproot annotate` | `cliproot doc annotate` |
| `cliproot cite` | `cliproot doc cite` |
| `cliproot doctor` | `cliproot doc coverage` |
| `cliproot project {create,list,use,delete}` | *(unchanged)* |
| `cliproot artifact {add,list,get,restore,link}` | *(unchanged)* |
| `cliproot pack {create,import,inspect,verify}` | *(unchanged)* |
| `cliproot activity {start,end}` | *(unchanged)* |
| `cliproot session {start,end}` | *(unchanged; adds `record`, `consolidate`)* |
| `cliproot remote add` | *(unchanged)* |
| `cliproot remote remove` | `cliproot remote delete` |
| `cliproot remote list` | *(unchanged)* |
| `cliproot push` | `cliproot remote push` |
| `cliproot pull` | `cliproot remote pull` |
| `cliproot search` | `cliproot remote search` |
| `cliproot login` | `cliproot remote login` |
| `cliproot logout` | `cliproot remote logout` |
| `cliproot config {get,set}` | *(unchanged)* |

### 4.2 MCP

| Current | New | Notes |
|---|---|---|
| `cliproot_clip` | `cliproot_clip_new` | aligns with `cliproot clip new` |
| `cliproot_derive` | `cliproot_clip_derive` | |
| `cliproot_inspect` | `cliproot_clip_get` | |
| `cliproot_trace` | `cliproot_clip_trace` | |
| `cliproot_verify` | `cliproot_clip_verify` | |
| `cliproot_list` | `cliproot_clip_list` | |
| `cliproot_search` | `cliproot_clip_search` | **Resolves the name collision.** |
| `cliproot_export` | `cliproot_bundle_export` | |
| `cliproot_annotate` | `cliproot_doc_annotate` | |
| `cliproot_cite` | `cliproot_doc_cite` | |
| `cliproot_doctor` | `cliproot_doc_coverage` | matches CLI rename |
| `cliproot_consolidate` | `cliproot_session_consolidate` | matches CLI placement |
| `cliproot_wiki_lint` | `cliproot_wiki_lint` | *(unchanged)* |
| `cliproot_query` | `cliproot_wiki_query` | |
| `cliproot_project_*`, `cliproot_artifact_*`, `cliproot_pack_*`, `cliproot_activity_*`, `cliproot_session_*` | *(unchanged)* | already noun-first |

---

## 5. Cutover Plan

Single PR. Order within the PR matters for reviewability but all changes land together.

1. **Grammar** — refactor `main.rs` Commands enum into noun-first subcommand enums. Adjust `run()` dispatch. No behavior changes.
2. **Hook generators** — update `hook_config.rs` to emit the new command strings for `.claude/settings.json` and `.cursor/hooks.json`. Update the assertion tests in the same file.
3. **Plugin hook scripts** — rewrite the 5 files in `.claude-plugin/hooks/*.sh` to `exec cliproot hook <verb> --harness claude-code`.
4. **Plugin commands + plugin.json** — the 5 `.claude-plugin/commands/*.md` docs get their CLI + MCP references updated. `plugin.json` unchanged (still `"args": ["mcp"]`).
5. **Cursor integration** — `integrations/cursor/hooks.json` rewrite.
6. **MCP tool names** — rename 13 tools in `service.rs`. CLI dispatch paths inside MCP service update to new CLI paths (MCP shells to CLI for `consolidate`, `wiki-lint`, `query`).
7. **Skills** — update `skills/cliproot-capture/SKILL.md` and `skills/cliproot-session/SKILL.md` to reference new MCP names. Update `verify-provenance.sh` to call `cliproot clip verify` / `cliproot doc coverage`.
8. **README** — rewrite ~40 usage examples.
9. **CHANGELOG entry** — document the cutover, list every rename.
10. **Full test run** — every `init`-generated hook config is asserted by test; plugin hooks are runtime-validated when the plugin is installed; skills are snapshot-tested via `include_str!`. All three gates should catch drift.

**No deprecation window.** A user who has `cliproot 0.N` installed and hits `cliproot 0.(N+1)` sees a hard break. This is acceptable per v3 §1.1 and v4 §15.1 framing — no active audience.

---

## 6. Open Questions for JW (before implementation)

These are the decisions I want confirmed before the implementer starts writing code. Recommendations shown; please flag any you want to change.

1. **`clip` verb naming** — recommend `clip new` (matches `cargo new` pattern, short). Alternatives: `clip add` (symmetric with `artifact add`), `clip create` (symmetric with `project create`, `pack create`). Consistency-wise, `clip create` is the most uniform choice across the surface. → **Recommend `clip new` for brevity, but leaning toward `clip create` if uniformity matters more.**
[JW] Go with `clip create`
2. **`inspect` → `clip get` vs top-level `get`** — recommend `clip get`. Alternative is `cliproot get <hash>` as a 15th top-level that dispatches across entity types. That pattern exists in kubectl. Downside: breaks the noun-first rule.
[JW] Go with `clip get`
3. **`doctor` → `doc coverage`** — confirmed rename? The word "coverage" is accurate; "doctor" is misleading today. **Recommend yes.**
[JW] Yes
4. **`ingest` → `bundle import`** — or keep `ingest` as the top-level? "Ingest" is more distinctive but less discoverable. **Recommend `bundle import`** (symmetric with `pack import`).
[JW] `bundle import`
5. **Full MCP rename in the same PR** — recommended in §2.5, but this is the single biggest multiplier of risk. If you'd rather do only the `cliproot_search` → `cliproot_clip_search` rename now and defer the other 13, say so; the plan doc can be amended to keep the flat MCP names.
[JW] Let's go ahead with the change
6. **`session record` vs keeping `record` top-level** — `record` is heavily used during manual workflows. Burying it one level may cost ergonomically. If that's a concern, `record` could stay top-level as a singleton. **Recommend `session record`** for consistency; promote if usage-data later shows ergonomics loss.
[JW] `session record`
7. **`remote remove` → `remote delete`** — mostly a symmetry concern with `project delete`. Both verbs read fine. **Recommend `delete`** for uniformity; could also standardize project on `remove` instead — pick one direction.
[JW] Go with delete

---

## 7. Post-Cutover Follow-ups (not in scope)

- `cliproot <noun> --help` text audit — make sure every noun's help is an index of its verbs, not a restatement of the top-level help.
- JSON-output contract doc — once the grammar is stable, write down the JSON shape for each `cliproot <noun> <verb> --format json` call as a reference for skill authors. (Not needed for this PR, but natural next step.)
- MCP tool parameter audit — noun-first MCP renames are a chance to also normalize parameter names (`hash_or_id` vs `hash` vs `id`). Optional, but cheapest to do while touching the file.
