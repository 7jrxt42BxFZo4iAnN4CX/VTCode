use vtcode_agent_plugins::{FileSystemPluginLoader, PluginLoader};

pub(super) async fn handle_list() -> anyhow::Result<()> {
    println!("Installed Agent Plugins:");
    println!();
    let roots = super::plugin_roots();
    let loader = FileSystemPluginLoader::new();
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&root)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let plugin_json = path.join("plugin.json");
            if !plugin_json.is_file() {
                continue;
            }
            match loader.load(&path) {
                Ok(loaded) => {
                    let skill_count = loaded.skills.len();
                    let mcp_count = loaded.mcp.as_ref().map(|m| m.servers.len()).unwrap_or(0);
                    println!(
                        "  {} (v{}) — {} skill(s), {} MCP server(s) — {}",
                        loaded.manifest.name,
                        loaded.manifest.version.as_deref().unwrap_or("unknown"),
                        skill_count,
                        mcp_count,
                        path.display()
                    );
                }
                Err(e) => {
                    eprintln!("  {} — invalid: {}", path.display(), e);
                }
            }
        }
    }
    Ok(())
}
