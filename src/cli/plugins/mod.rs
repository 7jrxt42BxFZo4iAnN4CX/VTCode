use std::path::PathBuf;

use vtcode_core::cli::args::PluginsSubcommand;

use crate::startup::StartupContext;

pub(super) async fn dispatch_plugins_command(_startup: &StartupContext, cmd: PluginsSubcommand) -> anyhow::Result<()> {
    match cmd {
        PluginsSubcommand::List => list::handle_list().await,
        PluginsSubcommand::Info { name } => info::handle_info(&name).await,
        PluginsSubcommand::Validate { path } => validate::handle_validate(&path).await,
        PluginsSubcommand::Add { source, name } => add::handle_add(source, name).await,
        PluginsSubcommand::Remove { name } => remove::handle_remove(&name).await,
    }
}

fn plugin_roots() -> Vec<PathBuf> {
    vec![
        std::env::current_dir()
            .ok()
            .map(|c| c.join(".agents/plugins"))
            .unwrap_or_default(),
        dirs::home_dir().map(|h| h.join(".agents/plugins")).unwrap_or_default(),
    ]
}

mod add;
mod info;
mod list;
mod remove;
mod validate;
