const MAX_FILE_BYTES = 2 * 1024 * 1024;
const MAX_CHANGES = 32;
const MAX_FRAME_BYTES = 1024 * 1024;
const REQUEST_TIMEOUT_MS = 30_000;
const MAX_TURN_PROMPT_BYTES = 16 * 1024;
const TURN_DIFF_PREFIX = "\n\nReview this browser draft unified diff:\n\n```diff\n";
const TURN_DIFF_SUFFIX = "\n```";
const TURN_DIFF_TRUNCATION = "\n[diff truncated by the browser prompt limit]\n";

const SEED_FILES = Object.freeze({
  "README.md": "# hello-world\n\nA tiny project for the VT Code WebMCP Challenge.\n\nThe workflow is inspect → edit → review → approve → verify.",
  "src/greeting.js": "import { name } from './config.js';\n\nexport function greeting() {\n  return `Hello, ${name}!`;\n}\n",
  "src/config.js": "export const name = 'WebMCP';\n",
});

const cloneFiles = (files) => {
  const clone = Object.create(null);
  for (const [path, content] of Object.entries(files)) clone[path] = content;
  return clone;
};

function backendError(message, code = "backend_error") {
  const error = new Error(message);
  error.code = code;
  return error;
}

function validatePath(path) {
  if (typeof path !== "string" || path.length === 0 || path.length > 4096 || path.includes("\0")) {
    throw backendError("Workspace path is invalid", "path_rejected");
  }
  if (path.startsWith("/") || path.split("/").some((part) => part === "..")) {
    throw backendError("Workspace paths must remain relative to the workspace root", "path_rejected");
  }
}

export async function digest(text) {
  const bytes = new TextEncoder().encode(text);
  if (globalThis.crypto?.subtle) {
    const hash = await globalThis.crypto.subtle.digest("SHA-256", bytes);
    return `sha256:${[...new Uint8Array(hash)].map((byte) => byte.toString(16).padStart(2, "0")).join("")}`;
  }
  let hash = 2166136261;
  for (const byte of bytes) hash = Math.imul(hash ^ byte, 16777619);
  return `sha256:fallback-${(hash >>> 0).toString(16)}`;
}

function splitLines(text) { return text.split("\n"); }

export function createUnifiedDiff(changes, beforeByPath) {
  const lines = [];
  for (const change of changes) {
    const beforeLines = splitLines(beforeByPath[change.path] ?? "");
    const afterLines = splitLines(change.content);
    let start = 0;
    while (start < beforeLines.length && start < afterLines.length && beforeLines[start] === afterLines[start]) start += 1;
    let endBefore = beforeLines.length - 1;
    let endAfter = afterLines.length - 1;
    while (endBefore >= start && endAfter >= start && beforeLines[endBefore] === afterLines[endAfter]) {
      endBefore -= 1;
      endAfter -= 1;
    }
    lines.push(`--- a/${change.path}`, `+++ b/${change.path}`);
    lines.push(`@@ -${start + 1},${Math.max(0, endBefore - start + 1)} +${start + 1},${Math.max(0, endAfter - start + 1)} @@`);
    for (const line of beforeLines.slice(start, endBefore + 1)) lines.push(`-${line}`);
    for (const line of afterLines.slice(start, endAfter + 1)) lines.push(`+${line}`);
  }
  return lines.join("\n");
}

export function buildTurnPrompt(prompt, unifiedDiff = "") {
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();
  const requestedBase = typeof prompt === "string" && prompt.trim() ? prompt.trim() : "Review the staged WebMCP patch";
  const baseBytes = encoder.encode(requestedBase);
  const base = baseBytes.length <= MAX_TURN_PROMPT_BYTES
    ? requestedBase
    : decoder.decode(baseBytes.slice(0, MAX_TURN_PROMPT_BYTES));
  if (typeof unifiedDiff !== "string" || unifiedDiff.length === 0) return base;

  const baseSizeBytes = encoder.encode(base).length;
  const framingBytes = encoder.encode(`${TURN_DIFF_PREFIX}${TURN_DIFF_SUFFIX}`).length;
  const availableDiffBytes = MAX_TURN_PROMPT_BYTES - baseSizeBytes - framingBytes;
  if (availableDiffBytes <= 0) return base;

  const diffBytes = encoder.encode(unifiedDiff);
  if (diffBytes.length <= availableDiffBytes) return `${base}${TURN_DIFF_PREFIX}${unifiedDiff}${TURN_DIFF_SUFFIX}`;

  const truncationBytes = encoder.encode(TURN_DIFF_TRUNCATION).length;
  const contentBytes = Math.max(0, availableDiffBytes - truncationBytes);
  let clippedEnd = contentBytes;
  while (clippedEnd > 0 && (diffBytes[clippedEnd] & 0xc0) === 0x80) clippedEnd -= 1;
  const clipped = decoder.decode(diffBytes.slice(0, clippedEnd));
  return `${base}${TURN_DIFF_PREFIX}${clipped}${TURN_DIFF_TRUNCATION}${TURN_DIFF_SUFFIX}`;
}

function validateChanges(changes) {
  if (!Array.isArray(changes) || changes.length === 0 || changes.length > MAX_CHANGES) {
    throw backendError("A proposal must contain between one and 32 file changes", "limit_exceeded");
  }
  const seen = new Set();
  for (const change of changes) {
    validatePath(change?.path);
    if (seen.has(change.path)) throw backendError(`Duplicate change path: ${change.path}`, "invalid_request");
    if (typeof change.content !== "string" || new TextEncoder().encode(change.content).length > MAX_FILE_BYTES) {
      throw backendError("Proposed file content exceeds the size limit", "limit_exceeded");
    }
    if (typeof change.base_digest !== "string" || change.base_digest.length > 200) {
      throw backendError(`Missing base digest for ${change.path}`, "invalid_request");
    }
    seen.add(change.path);
  }
}

function normalizeChange(change) {
  return { path: change?.path, base_digest: change?.base_digest ?? change?.baseDigest, content: change?.content };
}

export class WorkspaceBackend {
  listFiles() { throw new Error("WorkspaceBackend.listFiles is not implemented"); }
  readFile() { throw new Error("WorkspaceBackend.readFile is not implemented"); }
  proposeChanges() { throw new Error("WorkspaceBackend.proposeChanges is not implemented"); }
  applyProposal() { throw new Error("WorkspaceBackend.applyProposal is not implemented"); }
  runChecks() { throw new Error("WorkspaceBackend.runChecks is not implemented"); }
  revertLastChange() { throw new Error("WorkspaceBackend.revertLastChange is not implemented"); }
  requestTurn() { throw new Error("WorkspaceBackend.requestTurn is not implemented"); }
  subscribeToEvents() { return () => {}; }
}

export class InMemoryBackend extends WorkspaceBackend {
  constructor(initialFiles = SEED_FILES) {
    super();
    this.files = cloneFiles(initialFiles);
    this.proposals = new Map();
    this.lastChange = null;
    this.listeners = new Set();
    this.kind = "fallback";
    this.connected = false;
    this.mutationTail = Promise.resolve();
  }

  async listFiles() { return Object.keys(this.files).sort(); }

  async readFile(path) {
    validatePath(path);
    if (!(path in this.files)) throw backendError(`Unknown project file: ${path}`, "not_found");
    const content = this.files[path];
    return { path, content, digest: await digest(content), size_bytes: new TextEncoder().encode(content).length };
  }

  async proposeChanges(rawChanges) {
    return this.withMutation(async () => {
      const changes = Array.isArray(rawChanges) ? rawChanges.map(normalizeChange) : [];
      validateChanges(changes);
      const beforeByPath = Object.create(null);
      for (const change of changes) {
        const snapshot = await this.readFile(change.path);
        if (snapshot.digest !== change.base_digest) throw backendError(`Stale patch: external change detected for ${change.path}`, "conflict");
        beforeByPath[change.path] = snapshot.content;
      }
      const proposal = {
        proposal_id: `fallback-${cryptoRandomId()}`,
        changes,
        unified_diff: createUnifiedDiff(changes, beforeByPath),
      };
      this.proposals.set(proposal.proposal_id, { proposal, beforeByPath });
      return proposal;
    });
  }

  async applyProposal(proposalId) {
    return this.withMutation(async () => {
      const stored = this.proposals.get(proposalId);
      if (!stored) throw backendError("Proposal was not found", "proposal_not_found");
      const before = [];
      for (const change of stored.proposal.changes) {
        const current = await this.readFile(change.path);
        if (current.digest !== change.base_digest) throw backendError(`Stale patch: external change detected for ${change.path}`, "conflict");
        before.push(current);
      }
      for (const change of stored.proposal.changes) this.files[change.path] = change.content;
      const after = [];
      for (const change of stored.proposal.changes) after.push(await this.readFile(change.path));
      const applied = { change_id: `fallback-${cryptoRandomId()}`, paths: stored.proposal.changes.map((change) => change.path) };
      this.lastChange = { applied, before, after };
      this.proposals.delete(proposalId);
      this.emit({ type: "workspace.updated", paths: applied.paths });
      return applied;
    });
  }

  async runChecks() {
    const greeting = await this.readFile("src/greeting.js");
    const config = await this.readFile("src/config.js");
    const checks = [greeting.content.includes("return `Hello, ${name}!"), config.content.includes("export const name =")];
    return {
      command: "in-memory deterministic checks",
      passed: checks.every(Boolean),
      checks: checks.length,
      failures: checks.flatMap((ok, index) => ok ? [] : [`assertion_${index + 1}`]),
      stdout: checks.every(Boolean) ? "2 checks passed" : "deterministic check failed",
      stderr: "",
      exit_code: checks.every(Boolean) ? 0 : 1,
    };
  }

  async revertLastChange(changeId) {
    return this.withMutation(async () => {
      if (!this.lastChange || this.lastChange.applied.change_id !== changeId) throw backendError("Last change was not found", "change_not_found");
      for (const snapshot of this.lastChange.after) {
        const current = await this.readFile(snapshot.path);
        if (current.digest !== snapshot.digest) throw backendError(`File changed after apply: ${snapshot.path}`, "conflict");
      }
      for (const snapshot of this.lastChange.before) this.files[snapshot.path] = snapshot.content;
      const reverted = this.lastChange.applied;
      this.lastChange = null;
      this.emit({ type: "workspace.reverted", paths: reverted.paths });
      return reverted;
    });
  }

  async requestTurn(prompt) {
    if (typeof prompt !== "string" || new TextEncoder().encode(prompt).length > MAX_TURN_PROMPT_BYTES) {
      throw backendError("Prompt is too long", "limit_exceeded");
    }
    return {
      accepted: false,
      mode: this.kind,
      reason: "Fallback mode is browser-only. Pair a WebMCP bridge for workspace access; agent turns require an active VT Code runtime.",
      prompt,
    };
  }

  subscribeToEvents(listener) {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  async writeFile({ path, content, baseDigest, base_digest }) {
    const current = await this.readFile(path);
    const proposal = await this.proposeChanges([{ path, content, base_digest: base_digest ?? baseDigest ?? current.digest }]);
    await this.applyProposal(proposal.proposal_id);
    return this.readFile(path);
  }

  async check() { return this.runChecks(); }

  async withMutation(operation) {
    const previous = this.mutationTail;
    let release;
    this.mutationTail = new Promise((resolve) => { release = resolve; });
    await previous;
    try { return await operation(); } finally { release(); }
  }

  emit(event) { for (const listener of this.listeners) listener(event); }
}

export class VtCodeBackend extends WorkspaceBackend {
  constructor(url) {
    super();
    if (!/^wss?:\/\//.test(url)) throw backendError("WebMCP URL must use ws:// or wss://", "invalid_request");
    this.url = url;
    this.kind = "websocket";
    this.connected = false;
    this.socket = null;
    this.token = null;
    this.nextId = 1;
    this.pending = new Map();
    this.listeners = new Set();
    this.sequence = 0;
    this.opening = null;
    this.openingReject = null;
    this.resuming = null;
    this.statusPayload = null;
  }

  rejectPending(error) {
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timeout);
      pending.reject(error);
    }
    this.pending.clear();
  }

  async pair(code) {
    if (typeof code !== "string" || !/^[A-Z0-9-]{4,64}$/.test(code)) throw backendError("Enter the pairing code shown by VT Code", "invalid_request");
    await this.open();
    const payload = await this.send("pair", {
      code,
      origin: globalThis.location?.origin,
      after_sequence: this.sequence || undefined,
    }, false);
    this.token = payload.token;
    this.connected = true;
    return payload;
  }

  async resume() {
    if (!this.token) throw backendError("Pair this browser with VT Code first", "unauthorized");
    if (this.resuming) return this.resuming;
    const token = this.token;
    const request = this.send("pair", {
      resume_token: token,
      origin: globalThis.location?.origin,
      after_sequence: this.sequence || undefined,
    }, false);
    const resuming = request.then((payload) => {
      if (this.token === token) this.connected = true;
      return payload;
    });
    this.resuming = resuming;
    try { return await resuming; } finally { if (this.resuming === resuming) this.resuming = null; }
  }

  async open() {
    if (this.socket?.readyState === WebSocket.OPEN) return;
    if (this.opening) return this.opening;
    const opening = new Promise((resolve, reject) => {
      const socket = new WebSocket(this.url);
      this.socket = socket;
      let settled = false;
      this.openingReject = reject;
      const fail = (error) => {
        if (!settled) {
          settled = true;
          if (this.openingReject === reject) this.openingReject = null;
          reject(error);
        }
      };
      socket.onopen = () => {
        settled = true;
        if (this.openingReject === reject) this.openingReject = null;
        resolve();
      };
      socket.onerror = () => {
        if (this.socket === socket) {
          fail(backendError(
            "VT Code WebSocket connection failed. Keep the current bridge running and use its newest WebSocket URL and pairing code.",
            "connection_failed",
          ));
        }
      };
      socket.onclose = () => {
        if (this.socket !== socket) return;
        this.connected = false;
        this.socket = null;
        this.rejectPending(backendError("VT Code WebSocket disconnected", "connection_closed"));
        fail(backendError("VT Code WebSocket closed before pairing", "connection_closed"));
      };
      socket.onmessage = (event) => this.receive(event.data);
    });
    this.opening = opening;
    try { await opening; } finally { if (this.opening === opening) this.opening = null; }
  }

  receive(raw) {
    if (typeof raw !== "string" || new TextEncoder().encode(raw).length > MAX_FRAME_BYTES) {
      this.close();
      return;
    }
    let message;
    try { message = JSON.parse(raw); } catch { this.close(); return; }
    if (!message || typeof message !== "object") { this.close(); return; }
    if (message.type === "event") {
      if (!Number.isSafeInteger(message.sequence) || message.sequence < 1 || (this.sequence > 0 && message.sequence !== this.sequence + 1)) {
        this.close();
        return;
      }
      this.sequence = message.sequence;
      for (const listener of this.listeners) listener(message);
      return;
    }
    const pending = this.pending.get(message.request_id);
    if (!pending) return;
    this.pending.delete(message.request_id);
    clearTimeout(pending.timeout);
    if (message.ok) pending.resolve(message.payload);
    else pending.reject(backendError(message.error?.message || "VT Code request failed", message.error?.code || "runtime_error"));
  }

  async send(type, payload = {}, authenticated = true) {
    await this.open();
    if (authenticated && this.token && !this.connected) await this.resume();
    if (authenticated && !this.token) throw backendError("Pair this browser with VT Code first", "unauthorized");
    const requestId = `browser-${this.nextId++}`;
    const request = { type, request_id: requestId, ...(authenticated ? { token: this.token } : {}), ...payload };
    const serialized = JSON.stringify(request);
    if (new TextEncoder().encode(serialized).length > MAX_FRAME_BYTES) throw backendError("Request exceeds the frame limit", "limit_exceeded");
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        const pending = this.pending.get(requestId);
        if (!pending) return;
        this.pending.delete(requestId);
        pending.reject(backendError("VT Code request timed out", "request_timeout"));
      }, REQUEST_TIMEOUT_MS);
      this.pending.set(requestId, { resolve, reject, timeout });
      try {
        this.socket.send(serialized);
      } catch (error) {
        clearTimeout(timeout);
        this.pending.delete(requestId);
        reject(error);
      }
    });
  }

  listFiles() { return this.send("workspace.list_files"); }
  readFile(path) { return this.send("workspace.read_file", { path }); }
  proposeChanges(changes) {
    const normalized = Array.isArray(changes) ? changes.map(normalizeChange) : [];
    return this.send("patch.propose", { changes: normalized });
  }
  applyProposal(proposalId) { return this.send("patch.apply", { proposal_id: proposalId }); }
  runChecks(command = "cargo check --locked") { return this.send("checks.run", { command }); }
  revertLastChange(changeId) { return this.send("patch.revert", { change_id: changeId }); }
  status() { return this.send("status"); }
  requestTurn(prompt) { return this.send("turn.request", { prompt }); }
  subscribeToEvents(listener) { this.listeners.add(listener); return () => this.listeners.delete(listener); }
  close() {
    const socket = this.socket;
    this.socket = null;
    this.token = null;
    this.connected = false;
    this.statusPayload = null;
    this.openingReject?.(backendError("VT Code WebSocket disconnected", "connection_closed"));
    this.openingReject = null;
    this.rejectPending(backendError("VT Code WebSocket disconnected", "connection_closed"));
    socket?.close();
  }

  async writeFile({ path, content, baseDigest, base_digest }) {
    const current = await this.readFile(path);
    const proposal = await this.proposeChanges([{ path, content, base_digest: base_digest ?? baseDigest ?? current.digest }]);
    return this.applyProposal(proposal.proposal_id);
  }

  async check() { return this.runChecks(); }
}

export const WebSocketVtCodeBackend = VtCodeBackend;

export function createBackend() { return new InMemoryBackend(); }

export async function connectVtCode(url, code) {
  const backend = new VtCodeBackend(url);
  try {
    await backend.pair(code);
    backend.statusPayload = await backend.status();
    return backend;
  } catch (error) {
    backend.close();
    throw error;
  }
}

function cryptoRandomId() {
  if (globalThis.crypto?.randomUUID) return globalThis.crypto.randomUUID();
  return `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

export { MAX_FILE_BYTES, MAX_TURN_PROMPT_BYTES, SEED_FILES };
