import { buildTurnPrompt, connectVtCode, createBackend, createUnifiedDiff, digest, MAX_FILE_BYTES } from "./backend.js";
import { CodeEditor } from "./editor.js";
import "../styles.css";

const $ = (id) => document.getElementById(id);
const MAX_HYDRATION_EVENTS = 256;
const MAX_SEARCH_FILES = 128;
const MAX_SEARCH_BYTES = 8 * 1024 * 1024;
let backend = createBackend();
let toastTimer;
let openRequest = 0;

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

function runtimeStatus() { return backend.statusPayload?.runtime; }
function isHeadlessBridge() {
  const runtime = runtimeStatus();
  return backend.kind === "websocket" && runtime?.mutations_allowed === false && runtime.approval_authority?.startsWith("headless");
}
function isActiveRuntime() {
  return backend.kind === "websocket" && runtimeStatus()?.turns_available === true;
}

function updateTurnControl() {
  const detail = $("promptDetail");
  const button = $("requestTurn");
  const headless = backend.kind === "websocket" && runtimeStatus()?.turns_available === false;
  button.disabled = headless;
  if (backend.kind === "fallback") {
    detail.textContent = "Browser-only fallback: edit and review in memory. Pair a bridge for workspace access; agent turns need an active runtime.";
    button.textContent = "Pair editor first";
    button.title = "Open the pairing panel for workspace access";
  } else if (headless) {
    detail.textContent = "This standalone bridge exposes workspace operations only. Start `vtcode chat`, run `/webmcp pair <origin>` in that same session, then pair its new URL and code.";
    button.textContent = "Active VT Code session required";
    button.title = "Pair the bridge created by /webmcp pair in an active VT Code session";
  } else {
    detail.textContent = "The reviewed draft diff is attached; VT Code remains the execution and policy authority.";
    button.textContent = "Request VT Code turn";
    button.title = "Send the prompt and reviewed draft diff to VT Code";
  }
}

function paths() { return [...state.files.keys()].sort(); }
function snapshot(path) { return state.snapshots.get(path); }
function current(path) { return state.drafts.get(path) ?? snapshot(path)?.content ?? ""; }
function isDirty(path) { return state.drafts.has(path) && state.drafts.get(path) !== snapshot(path)?.content; }
function dirtyPaths() { return paths().filter(isDirty); }

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
  if (!state.files.has(path)) throw new Error(`Unknown project file: ${path}`);
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
  $("reviewChanges").disabled = dirtyPaths().length === 0;
  $("approvePatch").disabled = !state.clientProposal || Boolean(state.serverProposal);
  const requiresAgentTurn = isActiveRuntime() && runtimeStatus()?.mutations_allowed === false;
  $("applyPatch").disabled = !state.serverProposal || (backend.kind === "fallback" && !state.approved) || requiresAgentTurn;
  $("revertPatch").disabled = !state.lastChange;
  $("approvePatch").textContent = isActiveRuntime()
    ? "Stage for VT Code turn"
    : backend.kind === "websocket" ? "Request VT Code approval" : "Approve patch";
  $("applyPatch").textContent = requiresAgentTurn
    ? "Apply via VT Code turn"
    : backend.kind === "websocket" ? "Apply after terminal approval" : "Apply approved patch";
  const directChecksUnavailable = isHeadlessBridge() || isActiveRuntime();
  $("runChecks").disabled = directChecksUnavailable;
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
  if (!changes.length) throw new Error("Edit at least one file before reviewing changes");
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

async function requestApproval() {
  if (!state.clientProposal) await reviewChanges();
  if (state.serverProposal) throw new Error("This proposal has already been sent");
  selectTerminal("changes");
  state.serverProposal = await backend.proposeChanges(state.clientProposal.changes);
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
  selectTerminal("turn");
  if (backend.kind === "fallback") $("connectionPanel").open = true;
  if (dirtyPaths().length && !state.clientProposal) await reviewChanges();
  const proposal = state.serverProposal || state.clientProposal;
  const prompt = buildTurnPrompt($("promptInput").value, proposal?.unified_diff);
  const result = await backend.requestTurn(prompt);
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
  renderTree();
  renderTabs();
  renderProposal();
  updateEditorFooter();
  log(`Discarded draft for ${path}`);
  status("Draft discarded", "The editor now shows the last backend snapshot.");
}

function search() {
  state.filter = $("searchInput").value.trim();
  renderTree();
}

async function connect() {
  const url = $("bridgeUrl").value.trim();
  const code = $("pairingCode").value.trim().toUpperCase();
  if (!url || !code) throw new Error("Enter the WebMCP WebSocket URL and the terminal pairing code");
  const nextBackend = await connectVtCode(url, code);
  try {
    await loadWorkspace(nextBackend);
  } catch (error) {
    nextBackend.close();
    throw error;
  }
  $("connectionPanel").open = false;
  log("Connected to authenticated VT Code WebMCP");
  status("Connected to VT Code", runtimeStatus()?.turns_available === false
    ? "Workspace bridge connected, but this standalone headless adapter cannot run agent turns."
    : "Active VT Code session connected; prompts and policy remain in the terminal.");
  toast("Connected to VT Code");
}

async function loadWorkspace(nextBackend) {
  const nextFiles = new Map();
  const hydrationEvents = [];
  let hydrationOverflow = false;
  let hydrationComplete = false;
  const stopHydration = nextBackend.subscribeToEvents?.((event) => {
    if (hydrationComplete) recordRuntimeEvent(event);
    else if (hydrationEvents.length < MAX_HYDRATION_EVENTS) hydrationEvents.push(event);
    else hydrationOverflow = true;
  });
  try {
    for (const entry of await nextBackend.listFiles()) {
      const path = typeof entry === "string" ? entry : entry?.path;
      if (typeof path !== "string" || !path) throw new Error("Backend returned an invalid workspace path");
      nextFiles.set(path, typeof entry === "string" ? { path } : entry);
    }
  } catch (error) {
    stopHydration?.();
    throw error;
  }
  state.unsubscribe?.();
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
  hydrationComplete = true;
  state.unsubscribe = stopHydration;
  for (const event of hydrationEvents) recordRuntimeEvent(event);
  if (hydrationEvents.length || hydrationOverflow) {
    status("Runtime event received", hydrationOverflow
      ? "Events arrived during workspace load; refresh files to reconcile the latest snapshot."
      : "Refresh a clean file to inspect the latest backend snapshot.");
  }
  const runtime = runtimeStatus();
  const workspaceRoot = runtime?.workspace_root;
  $("modeTitle").textContent = workspaceRoot
    ? workspaceRoot.split(/[\\/]/).filter(Boolean).at(-1) || "workspace"
    : "hello-world";
  $("modeDetail").textContent = backend.kind === "fallback"
    ? "In-memory fallback · no filesystem access."
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

  const first = paths()[0];
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

async function registerWebMcp() {
  const modelContext = document.modelContext;
  if (!modelContext?.registerTool) {
    $("webmcpCapability").textContent = "WebMCP browser API unavailable; the editor remains fully usable.";
    return;
  }
  const handlers = {
    list_project_files: () => backend.listFiles(),
    read_file: ({ path } = {}) => backend.readFile(path),
    search_code: async ({ query = "" } = {}) => {
      const results = [];
      const normalizedQuery = typeof query === "string" ? query.toLowerCase() : "";
      let scannedFiles = 0;
      let scannedBytes = 0;
      for (const path of paths()) {
        if (scannedFiles >= MAX_SEARCH_FILES || scannedBytes >= MAX_SEARCH_BYTES) break;
        let content = current(path);
        if (!snapshot(path)) {
          const file = await backend.readFile(path);
          if (typeof file?.content !== "string") throw new Error(`Backend returned invalid content for ${path}`);
          content = file.content;
        }
        const bytes = contentSizeBytes(content);
        if (scannedBytes + bytes > MAX_SEARCH_BYTES) break;
        scannedFiles += 1;
        scannedBytes += bytes;
        for (const [line, text] of content.split("\n").entries()) {
          if (text.toLowerCase().includes(normalizedQuery)) results.push({ path, line: line + 1, text });
          if (results.length >= 200) return results;
        }
      }
      return results;
    },
  };
  try {
    for (const [name, execute] of Object.entries(handlers)) {
      const inputSchema = name === "search_code"
        ? { type: "object", properties: { query: { type: "string", maxLength: 120 } }, additionalProperties: false }
        : name === "read_file"
          ? { type: "object", properties: { path: { type: "string", maxLength: 4096 } }, required: ["path"], additionalProperties: false }
          : { type: "object", additionalProperties: false };
      await modelContext.registerTool({
        name,
        title: name.replaceAll("_", " "),
        description: "Bounded read-only VT Code WebMCP challenge operation.",
        inputSchema,
        annotations: { readOnlyHint: true, untrustedContentHint: name !== "list_project_files" },
        execute,
      });
    }
    $("webmcpCapability").textContent = "Browser WebMCP tools registered: bounded read-only inspection.";
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
    status(error?.code === "unsupported" ? "VT Code turn unavailable" : "Action could not complete", detail);
    toast(detail);
  }
}

function confirmAction(title, copy, label, action) {
  const dialog = $("confirmDialog");
  $("dialogTitle").textContent = title;
  $("dialogCopy").textContent = copy;
  $("dialogConfirm").textContent = label;
  $("dialogConfirm").onclick = (event) => { event.preventDefault(); dialog.close(); void run(action); };
  dialog.showModal();
}

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
$("selfCheck").onclick = () => run(async () => {
  if (dirtyPaths().length || state.serverProposal || state.lastChange) throw new Error("Finish the current change before running the self-check");
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
});
document.addEventListener("keydown", (event) => {
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
    event.preventDefault();
    $("searchInput").focus();
  }
});

async function init() {
  try {
    await loadWorkspace(backend);
    log("Demo ready · deterministic fallback workspace");
    status("Ready for inspection", "This is a real editor; fallback mode keeps all changes in page memory.");
    await registerWebMcp();
  } catch (error) {
    status("Backend unavailable", message(error));
    toast(message(error));
  }
}

void init();
