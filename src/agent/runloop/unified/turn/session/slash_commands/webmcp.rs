use anyhow::Result;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::unified::webmcp::ActiveWebmcpBridge;

use super::{SlashCommandContext, SlashCommandControl};

pub(crate) async fn handle_start_webmcp(ctx: SlashCommandContext<'_>, origin: String) -> Result<SlashCommandControl> {
    if ctx.webmcp_bridge.is_some() {
        ctx.renderer
            .line(MessageStyle::Info, "WebMCP is already listening in this VT Code session.")?;
        return Ok(SlashCommandControl::Continue);
    }

    let bridge = ActiveWebmcpBridge::start(
        &ctx.config.workspace,
        ctx.vt_cfg.as_ref(),
        &origin,
        ctx.webmcp_prompt_sender.clone(),
    )
    .await?;
    let endpoint = bridge.endpoint().to_string();
    let pairing_code = bridge.pairing_code().to_string();
    let expires_in_secs = bridge.pairing_expires_in_secs();
    if let Some(emitter) = ctx.harness_emitter {
        emitter.attach_webmcp_event_hub(bridge.event_hub());
    }
    *ctx.webmcp_bridge = Some(bridge);

    ctx.renderer.line(MessageStyle::Info, "Active WebMCP bridge started.")?;
    ctx.renderer.line(MessageStyle::Info, &format!("WebSocket: {endpoint}"))?;
    ctx.renderer
        .line(MessageStyle::Info, &format!("Pairing code: {pairing_code} (expires in {expires_in_secs} seconds)"))?;
    ctx.renderer.line(
        MessageStyle::Info,
        "Browser writes remain proposals; VT Code's terminal permission flow is authoritative.",
    )?;
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_show_webmcp_status(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    if let Some(bridge) = ctx.webmcp_bridge.as_ref() {
        ctx.renderer
            .line(MessageStyle::Info, "WebMCP is listening in the active VT Code session.")?;
        ctx.renderer
            .line(MessageStyle::Info, &format!("WebSocket: {}", bridge.endpoint()))?;
        ctx.renderer
            .line(MessageStyle::Info, &format!("Pairing code: {}", bridge.pairing_code()))?;
    } else {
        ctx.renderer.line(
            MessageStyle::Info,
            "WebMCP is not listening. Use `/webmcp pair http://localhost:5173` to attach the browser editor.",
        )?;
    }
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_stop_webmcp(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    if ctx.webmcp_bridge.take().is_some() {
        ctx.renderer
            .line(MessageStyle::Info, "WebMCP stopped; browser pairings were revoked.")?;
    } else {
        ctx.renderer
            .line(MessageStyle::Info, "WebMCP was not listening in this session.")?;
    }
    Ok(SlashCommandControl::Continue)
}
