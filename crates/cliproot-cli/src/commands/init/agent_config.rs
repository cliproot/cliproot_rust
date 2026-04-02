use std::fs;
use std::path::{Path, PathBuf};

use crate::skills;

// ── Result types ────────────────────────────────────────────────────────────

pub enum ConfigAction {
    Created(PathBuf),
    Merged(PathBuf),
    Skipped(PathBuf),
}

impl ConfigAction {
    pub fn symbol(&self) -> &str {
        match self {
            ConfigAction::Created(_) => "+",
            ConfigAction::Merged(_) => "~",
            ConfigAction::Skipped(_) => "=",
        }
    }

    pub fn path_display(&self, root: &Path) -> String {
        let path = match self {
            ConfigAction::Created(p) | ConfigAction::Merged(p) | ConfigAction::Skipped(p) => p,
        };
        path.strip_prefix(root)
            .unwrap_or(path)
            .display()
            .to_string()
    }
}

// ── Public API ──────────────────────────────────────────────────────────────

pub fn generate_all(root: &Path) -> Result<Vec<ConfigAction>, Box<dyn std::error::Error>> {
    let mut actions = Vec::new();

    // Claude Code
    actions.push(upsert_mcp_json(root, ".mcp.json", "mcpServers")?);
    actions.extend(write_skill_dir(root, ".claude/skills/cliproot-capture")?);

    // Cursor
    actions.push(upsert_mcp_json(root, ".cursor/mcp.json", "mcpServers")?);
    actions.push(write_cursor_rule(root)?);

    // VS Code / Copilot
    actions.push(upsert_mcp_json(root, ".vscode/mcp.json", "servers")?);

    // Universal Agent Skills (Codex, Gemini CLI, Junie, Goose, etc.)
    actions.extend(write_skill_dir(root, ".agents/skills/cliproot-capture")?);
    actions.push(write_codex_yaml(root)?);

    // Windsurf
    actions.push(write_windsurf_rule(root)?);

    Ok(actions)
}

pub fn print_codex_instructions() {
    println!();
    println!("To configure OpenAI Codex, add to ~/.codex/config.toml:");
    println!();
    println!("  [mcp_servers.cliproot]");
    println!("  command = \"cliproot\"");
    println!("  args = [\"mcp\"]");
}

// ── JSON MCP config merge ───────────────────────────────────────────────────

fn upsert_mcp_json(
    root: &Path,
    rel_path: &str,
    server_key: &str,
) -> Result<ConfigAction, Box<dyn std::error::Error>> {
    let path = root.join(rel_path);
    let cliproot_entry = serde_json::json!({
        "command": "cliproot",
        "args": ["mcp"]
    });

    let existed = path.exists();
    let mut doc = if existed {
        let content = fs::read_to_string(&path)?;
        serde_json::from_str::<serde_json::Value>(&content)?
    } else {
        serde_json::json!({})
    };

    let servers = doc
        .as_object_mut()
        .ok_or("MCP config is not a JSON object")?
        .entry(server_key)
        .or_insert_with(|| serde_json::json!({}));

    let servers_obj = servers
        .as_object_mut()
        .ok_or("servers key is not a JSON object")?;

    if servers_obj.contains_key("cliproot") {
        return Ok(ConfigAction::Skipped(path));
    }

    servers_obj.insert("cliproot".to_string(), cliproot_entry);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_json::to_string_pretty(&doc)? + "\n")?;

    Ok(if existed {
        ConfigAction::Merged(path)
    } else {
        ConfigAction::Created(path)
    })
}

// ── Skill directory writer ──────────────────────────────────────────────────

fn write_skill_dir(
    root: &Path,
    rel_dir: &str,
) -> Result<Vec<ConfigAction>, Box<dyn std::error::Error>> {
    let base = root.join(rel_dir);

    let actions = vec![write_file(base.join("SKILL.md"), skills::SKILL_MD)?];

    Ok(actions)
}

// ── Platform-specific rules ─────────────────────────────────────────────────

fn write_cursor_rule(root: &Path) -> Result<ConfigAction, Box<dyn std::error::Error>> {
    let body = strip_yaml_frontmatter(skills::SKILL_MD);
    let content = format!(
        "---\n\
         description: \"Lightweight provenance capture using Cliproot. Activate when doing research or writing cited documents.\"\n\
         alwaysApply: false\n\
         ---\n\
         {body}"
    );
    write_file(root.join(".cursor/rules/cliproot-capture.mdc"), &content)
}

fn write_windsurf_rule(root: &Path) -> Result<ConfigAction, Box<dyn std::error::Error>> {
    let body = strip_yaml_frontmatter(skills::SKILL_MD);
    write_file(root.join(".windsurf/rules/cliproot-capture.md"), body)
}

fn write_codex_yaml(root: &Path) -> Result<ConfigAction, Box<dyn std::error::Error>> {
    write_file(
        root.join(".agents/skills/cliproot-capture/agents/openai.yaml"),
        skills::OPENAI_YAML,
    )
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn write_file(path: PathBuf, content: &str) -> Result<ConfigAction, Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, content)?;
    Ok(ConfigAction::Created(path))
}

/// Strip YAML frontmatter (delimited by `---`) from a markdown document,
/// returning the body after the closing `---`.
fn strip_yaml_frontmatter(md: &str) -> &str {
    let trimmed = md.trim_start();
    if !trimmed.starts_with("---") {
        return md;
    }
    // Find the closing ---
    if let Some(end) = trimmed[3..].find("\n---") {
        let after_closing = &trimmed[3 + end + 4..]; // skip past "\n---"
        after_closing.trim_start_matches('\n')
    } else {
        md
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fresh_mcp_json_created() {
        let dir = tempfile::tempdir().unwrap();
        let action = upsert_mcp_json(dir.path(), ".mcp.json", "mcpServers").unwrap();
        assert!(matches!(action, ConfigAction::Created(_)));

        let content: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.path().join(".mcp.json")).unwrap())
                .unwrap();
        assert_eq!(content["mcpServers"]["cliproot"]["command"], "cliproot");
        assert_eq!(content["mcpServers"]["cliproot"]["args"][0], "mcp");
    }

    #[test]
    fn test_mcp_json_merged_with_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".mcp.json");
        fs::write(
            &path,
            r#"{"mcpServers": {"other-server": {"command": "other", "args": []}}}"#,
        )
        .unwrap();

        let action = upsert_mcp_json(dir.path(), ".mcp.json", "mcpServers").unwrap();
        assert!(matches!(action, ConfigAction::Merged(_)));

        let content: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        // Original server preserved
        assert_eq!(content["mcpServers"]["other-server"]["command"], "other");
        // Cliproot added
        assert_eq!(content["mcpServers"]["cliproot"]["command"], "cliproot");
    }

    #[test]
    fn test_mcp_json_skipped_if_present() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".mcp.json");
        fs::write(
            &path,
            r#"{"mcpServers": {"cliproot": {"command": "cliproot", "args": ["mcp"]}}}"#,
        )
        .unwrap();

        let action = upsert_mcp_json(dir.path(), ".mcp.json", "mcpServers").unwrap();
        assert!(matches!(action, ConfigAction::Skipped(_)));
    }

    #[test]
    fn test_vscode_uses_servers_key() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".vscode")).unwrap();
        upsert_mcp_json(dir.path(), ".vscode/mcp.json", "servers").unwrap();

        let content: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.path().join(".vscode/mcp.json")).unwrap())
                .unwrap();
        assert_eq!(content["servers"]["cliproot"]["command"], "cliproot");
        // Should NOT have mcpServers key
        assert!(content.get("mcpServers").is_none());
    }

    #[test]
    fn test_cursor_mdc_has_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        write_cursor_rule(dir.path()).unwrap();

        let content =
            fs::read_to_string(dir.path().join(".cursor/rules/cliproot-capture.mdc")).unwrap();
        assert!(content.starts_with("---\n"));
        assert!(content.contains("alwaysApply: false"));
        // Should contain skill body but NOT the Agent Skills frontmatter
        assert!(content.contains("## Principles"));
        assert!(!content.contains("name: cliproot-capture"));
    }

    #[test]
    fn test_skill_directory_complete() {
        let dir = tempfile::tempdir().unwrap();
        let actions = write_skill_dir(dir.path(), ".claude/skills/cliproot-capture").unwrap();
        assert_eq!(actions.len(), 1);

        let base = dir.path().join(".claude/skills/cliproot-capture");
        assert!(base.join("SKILL.md").exists());
    }

    #[test]
    fn test_strip_yaml_frontmatter() {
        let input = "---\nname: test\n---\n\nHello world";
        assert_eq!(strip_yaml_frontmatter(input), "Hello world");
    }

    #[test]
    fn test_strip_yaml_frontmatter_no_frontmatter() {
        let input = "Hello world";
        assert_eq!(strip_yaml_frontmatter(input), "Hello world");
    }

    #[test]
    fn test_generate_all_creates_all_files() {
        let dir = tempfile::tempdir().unwrap();
        let actions = generate_all(dir.path()).unwrap();

        // Should have: 3 MCP JSONs + 1 Claude skill file + 1 Cursor rule +
        //              1 Agent skill file + 1 openai.yaml + 1 Windsurf rule = 8
        assert_eq!(actions.len(), 8);

        // Spot check key files
        assert!(dir.path().join(".mcp.json").exists());
        assert!(dir
            .path()
            .join(".claude/skills/cliproot-capture/SKILL.md")
            .exists());
        assert!(dir.path().join(".cursor/mcp.json").exists());
        assert!(dir
            .path()
            .join(".cursor/rules/cliproot-capture.mdc")
            .exists());
        assert!(dir.path().join(".vscode/mcp.json").exists());
        assert!(dir
            .path()
            .join(".agents/skills/cliproot-capture/SKILL.md")
            .exists());
        assert!(dir
            .path()
            .join(".agents/skills/cliproot-capture/agents/openai.yaml")
            .exists());
        assert!(dir
            .path()
            .join(".windsurf/rules/cliproot-capture.md")
            .exists());
    }
}
