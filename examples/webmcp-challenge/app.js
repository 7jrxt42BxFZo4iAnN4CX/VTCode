const SEED_FILES = Object.freeze({
  "README.md":
    "# hello-world\n\nA tiny project for the VT Code WebMCP Challenge.\n\nThe workflow is inspect → propose → approve → verify.",
  "src/greeting.js":
    "import { name } from './config.js';\n\nexport function greeting() {\n  return `Hello, ${name}!`;\n}\n",
  "src/config.js": "export const name = 'WebMCP';\n",
});

const PATCH_PATH = "src/greeting.js";
const PATCH_BEFORE = "return `Hello, ${name}!`;";
const PATCH_AFTER = "return `Hello, ${name}! Welcome.`;";
const MAX_QUERY_LENGTH = 120;
const MAX_PATH_LENGTH = 120;

const files = { ...SEED_FILES };
const state = {
  selectedPath: "README.md",
  proposal: null,
  lastChange: null,
  patchStatus: "empty",
  uiApproval: false,
  revertConfirmation: false,
  webMcpRegistered: false,
  registeredToolControllers: new Map(),
};

const element = (id) => document.getElementById(id);
let toastTimer;

function errorMessage(error) {
  return error instanceof Error ? error.message : String(error);
}

function showToast(message) {
  const toast = element("toast");
  toast.textContent = message;
  toast.classList.add("show");
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => toast.classList.remove("show"), 2600);
}

function setStatus(title, detail) {
  element("statusText").textContent = title;
  element("statusDetail").textContent = detail;
}

function log(message) {
  const item = document.createElement("li");
  item.append(document.createTextNode(message));

  const time = document.createElement("time");
  time.textContent = new Date().toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
  item.append(time);
  element("activityLog").prepend(item);
}

function isKnownFile(path) {
  return Object.prototype.hasOwnProperty.call(files, path);
}

function normalizeInput(input) {
  if (input === undefined) {
    return {};
  }
  if (input === null || typeof input !== "object" || Array.isArray(input)) {
    throw new TypeError("Tool input must be an object");
  }
  return input;
}

function assertNoUnknownKeys(input, allowedKeys) {
  for (const key of Object.keys(input)) {
    if (!allowedKeys.includes(key)) {
      throw new Error(`Unknown tool input: ${key}`);
    }
  }
}

function requireBoundedString(input, key, maxLength) {
  const value = input[key];
  if (typeof value !== "string" || value.length === 0) {
    throw new TypeError(`${key} must be a non-empty string`);
  }
  if (value.length > maxLength) {
    throw new RangeError(`${key} must be at most ${maxLength} characters`);
  }
  return value;
}

function updateChangeControls() {
  element("proposePatch").disabled = Boolean(state.proposal || state.lastChange);
  element("approvePatch").disabled = !state.proposal || state.uiApproval;
  element("applyPatch").disabled = !state.proposal || !state.uiApproval;
  element("revertPatch").disabled = !state.lastChange;
}

function setPatchStatus(label, className = "") {
  const badge = element("proposalState");
  badge.textContent = label;
  badge.className = `state-badge${className ? ` ${className}` : ""}`;
}

function renderTree() {
  const tree = element("fileTree");
  tree.replaceChildren();

  for (const path of Object.keys(files)) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = `file-item${path === state.selectedPath ? " active" : ""}`;
    button.setAttribute("aria-pressed", String(path === state.selectedPath));

    const icon = document.createElement("span");
    icon.className = "file-icon";
    icon.setAttribute("aria-hidden", "true");
    icon.textContent = path.endsWith(".md") ? "▤" : "◇";

    button.append(icon, document.createTextNode(path));
    button.addEventListener("click", () => runUiAction(() => selectFile(path)));
    tree.append(button);
  }
}

function renderCode(path) {
  const viewer = element("codeViewer");
  const source = files[path];
  const changedLine = state.lastChange?.path === path
    ? state.lastChange.after.split("\n").findIndex((line) => line.includes(PATCH_AFTER))
    : -1;

  viewer.replaceChildren();
  source.split("\n").forEach((lineText, index, lines) => {
    const line = document.createElement("span");
    line.className = `code-line${index === changedLine ? " changed" : ""}`;
    line.textContent = lineText || " ";
    viewer.append(line);

    if (index < lines.length - 1) {
      viewer.append(document.createTextNode("\n"));
    }
  });
}

function selectFile(path, { record = true } = {}) {
  if (!isKnownFile(path)) {
    throw new Error("Unknown project file");
  }

  state.selectedPath = path;
  element("fileTitle").textContent = path;
  element("fileType").textContent = path.split(".").pop().toUpperCase();
  renderCode(path);
  element("lineCount").textContent = `${files[path].split("\n").length} lines`;
  const editorState = element("editorState");
  const changed = state.lastChange?.path === path;
  editorState.textContent = changed ? "Modified in memory · revert available" : "Original snapshot · in-memory";
  editorState.classList.toggle("changed", changed);
  renderTree();

  if (record) {
    log(`Inspected ${path}`);
    setStatus("Ready for inspection", "Selected file is shown in the read-only viewer.");
  }
}

function renderEmptyDiff(message = "Use “Stage greeting update” to preview a deterministic patch.") {
  const empty = document.createElement("div");
  empty.className = "empty-state";

  const symbol = document.createElement("span");
  symbol.setAttribute("aria-hidden", "true");
  symbol.textContent = "⌘";
  const title = document.createElement("strong");
  title.textContent = "No changes staged";
  const detail = document.createElement("small");
  detail.textContent = message;
  empty.append(symbol, title, detail);
  element("diffView").replaceChildren(empty);
}

function renderDiffLine(text, className = "") {
  const line = document.createElement("div");
  line.className = `diff-line${className ? ` ${className}` : ""}`;
  line.textContent = text;
  return line;
}

function renderDiff() {
  const diff = element("diffView");
  diff.replaceChildren();

  if (state.proposal) {
    diff.append(
      renderDiffLine(`@@ ${state.proposal.path}`),
      renderDiffLine(`− ${PATCH_BEFORE}`, "removed"),
      renderDiffLine(`+ ${PATCH_AFTER}`, "added"),
    );
    return;
  }

  if (state.lastChange) {
    const beforeLine = state.lastChange.before.split("\n").find((line) => line.includes(PATCH_BEFORE)) ?? PATCH_BEFORE;
    const afterLine = state.lastChange.after.split("\n").find((line) => line.includes(PATCH_AFTER)) ?? PATCH_AFTER;
    diff.append(
      renderDiffLine("✓ Applied to in-memory project · source viewer updated", "applied"),
      renderDiffLine(`− ${beforeLine}`, "removed"),
      renderDiffLine(`+ ${afterLine}`, "added"),
    );
    return;
  }

  if (state.patchStatus === "reverted") {
    diff.append(renderDiffLine("↶ Last change reverted."));
    return;
  }

  renderEmptyDiff();
}

function listProjectFiles(input = {}) {
  const normalized = normalizeInput(input);
  assertNoUnknownKeys(normalized, []);
  return Object.keys(files);
}

function searchCode(input = {}) {
  const normalized = normalizeInput(input);
  assertNoUnknownKeys(normalized, ["query"]);
  const query = normalized.query ?? "";
  if (typeof query !== "string") {
    throw new TypeError("query must be a string");
  }
  if (query.length > MAX_QUERY_LENGTH) {
    throw new RangeError(`query must be at most ${MAX_QUERY_LENGTH} characters`);
  }

  const normalizedQuery = query.toLowerCase();
  if (!normalizedQuery) {
    return [];
  }

  return Object.entries(files).flatMap(([path, text]) =>
    text
      .split("\n")
      .map((line, index) => ({ path, line: index + 1, text: line }))
      .filter((result) => result.text.toLowerCase().includes(normalizedQuery)),
  );
}

function readFile(input = {}) {
  const normalized = normalizeInput(input);
  assertNoUnknownKeys(normalized, ["path"]);
  const path = requireBoundedString(normalized, "path", MAX_PATH_LENGTH);
  if (!isKnownFile(path)) {
    throw new Error("Unknown project file");
  }
  return files[path];
}

function proposePatch(input = {}) {
  const normalized = normalizeInput(input);
  assertNoUnknownKeys(normalized, []);

  if (state.lastChange) {
    throw new Error("Revert the current change before staging another proposal");
  }
  if (state.proposal) {
    return { ...state.proposal, requiresApproval: true };
  }

  const before = files[PATCH_PATH];
  if (!before.includes(PATCH_BEFORE)) {
    throw new Error("The deterministic patch target is no longer available");
  }

  state.proposal = {
    path: PATCH_PATH,
    before,
    after: before.replace(PATCH_BEFORE, PATCH_AFTER),
  };
  state.patchStatus = "ready";
  setPatchStatus("Awaiting approval", "ready");
  element("approvalCopy").textContent =
    "A deterministic greeting update is staged locally. Review the diff, then approve it.";
  renderDiff();
  updateChangeControls();
  log("Staged proposal for src/greeting.js");
  setStatus("Patch awaiting approval", "The sample project has not changed.");
  showToast("Patch staged — review before applying");
  return { ...state.proposal, requiresApproval: true };
}

async function approvePatch(input = {}) {
  const normalized = normalizeInput(input);
  assertNoUnknownKeys(normalized, []);
  if (!state.proposal) {
    throw new Error("Stage a patch before approving it");
  }
  state.uiApproval = true;
  state.patchStatus = "approved";
  setPatchStatus("Approved · ready to apply", "approved");
  element("approvalCopy").textContent =
    "This patch is approved. Apply it from the UI or let an agent invoke the enabled apply tool.";
  updateChangeControls();
  log("Approved staged patch");
  setStatus("Patch approved", "The sample project has not changed. Apply is now enabled.");
  showToast("Patch approved — ready to apply");
  try {
    await registerMutationTool("apply_approved_patch");
  } catch (error) {
    setSupportStatus("WebMCP lifecycle error · UI fallback");
    log(`WebMCP lifecycle error: ${errorMessage(error)}`);
  }
  return { approved: true, path: state.proposal.path };
}

function runChecks(input = {}) {
  const normalized = normalizeInput(input);
  assertNoUnknownKeys(normalized, []);

  const greetingSource = files["src/greeting.js"];
  const assertions = [
    greetingSource.includes("import { name } from './config.js';") &&
      greetingSource.includes("return `Hello, ${name}!"),
    files["src/config.js"].includes("export const name ="),
  ];
  const passed = assertions.every(Boolean);
  const failures = assertions
    .map((assertion, index) => (assertion ? null : `assertion_${index + 1}`))
    .filter(Boolean);

  log(passed ? `Checks passed (${assertions.length}/${assertions.length})` : "Checks found an issue");
  setStatus(
    passed ? "Checks passed" : "Checks need attention",
    "Deterministic in-browser checks · no external services",
  );
  showToast(passed ? "All local checks passed" : "A local check failed");
  return { passed, checks: assertions.length, failures };
}

async function applyApprovedPatch(input = {}) {
  const normalized = normalizeInput(input);
  assertNoUnknownKeys(normalized, []);
  if (!state.proposal) {
    throw new Error("No patch is staged");
  }
  if (!state.uiApproval) {
    throw new Error("Explicit UI approval is required before applying a patch");
  }

  const change = { ...state.proposal };
  files[change.path] = change.after;
  state.lastChange = change;
  state.proposal = null;
  state.patchStatus = "applied";
  state.uiApproval = false;
  setPatchStatus("Applied", "approved");
  element("approvalCopy").textContent =
    "The approved patch is active in memory. Revert it whenever you want to restore the original file.";
  renderDiff();
  updateChangeControls();
  selectFile(change.path, { record: false });
  log("Applied approved patch");
  setStatus("Change applied", "Only the in-memory sample project changed.");
  showToast("Approved patch applied");
  await transitionMutationTool("apply_approved_patch", "revert_last_change");
  return { applied: true, path: change.path };
}

function revertLastChange(input = {}) {
  const normalized = normalizeInput(input);
  assertNoUnknownKeys(normalized, []);
  if (!state.lastChange) {
    throw new Error("No change to revert");
  }
  if (!state.revertConfirmation) {
    throw new Error("Explicit UI confirmation is required before reverting a change");
  }

  const change = state.lastChange;
  files[change.path] = change.before;
  state.lastChange = null;
  state.patchStatus = "reverted";
  state.revertConfirmation = false;
  setPatchStatus("Reverted");
  element("approvalCopy").textContent =
    "The last approved change was restored. You can stage the deterministic proposal again.";
  renderDiff();
  updateChangeControls();
  selectFile(change.path, { record: false });
  log("Reverted last change");
  setStatus("Change reverted", "The original in-memory file content is restored.");
  showToast("Last change reverted");
  unregisterTool("revert_last_change");
  return { reverted: true, path: change.path };
}

async function withPermission(permission, action) {
  state[permission] = true;
  try {
    return await action();
  } finally {
    state[permission] = false;
  }
}

function showConfirmation({ title, copy, confirmLabel, action }) {
  const dialog = element("confirmDialog");
  element("dialogTitle").textContent = title;
  element("dialogCopy").textContent = copy;
  element("dialogConfirm").textContent = confirmLabel;
  element("dialogConfirm").onclick = (event) => {
    event.preventDefault();
    dialog.close("confirm");
    void action();
  };
  dialog.showModal();
}

async function runUiAction(action) {
  try {
    return await action();
  } catch (error) {
    const message = errorMessage(error);
    log(`Action failed: ${message}`);
    setStatus("Action could not complete", message);
    showToast(message);
    return null;
  }
}

async function runSelfCheck() {
  if (state.proposal || state.lastChange) {
    throw new Error("Finish or revert the current change before running self-check");
  }

  const proposal = proposePatch();
  const approval = await approvePatch();
  const applied = await applyApprovedPatch();
  const checks = runChecks();
  const reverted = await withPermission("revertConfirmation", () => revertLastChange());
  const passed = Boolean(
    proposal.requiresApproval && approval.approved && applied.applied && checks.passed && reverted.reverted,
  );
  log(passed ? "Self-check passed: gate, apply, verify, revert" : "Self-check failed");
  setStatus(
    passed ? "Self-check passed" : "Self-check failed",
    "Proposal → approval → apply → verify → revert",
  );
  showToast(passed ? "Self-check passed" : "Self-check failed");
  return { passed };
}

function renderSearch() {
  const results = searchCode({ query: element("searchInput").value });
  const container = element("searchResults");
  container.replaceChildren();

  if (!results.length) {
    const empty = document.createElement("small");
    empty.textContent = "No matching lines.";
    container.append(empty);
    return;
  }

  results.slice(0, 8).forEach((result) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "result";
    button.setAttribute("aria-label", `Open ${result.path}, line ${result.line}`);
    button.textContent = `${result.path}:${result.line}`;

    const preview = document.createElement("small");
    preview.textContent = result.text.trim();
    button.append(preview);
    button.addEventListener("click", () => runUiAction(() => selectFile(result.path)));
    container.append(button);
  });
}

function createToolExecutor(name, handler) {
  return async (input = {}, options = {}) => {
    if (options.signal?.aborted) {
      throw options.signal.reason ?? new DOMException("Tool execution was cancelled", "AbortError");
    }
    log(`Agent called ${name}`);
    const result = await handler(input, options);
    if (options.signal?.aborted) {
      throw options.signal.reason ?? new DOMException("Tool execution was cancelled", "AbortError");
    }
    return result;
  };
}

const toolDefinitions = [
  {
    name: "list_project_files",
    title: "List project files",
    description: "List the files available in the bounded in-memory sample project.",
    inputSchema: { type: "object", properties: {}, additionalProperties: false },
    annotations: { readOnlyHint: true },
    execute: createToolExecutor("list_project_files", listProjectFiles),
  },
  {
    name: "search_code",
    title: "Search code",
    description: "Find matching lines in the bounded sample project and return file and line locations.",
    inputSchema: {
      type: "object",
      properties: { query: { type: "string" } },
      required: ["query"],
      additionalProperties: false,
    },
    annotations: { readOnlyHint: true, untrustedContentHint: true },
    execute: createToolExecutor("search_code", searchCode),
  },
  {
    name: "read_file",
    title: "Read a project file",
    description: "Read the contents of one known file from the bounded sample project.",
    inputSchema: {
      type: "object",
      properties: { path: { type: "string" } },
      required: ["path"],
      additionalProperties: false,
    },
    annotations: { readOnlyHint: true, untrustedContentHint: true },
    execute: createToolExecutor("read_file", readFile),
  },
  {
    name: "propose_patch",
    title: "Propose a patch",
    description: "Stage a deterministic greeting update for the developer to review.",
    inputSchema: { type: "object", properties: {}, additionalProperties: false },
    annotations: { untrustedContentHint: true },
    execute: createToolExecutor("propose_patch", proposePatch),
  },
  {
    name: "run_checks",
    title: "Run local checks",
    description: "Run deterministic content checks against the in-memory sample project.",
    inputSchema: { type: "object", properties: {}, additionalProperties: false },
    annotations: { readOnlyHint: true },
    execute: createToolExecutor("run_checks", runChecks),
  },
  {
    name: "apply_approved_patch",
    title: "Apply an approved patch",
    description: "Apply the staged patch after the developer has approved it in the interface.",
    inputSchema: { type: "object", properties: {}, additionalProperties: false },
    execute: createToolExecutor("apply_approved_patch", applyApprovedPatch),
  },
  {
    name: "revert_last_change",
    title: "Revert the last change",
    description: "Restore the last applied patch after the developer confirms the revert in the interface.",
    inputSchema: { type: "object", properties: {}, additionalProperties: false },
    execute: createToolExecutor("revert_last_change", revertLastChange),
  },
];

const toolDefinitionByName = new Map(toolDefinitions.map((tool) => [tool.name, tool]));
const SAFE_TOOL_NAMES = [
  "list_project_files",
  "search_code",
  "read_file",
  "propose_patch",
  "run_checks",
];

function unregisterTool(name) {
  const controller = state.registeredToolControllers.get(name);
  if (controller) {
    controller.abort();
    state.registeredToolControllers.delete(name);
    log(`Unregistered ${name}`);
  }
}

async function registerTools(names) {
  let modelContext;
  try {
    modelContext = document.modelContext;
  } catch (error) {
    throw new Error(`WebMCP unavailable: ${errorMessage(error)}`);
  }
  if (!modelContext || typeof modelContext.registerTool !== "function") {
    throw new Error("WebMCP unavailable");
  }

  const controllers = new Map();
  try {
    for (const name of names) {
      if (state.registeredToolControllers.has(name)) continue;
      const controller = new AbortController();
      controllers.set(name, controller);
      await modelContext.registerTool(toolDefinitionByName.get(name), {
        signal: controller.signal,
      });
      state.registeredToolControllers.set(name, controller);
    }
  } catch (error) {
    for (const [name, controller] of controllers) {
      controller.abort();
      state.registeredToolControllers.delete(name);
    }
    throw error;
  }
}

async function registerMutationTool(name) {
  if (!state.webMcpRegistered) return;
  await registerTools([name]);
  setSupportStatus(`WebMCP available · ${state.registeredToolControllers.size} tools`, true);
  log(`Registered ${name}`);
}

async function transitionMutationTool(removeName, addName) {
  unregisterTool(removeName);
  try {
    await registerMutationTool(addName);
  } catch (error) {
    setSupportStatus("WebMCP lifecycle error · UI fallback");
    log(`WebMCP lifecycle error: ${errorMessage(error)}`);
  }
}

function setSupportStatus(label, available = false) {
  const status = element("supportStatus");
  status.textContent = label;
  status.classList.toggle("available", available);
}

async function registerWebMcp() {
  let modelContext;
  try {
    modelContext = document.modelContext;
  } catch (error) {
    setSupportStatus("WebMCP unavailable · UI fallback");
    log(`WebMCP unavailable: ${errorMessage(error)}`);
    return false;
  }

  if (!modelContext || typeof modelContext.registerTool !== "function") {
    setSupportStatus("WebMCP unavailable · UI fallback");
    return false;
  }

  try {
    await registerTools(SAFE_TOOL_NAMES);
    state.webMcpRegistered = true;
    setSupportStatus(`WebMCP available · ${SAFE_TOOL_NAMES.length} tools`, true);
    log(`Registered ${SAFE_TOOL_NAMES.length} safe WebMCP tools`);
    return true;
  } catch (error) {
    for (const name of [...state.registeredToolControllers.keys()]) unregisterTool(name);
    setSupportStatus("WebMCP registration failed · UI fallback");
    log(`WebMCP registration failed: ${errorMessage(error)}`);
    return false;
  }
}

element("searchInput").addEventListener("input", () => runUiAction(renderSearch));
element("proposePatch").addEventListener("click", () => runUiAction(proposePatch));
element("applyPatch").addEventListener("click", () => {
  showConfirmation({
    title: "Apply this patch?",
    copy: "Only the in-memory sample project will change.",
    confirmLabel: "Apply approved patch",
    action: () => runUiAction(() => applyApprovedPatch()),
  });
});
element("approvePatch").addEventListener("click", () => {
  showConfirmation({
    title: "Approve this patch?",
    copy: "Approval enables the apply action but does not change the project yet.",
    confirmLabel: "Approve patch",
    action: () => runUiAction(() => approvePatch()),
  });
});
element("revertPatch").addEventListener("click", () => {
  showConfirmation({
    title: "Revert last change?",
    copy: "The applied patch will be restored to its previous content.",
    confirmLabel: "Revert change",
    action: () => runUiAction(() => withPermission("revertConfirmation", revertLastChange)),
  });
});
element("runChecks").addEventListener("click", () => runUiAction(runChecks));
element("selfCheck").addEventListener("click", () => {
  showConfirmation({
    title: "Run self-check flow?",
    copy: "This will stage, approve, verify, and revert the demo patch.",
    confirmLabel: "Run self-check",
    action: () => runUiAction(runSelfCheck),
  });
});
document.addEventListener("keydown", (event) => {
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
    event.preventDefault();
    element("searchInput").focus();
  }
});

renderTree();
selectFile(state.selectedPath, { record: false });
renderEmptyDiff();
updateChangeControls();
log("Demo ready · memory-only workspace");
void registerWebMcp();
