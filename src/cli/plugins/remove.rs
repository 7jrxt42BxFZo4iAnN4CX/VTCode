use vtcode_agent_plugins::{FileSystemPluginInstaller, PluginInstaller};

pub(super) async fn handle_remove(name: &str) -> anyhow::Result<()> {
    let installer = FileSystemPluginInstaller::new();
    installer.remove(name).map_err(|e| anyhow::anyhow!("{}", e))?;
    println!("Removed plugin {}", name);
    Ok(())
}
