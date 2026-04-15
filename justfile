verify-plugin:
    # Symlinks resolve
    test -e .claude-plugin/skills/cliproot-capture/SKILL.md
    test -e .claude-plugin/skills/cliproot-session/SKILL.md
    # install-cliproot.sh is executable
    test -x .claude-plugin/bin/install-cliproot.sh
    # session-start shell wrapper is executable (Phase D)
    test -x .claude-plugin/hooks/cliproot-session-start-hook.sh
    # plugin.json valid JSON
    jq empty .claude-plugin/plugin.json
    # .mcp.json valid JSON
    jq empty .claude-plugin/.mcp.json
    # hooks.json valid JSON
    jq empty .claude-plugin/hooks/hooks.json
    # hooks.json advertises the SessionStart entry (Phase D)
    jq -e '.hooks.SessionStart[0].hooks[0].command | test("cliproot-session-start-hook.sh$")' .claude-plugin/hooks/hooks.json

# Sync plugin skill symlinks from the authoritative skills/ directory.
# Run this after adding a new skill or if a symlink gets accidentally replaced.
sync-skills:
    ln -sfn ../../../skills/cliproot-capture .claude-plugin/skills/cliproot-capture
    ln -sfn ../../../skills/cliproot-session .claude-plugin/skills/cliproot-session
    # When .codex-plugin/ is added (Step 4), extend here:
    # ln -sfn ../../../skills/cliproot-capture .codex-plugin/skills/cliproot-capture
    # ln -sfn ../../../skills/cliproot-session .codex-plugin/skills/cliproot-session
