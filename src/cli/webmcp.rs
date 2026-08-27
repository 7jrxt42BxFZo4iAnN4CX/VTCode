use anyhow::{Context, Result, bail};
use std::path::PathBuf;
use std::sync::Arc;
use vtcode_core::cli::args::{WebmcpCommand, WebmcpPairArgs, WebmcpServeArgs};
use vtcode_webmcp::{FilesystemWorkspace, WebmcpServer, WebmcpServerConfig};

use crate::startup::{StartupContext, require_full_auto_workspace_trust};

/// Dispatch the standalone WebMCP command family.
pub(crate) async fn handle_webmcp_command(startup: &StartupContext, command: WebmcpCommand) -> Result<()> {
    match command {
        WebmcpCommand::Serve(args) => run_server(startup, &args, None).await,
        WebmcpCommand::Pair(args) => run_pair(startup, &args).await,
        WebmcpCommand::Status => print_status(startup),
        WebmcpCommand::Tools => print_tools(),
        WebmcpCommand::Roots => print_roots(startup),
        WebmcpCommand::Unpair => {
            println!(
                "WebMCP pairing revocation is owned by the running server process; stop that process to revoke all in-memory sessions."
            );
            Ok(())
        }
    }
}

async fn run_pair(startup: &StartupContext, args: &WebmcpPairArgs) -> Result<()> {
    let serve_args = WebmcpServeArgs {
        host: args.host.clone(),
        port: args.port,
        origins: vec![args.origin.clone()],
        allowed_roots: Vec::new(),
        allow_remote: args.allow_remote,
        public_url: args.public_url.clone(),
    };
    run_server(startup, &serve_args, Some(args.origin.as_str())).await
}

async fn run_server(startup: &StartupContext, args: &WebmcpServeArgs, pairing_origin: Option<&str>) -> Result<()> {
    let configured = &startup.config.webmcp;
    let host = args.host.clone().unwrap_or_else(|| configured.host.clone());
    let port = args.port.unwrap_or(configured.port);
    let origins = if args.origins.is_empty() {
        configured.allowed_origins.clone()
    } else {
        args.origins.clone()
    };
    if origins.is_empty() {
        bail!("WebMCP requires at least one explicit --origin (or webmcp.allowed_origins entry)");
    }
    if let Some(pairing_origin) = pairing_origin
        && !origins.iter().any(|origin| origin == pairing_origin)
    {
        bail!("pairing origin must be present in the explicit origin allowlist");
    }

    let allowed_roots = if args.allowed_roots.is_empty() {
        configured.allowed_roots.clone()
    } else {
        args.allowed_roots.clone()
    };
    if allowed_roots.len() > 1 {
        bail!(
            "headless WebMCP currently exposes one workspace root; run one bridge per root until root selection is available"
        );
    }
    let workspace_root = allowed_roots.first().cloned().unwrap_or_else(|| startup.workspace.clone());
    let roots = if allowed_roots.is_empty() {
        vec![workspace_root.clone()]
    } else {
        allowed_roots
    };
    let full_auto_enabled = startup.full_auto_requested && startup.config.automation.full_auto.enabled;
    if full_auto_enabled
        && (full_auto_allows_tool(startup, vtcode_core::config::constants::tools::APPLY_PATCH)
            || full_auto_allows_tool(startup, vtcode_core::config::constants::tools::EXEC_COMMAND))
    {
        require_full_auto_workspace_trust(&workspace_root, "headless WebMCP mutations and checks", "webmcp serve")
            .await?;
    }
    let mutations_allowed = full_auto_allows_tool(startup, vtcode_core::config::constants::tools::APPLY_PATCH);
    let checks_allowed = full_auto_allows_tool(startup, vtcode_core::config::constants::tools::EXEC_COMMAND);
    let adapter = FilesystemWorkspace::new(&workspace_root, roots, mutations_allowed)
        .await
        .with_context(|| format!("failed to initialize WebMCP workspace {}", workspace_root.display()))?
        .with_checks_allowed(checks_allowed);

    let config = WebmcpServerConfig {
        host,
        port,
        allowed_origins: origins,
        pairing_ttl_secs: configured.pairing_ttl_secs,
        max_frame_bytes: configured.max_frame_bytes,
        max_in_flight_requests: configured.max_in_flight_requests,
        allow_remote: args.allow_remote,
        public_url: args.public_url.clone(),
        ..Default::default()
    };
    let server = WebmcpServer::new(Arc::new(adapter), config)?;
    let pairing = match pairing_origin {
        Some(origin) => server.begin_pairing_for_origin(origin.to_string())?,
        None => server.begin_pairing(),
    };
    let listener = server.bind().await?;
    let address = listener.local_addr().context("failed to determine WebMCP listener address")?;
    println!("WebMCP listening locally at ws://{address}/webmcp");
    println!(
        "Agent turns are unavailable in standalone mode; use `/webmcp pair <origin>` inside an interactive `vtcode chat` session for VT Code turns."
    );
    if let Some(public_url) = args.public_url.as_deref() {
        println!(
            "Remote access requires a TLS-terminating reverse proxy at {public_url} forwarding to this loopback listener."
        );
    }
    println!("Pairing code: {} (expires in {} seconds)", pairing.code(), pairing.expires_in().as_secs());
    if !mutations_allowed {
        println!(
            "Mutations remain terminal-approved; headless apply is disabled without explicit full-auto permissions."
        );
    }
    tokio::select! {
        result = server.serve_listener(listener) => result.map_err(anyhow::Error::from),
        signal = tokio::signal::ctrl_c() => signal.context("failed to listen for WebMCP shutdown")
    }
}

fn full_auto_allows_tool(startup: &StartupContext, tool: &str) -> bool {
    startup.full_auto_requested
        && startup.config.automation.full_auto.enabled
        && startup.config.automation.full_auto.allowed_tools.iter().any(|allowed| {
            let allowed = allowed.trim();
            allowed == tool || allowed == vtcode_core::config::constants::tools::WILDCARD_ALL
        })
}

fn print_status(startup: &StartupContext) -> Result<()> {
    let config = &startup.config.webmcp;
    println!("WebMCP enabled: {}", config.enabled);
    println!("Bind: {}:{}", config.host, config.port);
    println!("Allowed origins: {}", config.allowed_origins.len());
    println!("Allowed roots: {}", config.allowed_roots.len());
    println!("Pairing TTL: {} seconds", config.pairing_ttl_secs);
    Ok(())
}

fn print_tools() -> Result<()> {
    println!("WebMCP tools:");
    for tool in [
        "workspace.list_files",
        "workspace.read_file",
        "patch.propose",
        "patch.apply (terminal approval required)",
        "checks.run (allowlisted argv only)",
        "patch.revert (current-file validation required)",
        "turn.request",
        "cancel",
    ] {
        println!("  {tool}");
    }
    Ok(())
}

fn print_roots(startup: &StartupContext) -> Result<()> {
    let roots: &[PathBuf] = &startup.config.webmcp.allowed_roots;
    if roots.is_empty() {
        println!(
            "No configured headless roots. `vtcode webmcp serve --allowed-root <path>` is required for root changes."
        );
    } else {
        for root in roots {
            println!("{}", root.display());
        }
    }
    Ok(())
}
