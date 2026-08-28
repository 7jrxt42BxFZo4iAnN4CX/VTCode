import { buildTurnPrompt, connectVtCode, createBackend, createUnifiedDiff, digest, MAX_FILE_BYTES } from "./backend.js";
import { CodeEditor } from "./editor.js";
import { loadBrowserSettings, loadBrowserState, saveBrowserSettings, saveBrowserState } from "./persistence.js";
import { createWebMcpTools, registerWebMcpTools, replaceExactText } from "./webmcp.js";
import "../styles.css";

const $ = (id) => document.getElementById(id);
const MAX_HYDRATION_EVENTS = 256;
const MAX_SEARCH_FILES = 128;
const MAX_SEARCH_BYTES = 8 * 1024 * 1024;
const MAX_WEBMCP_RESULT_BYTES = 128 * 1024;
const PERSIST_DEBOUNCE_MS = 250;
const APP_INSTANCE = typeof __VTCODE_APP_INSTANCE__ === "string" ? __VTCODE_APP_INSTANCE__ : "development";

function browserStorage() {
  try { return globalThis.localStorage; } catch { return null; }
}

const persistedBrowserState = loadBrowserState(browserStorage(), APP_INSTANCE);
const persistedBrowserSettings = loadBrowserSettings(browserStorage(), APP_INSTANCE);
let backend = createBackend(persistedBrowserState?.fallback_files);
let toastTimer;
let openRequest = 0;
let webMcpRegistration = null;
let fallbackPersistenceTimer = null;
let persistenceWarningShown = false;
let settingsPersistenceWarningShown = false;

const state = {
  files: new Map(),
  snapshots: new Map(),
  drafts: new Map(),
  openTabs: [],
  selected: null,
  expandedDirs: new Set(),
  filter: "",
  clientProposal: null,
  serverProposal: null,
  approved: false,
  pendingTerminalApproval: false,
  lastChange: null,
  conflicts: new Set(),
  unsubscribe: null,
  unsubscribeConnection: null,
  unsubscribeStatus: null,
};

const editor = new CodeEditor($("editor"), {
  onChange: (path, content) => {
    if (content === state.snapshots.get(path)?.content) state.drafts.delete(path);
    else state.drafts.set(path, content);
    editor.updateDirty(isDirty(path));
    state.clientProposal = null;
    state.serverProposal = null;
    state.approved = false;
    state.pendingTerminalApproval = false;
    persistBrowserWorkspace();
    renderTree();
    renderTabs();
    renderProposal();
    updateEditorFooter();
    status("Draft buffer changed", "Save or review to create a proposal; the workspace is unchanged.");
  },
  onSave: () => { void run(reviewChanges); },
  onSelectionChange: () => updateEditorFooter(),
});

function message(error) { return error instanceof Error ? error.message : String(error); }

const contentSizeBytes = (content) => new TextEncoder().encode(typeof content === "string" ? content : "").length;
const snapshotSizeBytes = (file) => {
  const size = Number(file?.size_bytes);
  return Number.isSafeInteger(size) && size >= 0 ? size : contentSizeBytes(file?.content);
};

function truncateUtf8(text, maxBytes) {
  const bytes = new TextEncoder().encode(text);
  if (bytes.length <= maxBytes) return { text, truncated: false };
  const suffix = "\n[output truncated by the browser tool limit]";
  const suffixBytes = new TextEncoder().encode(suffix).length;
  let end = Math.max(0, maxBytes - suffixBytes);
  while (end > 0 && (bytes[end] & 0xc0) === 0x80) end -= 1;
  return { text: `${new TextDecoder().decode(bytes.slice(0, end))}${suffix}`, truncated: true };
}

function toast(text) {
  $("toast").textContent = text;
  $("toast").classList.add("show");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => $("toast").classList.remove("show"), 2800);
}

function log(text) {
  const item = document.createElement("li");
  item.textContent = text;
  const time = document.createElement("time");
  time.textContent = new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  item.append(time);
  $("activityLog").prepend(item);
}

function recordRuntimeEvent(event) {
  const sequence = event.sequence ? ` #${event.sequence}` : "";
  log(`Runtime event${sequence}`);
  status("Runtime event received", "Refresh a clean file to inspect the latest backend snapshot.");
}

function status(title, detail) {
  $("statusText").textContent = title;
  $("statusDetail").textContent = detail;
}

function browserOrigin() {
  return globalThis.location?.origin || "http://localhost:5173";
}

function formatBytes(bytes) {
  const value = Number(bytes);
  if (!Number.isFinite(value) || value < 0) return "not reported";
  if (value >= 1024 * 1024) return `${(value / (1024 * 1024)).toFixed(value % (1024 * 1024) ? 1 : 0)} MiB`;
  if (value >= 1024) return `${Math.round(value / 1024)} KiB`;
  return `${Math.round(value)} B`;
}

function renderSettings() {
  const runtime = runtimeStatus();
  const settings = backend.statusPayload?.settings;
  const paired = backend.kind === "websocket" && backend.connected;
  const runtimeLabel = backend.kind === "fallback"
    ? "Fallback"
    : runtime?.turns_available === true ? "Active VT Code TUI" : "Headless workspace bridge";
  const workspace = runtime?.workspace_root || (backend.kind === "fallback" ? "Browser memory" : "Not reported");
  const connection = backend.kind === "fallback"
    ? "In-memory fallback"
    : paired ? backend.url : "Bridge disconnected";
  const origin = backend.statusPayload?.authenticated_origin || browserOrigin();
  const ttl = Number(settings?.pairing_ttl_secs);
  const frameBytes = Number(settings?.max_frame_bytes);
  const inFlight = Number(settings?.max_in_flight_requests);
  const listener = settings
    ? `${settings.host}:${settings.port === 0 ? "auto" : settings.port} · ${settings.remote_enabled ? "remote proxy" : "loopback"}`
    : backend.kind === "fallback" ? "Browser only" : "Not reported by bridge";

  $("settingsConnection").textContent = connection;
  $("settingsWorkspace").textContent = workspace;
  $("settingsOrigin").textContent = origin;
  $("settingsRuntime").textContent = runtimeLabel;
  $("settingsPairingTtl").textContent = Number.isSafeInteger(ttl) && ttl > 0 ? `${ttl} seconds` : "Not reported";
  $("settingsLimits").textContent = Number.isSafeInteger(frameBytes) && Number.isSafeInteger(inFlight)
    ? `${formatBytes(frameBytes)} · ${inFlight} in flight`
    : "Not reported";
  $("settingsListener").textContent = listener;

  const syncState = $("settingsSyncState");
  syncState.textContent = backend.kind === "fallback" ? "Fallback defaults" : paired ? "Synced from VT Code" : "Pairing required";
  syncState.className = `settings-sync-state${paired ? " connected" : backend.kind === "websocket" ? " warning" : ""}`;
  $("settingsSyncNote").textContent = backend.kind === "fallback"
    ? "No bridge is paired. Browser edits stay in memory and never touch the filesystem."
    : paired
      ? "These values are read from the paired VT Code bridge. The terminal owns workspace roots, policy, pairing, and writes."
      : "The previous bridge is not connected. Enter a fresh one-time code; bridge settings will appear after pairing.";
}

function shellQuote(value) {
  return `'${value.replaceAll("'", "'\\''")}'`;
}

function workspacePathValue() {
  return $("workspacePath").value.trim() || "/absolute/path/to/workspace";
}

function renderWorkspaceSetup() {
  const origin = browserOrigin();
  $("pairingCommand").textContent = `/webmcp pair ${origin}`;
  const path = shellQuote(workspacePathValue());
  $("browserOrigin").textContent = origin;
  $("activeSetupCommand").textContent = `vtcode --workspace ${path} chat\n\nThen in the TUI:\n/webmcp pair ${origin}`;
  $("headlessSetupCommand").textContent = `vtcode webmcp serve \\\n  --origin ${origin} \\\n  --allowed-root ${path}`;
}

function openSettings(section = null) {
  if ($("confirmDialog").open) return;
  if ($("quickActionDialog").open) $("quickActionDialog").close();
  if ($("helpDialog").open) $("helpDialog").close();
  const dialog = $("settingsDialog");
  if (!dialog.open) dialog.showModal();
  if (section === "connection") {
    $("connectionPanel").open = true;
    $("workspaceSetupPanel").open = false;
  } else if (section === "workspace") {
    $("connectionPanel").open = false;
    $("workspaceSetupPanel").open = true;
  }
  renderWorkspaceSetup();
  renderSettings();
}

function openWorkspaceSetup() {
  openSettings("workspace");
  $("workspacePath").focus();
}

function openConnectionPanel() {
  openSettings("connection");
  const field = $("bridgeUrl").value.trim() ? $("pairingCode") : $("bridgeUrl");
  field.focus();
}

function openSettingsDialog() {
  if ($("settingsDialog").open) $("settingsDialog").close();
  else openSettings();
}

function selectTerminal(panel) {
  for (const tab of document.querySelectorAll("[data-terminal]")) {
    const active = tab.dataset.terminal === panel;
    tab.classList.toggle("active", active);
    tab.setAttribute("aria-selected", String(active));
  }
  for (const pane of ["activity", "changes", "turn"]) {
    const element = $(`terminal${pane[0].toUpperCase()}${pane.slice(1)}`);
    const active = pane === panel;
    element.hidden = !active;
    element.classList.toggle("active", active);
  }
}

function openTurnComposer() {
  selectTerminal("turn");
  $("promptInput").focus();
}

function runtimeStatus() { return backend.statusPayload?.runtime; }
function isBridgeConnected() { return backend.kind !== "websocket" || backend.connected; }
function isHeadlessBridge() {
  const runtime = runtimeStatus();
  return isBridgeConnected() && backend.kind === "websocket" && runtime?.mutations_allowed === false && runtime.approval_authority?.startsWith("headless");
}
function isActiveRuntime() {
  return isBridgeConnected() && backend.kind === "websocket" && runtimeStatus()?.turns_available === true;
}

function updateTurnControl() {
  const detail = $("promptDetail");
  const button = $("requestTurn");
  const label = $("requestTurnLabel");
  if (backend.kind === "websocket" && !backend.connected) {
    detail.textContent = `The bridge is disconnected. Active sessions rerun \`/webmcp pair ${browserOrigin()}\`; standalone bridges restart \`vtcode webmcp serve\`, then pair with the newest URL and code.`;
    button.disabled = true;
    label.textContent = "Pair with VT Code first";
    button.title = "Open the WebMCP pairing instructions";
    return;
  }
  const headless = backend.kind === "websocket" && runtimeStatus()?.turns_available === false;
  button.disabled = headless;
  if (backend.kind === "fallback") {
    detail.textContent = `For a real turn, run \`/webmcp pair ${browserOrigin()}\` in the same VT Code TUI, then paste its values above.`;
    label.textContent = "Open pairing instructions";
    button.title = "Show the two-step VT Code pairing instructions";
  } else if (headless) {
    detail.textContent = "This standalone bridge exposes workspace operations only. Start `vtcode chat`, run `/webmcp pair <origin>` in that same session, then pair its new URL and code.";
    label.textContent = "Active VT Code session required";
    button.title = "Pair the bridge created by /webmcp pair in an active VT Code session";
  } else {
    detail.textContent = "The reviewed draft diff is attached; VT Code remains the execution and policy authority.";
    label.textContent = "Request VT Code turn";
    button.title = "Send the prompt and reviewed draft diff to VT Code (Cmd/Ctrl+Enter)";
  }
}

function handleBackendConnection(event) {
  if (backend.kind !== "websocket") return;
  if (event.state === "connected") {
    $("supportStatus").textContent = "VT Code connected";
    $("supportStatus").classList.add("available");
    status("Connected to VT Code", runtimeStatus()?.turns_available === false
      ? "Workspace bridge connected, but this standalone headless adapter cannot run agent turns."
      : "Active VT Code session connected; prompts and policy remain in the terminal.");
  } else if (event.state === "reconnecting") {
    $("supportStatus").textContent = "Reconnecting…";
    $("supportStatus").classList.add("available");
    status("Reconnecting to VT Code", "The bridge connection dropped; the browser is retrying with the in-memory session token.");
  } else if (event.state === "reauthorize") {
    $("supportStatus").textContent = "Pair again";
    $("supportStatus").classList.remove("available");
    openConnectionPanel();
    $("connectionSummary").textContent = "New pairing required";
    $("pairingCode").value = "";
    updateTurnControl();
    status("VT Code pairing expired", `For an active session rerun \`/webmcp pair ${browserOrigin()}\`; for a standalone bridge restart \`vtcode webmcp serve\`. Then enter the new URL and one-time code.`);
  } else if (event.state === "disconnected") {
    $("supportStatus").textContent = "Bridge disconnected";
    $("supportStatus").classList.remove("available");
    updateTurnControl();
    status("VT Code bridge disconnected", "Keep the current bridge running; the browser will reconnect automatically. If it was restarted, pair again with its newest URL and code.");
  }
  renderSettings();
  renderProposal();
}

function handleBackendStatus() {
  if (backend.kind !== "websocket") return;
  renderSettings();
  updateTurnControl();
  renderProposal();
}

function paths() { return [...state.files.keys()].sort(); }
function snapshot(path) { return state.snapshots.get(path); }
function current(path) { return state.drafts.get(path) ?? snapshot(path)?.content ?? ""; }
function isDirty(path) { return state.drafts.has(path) && state.drafts.get(path) !== snapshot(path)?.content; }
function dirtyPaths() { return paths().filter(isDirty); }

function persistBrowserSettings() {
  const saved = saveBrowserSettings(browserStorage(), APP_INSTANCE, {
    workspace_path: $("workspacePath")?.value.trim() || "",
    bridge_url: $("bridgeUrl")?.value.trim() || "",
  });
  if (!saved && !settingsPersistenceWarningShown) {
    settingsPersistenceWarningShown = true;
    status("Settings not saved", "Browser storage is unavailable or full; setup values may be lost on refresh.");
    toast("Settings could not be saved");
  } else if (saved) {
    settingsPersistenceWarningShown = false;
  }
  return saved;
}

function saveFallbackWorkspaceNow(silent = false) {
  if (backend.kind !== "fallback" || typeof backend.exportFiles !== "function") return true;
  const saved = saveBrowserState(browserStorage(), APP_INSTANCE, {
    fallback_files: backend.exportFiles(),
    drafts: Object.fromEntries(state.drafts),
    open_tabs: state.openTabs,
    selected: state.selected,
    expanded_dirs: [...state.expandedDirs],
    filter: state.filter,
    workspace_path: $("workspacePath")?.value.trim() || "",
  });
  if (!saved && !silent && !persistenceWarningShown) {
    persistenceWarningShown = true;
    status("Browser state not fully saved", "Storage quota was reached; refresh may lose fallback edits. Export or reduce the workspace before refreshing.");
    toast("Browser state could not be saved");
    log("Browser fallback state persistence failed");
  } else if (saved) {
    persistenceWarningShown = false;
  }
  return saved;
}

function flushBrowserPersistence({ silent = false } = {}) {
  if (fallbackPersistenceTimer) clearTimeout(fallbackPersistenceTimer);
  fallbackPersistenceTimer = null;
  return saveFallbackWorkspaceNow(silent);
}

function persistBrowserWorkspace({ flush = false, silent = false } = {}) {
  if (backend.kind !== "fallback" || typeof backend.exportFiles !== "function") return true;
  if (flush) return flushBrowserPersistence({ silent });
  if (fallbackPersistenceTimer) clearTimeout(fallbackPersistenceTimer);
  fallbackPersistenceTimer = setTimeout(() => {
    fallbackPersistenceTimer = null;
    saveFallbackWorkspaceNow();
  }, PERSIST_DEBOUNCE_MS);
  return true;
}

function fileName(path) { return path.split("/").at(-1) || path; }

function expandAncestors(path) {
  const parts = path.split("/");
  for (let index = 1; index < parts.length; index += 1) {
    state.expandedDirs.add(parts.slice(0, index).join("/"));
  }
}

function treeFor(filePaths) {
  const root = { directories: new Map(), files: [] };
  for (const path of filePaths) {
    const parts = path.split("/");
    let node = root;
    for (let index = 0; index < parts.length - 1; index += 1) {
      const name = parts[index];
      if (!node.directories.has(name)) node.directories.set(name, { directories: new Map(), files: [] });
      node = node.directories.get(name);
    }
    node.files.push(path);
  }
  return root;
}

function appendTreeNode(parent, node, prefix, depth, filterActive) {
  for (const [name, directory] of [...node.directories.entries()].sort(([left], [right]) => left.localeCompare(right))) {
    const path = prefix ? `${prefix}/${name}` : name;
    const expanded = filterActive || state.expandedDirs.has(path);
    const row = document.createElement("button");
    row.type = "button";
    row.className = "file-tree-row directory-item";
    row.style.paddingInlineStart = `${10 + depth * 14}px`;
    row.setAttribute("aria-expanded", String(expanded));
    row.title = path;
    const icon = document.createElement("span");
    icon.className = "tree-chevron";
    icon.textContent = expanded ? "▾" : "▸";
    const label = document.createElement("span");
    label.className = "tree-label";
    label.textContent = name;
    row.append(icon, label);
    row.onclick = () => {
      if (expanded) state.expandedDirs.delete(path);
      else state.expandedDirs.add(path);
      renderTree();
      persistBrowserWorkspace();
    };
    parent.append(row);
    if (expanded) appendTreeNode(parent, directory, path, depth + 1, filterActive);
  }

  for (const path of [...node.files].sort((left, right) => left.localeCompare(right))) {
    const row = document.createElement("button");
    row.type = "button";
    row.className = `file-tree-row file-item${path === state.selected ? " active" : ""}`;
    row.style.paddingInlineStart = `${10 + depth * 14}px`;
    row.setAttribute("aria-pressed", String(path === state.selected));
    row.title = path;
    const icon = document.createElement("span");
    icon.className = "file-icon";
    icon.textContent = fileName(path).endsWith(".md") ? "MD" : "<>";
    const label = document.createElement("span");
    label.className = "tree-label";
    label.textContent = fileName(path);
    const marker = document.createElement("span");
    marker.className = "dirty-marker";
    marker.textContent = isDirty(path) ? "*" : state.conflicts.has(path) ? "!" : "";
    row.append(icon, label, marker);
    row.onclick = () => run(() => openFile(path));
    parent.append(row);
  }
}

function renderTree() {
  const tree = $("fileTree");
  tree.replaceChildren();
  const query = state.filter.toLowerCase();
  const visiblePaths = paths().filter((path) => !query || path.toLowerCase().includes(query));
  if (visiblePaths.length) appendTreeNode(tree, treeFor(visiblePaths), "", 0, Boolean(query));
  else {
    const empty = document.createElement("div");
    empty.className = "tree-empty";
    empty.textContent = query ? "No matching files" : "No files in this workspace";
    tree.append(empty);
  }
  $("fileCount").textContent = query ? `${visiblePaths.length}/${paths().length} files` : `${paths().length} files`;
  const filterStatus = $("searchResults");
  filterStatus.textContent = query ? `${visiblePaths.length} matching files` : "Filter files by name or path";
}

function renderTabs() {
  const tabs = $("fileTabs");
  tabs.replaceChildren();
  for (const path of state.openTabs) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = `file-tab${path === state.selected ? " active" : ""}`;
    button.onclick = () => run(() => openFile(path));
    const label = document.createElement("span");
    label.className = "tab-label";
    label.textContent = fileName(path);
    label.title = path;
    const marker = document.createElement("span");
    marker.className = "dirty-marker";
    marker.textContent = isDirty(path) ? "*" : "";
    button.append(label, marker);
    tabs.append(button);
  }
}

function updateEditorFooter() {
  if (!editor.view) return;
  const line = editor.view.state.doc.lineAt(editor.view.state.selection.main.head);
  $("cursorPosition").textContent = `Ln ${line.number}, Col ${editor.view.state.selection.main.head - line.from + 1}`;
  const changed = state.selected && isDirty(state.selected);
  const conflict = state.selected && state.conflicts.has(state.selected);
  $("editorState").textContent = conflict ? "External change conflict" : changed ? "Dirty draft · not proposed" : "Workspace snapshot";
  $("editorState").classList.toggle("dirty", Boolean(changed || conflict));
}

function renderSelectedEditor() {
  if (!state.selected || !snapshot(state.selected)) return;
  const path = state.selected;
  editor.open(path, current(path), isDirty(path));
  $("fileTitle").textContent = path;
  $("fileType").textContent = path.includes(".") ? path.split(".").pop().toUpperCase() : "TEXT";
  updateEditorFooter();
}

async function openFile(path, record = true) {
  if (!state.files.has(path)) throw webMcpFileNotFoundError(path);
  const request = ++openRequest;
  state.selected = path;
  expandAncestors(path);
  if (!snapshot(path)) status("Loading file", path);
  if (!snapshot(path)) await refreshFile(path, false, false);
  if (request !== openRequest || state.selected !== path) return;
  if (!state.openTabs.includes(path)) state.openTabs.push(path);
  renderSelectedEditor();
  renderTree();
  renderTabs();
  if (record) {
    log(`Inspected ${path}`);
    status("Ready for inspection", "Edit a local draft; save opens review instead of writing immediately.");
  }
  persistBrowserWorkspace();
}

function collectChanges() {
  return dirtyPaths().map((path) => ({
    path,
    base_digest: snapshot(path).digest,
    content: current(path),
  }));
}

function renderProposal() {
  const view = $("diffView");
  view.replaceChildren();
  const proposal = state.serverProposal || state.clientProposal;
  if (!proposal) {
    const empty = document.createElement("div");
    empty.className = "empty-state";
    empty.innerHTML = "<strong>No changes staged</strong><small>Edit a buffer, then review the draft.</small>";
    view.append(empty);
    $("proposalState").textContent = "No proposal";
    $("proposalState").className = "state-badge";
  } else {
    const diff = proposal.unified_diff || state.clientProposal?.unified_diff || "";
    for (const line of diff.split("\n")) {
      const item = document.createElement("div");
      item.className = `diff-line ${line.startsWith("+") && !line.startsWith("+++") ? "added" : line.startsWith("-") && !line.startsWith("---") ? "removed" : "meta"}`;
      item.textContent = line || " ";
      view.append(item);
    }
    const stateLabel = state.pendingTerminalApproval
      ? "Awaiting terminal"
      : state.approved
        ? "Approved · ready"
        : state.serverProposal
          ? isActiveRuntime() ? "Ready for VT Code turn" : "Proposal sent"
          : "Review ready";
    $("proposalState").textContent = stateLabel;
    $("proposalState").className = `state-badge ${state.approved ? "approved" : state.pendingTerminalApproval ? "pending" : "ready"}`;
  }
  const bridgeUnavailable = backend.kind === "websocket" && !backend.connected;
  $("reviewChanges").disabled = dirtyPaths().length === 0;
  $("approvePatch").disabled = bridgeUnavailable || !state.clientProposal || Boolean(state.serverProposal);
  const requiresAgentTurn = isActiveRuntime() && runtimeStatus()?.mutations_allowed === false;
  $("applyPatch").disabled = bridgeUnavailable || !state.serverProposal || (backend.kind === "fallback" && !state.approved) || requiresAgentTurn;
  $("revertPatch").disabled = bridgeUnavailable || !state.lastChange;
  $("approvePatch").textContent = isActiveRuntime()
    ? "Stage for VT Code turn"
    : backend.kind === "websocket" ? "Request VT Code approval" : "Approve patch";
  $("applyPatch").textContent = requiresAgentTurn
    ? "Apply via VT Code turn"
    : backend.kind === "websocket" ? "Apply after terminal approval" : "Apply approved patch";
  const directChecksUnavailable = isHeadlessBridge() || isActiveRuntime();
  $("runChecks").disabled = bridgeUnavailable || directChecksUnavailable;
  $("runChecks").title = directChecksUnavailable
    ? "Ask VT Code to run checks through the terminal"
    : "Run the selected backend checks";
  if (isHeadlessBridge()) {
    $("approvePatch").textContent = "Stage proposal";
    $("applyPatch").disabled = true;
    $("applyPatch").textContent = "Apply unavailable in headless mode";
  }
}

async function reviewChanges() {
  const changes = collectChanges();
  if (!changes.length) throw new Error("No browser draft is ready to review; edit a file or call stage_text_edit first");
  const beforeByPath = Object.fromEntries(changes.map((change) => [change.path, snapshot(change.path).content]));
  state.clientProposal = { changes, unified_diff: createUnifiedDiff(changes, beforeByPath) };
  state.serverProposal = null;
  state.approved = false;
  state.pendingTerminalApproval = false;
  selectTerminal("changes");
  renderProposal();
  log(`Reviewed unified diff for ${changes.length} file${changes.length === 1 ? "" : "s"}`);
  status("Review ready", "The diff is client-generated; send it to the selected backend when it looks right.");
  toast("Unified diff ready for review");
}

function proposalPaths(proposal) {
  return [...new Set((proposal?.changes || [])
    .map((change) => change?.path)
    .filter((path) => typeof path === "string" && path.length > 0))];
}

async function recoverStaleProposal(proposal) {
  const affectedPaths = proposalPaths(proposal);
  state.clientProposal = null;
  state.serverProposal = null;
  state.approved = false;
  state.pendingTerminalApproval = false;
  for (const path of affectedPaths) state.conflicts.add(path);
  renderProposal();
  renderTree();
  renderTabs();
  updateEditorFooter();

  let refreshed = 0;
  for (const path of affectedPaths) {
    try {
      const fresh = await backend.readFile(path);
      if (typeof fresh?.content !== "string") throw new Error(`Backend returned invalid content for ${path}`);
      if (snapshotSizeBytes(fresh) > MAX_FILE_BYTES) throw new Error(`File exceeds the browser size limit: ${path}`);
      state.snapshots.set(path, fresh);
      if (state.drafts.get(path) === fresh.content) state.drafts.delete(path);
      state.conflicts.delete(path);
      refreshed += 1;
    } catch (error) {
      log(`Could not refresh ${path} after the stale proposal: ${message(error)}`);
    }
  }

  if (refreshed > 0 && affectedPaths.includes(state.selected)) renderSelectedEditor();
  renderTree();
  renderTabs();
  updateEditorFooter();
  persistBrowserWorkspace();
  log(`Cleared stale proposal for ${affectedPaths.length} file${affectedPaths.length === 1 ? "" : "s"}`);
  const detail = refreshed === affectedPaths.length
    ? "The latest snapshots are loaded and your draft is preserved. Review changes again before requesting the VT Code turn."
    : "Some files could not be refreshed. Reconnect or reload them, then review changes again before requesting the VT Code turn.";
  status("Proposal became stale", detail);
  return detail;
}

async function requestApproval() {
  if (!state.clientProposal) await reviewChanges();
  if (state.serverProposal) throw new Error("This proposal has already been sent");
  selectTerminal("changes");
  try {
    state.serverProposal = await backend.proposeChanges(state.clientProposal.changes);
  } catch (error) {
    if (error?.code === "conflict") error.proposalRecovery = await recoverStaleProposal(state.clientProposal);
    throw error;
  }
  if (backend.kind === "fallback") {
    state.approved = true;
    log("Fallback proposal approved in the browser");
    status("Patch approved", "Fallback mode changes only the in-memory project; apply is still explicit.");
  } else if (isHeadlessBridge()) {
    log("Proposal staged; headless bridge cannot authorize a filesystem apply");
    status("Proposal staged", "This standalone bridge has no terminal runtime for approval. Use explicit full-auto only for a disposable workspace.");
  } else if (isActiveRuntime()) {
    log("Proposal staged for the active VT Code session");
    status("Proposal staged", "Use Request VT Code turn; any write is performed through the terminal permission flow.");
  } else {
    state.pendingTerminalApproval = true;
    log("Requested terminal approval from VT Code");
    status("Awaiting terminal approval", "The browser cannot authorize a real filesystem write.");
  }
  renderProposal();
  if (isActiveRuntime()) openTurnComposer();
}

async function applyProposal() {
  if (!state.serverProposal) throw new Error("Send a proposal before applying it");
  if (backend.kind === "fallback" && !state.approved) throw new Error("Explicit approval is required");
  if (isActiveRuntime() && runtimeStatus()?.mutations_allowed === false) {
    throw new Error("Ask the active VT Code session to apply this proposal through a turn");
  }
  selectTerminal("changes");
  const before = state.serverProposal.changes.map((change) => ({ path: change.path, content: snapshot(change.path).content }));
  try {
    const result = await backend.applyProposal(state.serverProposal.proposal_id);
    state.lastChange = { change_id: result.change_id, paths: result.paths, before };
    for (const path of state.serverProposal.changes.map((change) => change.path)) await refreshFile(path, true);
    state.clientProposal = null;
    state.serverProposal = null;
    state.approved = false;
    state.pendingTerminalApproval = false;
    persistBrowserWorkspace();
    renderProposal();
    log(`Applied approved patch (${backend.kind === "fallback" ? "in memory" : "VT Code"})`);
    status("Change applied", backend.kind === "fallback" ? "Only the deterministic in-memory project changed." : "VT Code completed the authorized workspace change.");
    toast("Patch applied");
  } catch (error) {
    if (error?.code === "approval_required") {
      state.pendingTerminalApproval = true;
      renderProposal();
      status("Still awaiting terminal approval", "VT Code rejected browser-only authorization; no file was changed.");
    }
    throw error;
  }
}

async function revertLastChange() {
  if (!state.lastChange) throw new Error("There is no applied change to revert");
  selectTerminal("changes");
  const result = await backend.revertLastChange(state.lastChange.change_id);
  for (const path of result.paths) await refreshFile(path, true);
  log(`Reverted ${result.paths.join(", ")}`);
  state.lastChange = null;
  persistBrowserWorkspace();
  renderProposal();
  status("Change reverted", "The backend validated the current file before restoring its prior snapshot.");
  toast("Last change reverted");
}

async function runChecks() {
  if (isHeadlessBridge()) {
    throw new Error("Checks are unavailable in the headless bridge; enable explicit full-auto policy for a disposable workspace");
  }
  const result = await backend.runChecks(backend.kind === "fallback" ? undefined : "cargo check --locked");
  selectTerminal("activity");
  $("checkOutput").textContent = [result.stdout, result.stderr].filter(Boolean).join("\n") || JSON.stringify(result, null, 2);
  log(result.exit_code === 0 ? "Checks passed" : "Checks found an issue");
  status(result.exit_code === 0 ? "Checks passed" : "Checks need attention", backend.kind === "fallback" ? "Deterministic browser checks · no filesystem" : "Command executed by VT Code policy");
  toast(result.exit_code === 0 ? "Checks passed" : "A check failed");
}

async function requestTurn() {
  openTurnComposer();
  if (backend.kind === "fallback") openConnectionPanel();
  if (dirtyPaths().length && !state.clientProposal) await reviewChanges();
  if (isActiveRuntime() && state.clientProposal && !state.serverProposal) await requestApproval();
  const proposal = state.serverProposal || state.clientProposal;
  const prompt = backend.kind === "websocket"
    ? $("promptInput").value
    : buildTurnPrompt($("promptInput").value, proposal?.unified_diff);
  let result;
  try {
    result = await backend.requestTurn(prompt, proposal?.proposal_id);
  } catch (error) {
    if (error?.code === "conflict") error.proposalRecovery = await recoverStaleProposal(proposal);
    throw error;
  }
  if (!result.accepted) {
    const reason = result.reason || "The selected backend did not accept the agent turn.";
    $("turnOutput").textContent = reason;
    log(`VT Code turn unavailable: ${reason}`);
    status("VT Code turn unavailable", reason);
    toast("VT Code turn unavailable");
    return;
  }
  const turnId = result.turn_id || "accepted";
  log(`Requested agent turn (${turnId})`);
  $("turnOutput").textContent = result.output || "VT Code accepted the bounded request; waiting for runtime events.";
  status("Turn requested", "VT Code received the prompt and reviewed draft diff; runtime events will appear in the activity panel.");
  toast("VT Code turn requested");
}

async function refreshFile(path, force = false, render = true) {
  const fresh = await backend.readFile(path);
  if (typeof fresh?.content !== "string") throw new Error(`Backend returned invalid content for ${path}`);
  if (snapshotSizeBytes(fresh) > MAX_FILE_BYTES) throw new Error(`File exceeds the browser size limit: ${path}`);
  if (isDirty(path) && !force && snapshot(path) && fresh.digest !== snapshot(path).digest) {
    state.conflicts.add(path);
    throw new Error(`External change conflict for ${path}; discard the draft before reloading`);
  }
  state.snapshots.set(path, fresh);
  state.conflicts.delete(path);
  if (force || !isDirty(path)) state.drafts.delete(path);
  if (render && state.selected === path) renderSelectedEditor();
  renderTree();
  renderTabs();
  persistBrowserWorkspace();
}

async function reloadFile() {
  if (!state.selected) return;
  await refreshFile(state.selected);
  log(`Reloaded ${state.selected} from ${backend.kind}`);
  status("File reloaded", "The draft buffer was compared with the backend snapshot.");
}

function discardDraft() {
  if (!state.selected || !isDirty(state.selected)) throw new Error("The selected file has no draft");
  const path = state.selected;
  state.drafts.delete(path);
  state.conflicts.delete(path);
  state.clientProposal = null;
  state.serverProposal = null;
  state.approved = false;
  editor.open(path, snapshot(path).content, false);
  persistBrowserWorkspace();
  renderTree();
  renderTabs();
  renderProposal();
  updateEditorFooter();
  log(`Discarded draft for ${path}`);
  status("Draft discarded", "The editor now shows the last backend snapshot.");
}

async function runSelfCheck() {
  if (dirtyPaths().length || state.serverProposal || state.lastChange) {
    throw new Error("Finish the current change before running the self-check");
  }
  if (!state.selected) throw new Error("Select a file before running the self-check");
  const path = state.selected;
  state.drafts.set(path, `${current(path)}\n`);
  editor.open(path, current(path), true);
  await reviewChanges();
  await requestApproval();
  await applyProposal();
  await runChecks();
  await revertLastChange();
  toast("Editor self-check passed");
}

function search() {
  state.filter = $("searchInput").value.trim();
  renderTree();
  persistBrowserWorkspace();
}

async function connect() {
  const url = $("bridgeUrl").value.trim();
  const code = $("pairingCode").value.trim().toUpperCase();
  if (!url || !code) throw new Error("Enter the WebMCP WebSocket URL and the terminal pairing code");
  flushBrowserPersistence({ silent: true });
  persistBrowserSettings();
  const nextBackend = await connectVtCode(url, code);
  try {
    await loadWorkspace(nextBackend);
  } catch (error) {
    nextBackend.close();
    throw error;
  }
  $("settingsDialog").close();
  if ($("quickActionDialog").open) $("quickActionDialog").close();
  $("connectionPanel").open = false;
  $("pairingCode").value = "";
  $("connectionSummary").textContent = isActiveRuntime() ? "Active TUI connected" : "Workspace bridge connected";
  log("Connected to authenticated VT Code WebMCP");
  status("Connected to VT Code", runtimeStatus()?.turns_available === false
    ? "Workspace bridge connected, but this standalone headless adapter cannot run agent turns."
    : "Active VT Code session connected; prompts and policy remain in the terminal.");
  toast("Connected to VT Code");
  editor.focus();
}

async function loadWorkspace(nextBackend, restoreState = null) {
  const nextFiles = new Map();
  const hydrationEvents = [];
  let hydrationOverflow = false;
  let hydrationComplete = false;
  const stopHydration = nextBackend.subscribeToEvents?.((event) => {
    if (hydrationComplete) recordRuntimeEvent(event);
    else if (hydrationEvents.length < MAX_HYDRATION_EVENTS) hydrationEvents.push(event);
    else hydrationOverflow = true;
  });
  const stopConnection = nextBackend.subscribeToConnection?.(handleBackendConnection);
  const stopStatus = nextBackend.subscribeToStatus?.(handleBackendStatus);
  try {
    for (const entry of await nextBackend.listFiles()) {
      const path = typeof entry === "string" ? entry : entry?.path;
      if (typeof path !== "string" || !path) throw new Error("Backend returned an invalid workspace path");
      nextFiles.set(path, typeof entry === "string" ? { path } : entry);
    }
  } catch (error) {
    stopHydration?.();
    stopConnection?.();
    stopStatus?.();
    throw error;
  }
  state.unsubscribe?.();
  state.unsubscribeConnection?.();
  state.unsubscribeStatus?.();
  if (nextBackend !== backend) backend.close?.();
  backend = nextBackend;
  openRequest += 1;
  state.files.clear();
  for (const [path, file] of nextFiles) state.files.set(path, file);
  state.snapshots.clear();
  state.drafts.clear();
  state.openTabs = [];
  state.selected = null;
  state.expandedDirs.clear();
  state.filter = "";
  $("searchInput").value = "";
  state.clientProposal = null;
  state.serverProposal = null;
  state.approved = false;
  state.pendingTerminalApproval = false;
  state.lastChange = null;
  state.conflicts.clear();
  $("turnOutput").textContent = "No VT Code turn requested.";
  const restoreFallback = nextBackend.kind === "fallback" && restoreState?.app_instance === APP_INSTANCE;
  if (restoreFallback) {
    const available = (items) => Array.isArray(items) ? items.filter((path) => nextFiles.has(path)) : [];
    const nextDirectories = new Set();
    for (const path of nextFiles.keys()) {
      let separator = path.indexOf("/");
      while (separator > 0) {
        nextDirectories.add(path.slice(0, separator));
        separator = path.indexOf("/", separator + 1);
      }
    }
    const availableDirectories = (items) => Array.isArray(items)
      ? items.filter((path) => typeof path === "string" && nextDirectories.has(path))
      : [];
    state.openTabs = available(restoreState.open_tabs);
    state.selected = nextFiles.has(restoreState.selected) ? restoreState.selected : null;
    state.expandedDirs = new Set(availableDirectories(restoreState.expanded_dirs));
    state.filter = restoreState.filter;
    $("searchInput").value = state.filter;
    for (const [path, content] of Object.entries(restoreState.drafts)) {
      if (!nextFiles.has(path)) continue;
      try {
        const fresh = await nextBackend.readFile(path);
        state.snapshots.set(path, fresh);
        if (content !== fresh.content) state.drafts.set(path, content);
      } catch {
        // The persisted draft is discarded when its fallback file no longer exists.
      }
    }
  }
  hydrationComplete = true;
  state.unsubscribe = stopHydration;
  state.unsubscribeConnection = stopConnection;
  state.unsubscribeStatus = stopStatus;
  for (const event of hydrationEvents) recordRuntimeEvent(event);
  if (hydrationEvents.length || hydrationOverflow) {
    status("Runtime event received", hydrationOverflow
      ? "Events arrived during workspace load; refresh files to reconcile the latest snapshot."
      : "Refresh a clean file to inspect the latest backend snapshot.");
  }
  const runtime = runtimeStatus();
  const workspaceRoot = runtime?.workspace_root;
  if (backend.kind === "fallback") {
    $("connectionPanel").open = true;
    $("connectionSummary").textContent = "Pair to connect VT Code";
  }
  renderSettings();
  $("modeTitle").textContent = workspaceRoot
    ? workspaceRoot.split(/[\\/]/).filter(Boolean).at(-1) || "workspace"
    : "hello-world";
  $("modeDetail").textContent = backend.kind === "fallback"
    ? "In-memory fallback · no filesystem; state survives refreshes until Vite restarts."
    : runtime?.turns_available === false
      ? "Authenticated workspace bridge · no active agent runtime."
      : "Active VT Code session · real workspace and agent turns.";
  $("boundaryDetail").textContent = backend.kind === "fallback"
    ? "In-memory · no filesystem"
    : isHeadlessBridge()
      ? "Headless policy · explicit full-auto only"
      : "Terminal policy · explicit origin";
  $("supportStatus").textContent = backend.kind === "fallback"
    ? "Fallback mode"
    : runtime?.turns_available === false ? "Workspace bridge only" : "VT Code connected";
  $("supportStatus").classList.toggle("available", backend.kind !== "fallback");
  updateTurnControl();
  renderTree();
  renderTabs();
  renderProposal();

  const first = state.selected || state.openTabs[0] || paths()[0];
  if (first) {
    try {
      await openFile(first, false);
    } catch (error) {
      const detail = message(error);
      log(`Initial file read failed: ${detail}`);
      status("Workspace loaded with read error", detail);
    }
  }
}

async function readCurrentFile(path) {
  if (!state.files.has(path)) throw webMcpFileNotFoundError(path);
  const draft = state.drafts.get(path);
  const base = snapshot(path);
  if (draft !== undefined) {
    const draftDigest = await digest(draft);
    if (base) {
      return {
        ...base,
        content: draft,
        digest: draftDigest,
        size_bytes: contentSizeBytes(draft),
        base_digest: base.digest,
        draft: true,
      };
    }
    return {
      path,
      content: draft,
      digest: draftDigest,
      size_bytes: contentSizeBytes(draft),
      base_digest: null,
      draft: true,
    };
  }
  const file = await backend.readFile(path);
  return { ...file, draft: false };
}

function webMcpFileNotFoundError(path) {
  const error = new Error(`No workspace file named "${path}"; call list_project_files or search_code and retry with a returned path`);
  error.code = "not_found";
  return error;
}

function webMcpEnvironmentState() {
  return {
    browsing_context_required: true,
    origin_agent_cluster: typeof window.originAgentCluster === "boolean" ? window.originAgentCluster : null,
    tools_permission_allowed: typeof document.permissionsPolicy?.allowsFeature === "function"
      ? document.permissionsPolicy.allowsFeature("tools")
      : null,
  };
}

function editorStateForWebMcp() {
  const dirtyFiles = dirtyPaths();
  const workflowState = dirtyFiles.length
    ? "draft_needs_review"
    : state.selected
      ? "file_selected"
      : "workspace_ready";
  const recommendedNextTools = dirtyFiles.length
    ? ["review_draft", "open_panel"]
    : state.selected
      ? ["read_file", "search_code"]
      : ["list_project_files", "search_code"];
  return {
    backend: backend.kind,
    connected: isBridgeConnected(),
    workspace_root: runtimeStatus()?.workspace_root || null,
    bridge_settings: backend.statusPayload?.settings || null,
    authenticated_origin: backend.statusPayload?.authenticated_origin || null,
    selected: state.selected,
    open_tabs: [...state.openTabs],
    dirty_files: dirtyFiles,
    has_client_proposal: Boolean(state.clientProposal),
    has_server_proposal: Boolean(state.serverProposal),
    active_panel: document.querySelector("[data-terminal].active")?.dataset.terminal || "activity",
    workflow_state: workflowState,
    recommended_next_tools: recommendedNextTools,
    webmcp_context: webMcpEnvironmentState(),
  };
}

async function stageTextEditForWebMcp({ path, find, replace, expected_digest: expectedDigest }, { signal } = {}) {
  if (!state.files.has(path)) throw webMcpFileNotFoundError(path);
  if (!snapshot(path)) await openFile(path, false);
  if (signal?.aborted) throw signal.reason || new Error("The WebMCP edit was aborted");
  if (isDirty(path)) {
    const error = new Error(`Draft already contains changes for ${path}; call review_draft or discard the draft before staging another edit`);
    error.code = "draft_conflict";
    throw error;
  }
  const base = snapshot(path);
  if (base.digest !== expectedDigest) {
    const error = new Error(`Stale edit for ${path}; call read_file again and use its fresh digest before retrying`);
    error.code = "conflict";
    throw error;
  }
  const content = base.content;
  let next;
  try {
    next = replaceExactText(content, find, replace);
  } catch (error) {
    error.message = `${error.message} in ${path}`;
    throw error;
  }
  if (contentSizeBytes(next) > MAX_FILE_BYTES) {
    const error = new Error(`Edited file exceeds the browser size limit: ${path}; shorten the replacement and retry`);
    error.code = "limit_exceeded";
    throw error;
  }
  const draftDigest = await digest(next);
  if (signal?.aborted) throw signal.reason || new Error("The WebMCP edit was aborted");

  state.drafts.set(path, next);
  state.clientProposal = null;
  state.serverProposal = null;
  state.approved = false;
  state.pendingTerminalApproval = false;
  editor.open(path, next, true);
  persistBrowserWorkspace();
  renderTree();
  renderTabs();
  renderProposal();
  updateEditorFooter();
  selectTerminal("changes");
  log(`Staged browser draft edit for ${path}`);
  status("Agent draft staged", "Review the unified diff before requesting VT Code approval.");
  toast("Agent draft ready for review");
  return {
    staged: true,
    path,
    base_digest: base.digest,
    draft_digest: draftDigest,
    replacement_count: 1,
    requires_review: true,
  };
}

async function reviewDraftForWebMcp() {
  await reviewChanges();
  const diff = truncateUtf8(state.clientProposal?.unified_diff || "", MAX_WEBMCP_RESULT_BYTES);
  return {
    reviewed: true,
    files: state.clientProposal?.changes.map(({ path, base_digest, content }) => ({
      path,
      base_digest,
      size_bytes: contentSizeBytes(content),
    })) || [],
    unified_diff: diff.text,
    diff_truncated: diff.truncated,
  };
}

function openPanelForWebMcp(panel) {
  if (!["activity", "changes", "turn"].includes(panel)) {
    const error = new Error(`Unknown editor panel "${panel}"; choose one of: activity, changes, turn`);
    error.code = "invalid_input";
    throw error;
  }
  selectTerminal(panel);
}

async function registerWebMcp() {
  const modelContext = document.modelContext;
  const environment = webMcpEnvironmentState();
  if (environment.origin_agent_cluster === false) {
    $("webmcpCapability").textContent = "WebMCP unavailable: this document is not origin-isolated; open the top-level page in a supported browser. Editor fallback remains active.";
    return;
  }
  if (environment.tools_permission_allowed === false) {
    $("webmcpCapability").textContent = "WebMCP unavailable: the tools Permissions Policy denies this browsing context; open the page directly or delegate tools from a trusted embedder. Editor fallback remains active.";
    return;
  }
  if (!modelContext?.registerTool) {
    $("webmcpCapability").textContent = "WebMCP browser API unavailable in this browsing context; use Chrome 149+ with the origin trial or testing flag. The editor remains fully usable.";
    return;
  }
  const searchCode = async (query = "", { signal } = {}) => {
    const results = [];
    const normalizedQuery = typeof query === "string" ? query.toLowerCase() : "";
    let scannedFiles = 0;
    let scannedBytes = 0;
    let resultBytes = 0;
    let truncated = false;
    const output = () => ({
      matches: results,
      truncated,
      scanned_files: scannedFiles,
      scanned_bytes: scannedBytes,
      ...(truncated
        ? { hint: "Results are truncated; narrow the query or inspect the returned paths." }
        : results.length === 0
          ? { hint: "No matches found; try a shorter or different query." }
          : {}),
    });
    for (const path of paths()) {
      if (signal?.aborted) throw signal.reason || new Error("The WebMCP search was aborted");
      if (scannedFiles >= MAX_SEARCH_FILES || scannedBytes >= MAX_SEARCH_BYTES) {
        truncated = true;
        break;
      }
      let content = current(path);
      if (!snapshot(path)) {
        const file = await backend.readFile(path);
        if (typeof file?.content !== "string") throw new Error(`Backend returned invalid content for ${path}`);
        content = file.content;
      }
      const bytes = contentSizeBytes(content);
      if (scannedBytes + bytes > MAX_SEARCH_BYTES) {
        truncated = true;
        break;
      }
      scannedFiles += 1;
      scannedBytes += bytes;
      for (const [line, text] of content.split("\n").entries()) {
        if (text.toLowerCase().includes(normalizedQuery)) {
          const result = { path, line: line + 1, text };
          const nextBytes = contentSizeBytes(JSON.stringify(result));
          if (resultBytes + nextBytes > MAX_WEBMCP_RESULT_BYTES) {
            truncated = true;
            return output();
          }
          resultBytes += nextBytes;
          results.push(result);
        }
        if (results.length >= 200) {
          truncated = true;
          return output();
        }
      }
    }
    return output();
  };
  const tools = createWebMcpTools({
    listFiles: () => backend.listFiles(),
    readFile: readCurrentFile,
    searchCode,
    getEditorState: editorStateForWebMcp,
    openFile: (path) => openFile(path),
    stageTextEdit: stageTextEditForWebMcp,
    reviewDraft: reviewDraftForWebMcp,
    openPanel: openPanelForWebMcp,
  });
  webMcpRegistration?.dispose();
  webMcpRegistration = null;
  try {
    webMcpRegistration = await registerWebMcpTools(modelContext, tools, {
      onToolChange: (names) => {
        $("webmcpCapability").textContent = `WebMCP tools available: ${names.length}; browser agent tool set changed.`;
      },
    });
    $("webmcpCapability").textContent = `WebMCP tools registered: ${webMcpRegistration.names.length} editor and workspace tools.`;
  } catch (error) {
    $("webmcpCapability").textContent = `WebMCP registration failed; editor fallback remains active (${message(error)}).`;
  }
}

async function run(action) {
  try {
    await action();
  } catch (error) {
    const detail = message(error);
    if (error?.code === "unsupported") $("turnOutput").textContent = detail;
    log(`Action failed: ${detail}`);
    if (error?.code === "unauthorized" || error?.code === "pairing_expired") {
      openConnectionPanel();
      $("connectionSummary").textContent = "New pairing required";
      $("pairingCode").value = "";
      status("VT Code pairing expired", `For an active session rerun \`/webmcp pair ${browserOrigin()}\`; for a standalone bridge restart \`vtcode webmcp serve\`. Then enter the new URL and one-time code.`);
    } else if (error?.code === "connection_closed") {
      status("VT Code bridge disconnected", "Keep the current bridge running; the browser will reconnect automatically. If it was restarted, pair again with its newest URL and code.");
    } else if (error?.proposalRecovery) {
      status("Proposal became stale", error.proposalRecovery);
    } else {
      status(error?.code === "unsupported" ? "VT Code turn unavailable" : "Action could not complete", detail);
    }
    toast(detail);
  }
}

function confirmAction(title, copy, label, action) {
  const dialog = $("confirmDialog");
  if ($("settingsDialog").open) $("settingsDialog").close();
  $("dialogTitle").textContent = title;
  $("dialogCopy").textContent = copy;
  $("dialogConfirm").textContent = label;
  $("dialogConfirm").onclick = (event) => { event.preventDefault(); dialog.close(); void run(action); };
  dialog.showModal();
}

const QUICK_ACTIONS = [
  {
    id: "review",
    label: "Review changes",
    description: "Create a unified diff from the current draft.",
    shortcut: "⌘/Ctrl S",
    enabled: () => dirtyPaths().length > 0,
    execute: reviewChanges,
  },
  {
    id: "reload",
    label: "Reload file",
    description: "Read the selected file from the current backend.",
    enabled: () => Boolean(state.selected),
    execute: reloadFile,
  },
  {
    id: "discard",
    label: "Discard draft",
    description: "Remove the selected unsent browser draft.",
    enabled: () => Boolean(state.selected && isDirty(state.selected)),
    execute: discardDraft,
  },
  {
    id: "run-checks",
    label: "Run checks",
    description: "Run the checks allowed by the selected backend.",
    enabled: () => !$('runChecks').disabled,
    execute: runChecks,
  },
  {
    id: "filter",
    label: "Focus file filter",
    description: "Search workspace paths in the explorer.",
    shortcut: "⌘/Ctrl K",
    execute: () => $("searchInput").focus(),
  },
  {
    id: "settings",
    label: "Open settings",
    description: "Manage workspace setup and VT Code pairing.",
    shortcut: "⌘/Ctrl ,",
    execute: openSettingsDialog,
  },
  {
    id: "changes",
    label: "Open changes panel",
    description: "Review staged and proposed changes.",
    execute: () => selectTerminal("changes"),
  },
  {
    id: "turn",
    label: "Open VT Code panel",
    description: "Compose an agent turn for an active VT Code session.",
    execute: () => selectTerminal("turn"),
  },
  {
    id: "pair",
    label: "Open pairing settings",
    description: "Connect this editor to a local VT Code bridge.",
    execute: openConnectionPanel,
  },
  {
    id: "workspace",
    label: "Open workspace settings",
    description: "Generate terminal commands for a workspace.",
    execute: openWorkspaceSetup,
  },
  {
    id: "self-check",
    label: "Run self-check",
    description: "Exercise the fallback review, apply, check, and revert flow.",
    enabled: () => !dirtyPaths().length
      && !state.serverProposal
      && !state.lastChange
      && Boolean(state.selected)
      && !$('runChecks').disabled,
    execute: runSelfCheck,
  },
  {
    id: "help",
    label: "Show keyboard help",
    description: "Open shortcut and workflow help.",
    shortcut: "?",
    execute: openHelp,
  },
];

function actionEnabled(action) {
  return !action.enabled || action.enabled();
}

function executeQuickAction(action) {
  if (!actionEnabled(action)) {
    renderQuickActions($("quickActionSearch").value);
    return;
  }
  $("quickActionDialog").close();
  void run(action.execute);
}

function renderQuickActions(query = "") {
  const normalized = query.trim().toLowerCase();
  const actions = QUICK_ACTIONS.filter((action) => {
    const searchable = `${action.id} ${action.label} ${action.description}`.toLowerCase();
    return !normalized || searchable.includes(normalized);
  });
  const list = $("quickActionList");
  list.replaceChildren();
  if (!actions.length) {
    const empty = document.createElement("div");
    empty.className = "quick-action-empty";
    empty.textContent = "No matching actions";
    list.append(empty);
    return;
  }
  for (const action of actions) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "quick-action";
    button.disabled = !actionEnabled(action);
    button.title = action.description;
    button.onclick = () => executeQuickAction(action);

    const copy = document.createElement("span");
    copy.className = "quick-action-copy";
    const label = document.createElement("span");
    label.className = "quick-action-label";
    label.textContent = action.label;
    const description = document.createElement("span");
    description.className = "quick-action-description";
    description.textContent = action.description;
    copy.append(label, description);
    button.append(copy);
    if (action.shortcut) {
      const shortcut = document.createElement("kbd");
      shortcut.className = "quick-action-shortcut";
      shortcut.textContent = action.shortcut;
      button.append(shortcut);
    }
    list.append(button);
  }
}

function openQuickActions() {
  const dialog = $("quickActionDialog");
  if (dialog.open) {
    dialog.close();
    return;
  }
  if ($("confirmDialog").open) return;
  if ($("helpDialog").open) $("helpDialog").close();
  if ($("settingsDialog").open) $("settingsDialog").close();
  $("quickActionSearch").value = "";
  renderQuickActions();
  dialog.showModal();
  $("quickActionSearch").focus();
}

function openHelp() {
  const dialog = $("helpDialog");
  if (dialog.open) {
    dialog.close();
    return;
  }
  if ($("confirmDialog").open) return;
  if ($("quickActionDialog").open) $("quickActionDialog").close();
  if ($("settingsDialog").open) $("settingsDialog").close();
  dialog.showModal();
  dialog.querySelector("button")?.focus();
}

function isTypingTarget(target) {
  return Boolean(target?.matches?.("input, textarea, select, [contenteditable='true']")
    || target?.closest?.(".cm-editor"));
}

async function copyPairingCommand() {
  const command = $("pairingCommand").textContent.trim();
  try {
    await copyText(command);
    log("Copied the active pairing command");
    toast("Pairing command copied");
  } catch {
    status("Copy unavailable", "Select the command in the pairing panel and copy it manually.");
    toast("Copy unavailable; copy the command manually");
  }
}

async function copyText(value) {
  if (!navigator.clipboard?.writeText) throw new Error("Clipboard access is unavailable");
  await navigator.clipboard.writeText(value);
}

async function copySetupCommand(id, label) {
  renderWorkspaceSetup();
  try {
    await copyText($(id).textContent.trim());
    log(`Copied ${label} setup command`);
    toast(`${label} setup copied`);
  } catch {
    status("Copy unavailable", `Select the ${label.toLowerCase()} setup command and copy it manually.`);
    toast("Copy unavailable; copy the command manually");
  }
}

$("workspacePath").value = persistedBrowserSettings?.workspace_path || persistedBrowserState?.workspace_path || "";
$("bridgeUrl").value = persistedBrowserSettings?.bridge_url || "";
$("workspacePath").oninput = () => {
  renderWorkspaceSetup();
  persistBrowserSettings();
  persistBrowserWorkspace();
};
$("bridgeUrl").oninput = persistBrowserSettings;
renderWorkspaceSetup();
renderSettings();
$("searchInput").oninput = search;
for (const tab of document.querySelectorAll("[data-terminal]")) {
  tab.onclick = () => selectTerminal(tab.dataset.terminal);
}
$("reviewChanges").onclick = () => run(reviewChanges);
$("approvePatch").onclick = () => backend.kind === "fallback"
  ? confirmAction("Approve this patch?", "Approval stages an in-memory proposal; it does not write to a local filesystem.", "Approve patch", requestApproval)
  : run(requestApproval);
$("applyPatch").onclick = () => backend.kind === "fallback"
  ? confirmAction("Apply this patch?", "The approved proposal will change only the in-memory fallback project.", "Apply patch", applyProposal)
  : run(applyProposal);
$("revertPatch").onclick = () => backend.kind === "fallback"
  ? confirmAction("Revert last change?", "The backend will validate the current snapshot before restoring it.", "Revert change", revertLastChange)
  : run(revertLastChange);
$("reloadFile").onclick = () => run(reloadFile);
$("discardDraft").onclick = () => run(discardDraft);
$("runChecks").onclick = () => run(runChecks);
$("requestTurn").onclick = () => run(requestTurn);
$("connectBridge").onclick = () => run(connect);
$("closeSettings").onclick = () => $("settingsDialog").close();
$("copyPairingCommand").onclick = () => run(copyPairingCommand);
$("settingsButton").onclick = openSettingsDialog;
$("copyActiveSetup").onclick = () => run(() => copySetupCommand("activeSetupCommand", "Active"));
$("copyHeadlessSetup").onclick = () => run(() => copySetupCommand("headlessSetupCommand", "Workspace"));
$("showConnection").onclick = openConnectionPanel;
$("selfCheck").onclick = () => run(runSelfCheck);
$("quickActions").onclick = openQuickActions;
$("helpButton").onclick = openHelp;
$("quickActionSearch").oninput = () => renderQuickActions($("quickActionSearch").value);
$("quickActionSearch").onkeydown = (event) => {
  if (event.key === "ArrowDown") {
    event.preventDefault();
    $("quickActionList").querySelector(".quick-action:not(:disabled)")?.focus();
  } else if (event.key === "Enter") {
    const available = [...$("quickActionList").querySelectorAll(".quick-action:not(:disabled)")];
    if (available.length === 1) {
      event.preventDefault();
      available[0].click();
    }
  }
};
document.addEventListener("keydown", (event) => {
  if ((event.metaKey || event.ctrlKey) && event.shiftKey && event.code === "KeyP") {
    event.preventDefault();
    openQuickActions();
    return;
  }
  if ((event.metaKey || event.ctrlKey) && !event.shiftKey && !event.altKey
    && event.key === "Enter" && event.target === $("promptInput") && !$("requestTurn").disabled) {
    event.preventDefault();
    $("requestTurn").click();
    return;
  }
  if ((event.metaKey || event.ctrlKey) && !event.shiftKey && !event.altKey
    && event.code === "Comma" && (!isTypingTarget(event.target) || $("settingsDialog").open)) {
    event.preventDefault();
    openSettingsDialog();
    return;
  }
  if (event.key === "?" && !isTypingTarget(event.target)) {
    event.preventDefault();
    openHelp();
    return;
  }
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
    event.preventDefault();
    $("searchInput").focus();
  }
});

document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "hidden") flushBrowserPersistence({ silent: true });
});
globalThis.addEventListener?.("pagehide", () => flushBrowserPersistence({ silent: true }));

async function init() {
  try {
    await loadWorkspace(backend, persistedBrowserState);
    log(persistedBrowserState ? "Demo ready · restored browser workspace" : "Demo ready · deterministic fallback workspace");
    status("Ready for inspection", persistedBrowserState
      ? "Restored the fallback workspace and drafts for this Vite app instance."
      : "This is a real editor; fallback mode keeps all changes in page memory.");
    await registerWebMcp();
    if (backend.kind === "fallback") openConnectionPanel();
    else openQuickActions();
  } catch (error) {
    status("Backend unavailable", message(error));
    toast(message(error));
  }
}

void init();
