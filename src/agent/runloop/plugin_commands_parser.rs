use anyhow::Result;

use super::plugin_commands::PluginCommandAction;

pub(super) fn parse_plugin_command(input: &str) -> Result<Option<PluginCommandAction>> {
    let trimmed = input.trim();

    if !trimmed.starts_with("/plugin") {
        return Ok(None);
    }

    let rest = trimmed[7..].trim();

    if rest.is_empty() {
        return Ok(Some(PluginCommandAction::Interactive));
    }

    if matches!(rest, "manager" | "--manager" | "interactive" | "--interactive") {
        return Ok(Some(PluginCommandAction::Interactive));
    }

    if matches!(rest, "list" | "--list" | "-l") {
        return Ok(Some(PluginCommandAction::List));
    }

    if matches!(rest, "help" | "--help" | "-h") {
        return Ok(Some(PluginCommandAction::Help));
    }

    if matches!(rest, "refresh" | "--refresh") {
        return Ok(Some(PluginCommandAction::Refresh));
    }

    let parts: Vec<&str> = rest.splitn(2, ' ').collect();
    match parts[0] {
        "info" | "show" | "--info" | "--show" => {
            if let Some(name) = parts.get(1) {
                Ok(Some(PluginCommandAction::Info { name: name.to_string() }))
            } else {
                Err(anyhow::anyhow!("info: plugin name required"))
            }
        }
        "add" | "--add" => {
            if let Some(rest_str) = parts.get(1) {
                let tokens: Vec<&str> = rest_str.split_whitespace().collect();
                if tokens.is_empty() {
                    return Err(anyhow::anyhow!("add: source (git URL or local path) required"));
                }
                let source = tokens[0].to_string();
                let mut name = None;
                let mut index = 1;
                while index < tokens.len() {
                    match tokens[index] {
                        "--name" | "-n" => match tokens.get(index + 1) {
                            Some(value) => {
                                name = Some(value.to_string());
                                index += 2;
                            }
                            None => return Err(anyhow::anyhow!("add: --name requires a value")),
                        },
                        other => return Err(anyhow::anyhow!("add: unexpected argument '{other}'")),
                    }
                }
                Ok(Some(PluginCommandAction::Add { source, name }))
            } else {
                Err(anyhow::anyhow!("add: source (git URL or local path) required"))
            }
        }
        "remove" | "rm" | "--remove" => {
            if let Some(name) = parts.get(1) {
                Ok(Some(PluginCommandAction::Remove { name: name.to_string() }))
            } else {
                Err(anyhow::anyhow!("remove: plugin name required"))
            }
        }
        "validate" | "--validate" => {
            if let Some(path) = parts.get(1) {
                Ok(Some(PluginCommandAction::Validate { path: std::path::PathBuf::from(path) }))
            } else {
                Err(anyhow::anyhow!("validate: plugin path required"))
            }
        }
        cmd => Err(anyhow::anyhow!("unknown plugin subcommand: {cmd}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Result<Option<PluginCommandAction>> {
        parse_plugin_command(input)
    }

    #[test]
    fn bare_command_opens_interactive_manager() {
        assert_eq!(parse("/plugin").unwrap(), Some(PluginCommandAction::Interactive));
    }

    #[test]
    fn manager_aliases_open_interactive_manager() {
        for input in [
            "/plugin manager",
            "/plugin --manager",
            "/plugin interactive",
            "/plugin --interactive",
        ] {
            assert_eq!(parse(input).unwrap(), Some(PluginCommandAction::Interactive), "input: {input}");
        }
    }

    #[test]
    fn list_parses() {
        for input in ["/plugin list", "/plugin --list", "/plugin -l"] {
            assert_eq!(parse(input).unwrap(), Some(PluginCommandAction::List), "input: {input}");
        }
    }

    #[test]
    fn help_parses() {
        for input in ["/plugin help", "/plugin --help", "/plugin -h"] {
            assert_eq!(parse(input).unwrap(), Some(PluginCommandAction::Help), "input: {input}");
        }
    }

    #[test]
    fn refresh_parses() {
        assert_eq!(parse("/plugin refresh").unwrap(), Some(PluginCommandAction::Refresh));
        assert_eq!(parse("/plugin --refresh").unwrap(), Some(PluginCommandAction::Refresh));
    }

    #[test]
    fn info_parses() {
        for input in [
            "/plugin info my-plugin",
            "/plugin show my-plugin",
            "/plugin --info my-plugin",
        ] {
            assert_eq!(
                parse(input).unwrap(),
                Some(PluginCommandAction::Info { name: "my-plugin".to_string() }),
                "input: {input}"
            );
        }
    }

    #[test]
    fn info_requires_name() {
        assert!(parse("/plugin info").is_err());
    }

    #[test]
    fn add_parses_source_only() {
        assert_eq!(
            parse("/plugin add https://github.com/example/my-plugin.git").unwrap(),
            Some(PluginCommandAction::Add {
                source: "https://github.com/example/my-plugin.git".to_string(),
                name: None
            })
        );
    }

    #[test]
    fn add_parses_source_with_name() {
        assert_eq!(
            parse("/plugin add https://github.com/example/my-plugin.git --name my-plugin").unwrap(),
            Some(PluginCommandAction::Add {
                source: "https://github.com/example/my-plugin.git".to_string(),
                name: Some("my-plugin".to_string()),
            })
        );
    }

    #[test]
    fn add_requires_source() {
        assert!(parse("/plugin add").is_err());
    }

    #[test]
    fn remove_parses() {
        for input in [
            "/plugin remove my-plugin",
            "/plugin rm my-plugin",
            "/plugin --remove my-plugin",
        ] {
            assert_eq!(
                parse(input).unwrap(),
                Some(PluginCommandAction::Remove { name: "my-plugin".to_string() }),
                "input: {input}"
            );
        }
    }

    #[test]
    fn remove_requires_name() {
        assert!(parse("/plugin remove").is_err());
    }

    #[test]
    fn validate_parses() {
        assert_eq!(
            parse("/plugin validate ./my-plugin").unwrap(),
            Some(PluginCommandAction::Validate { path: std::path::PathBuf::from("./my-plugin") })
        );
    }

    #[test]
    fn validate_requires_path() {
        assert!(parse("/plugin validate").is_err());
    }

    #[test]
    fn unknown_subcommand_is_error() {
        assert!(parse("/plugin frobnicate").is_err());
    }

    #[test]
    fn non_plugin_command_returns_none() {
        assert_eq!(parse("/help").unwrap(), None);
        assert_eq!(parse("just a prompt").unwrap(), None);
    }
}
