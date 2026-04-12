# /cliproot:session

Begin a full-ceremony provenance-tracked research session with explicit session and activity tracking.

## When to use

- Multi-agent research that needs handoff context
- Long-running research requiring restorable state
- Work that needs explicit audit trails for compliance
- Projects requiring structured activity boundaries

## What it does

Activates the cliproot-session skill, which adds:
- Session lifecycle (start/end) for restorable context
- Activity tracking for prompt-scoped work units
- Full ceremony around project, session, activity, capture, and synthesis

## Initial steps

1. **Project**: `cliproot_project_use` to scope the work
2. **Session**: `cliproot_session_start` with project_id and agent_id
3. **Activity**: `cliproot_activity_start` when beginning focused work
4. **Capture**: `cliproot_clip` with activity_id and session_id
5. **Synthesize**: `cliproot_derive` for combined/summarized content
6. **Validate**: `cliproot_verify` and `cliproot_doctor` for integrity checks
7. **Output**: `cliproot_annotate`, `cliproot_cite`, `cliproot_artifact_add` for deliverables
8. **End**: `cliproot_activity_end` and `cliproot_session_end` to finalize

## Session lifecycle

Keep the session_id returned from `cliproot_session_start` and pass it through all subsequent calls. End with `cliproot_session_end` to create a restorable artifact that other agents can pick up.

## Consolidation

Same as capture mode — the hooks will prompt you to review unhighlighted sources at natural stopping points.
