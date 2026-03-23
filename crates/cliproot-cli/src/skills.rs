//! Embedded skill content for `cliproot init --agent`.
//!
//! These constants are compiled into the binary from the source-of-truth files
//! under `skills/cliproot-research/` at the workspace root.

pub const SKILL_MD: &str = include_str!("../../../skills/cliproot-research/SKILL.md");
pub const TOOL_REFERENCE_MD: &str =
    include_str!("../../../skills/cliproot-research/references/tool-reference.md");
pub const WORKFLOW_EXAMPLES_MD: &str =
    include_str!("../../../skills/cliproot-research/references/workflow-examples.md");
pub const VERIFY_SCRIPT: &str =
    include_str!("../../../skills/cliproot-research/scripts/verify-provenance.sh");
pub const OPENAI_YAML: &str =
    include_str!("../../../skills/cliproot-research/agents/openai.yaml");
