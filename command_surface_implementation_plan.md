# Cliproot Command Surface — Implementation Plan

Implementation companion to `cliproot_command_surface_plan_1.md`. This document
translates the plan's grammar decisions (§2–§4) into concrete file edits in
`cliproot_rust/`, sequenced into phases so a less-capable model can work through
them one at a time without losing the thread.

All work lands in **one PR** (no aliases, no deprecation window — per plan §5).
The phases below are *review ordering* within that PR, not separate PRs.

Paths are relative to `cliproot_rust/`. Every listed file below has been
inspected for this plan; do not assume additional files — grep before adding.

---

## 0. Prerequisites (read-only)

Before starting, load these files into context so the rename surface is fully
understood:

- `crates/cliproot-cli/src/main.rs` — the `Commands` enum and the `match` in `main()`.
- `crates/cliproot-mcp/src/service.rs` — `#[tool]` fn names + three subprocess
  shell-outs at lines 891, 925, 965 that currently spawn `cliproot consolidate`,
  `cliproot wiki-lint`, `cliproot query`.
- `crates/cliproot-cli/src/commands/init/hook_config.rs` — hard-coded hook
  command strings (lines 131–144, 168–202) **and asserted hook strings in tests**
  (lines 314, 317, 320, 325, 330, 381, 392, 486).
- `crates/cliproot-cli/src/commands/flush_hook.rs:152–165` — `flush-hook`
  re-spawns itself with `args = ["flush-hook", "--background", ...]`.
- `crates/cliproot-cli/src/commands/compile.rs:30–33` — `compile` re-spawns
  itself with `args = ["compile", "--background-child", ...]`.
- Plan `cliproot_command_surface_plan_1.md` §4 (the full old→new rename table).

---

## Rename reference (authoritative — copy from plan §4)

### CLI

| Old | New |
|---|---|
| `capture-hook` | `hook capture` |
| `consolidate-hook` | `hook consolidate` |
| `flush-hook` | `hook flush` |
| `session-start-hook` | `hook session-start` |
| `consolidate` (manual) | `session consolidate` |
| `clip <url> <quote>` | `clip create <url> <quote>` |
| `copy` | `clip copy` |
| `derive` | `clip derive` |
| `inspect` | `clip get` |
| `trace` | `clip trace` |
| `verify` | `clip verify` |
| `list` | `clip list` |
| `search` (remote) | `remote search` |
| `record` | `session record` |
| `compile` | `wiki compile` |
| `wiki-lint` | `wiki lint` |
| `query` | `wiki query` |
| `ingest` | `bundle import` |
| `export` | `bundle export` |
| `annotate` | `doc annotate` |
| `cite` | `doc cite` |
| `doctor` | `doc coverage` |
| `remote remove` | `remote delete` |
| `push` | `remote push` |
| `pull` | `remote pull` |
| `login` | `remote login` |
| `logout` | `remote logout` |

Unchanged: `init`, `mcp`, `project *`, `artifact *`, `pack *`, `activity *`,
`session {start,end}`, `config *`, `remote {add,list}`.

### MCP

| Old | New |
|---|---|
| `cliproot_clip` | `cliproot_clip_create` |
| `cliproot_derive` | `cliproot_clip_derive` |
| `cliproot_inspect` | `cliproot_clip_get` |
| `cliproot_trace` | `cliproot_clip_trace` |
| `cliproot_verify` | `cliproot_clip_verify` |
| `cliproot_list` | `cliproot_clip_list` |
| `cliproot_search` | `cliproot_clip_search` |
| `cliproot_export` | `cliproot_bundle_export` |
| `cliproot_annotate` | `cliproot_doc_annotate` |
| `cliproot_cite` | `cliproot_doc_cite` |
| `cliproot_doctor` | `cliproot_doc_coverage` |
| `cliproot_consolidate` | `cliproot_session_consolidate` |
| `cliproot_wiki_lint` | unchanged |
| `cliproot_query` | `cliproot_wiki_query` |

Note: the CLI verb is `clip create` (JW decision §6.1), so the MCP tool is
`cliproot_clip_create` (not `cliproot_clip_new` as drafted in plan §4.2). This
is the only place the plan's draft rename table should be adjusted.

---

## Phase 1 — CLI grammar restructure

Goal: `cliproot --help` shows the 14-noun + 2-singleton grammar. Behavior of every
command is unchanged; only the dispatch path changes.

### Files to edit

1. **`crates/cliproot-cli/src/main.rs`** — the big one.

   - Delete the flat enum variants being regrouped: `CaptureHook`,
     `ConsolidateHook`, `FlushHook`, `SessionStartHook`, `Consolidate`, `Clip`,
     `Copy`, `Derive`, `Inspect`, `Trace`, `Verify`, `List`, `Record`, `Compile`,
     `WikiLint`, `Query`, `Ingest`, `Export`, `Annotate`, `Cite`, `Doctor`,
     `Push`, `Pull`, `Search`, `Login`, `Logout`.
   - Add new top-level variants: `Clip{ command: ClipCommands }`,
     `Bundle{ command: BundleCommands }`, `Doc{ command: DocCommands }`,
     `Wiki{ command: WikiCommands }`, `Hook{ command: HookCommands }`.
   - Extend existing variants:
     - `SessionCommands` — add `Record{…}` (move all Record fields from the old
       top-level `Record`) and `Consolidate{…}` (fields: `session: String`,
       `emergency: bool`, `commit: bool` — copy from the current top-level
       `Consolidate`). Keep `Start`, `End`.
     - `RemoteCommands` — rename `Remove` to `Delete` (keep field `name: String`).
       Add `Push{…}`, `Pull{…}`, `Search{…}`, `Login{…}`, `Logout{…}` with the
       same fields the flat variants currently carry.
   - Add new subcommand enums:
     - `ClipCommands`: `Create{…}` (all Clip fields), `Copy{…}` (Copy fields),
       `Derive{…}` (Derive fields), `Get{ hash_or_id: String }`,
       `Trace{ hash_or_id: String }`, `Verify{ hash_or_id: Option<String> }`,
       `List{…}` (List fields). `Search` stays off this enum — it is remote-only
       now; the MCP local-text search is exposed only through MCP, not CLI.
     - `BundleCommands`: `Import{ path: String }`, `Export{ hash: String, output: Option<String> }`.
     - `DocCommands`: `Annotate{…}`, `Cite{…}`, `Coverage{…}` (all current fields from Doctor).
     - `WikiCommands`: `Compile{…}`, `Lint{…}`, `Query{…}` (copy fields verbatim).
     - `HookCommands`: `Capture{ harness }`, `Consolidate{ harness, emergency }`,
       `Flush{ harness, background, cliproot_dir }`, `SessionStart{ harness, cliproot_dir }`.
       Use `#[command(name = "session-start")]` for the `SessionStart` variant
       so the kebab-case invocation survives.
   - Update `fn main()`'s outer `match cli.command`:
     - Preserve all existing behavior. The function bodies called in
       `commands::<module>::run(...)` do **not** change. Only the path from CLI
       enum → function call changes.
     - For `Commands::Clip { command } => match command { ClipCommands::Create { … } => commands::clip::run(…), ClipCommands::Get { hash_or_id } => commands::inspect::run(&hash_or_id, &cli.format), … }`.
       Do *not* rename the `commands::` modules in this phase — that's Phase 2.

2. **`crates/cliproot-cli/src/commands/mod.rs`** — no changes yet. Keep the
   module names (`capture_hook`, `inspect`, `doctor`, …) as they are. Phase 2
   handles module renames.

### What *not* to touch in Phase 1

- The internals of any `commands::<name>::run(…)` function.
- Module names / filenames.
- Plugin/hook/cursor/skill/README files — Phases 3–6 handle those.

### Phase 1 test updates

- `cargo check -p cliproot-cli` must pass.
- `cargo build -p cliproot-cli` then sanity-invoke:
  - `cliproot clip --help` (must list create, get, list, copy, derive, trace, verify).
  - `cliproot hook --help` (capture, consolidate, flush, session-start).
  - `cliproot remote --help` (add, delete, list, push, pull, search, login, logout).
  - `cliproot wiki --help`, `cliproot doc --help`, `cliproot bundle --help`, `cliproot session --help`.
- No existing unit tests in `main.rs` — nothing to update here.
- `crates/cliproot-cli/src/commands/init/hook_config.rs` tests **will fail**
  after Phase 1 is merged with Phase 2. That is Phase 2's concern.

---

## Phase 2 — Internal self-spawns + hook config generator

Goal: every subprocess spawn and every hook-string literal inside the Rust crates
uses the new command paths. After this phase all tests in `hook_config.rs` pass.

### 2.1 `crates/cliproot-cli/src/commands/flush_hook.rs`

Lines 156–164 currently call `background::spawn(&["flush-hook", "--background", "--cliproot-dir", dir], "cliproot-flush-hook")`.

- Change `args[0]` from `"flush-hook"` to `"hook"` and insert `"flush"` as
  `args[1]`. Final args: `["hook", "flush", "--background", "--cliproot-dir", dir_str]`.
- Name string `"cliproot-flush-hook"` can stay (it's a process label only).
- Update the doc comment on `run()` (line 24) from `cliproot flush-hook` to
  `cliproot hook flush`.

### 2.2 `crates/cliproot-cli/src/commands/compile.rs`

Line 30–33 currently calls `background::spawn(&["compile", "--background-child", ...])`.

- Change to `&["wiki", "compile", "--background-child", ...]`.
- Update doc-comment references (line 1, 46 `eprintln!`) to `cliproot wiki compile`.

### 2.3 `crates/cliproot-mcp/src/service.rs` — three subprocess shell-outs

The MCP service shells out to the CLI to avoid circular crate deps. Update each.

- `cliproot_consolidate` → `cliproot_session_consolidate` (see Phase 4 for the
  MCP tool rename). The `cmd.arg("consolidate")` at line 892 becomes
  `.arg("session").arg("consolidate")`.
- `cliproot_wiki_lint`: `cmd.arg("wiki-lint")` at line 926 → `.arg("wiki").arg("lint")`.
- `cliproot_query` → `cliproot_wiki_query`. `cmd.arg("query")` at line 966 → `.arg("wiki").arg("query")`. The positional `params.prompt` stays the last arg before flags.

### 2.4 `crates/cliproot-cli/src/commands/init/hook_config.rs`

Production strings (install into generated config):

- Line 131: `"cliproot capture-hook"` → `"cliproot hook capture"`.
- Line 132: `"cliproot consolidate-hook"` → `"cliproot hook consolidate"`.
- Line 133: `"cliproot flush-hook"` → `"cliproot hook flush"`.
- Line 137: `"cliproot consolidate-hook --emergency"` → `"cliproot hook consolidate --emergency"`.
- Line 143: `"cliproot session-start-hook --harness claude-code"` → `"cliproot hook session-start --harness claude-code"`.
- Line 173, 181: `"cliproot capture-hook --harness cursor"` → `"cliproot hook capture --harness cursor"`.
- Line 189: `"cliproot consolidate-hook --harness cursor"` → `"cliproot hook consolidate --harness cursor"`.
- Line 197: `"cliproot consolidate-hook --harness cursor --emergency"` → `"cliproot hook consolidate --harness cursor --emergency"`.
- Line 243 (`has_cursor_cliproot_hooks` substring match): `"cliproot capture-hook"` → `"cliproot hook capture"`.

Test strings (assertions):

- Line 314: `"cliproot capture-hook"` → `"cliproot hook capture"`.
- Line 317: `"cliproot consolidate-hook"` → `"cliproot hook consolidate"`.
- Line 320: `"cliproot flush-hook"` → `"cliproot hook flush"`.
- Lines 324–325: `"cliproot consolidate-hook --emergency"` → `"cliproot hook consolidate --emergency"`.
- Lines 330–331: `"cliproot session-start-hook --harness claude-code"` → `"cliproot hook session-start --harness claude-code"`.
- Line 381: `"cliproot capture-hook"` → `"cliproot hook capture"`.
- Line 392 (seed JSON used for "already installed" test): replace every
  `cliproot capture-hook|consolidate-hook|flush-hook|session-start-hook` with
  the `hook <verb>` form (same substitutions above, applied inside the JSON string).
- Line 486 (cursor "already installed" seed JSON): `"cliproot capture-hook --harness cursor"` → `"cliproot hook capture --harness cursor"`.
- Line 244 (`has_cursor_cliproot_hooks` comment/match) already covered above.

### 2.5 Doc-comment / `eprintln!` housekeeping (non-behavioral)

These files contain `cliproot <old-verb>` in doc comments, log strings, or
module-header comments. Update them so grep-for-old-names returns clean:

- `crates/cliproot-cli/src/commands/flush_hook.rs` (lines 24, 50, 70, 108, 118) → `cliproot hook flush`.
- `crates/cliproot-cli/src/commands/session_start_hook.rs` (lines 1, 54, 64) → `cliproot hook session-start`.
- `crates/cliproot-cli/src/commands/compile.rs` (lines 1, 46) → `cliproot wiki compile`.
- `crates/cliproot-cli/src/commands/query.rs` (lines 1, 73, 74, 75) → `cliproot wiki query`.
- `crates/cliproot-cli/src/commands/wiki_lint.rs` (line 1) → `cliproot wiki lint`.
- `crates/cliproot-cli/src/commands/push.rs` (line 31 error string): `"run \`cliproot login\` first"` → `"run \`cliproot remote login\` first"`.
- `crates/cliproot-cli/src/knowledge/compile.rs` (lines 1, 48, 100) → `cliproot wiki compile`.
- `crates/cliproot-cli/src/knowledge/query.rs` (line 1) → `cliproot wiki query`.
- `crates/cliproot-cli/src/knowledge/lint.rs` (lines 1, 16) → `cliproot wiki lint`; `cliproot doctor` → `cliproot doc coverage`.
- `crates/cliproot-cli/src/knowledge/index.rs` (lines 3, 4) → `cliproot wiki compile`; `cliproot session-start-hook` → `cliproot hook session-start`.
- `crates/cliproot-cli/src/knowledge/state.rs` (line 32) → `cliproot wiki compile`.
- `crates/cliproot-store/src/repository.rs` (lines 111, 115) → `cliproot hook session-start`, `cliproot wiki compile`.

### Phase 2 verification

- `cargo test -p cliproot-cli` — all tests in `hook_config.rs` pass.
- `cargo test --workspace` — no regressions.
- `grep -rn "cliproot capture-hook\|cliproot consolidate-hook\|cliproot flush-hook\|cliproot session-start-hook\|cliproot wiki-lint\|cliproot inspect\|cliproot doctor\|cliproot ingest " crates/` returns no hits outside files renamed in Phase 3+.

---

## Phase 3 — Plugin + integration config files

Update generated/checked-in config that runs the CLI.

### 3.1 `.claude-plugin/hooks/*.sh` (5 files)

Edit each `exec cliproot …` line in place:

- `cliproot-capture-hook.sh`: `exec cliproot capture-hook --harness claude-code` → `exec cliproot hook capture --harness claude-code`.
- `cliproot-consolidate-hook.sh`: `consolidate-hook` → `hook consolidate`.
- `cliproot-flush-hook.sh`: `flush-hook` → `hook flush`.
- `cliproot-session-start-hook.sh`: `session-start-hook` → `hook session-start`.
- `cliproot-precompact-hook.sh`: `consolidate-hook --harness claude-code --emergency` → `hook consolidate --harness claude-code --emergency`.

Script filenames stay as-is (external plugin API: `hooks.json` references them
by path).

### 3.2 `.claude-plugin/hooks/hooks.json`

No edits — this file references the `.sh` scripts by `${CLAUDE_PLUGIN_ROOT}/hooks/<file>.sh`,
not CLI verbs. Double-check.

### 3.3 `.claude-plugin/plugin.json`

No edits — `"args": ["mcp"]` is unchanged (mcp singleton stays).

### 3.4 `.claude-plugin/commands/*.md` (5 files)

Rewrite every inline `cliproot <old-verb>` and `cliproot_<old-tool>`:

- `capture.md` — scan for any CLI/MCP names, apply rename table.
- `consolidate.md` line 14: `cliproot consolidate-hook --manual` → the
  manual-consolidate path is now `cliproot session consolidate` (no
  `--manual` flag exists in the current CLI — this was a prose inaccuracy;
  replace with the actual command `cliproot session consolidate --session <id>`).
- `query.md` line 14: `cliproot query` → `cliproot wiki query`; `cliproot_query` → `cliproot_wiki_query`. Line 19: `cliproot trace` → `cliproot clip trace`.
- `session.md` — scan and apply rename table.
- `wiki-lint.md` line 14: `cliproot wiki-lint` → `cliproot wiki lint`; `cliproot_wiki_lint` unchanged. Line 23: `cliproot doctor` → `cliproot doc coverage`.

### 3.5 `integrations/cursor/hooks.json`

Replace every `"command"` value:

- `cliproot capture-hook --harness cursor` → `cliproot hook capture --harness cursor`.
- `cliproot consolidate-hook --harness cursor` → `cliproot hook consolidate --harness cursor`.
- `cliproot consolidate-hook --harness cursor --emergency` → `cliproot hook consolidate --harness cursor --emergency`.

### 3.6 `integrations/cursor/README.md`

Line 73: `cliproot consolidate --session <session-id>` → `cliproot session consolidate --session <session-id>`.
Line 86: `cliproot capture-hook --harness cursor` and `cliproot consolidate-hook --harness cursor` → `cliproot hook capture --harness cursor`, `cliproot hook consolidate --harness cursor`.

### Phase 3 verification

- `bash tests/plugin_hook_composition.sh` still passes (it invokes the `.sh`
  scripts with canned stdin and checks a sentinel — it does not parse CLI verbs,
  so this confirms the scripts still run without syntax errors; note the
  `cliproot` binary inside the scripts may be a no-op in the test harness).
- `grep -rn "capture-hook\|consolidate-hook\|flush-hook\|session-start-hook\|cliproot wiki-lint\|cliproot inspect \|cliproot doctor \|cliproot ingest " .claude-plugin/ integrations/` returns nothing.

---

## Phase 4 — MCP tool renames

Goal: the 14 renamed MCP tools expose their new names; the three shell-outs
already use the new CLI paths (Phase 2.3).

### `crates/cliproot-mcp/src/service.rs`

Rename each async fn. The `#[tool]` proc-macro uses the fn name as the exposed
tool name, so renaming the fn is the rename.

| Old fn name | New fn name |
|---|---|
| `cliproot_clip` | `cliproot_clip_create` |
| `cliproot_derive` | `cliproot_clip_derive` |
| `cliproot_inspect` | `cliproot_clip_get` |
| `cliproot_trace` | `cliproot_clip_trace` |
| `cliproot_verify` | `cliproot_clip_verify` |
| `cliproot_list` | `cliproot_clip_list` |
| `cliproot_search` | `cliproot_clip_search` |
| `cliproot_export` | `cliproot_bundle_export` |
| `cliproot_annotate` | `cliproot_doc_annotate` |
| `cliproot_cite` | `cliproot_doc_cite` |
| `cliproot_doctor` | `cliproot_doc_coverage` |
| `cliproot_consolidate` | `cliproot_session_consolidate` |
| `cliproot_query` | `cliproot_wiki_query` |

`cliproot_wiki_lint` keeps its name.

- No change to param structs in `crates/cliproot-mcp/src/params.rs`.
- No change to the `#[tool(description = "…")]` strings (JSON shapes unchanged).
- The three shell-outs (Phase 2.3) already updated.

### MCP capture-matcher in Cursor hook config

`.cursor/hooks.json` emitted by `hook_config.rs` uses the glob
`"matcher": "mcp__cliproot__*"` — this wildcard covers all renamed tools, no change.

### Phase 4 verification

- `cargo test -p cliproot-mcp` (if there are any; currently none — verify via
  `ls crates/cliproot-mcp/tests`).
- `cargo build --workspace` passes.
- Manual: start `cliproot mcp` and list tools via an MCP client; confirm the
  new names appear and the old names don't.

---

## Phase 5 — Skills, scripts, tests

### 5.1 `skills/cliproot-capture/SKILL.md`

Scan for `cliproot_` and `cliproot ` references, apply rename table. Known hits
from this SKILL.md (from search):

- `cliproot_clip` → `cliproot_clip_create`.
- `cliproot_derive` → `cliproot_clip_derive`.
- `cliproot_query` → `cliproot_wiki_query`.
- `cliproot_consolidate` → `cliproot_session_consolidate`.
- `cliproot_annotate` → `cliproot_doc_annotate`.
- `cliproot_cite` → `cliproot_doc_cite`.
- `cliproot_project_use`, `cliproot_project_create` — unchanged.

### 5.2 `skills/cliproot-session/SKILL.md`

Similar scan. Hits include the tool-table at lines 95–118: apply rename across
every row. Unchanged tools (`project_*`, `activity_*`, `session_*`, `artifact_*`,
`pack_*`, `wiki_lint`) stay as-is.

### 5.3 `skills/cliproot-session/references/tool-reference.md`

Section headings at lines 7, 25, 47, 53, 59, 71, 77, 90, 96, 102, 112, 122, 132,
144, 155, 165, 177, 188 — apply renames. Also update any prose references in
the section bodies that name the tools.

### 5.4 `skills/cliproot-session/references/workflow-examples.md`

Example code blocks at lines 14, 15, 19, 23, 35, 39, 43, 57 — apply renames
(`cliproot_clip` → `cliproot_clip_create`, `cliproot_derive` → `cliproot_clip_derive`,
`cliproot_verify` → `cliproot_clip_verify`, `cliproot_trace` → `cliproot_clip_trace`,
`cliproot_annotate` → `cliproot_doc_annotate`).

### 5.5 `skills/cliproot-capture/agents/openai.yaml` and `skills/cliproot-session/agents/openai.yaml`

Grep for `cliproot_` inside — these YAMLs may enumerate allowed MCP tools or
example calls. Apply renames wherever names appear.

### 5.6 `skills/cliproot-session/scripts/verify-provenance.sh`

- Line 7: `cliproot verify` → `cliproot clip verify`.
- Line 12: `cliproot doctor "$1"` → `cliproot doc coverage "$1"`.

### 5.7 Skill embedding — no code change needed

`crates/cliproot-cli/src/skills.rs` uses `include_str!` to embed the two
`SKILL.md` files and two `openai.yaml` files. Updating the files under
`skills/` automatically re-embeds on next build. No edit to `skills.rs`.

But confirm: the `init --agent` flow writes these embedded files out to the
project's `.claude/skills/` etc. Any snapshot-test that compares written
content against a literal string (grep `agent_config.rs` for assertions with
`cliproot_`) — update the expected strings. Current grep shows no such
string-literal asserts in `agent_config.rs` (only MCP server wiring asserts),
so likely nothing to do, but double-check with
`grep -n "cliproot_" crates/cliproot-cli/src/commands/init/agent_config.rs`.

### 5.8 `tests/plugin_hook_composition.sh`

Scan for any `cliproot …` strings embedded in the test (beyond what the hook
`.sh` scripts already contain). The test currently invokes the `.sh` scripts
and does not parse CLI verbs — likely no change, but verify with
`grep -n "cliproot " tests/plugin_hook_composition.sh`.

### Phase 5 verification

- `cargo build --workspace` succeeds (include_str! still resolves).
- `cargo test --workspace` passes.
- `bash tests/plugin_hook_composition.sh` passes.
- `bash skills/cliproot-session/scripts/verify-provenance.sh` (manual smoke,
  requires a working repo) — the two lines now dispatch through the new grammar.

---

## Phase 6 — Documentation

### 6.1 `README.md`

~40 inline usage examples to update. Key clusters (line numbers from grep;
re-scan before editing since earlier phases may shift them):

- Lines 306–326: `cliproot clip --url …` → `cliproot clip create --url …`;
  `cliproot derive` → `cliproot clip derive`.
- Lines 361–362: `cliproot inspect` → `cliproot clip get`.
- Lines 368–372: `cliproot list` → `cliproot clip list`.
- Line 378: `cliproot trace` → `cliproot clip trace`.
- Lines 386–389: `cliproot verify` → `cliproot clip verify`.
- Lines 395–397: `cliproot export` → `cliproot bundle export`.
- Line 405: `cliproot ingest` → `cliproot bundle import`.
- Lines 446–460: `cliproot annotate` → `cliproot doc annotate`.
- Line 468+: `cliproot cite` → `cliproot doc cite`.
- Any `cliproot doctor` → `cliproot doc coverage`.
- Any `cliproot compile` / `wiki-lint` / `query` → `cliproot wiki compile` / `wiki lint` / `wiki query`.
- Any `cliproot push` / `pull` / `search` / `login` / `logout` → `cliproot remote <verb>`.
- Any hook examples → `cliproot hook <verb>`.
- Any `cliproot record` → `cliproot session record`.

Do a final `grep -n "cliproot " README.md` and sweep anything left.

### 6.2 `claude_skill_demo_script.md`

Same pattern. Specific hits from grep:

- Line 45: `cliproot capture-hook` → `cliproot hook capture`.
- Line 58: `cliproot list` → `cliproot clip list`.
- Line 59: `cliproot trace` → `cliproot clip trace`.
- Line 69: `cliproot flush-hook` (both the hook name and the sentence prose) → `cliproot hook flush`.
- Line 70: `cliproot compile` → `cliproot wiki compile`.
- Line 107–182: all `cliproot query` / `cliproot wiki-lint` / `cliproot compile` / `cliproot doctor` / `cliproot verify` / `cliproot list` / `cliproot inspect` / `cliproot trace` / `cliproot export` references — apply rename.
- Line 231+: error-recovery table — update embedded command names.

### 6.3 `AGENTS.md`, `CONTRIBUTING.md`

Grep for CLI/MCP names and update any that appear.

### 6.4 `CHANGELOG` entry

Add a top-of-file entry describing the cutover. Bump version in
`.claude-plugin/plugin.json` (`"version": "0.5.3"` → next patch/minor per
existing scheme — confirm with JW). Bump workspace crate versions if that's the
project convention (check `crates/*/Cargo.toml` for current pattern — if they
all track `plugin.json`, bump in lockstep; otherwise leave).

### 6.5 New or updated doc

No new docs required. Do **not** write a migration guide — plan §2.2 says no
aliases and no active external audience.

### Phase 6 verification

- `grep -rn "cliproot capture-hook\|cliproot consolidate-hook\|cliproot flush-hook\|cliproot session-start-hook\|cliproot wiki-lint\|cliproot inspect \|cliproot doctor \|cliproot ingest \|cliproot_inspect\|cliproot_doctor\|cliproot_consolidate\b" .` returns nothing except inside this planning doc and any archived `provenance_cloud/docs/` files.
- `grep -rn "^cliproot clip " README.md | head` shows the new examples.

---

## Phase 7 — Full workspace verification

1. `cargo fmt --all -- --check`.
2. `cargo clippy --workspace --all-targets -- -D warnings`.
3. `cargo test --workspace`.
4. `bash tests/plugin_hook_composition.sh`.
5. Build a release binary (`cargo build --release -p cliproot-cli`) and run:
   - `./target/release/cliproot --help` — sanity-check the 16 top-level entries
     match plan §3.
   - `./target/release/cliproot init --agent --hooks` in a scratch dir;
     inspect the generated `.claude/settings.json` and `.cursor/hooks.json`;
     confirm every `"command"` uses the new `hook <verb>` form.
   - Run `./target/release/cliproot mcp` and, from a separate terminal,
     enumerate tools via an MCP client; confirm the 14 renames.
6. Grep sweep (should come up clean):
   ```
   grep -rn "cliproot_inspect\|cliproot_doctor\|cliproot_consolidate\b\|cliproot_annotate\b\|cliproot_cite\b\|cliproot_export\b\|cliproot_clip\b\|cliproot_derive\b\|cliproot_trace\b\|cliproot_verify\b\|cliproot_list\b\|cliproot_search\b\|cliproot_query\b" cliproot_rust/
   grep -rn "capture-hook\|consolidate-hook\|flush-hook\|session-start-hook" cliproot_rust/ \
       | grep -v "^cliproot_rust/.claude-plugin/hooks/cliproot-.*\.sh:" \
       | grep -v "cliproot-hooks\|cliproot-capture-hook.sh\|cliproot-consolidate-hook.sh\|cliproot-flush-hook.sh\|cliproot-session-start-hook.sh\|cliproot-precompact-hook.sh"
   ```
   The only expected survivors: script filenames under `.claude-plugin/hooks/`
   (they keep their kebab-case names as an external-facing plugin contract) and
   any test/docs strings intentionally demonstrating a pre-rename literal (there
   should be none).

---

## Risk log (read before starting)

1. **Clap attribute name-vs-kebab.** `HookCommands::SessionStart` will render
   as `session-start` by clap default, but the explicit `#[command(name = "session-start")]`
   makes it unambiguous. Do the same for any multi-word subcommands if clap's
   default ever diverges.

2. **`clip create` field set.** The current top-level `Clip` variant has 10
   fields (`url`, `quote`, `source_type`, `id`, `document_id`, `project`,
   `title`, `activity`, `session`, `copy`). Move them verbatim into
   `ClipCommands::Create { … }`. Don't drop any.

3. **`session record` field set.** The current top-level `Record` has 10 fields
   and uses `commands::record::RecordOptions { … }` as an intermediate. Keep
   that struct; only the enum variant moves under `SessionCommands::Record`.

4. **`consolidate` has two callers.** The manual-consolidate flag set
   (`--session`, `--emergency`, `--commit`) becomes `session consolidate`. The
   hook-harness flag set (`--harness`, `--emergency`) becomes `hook consolidate`.
   They call different backend functions (`commands::consolidate::run` and
   `commands::consolidate_hook::run`). Do **not** unify them; only their CLI
   paths reorganize.

5. **MCP tool name collision resolved in one step.** `cliproot_search` (MCP
   local-text) and CLI `search` (remote registry) are disambiguated: MCP becomes
   `cliproot_clip_search`, CLI becomes `remote search`. There is no CLI
   counterpart to `cliproot_clip_search` — this is intentional (local-text
   search stays MCP-only per plan §3.1 and §4).

6. **`.claude-plugin/hooks/*.sh` filenames are plugin API.** Keep the
   kebab-case filenames (`cliproot-capture-hook.sh`, etc.) even though the
   internal verb changed. `hooks.json` references them by path and renaming
   would silently break installed plugins that cache the path.

7. **Version bump.** This is a breaking change. Confirm with JW whether the
   version bump is minor (`0.5.3` → `0.6.0`) or patch before shipping.

---

## Suggested PR structure (single PR, many commits)

Order commits inside the PR to match the phases above:

1. `cli: restructure Commands enum into noun-first grammar` (Phase 1)
2. `cli: update self-spawn args + mcp subprocess paths + hook_config generator` (Phase 2)
3. `plugin,cursor: rewrite hook scripts and hooks.json to hook-verb grammar` (Phase 3)
4. `mcp: rename 13 tools to noun-first` (Phase 4)
5. `skills: update SKILL.md tool references for new grammar` (Phase 5)
6. `docs: rewrite README and demo script for new grammar; CHANGELOG` (Phase 6)

Each commit should compile (except Phase 1 intentionally leaves
`hook_config.rs` tests failing; those are fixed in Phase 2's single commit —
avoid splitting 1 and 2 across multiple PRs).
