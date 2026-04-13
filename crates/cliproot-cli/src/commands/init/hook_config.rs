use std::fs;
use std::path::Path;

use super::agent_config::ConfigAction;

/// Which harness(es) to install hooks for
#[derive(Debug, Clone, Copy, Default)]
pub enum HarnessSelection {
    /// Install hooks for all detected harnesses (Claude + Cursor if configs exist)
    #[default]
    Auto,
    /// Install only Claude Code hooks
    #[allow(dead_code)]
    ClaudeOnly,
    /// Install only Cursor hooks
    #[allow(dead_code)]
    CursorOnly,
}

/// Install a single hook entry under `hooks.<event_name>` if not already present.
/// Returns true if the hook was newly added.
fn install_hook_entry(
    hooks_obj: &mut serde_json::Map<String, serde_json::Value>,
    event_name: &str,
    command: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    let event_arr = hooks_obj
        .entry(event_name)
        .or_insert_with(|| serde_json::json!([]));
    let arr = event_arr
        .as_array_mut()
        .ok_or(format!("{event_name} is not a JSON array"))?;

    let already_installed = arr.iter().any(|entry| {
        entry
            .get("hooks")
            .and_then(|h| h.as_array())
            .map(|hooks| {
                hooks.iter().any(|hook| {
                    hook.get("command")
                        .and_then(|c| c.as_str())
                        .map(|cmd| cmd.contains(command))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    });

    if already_installed {
        return Ok(false);
    }

    let hook_entry = serde_json::json!({
        "hooks": [{
            "type": "command",
            "command": command
        }]
    });
    arr.push(hook_entry);
    Ok(true)
}

/// Install hooks for the selected harness(es)
#[allow(dead_code)]
pub fn install_hooks(root: &Path) -> Result<Vec<ConfigAction>, Box<dyn std::error::Error>> {
    install_hooks_for(root, HarnessSelection::Auto)
}

/// Install hooks with explicit harness selection
pub fn install_hooks_for(
    root: &Path,
    selection: HarnessSelection,
) -> Result<Vec<ConfigAction>, Box<dyn std::error::Error>> {
    let mut actions = Vec::new();

    // Determine which harnesses to install
    let install_claude = matches!(
        selection,
        HarnessSelection::Auto | HarnessSelection::ClaudeOnly
    );
    let install_cursor = matches!(
        selection,
        HarnessSelection::Auto | HarnessSelection::CursorOnly
    );

    // Check for existing MCP configs to determine what's installed
    let claude_mcp_exists = root.join(".mcp.json").exists()
        || root.join(".claude/mcp.json").exists()
        || root.join(".claude/settings.json").exists();
    let cursor_mcp_exists = root.join(".cursor/mcp.json").exists();

    if install_claude && (matches!(selection, HarnessSelection::ClaudeOnly) || claude_mcp_exists) {
        if let Ok(action) = install_claude_hooks(root) {
            actions.push(action);
        }
    }

    if install_cursor && (matches!(selection, HarnessSelection::CursorOnly) || cursor_mcp_exists) {
        if let Ok(action) = install_cursor_hooks(root) {
            actions.push(action);
        }
    }

    // If auto mode and nothing detected, default to Claude hooks for backward compatibility
    if actions.is_empty() && matches!(selection, HarnessSelection::Auto) {
        actions.push(install_claude_hooks(root)?);
    }

    Ok(actions)
}

fn install_claude_hooks(root: &Path) -> Result<ConfigAction, Box<dyn std::error::Error>> {
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

    let hooks = obj.entry("hooks").or_insert_with(|| serde_json::json!({}));
    let hooks_obj = hooks.as_object_mut().ok_or("hooks is not a JSON object")?;

    let mut any_added = false;
    any_added |= install_hook_entry(hooks_obj, "PostToolUse", "cliproot capture-hook")?;
    any_added |= install_hook_entry(hooks_obj, "Stop", "cliproot consolidate-hook")?;
    any_added |= install_hook_entry(
        hooks_obj,
        "PreCompact",
        "cliproot consolidate-hook --emergency",
    )?;

    if !any_added {
        return Ok(ConfigAction::Skipped(path));
    }

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

/// Install Cursor hooks to `.cursor/hooks.json`
fn install_cursor_hooks(root: &Path) -> Result<ConfigAction, Box<dyn std::error::Error>> {
    let path = root.join(".cursor/hooks.json");
    let existed = path.exists();

    let hook_config = serde_json::json!({
        "version": 1,
        "hooks": {
            "postToolUse": [
                {
                    "command": "cliproot capture-hook --harness cursor",
                    "matcher": "WebFetch|Read|Write|Edit|Bash|Agent",
                    "type": "command",
                    "timeout": 30,
                    "failClosed": false
                },
                {
                    "command": "cliproot capture-hook --harness cursor",
                    "matcher": "mcp__cliproot__*",
                    "type": "command",
                    "timeout": 30,
                    "failClosed": false
                }
            ],
            "stop": [
                {
                    "command": "cliproot consolidate-hook --harness cursor",
                    "type": "command",
                    "timeout": 60,
                    "failClosed": false
                }
            ],
            "preCompact": [
                {
                    "command": "cliproot consolidate-hook --harness cursor --emergency",
                    "type": "command",
                    "timeout": 60,
                    "failClosed": false
                }
            ]
        }
    });

    let doc = if existed {
        let content = fs::read_to_string(&path)?;
        let existing: serde_json::Value = serde_json::from_str(&content)?;

        // Check if cliproot hooks are already present
        if has_cursor_cliproot_hooks(&existing) {
            return Ok(ConfigAction::Skipped(path));
        }

        // Merge: Cursor hooks are top-level, so we need to merge carefully
        merge_cursor_hooks(existing, &hook_config)?
    } else {
        hook_config
    };

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

/// Check if cursor hooks.json already has cliproot hooks installed
fn has_cursor_cliproot_hooks(doc: &serde_json::Value) -> bool {
    doc.get("hooks")
        .and_then(|h| h.get("postToolUse"))
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter().any(|entry| {
                entry
                    .get("command")
                    .and_then(|c| c.as_str())
                    .map(|cmd| cmd.contains("cliproot capture-hook"))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

/// Merge Cursor hooks config with existing
fn merge_cursor_hooks(
    existing: serde_json::Value,
    new_hooks: &serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut result = existing;

    let hooks_obj = result
        .as_object_mut()
        .ok_or("existing hooks.json is not an object")?
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));

    let hooks_map = hooks_obj.as_object_mut().ok_or("hooks is not an object")?;

    let new_hooks_map = new_hooks
        .get("hooks")
        .and_then(|h| h.as_object())
        .ok_or("new hooks config is invalid")?;

    for (event_name, new_entries) in new_hooks_map {
        let event_arr = hooks_map
            .entry(event_name.clone())
            .or_insert_with(|| serde_json::json!([]));
        let arr = event_arr
            .as_array_mut()
            .ok_or(format!("{event_name} is not an array"))?;

        // Add new entries that don't already exist
        if let Some(new_entries_arr) = new_entries.as_array() {
            for new_entry in new_entries_arr {
                let cmd = new_entry.get("command").and_then(|c| c.as_str());
                let already_exists = arr.iter().any(|existing_entry| {
                    existing_entry.get("command").and_then(|c| c.as_str()) == cmd
                });

                if !already_exists {
                    arr.push(new_entry.clone());
                }
            }
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_settings_created() {
        let dir = tempfile::tempdir().unwrap();
        let actions = install_hooks_for(dir.path(), HarnessSelection::ClaudeOnly).unwrap();
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], ConfigAction::Created(_)));

        let content: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(dir.path().join(".claude/settings.json")).unwrap(),
        )
        .unwrap();

        let hook = &content["hooks"]["PostToolUse"][0]["hooks"][0];
        assert_eq!(hook["type"], "command");
        assert_eq!(hook["command"], "cliproot capture-hook");

        let stop_hook = &content["hooks"]["Stop"][0]["hooks"][0];
        assert_eq!(stop_hook["command"], "cliproot consolidate-hook");

        let precompact_hook = &content["hooks"]["PreCompact"][0]["hooks"][0];
        assert_eq!(
            precompact_hook["command"],
            "cliproot consolidate-hook --emergency"
        );
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

        let actions = install_hooks_for(dir.path(), HarnessSelection::ClaudeOnly).unwrap();
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], ConfigAction::Merged(_)));

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
            r#"{"hooks":{"PostToolUse":[{"hooks":[{"type":"command","command":"cliproot capture-hook"}]}],"Stop":[{"hooks":[{"type":"command","command":"cliproot consolidate-hook"}]}],"PreCompact":[{"hooks":[{"type":"command","command":"cliproot consolidate-hook --emergency"}]}]}}"#,
        )
        .unwrap();

        let actions = install_hooks_for(dir.path(), HarnessSelection::ClaudeOnly).unwrap();
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], ConfigAction::Skipped(_)));
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

        let actions = install_hooks(dir.path()).unwrap();
        let action = actions
            .iter()
            .find(|a| a.path_display(dir.path()).contains("settings.json"))
            .expect("should have installed Claude hooks");
        assert!(matches!(action, ConfigAction::Merged(_)));

        let content: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(claude_dir.join("settings.json")).unwrap())
                .unwrap();

        let hooks = content["hooks"]["PostToolUse"].as_array().unwrap();
        assert_eq!(hooks.len(), 2);
    }

    // ── Cursor hooks tests ─────────────────────────────────────────────────

    #[test]
    fn cursor_hooks_created() {
        let dir = tempfile::tempdir().unwrap();

        // Simulate existing Cursor MCP config
        let cursor_dir = dir.path().join(".cursor");
        fs::create_dir_all(&cursor_dir).unwrap();
        fs::write(
            cursor_dir.join("mcp.json"),
            r#"{"mcpServers": {"cliproot": {"command": "cliproot", "args": ["mcp"]}}}"#,
        )
        .unwrap();

        let actions = install_hooks(dir.path()).unwrap();

        // Should install both Claude and Cursor hooks
        assert!(
            actions
                .iter()
                .any(|a| a.path_display(dir.path()).contains("hooks.json")),
            "should install Cursor hooks"
        );

        let content: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(dir.path().join(".cursor/hooks.json")).unwrap(),
        )
        .unwrap();

        assert_eq!(content["version"], 1);
        assert!(content["hooks"]["postToolUse"].as_array().unwrap().len() >= 1);

        // Check for cliproot capture-hook command
        let post_tool_use = content["hooks"]["postToolUse"].as_array().unwrap();
        assert!(post_tool_use.iter().any(|entry| {
            entry["command"]
                .as_str()
                .map(|c| c.contains("cliproot capture-hook"))
                .unwrap_or(false)
        }));

        // Check stop hook
        let stop = content["hooks"]["stop"].as_array().unwrap();
        assert!(stop.iter().any(|entry| {
            entry["command"]
                .as_str()
                .map(|c| c.contains("consolidate-hook"))
                .unwrap_or(false)
        }));
    }

    #[test]
    fn cursor_hooks_skipped_if_already_installed() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".cursor")).unwrap();
        fs::write(
            dir.path().join(".cursor/hooks.json"),
            r#"{"version":1,"hooks":{"postToolUse":[{"command":"cliproot capture-hook --harness cursor","matcher":"WebFetch"}]}}"#,
        )
        .unwrap();

        let actions = install_hooks_for(dir.path(), HarnessSelection::CursorOnly).unwrap();
        let action = actions.first().expect("should return one action");
        assert!(matches!(action, ConfigAction::Skipped(_)));
    }

    #[test]
    fn cursor_hooks_merged_with_existing() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".cursor")).unwrap();
        fs::write(
            dir.path().join(".cursor/hooks.json"),
            r#"{"hooks":{"postToolUse":[{"command":"other-tool hook","type":"command"}]}}"#,
        )
        .unwrap();

        let actions = install_hooks_for(dir.path(), HarnessSelection::CursorOnly).unwrap();
        let action = actions.first().expect("should return one action");
        assert!(matches!(action, ConfigAction::Merged(_)));

        let content: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(dir.path().join(".cursor/hooks.json")).unwrap(),
        )
        .unwrap();

        let post_tool = content["hooks"]["postToolUse"].as_array().unwrap();
        assert!(post_tool.len() > 1);
    }

    #[test]
    fn claude_only_selection() {
        let dir = tempfile::tempdir().unwrap();

        // Create both configs
        fs::create_dir_all(dir.path().join(".claude")).unwrap();
        fs::create_dir_all(dir.path().join(".cursor")).unwrap();
        fs::write(dir.path().join(".claude/settings.json"), r#"{}"#).unwrap();
        fs::write(dir.path().join(".cursor/mcp.json"), r#"{}"#).unwrap();

        let actions = install_hooks_for(dir.path(), HarnessSelection::ClaudeOnly).unwrap();

        // Should only have Claude action
        assert_eq!(actions.len(), 1);
        assert!(actions[0]
            .path_display(dir.path())
            .contains("settings.json"));
    }

    #[test]
    fn cursor_only_selection() {
        let dir = tempfile::tempdir().unwrap();

        // Create both configs
        fs::create_dir_all(dir.path().join(".claude")).unwrap();
        fs::create_dir_all(dir.path().join(".cursor")).unwrap();
        fs::write(dir.path().join(".claude/settings.json"), r#"{}"#).unwrap();
        fs::write(dir.path().join(".cursor/mcp.json"), r#"{}"#).unwrap();

        let actions = install_hooks_for(dir.path(), HarnessSelection::CursorOnly).unwrap();

        // Should only have Cursor action
        assert_eq!(actions.len(), 1);
        assert!(actions[0].path_display(dir.path()).contains("hooks.json"));
    }
}
