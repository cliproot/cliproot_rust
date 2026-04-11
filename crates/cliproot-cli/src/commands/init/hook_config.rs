use std::fs;
use std::path::Path;

use super::agent_config::ConfigAction;

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

    let hooks = obj.entry("hooks").or_insert_with(|| serde_json::json!({}));
    let hooks_obj = hooks.as_object_mut().ok_or("hooks is not a JSON object")?;

    let mut any_added = false;
    any_added |= install_hook_entry(hooks_obj, "PostToolUse", "cliproot capture-hook")?;
    any_added |= install_hook_entry(hooks_obj, "Stop", "cliproot consolidate-hook")?;
    any_added |=
        install_hook_entry(hooks_obj, "PreCompact", "cliproot consolidate-hook --emergency")?;

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
            r#"{"hooks":{"PostToolUse":[{"hooks":[{"type":"command","command":"cliproot capture-hook"}]}],"Stop":[{"hooks":[{"type":"command","command":"cliproot consolidate-hook"}]}],"PreCompact":[{"hooks":[{"type":"command","command":"cliproot consolidate-hook --emergency"}]}]}}"#,
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
