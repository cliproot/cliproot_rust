//! Embedded skill content for `cliproot init --agent`.
//!
//! These constants are compiled into the binary from the source-of-truth files
//! under `skills/cliproot-capture/` at the workspace root.

pub const SKILL_MD: &str = include_str!("../../../skills/cliproot-capture/SKILL.md");
pub const OPENAI_YAML: &str = include_str!("../../../skills/cliproot-capture/agents/openai.yaml");
