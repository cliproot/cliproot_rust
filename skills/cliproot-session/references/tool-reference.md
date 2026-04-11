# Cliproot MCP Tool Reference

Complete parameter documentation for the Phase 3 Cliproot MCP tools.

---

## cliproot_clip

Capture a source clip from a URL with exact quoted text.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `url` | string | yes | — | The source URL where the quoted text was found |
| `quote` | string | yes | — | The exact quoted text to capture |
| `source_type` | string | no | `"external-quoted"` | One of: `external-quoted`, `human-authored`, `ai-generated`, `ai-assisted`, `unknown` |
| `id` | string | no | — | Stable human-readable clip ID (e.g. `"clip-redis-001"`) |
| `document_id` | string | no | — | Document ID to group this clip with others |
| `title` | string | no | — | Human-readable title for the source |
| `project` | string | no | — | Project ID override |
| `activity_id` | string | no | — | Activity ID for prompt-scoped provenance |
| `session_id` | string | no | — | Session ID for session-scoped provenance |

---

## cliproot_derive

Create a derived clip from one or more parent clips.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `from` | string[] | yes | — | Parent clip hashes (`sha256-...`) or clip IDs to derive from |
| `quote` | string | yes | — | The derived text content |
| `transformation_type` | string | yes | — | One of: `verbatim`, `quote`, `summary`, `paraphrase`, `translate`, `combine`, `edit`, `ai_generate`, `unknown` |
| `agent` | string | no | — | Agent ID (e.g. model identifier like `"claude-opus-4"`) |
| `project` | string | no | — | Project ID override |
| `activity_id` | string | no | — | Activity ID for prompt-scoped provenance |
| `session_id` | string | no | — | Session ID for session-scoped provenance |

---

## cliproot_project_create / list / use / delete

Project management tools for scoping work before clips, artifacts, and packs are created.

---

## cliproot_artifact_add / list / get / link

Artifact management tools for storing markdown, JSON, prompt, session, and other file content.

---

## cliproot_pack_create / import / inspect / verify

Pack management tools for creating and restoring `.cliprootpack` archives.

---

## cliproot_activity_start / end

Start and end prompt-scoped activities. Activities capture:

- `activity_type`
- `prompt`
- optional `parameters`
- generated clip hashes
- used refs gathered during the activity

---

## cliproot_session_start / end

Start and end restorable agent sessions. Session end materializes a `session` artifact and links it to generated clips with `attached_to`.

---

## cliproot_inspect / trace / list / search

Exploration and lineage tools for understanding the current provenance graph before creating new work.

Use these to:

- inspect full clip metadata
- trace derivation chains
- search existing content
- avoid duplicating already-grounded research

---

## cliproot_verify / doctor

Validation tools for checking integrity and provenance coverage before final output.

---

## cliproot_annotate / cite / export

Output tools for surfacing provenance in final deliverables and sharing provenance lineage with others.

---

## cliproot_inspect

Display full details of a clip.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `hash_or_id` | string | yes | — | Clip hash (`sha256-...`) or clip ID |

---

## cliproot_trace

Show the ancestor lineage tree for a clip.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `hash_or_id` | string | yes | — | Clip hash (`sha256-...`) or clip ID to trace lineage for |

---

## cliproot_verify

Verify hash integrity of clips.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `hash_or_id` | string | no | — | Clip hash or ID to verify. If omitted, verifies all clips in the store |

---

## cliproot_list

List clips with optional filters.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `document_id` | string | no | — | Filter clips by document ID |
| `source_type` | string | no | — | Filter clips by source type |
| `limit` | integer | no | `50` | Maximum number of clips to return |

---

## cliproot_search

Search clip content by text.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `query` | string | yes | — | Text to search for in clip content (case-insensitive substring match) |
| `limit` | integer | no | `20` | Maximum number of results to return |

---

## cliproot_export

Export a clip with its full provenance lineage as a CRP bundle.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `hash_or_id` | string | yes | — | Clip hash (`sha256-...`) or clip ID to export |

---

## cliproot_annotate

Insert inline citations into a document by matching text against stored clips.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `document_text` | string | yes | — | The document text to annotate with citations |
| `style` | string | no | `"footnote"` | Annotation style: `footnote`, `inline-comment`, `bracket` |
| `threshold` | float | no | `0.4` | Minimum match confidence threshold (0.0–1.0) |

---

## cliproot_cite

Generate a bibliography/citation list from clip provenance.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `document_text` | string | yes | — | The document text to generate citations for |
| `threshold` | float | no | `0.4` | Minimum match confidence threshold (0.0–1.0) |

---

## cliproot_doctor

Produce a provenance coverage report for a document.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `document_text` | string | yes | — | The document text to analyze for provenance coverage |
| `threshold` | float | no | `0.4` | Minimum match confidence threshold (0.0–1.0) |
