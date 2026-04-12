# Sync plugin skill copies from the authoritative skills/ directory.
# Run this after editing any file under skills/.
sync-skills:
    cp skills/cliproot-capture/SKILL.md .claude-plugin/skills/cliproot-capture/SKILL.md
    cp skills/cliproot-session/SKILL.md .claude-plugin/skills/cliproot-session/SKILL.md
    # When .codex-plugin/ is added (Step 4), extend here:
    # cp skills/cliproot-capture/SKILL.md .codex-plugin/skills/cliproot-capture/SKILL.md
    # cp skills/cliproot-session/SKILL.md .codex-plugin/skills/cliproot-session/SKILL.md
