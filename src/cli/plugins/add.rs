use vtcode_agent_plugins::{FileSystemPluginInstaller, PluginInstaller};

pub(super) async fn handle_add(source: String, name: Option<String>) -> anyhow::Result<()> {
    let installer = FileSystemPluginInstaller::new();
    let installed = installer.install(&source, name).map_err(|e| anyhow::anyhow!("{}", e))?;

    println!("Installed plugin {} to {}", installed.name, installed.path.display());
    println!(
        "Validated: {} skill(s), {} MCP server(s)",
        installed.loaded.skills.len(),
        installed.loaded.mcp.as_ref().map(|m| m.servers.len()).unwrap_or(0)
    );
    Ok(())
}
