use std::fs;
use std::path::Path;

use super::agent_config::ConfigAction;

pub fn install_hooks(root: &Path) -> Result<ConfigAction, Box<dyn std::error::Error>> {
    let path = root.join(".claude/settings.json");
    let existed = path.exists();

    let mut doc = if existed {
        let content = fs::read_to_string(&path)?;
        serde_json::from_str::<serde_json::Value>(&content)?
    } else {
        serde_json::json!({})
    };

    let obj = doc
        .as_object_mut()
        .ok_or("settings.json is not a JSON object")?;

    // Navigate/create: hooks -> PostToolUse (array)
    let hooks = obj.entry("hooks").or_insert_with(|| serde_json::json!({}));
    let hooks_obj = hooks.as_object_mut().ok_or("hooks is not a JSON object")?;

    let post_tool_use = hooks_obj
        .entry("PostToolUse")
        .or_insert_with(|| serde_json::json!([]));
    let post_tool_use_arr = post_tool_use
        .as_array_mut()
        .ok_or("PostToolUse is not a JSON array")?;

    // Check if cliproot capture-hook is already installed
    let already_installed = post_tool_use_arr.iter().any(|entry| {
        entry
            .get("hooks")
            .and_then(|h| h.as_array())
            .map(|hooks| {
                hooks.iter().any(|hook| {
                    hook.get("command")
                        .and_then(|c| c.as_str())
                        .map(|cmd| cmd.contains("cliproot capture-hook"))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    });

    if already_installed {
        return Ok(ConfigAction::Skipped(path));
    }

    // Add the hook entry
    let hook_entry = serde_json::json!({
        "hooks": [{
            "type": "command",
            "command": "cliproot capture-hook"
        }]
    });
    post_tool_use_arr.push(hook_entry);

    // Write back
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_settings_created() {
        let dir = tempfile::tempdir().unwrap();
        let action = install_hooks(dir.path()).unwrap();
        assert!(matches!(action, ConfigAction::Created(_)));

        let content: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(dir.path().join(".claude/settings.json")).unwrap(),
        )
        .unwrap();

        let hook = &content["hooks"]["PostToolUse"][0]["hooks"][0];
        assert_eq!(hook["type"], "command");
        assert_eq!(hook["command"], "cliproot capture-hook");
    }

    #[test]
    fn merged_with_existing_settings() {
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();
        fs::write(
            claude_dir.join("settings.json"),
            r#"{"permissions": {"allow": ["Bash(ls)"]}}"#,
        )
        .unwrap();

        let action = install_hooks(dir.path()).unwrap();
        assert!(matches!(action, ConfigAction::Merged(_)));

        let content: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(claude_dir.join("settings.json")).unwrap())
                .unwrap();

        // Original permissions preserved
        assert_eq!(content["permissions"]["allow"][0], "Bash(ls)");
        // Hook added
        assert_eq!(
            content["hooks"]["PostToolUse"][0]["hooks"][0]["command"],
            "cliproot capture-hook"
        );
    }

    #[test]
    fn skipped_if_already_installed() {
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();
        fs::write(
            claude_dir.join("settings.json"),
            r#"{"hooks":{"PostToolUse":[{"hooks":[{"type":"command","command":"cliproot capture-hook"}]}]}}"#,
        )
        .unwrap();

        let action = install_hooks(dir.path()).unwrap();
        assert!(matches!(action, ConfigAction::Skipped(_)));
    }

    #[test]
    fn coexists_with_other_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();
        fs::write(
            claude_dir.join("settings.json"),
            r#"{"hooks":{"PostToolUse":[{"hooks":[{"type":"command","command":"other-tool hook"}]}]}}"#,
        )
        .unwrap();

        let action = install_hooks(dir.path()).unwrap();
        assert!(matches!(action, ConfigAction::Merged(_)));

        let content: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(claude_dir.join("settings.json")).unwrap())
                .unwrap();

        let hooks = content["hooks"]["PostToolUse"].as_array().unwrap();
        assert_eq!(hooks.len(), 2);
    }
}
