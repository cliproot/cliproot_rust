# /cliproot:consolidate

Manually trigger consolidation of unhighlighted sources.

## When to use

- When you want to review recent activity before moving on
- If you dismissed the automatic consolidation prompt but want to revisit
- To check for sources that might need clipping before context gets too long
- Before generating final output to ensure all key sources are highlighted

## What it does

Runs `cliproot session consolidate --session <id>` to scan recent tool usage and identify:
- Sources consulted but not yet clipped
- Files written that might need derivation records
- Opportunities to strengthen provenance before output

## Output

If unhighlighted sources are found, you'll see a report listing:
- URLs/documents consulted
- Files modified
- Recommended actions (clip specific passages, record derivations)

If everything is already captured, you'll see confirmation that no action is needed.
