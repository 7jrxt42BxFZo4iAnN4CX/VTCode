const MAX_FILE_BYTES = 2 * 1024 * 1024;
const MAX_CHANGES = 32;
const MAX_FRAME_BYTES = 1024 * 1024;
const REQUEST_TIMEOUT_MS = 30_000;
const MIN_HEARTBEAT_INTERVAL_MS = 250;
const MAX_HEARTBEAT_INTERVAL_MS = 30_000;
const DEFAULT_SESSION_TTL_SECS = 300;
const MAX_TURN_PROMPT_BYTES = 16 * 1024;
const TURN_DIFF_PREFIX = "\n\nReview this browser draft unified diff:\n\n```diff\n";
const TURN_DIFF_SUFFIX = "\n```";
const TURN_DIFF_TRUNCATION = "\n[diff truncated by the browser prompt limit]\n";
const DIFF_CONTEXT_LINES = 3;
const MAX_DIFF_TRACE_CELLS = 1_000_000;

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

function splitDiffLines(text) {
  const lines = [];
  let lineStart = 0;
  for (let index = 0; index < text.length; index += 1) {
    const character = text[index];
    if (character !== "\n" && character !== "\r") continue;
    const ending = character === "\r" && text[index + 1] === "\n" ? "\r\n" : character;
    lines.push({ text: text.slice(lineStart, index), ending });
    if (ending === "\r\n") index += 1;
    lineStart = index + 1;
  }
  if (lineStart < text.length) lines.push({ text: text.slice(lineStart), ending: "" });
  return lines;
}

function sameDiffLine(left, right) {
  return left.text === right.text && left.ending === right.ending;
}

function equalOperations(lines) {
  return lines.map((line) => ({ type: "equal", line }));
}

function replaceOperations(before, after) {
  return [
    ...before.map((line) => ({ type: "delete", line })),
    ...after.map((line) => ({ type: "insert", line })),
  ];
}

function backtrackDiff(trace, before, after) {
  let beforeIndex = before.length;
  let afterIndex = after.length;
  const reversed = [];

  for (let distance = trace.length - 1; distance > 0; distance -= 1) {
    const previous = trace[distance - 1];
    const diagonal = beforeIndex - afterIndex;
    const shouldInsert = diagonal === -distance
      || (diagonal !== distance
        && (previous.get(diagonal - 1) ?? -Infinity) < (previous.get(diagonal + 1) ?? -Infinity));
    const previousDiagonal = shouldInsert ? diagonal + 1 : diagonal - 1;
    const previousBeforeIndex = previous.get(previousDiagonal);
    if (previousBeforeIndex === undefined) return null;
    const previousAfterIndex = previousBeforeIndex - previousDiagonal;

    while (beforeIndex > previousBeforeIndex && afterIndex > previousAfterIndex) {
      reversed.push({ type: "equal", line: before[beforeIndex - 1] });
      beforeIndex -= 1;
      afterIndex -= 1;
    }

    if (beforeIndex === previousBeforeIndex) {
      reversed.push({ type: "insert", line: after[afterIndex - 1] });
      afterIndex -= 1;
    } else {
      reversed.push({ type: "delete", line: before[beforeIndex - 1] });
      beforeIndex -= 1;
    }
  }

  while (beforeIndex > 0 && afterIndex > 0) {
    reversed.push({ type: "equal", line: before[beforeIndex - 1] });
    beforeIndex -= 1;
    afterIndex -= 1;
  }
  while (beforeIndex > 0) {
    reversed.push({ type: "delete", line: before[beforeIndex - 1] });
    beforeIndex -= 1;
  }
  while (afterIndex > 0) {
    reversed.push({ type: "insert", line: after[afterIndex - 1] });
    afterIndex -= 1;
  }
  return reversed.reverse();
}

function myersDiff(before, after) {
  if (!before.length) return after.map((line) => ({ type: "insert", line }));
  if (!after.length) return before.map((line) => ({ type: "delete", line }));

  const maxDistance = before.length + after.length;
  const trace = [];
  let traceCells = 0;
  let frontier = new Map([[0, 0]]);

  for (let distance = 0; distance <= maxDistance; distance += 1) {
    const next = new Map();
    for (let diagonal = -distance; diagonal <= distance; diagonal += 2) {
      const shouldInsert = diagonal === -distance
        || (diagonal !== distance
          && (frontier.get(diagonal - 1) ?? -Infinity) < (frontier.get(diagonal + 1) ?? -Infinity));
      let beforeIndex = shouldInsert
        ? frontier.get(diagonal + 1) ?? 0
        : (frontier.get(diagonal - 1) ?? 0) + 1;
      let afterIndex = beforeIndex - diagonal;
      while (beforeIndex < before.length && afterIndex < after.length
        && sameDiffLine(before[beforeIndex], after[afterIndex])) {
        beforeIndex += 1;
        afterIndex += 1;
      }
      next.set(diagonal, beforeIndex);
    }

    traceCells += next.size;
    if (traceCells > MAX_DIFF_TRACE_CELLS) return null;
    trace.push(next);
    const completed = next.get(before.length - after.length);
    if (completed === before.length) return backtrackDiff(trace, before, after);
    frontier = next;
  }
  return null;
}

function diffOperations(before, after) {
  let prefix = 0;
  while (prefix < before.length && prefix < after.length && sameDiffLine(before[prefix], after[prefix])) prefix += 1;

  let suffix = 0;
  while (before.length - suffix > prefix && after.length - suffix > prefix
    && sameDiffLine(before[before.length - suffix - 1], after[after.length - suffix - 1])) suffix += 1;

  const beforeMiddle = before.slice(prefix, before.length - suffix);
  const afterMiddle = after.slice(prefix, after.length - suffix);
  const middle = myersDiff(beforeMiddle, afterMiddle) || replaceOperations(beforeMiddle, afterMiddle);
  return [
    ...equalOperations(before.slice(0, prefix)),
    ...middle,
    ...equalOperations(before.slice(before.length - suffix)),
  ];
}

function formatDiffRange(start, count) {
  return count === 1 ? `${start}` : `${start},${count}`;
}

function renderDiffHunks(operations) {
  const changed = operations
    .map((operation, index) => operation.type === "equal" ? -1 : index)
    .filter((index) => index >= 0);
  if (!changed.length) return [];

  const hunks = [];
  for (const index of changed) {
    const start = Math.max(0, index - DIFF_CONTEXT_LINES);
    const end = Math.min(operations.length, index + DIFF_CONTEXT_LINES + 1);
    const previous = hunks.at(-1);
    if (previous && start <= previous.end) previous.end = Math.max(previous.end, end);
    else hunks.push({ start, end });
  }

  const beforeOffsets = [0];
  const afterOffsets = [0];
  for (const operation of operations) {
    beforeOffsets.push(beforeOffsets.at(-1) + (operation.type === "insert" ? 0 : 1));
    afterOffsets.push(afterOffsets.at(-1) + (operation.type === "delete" ? 0 : 1));
  }

  const lines = [];
  for (const hunk of hunks) {
    const beforeStartCount = beforeOffsets[hunk.start];
    const afterStartCount = afterOffsets[hunk.start];
    const beforeCount = beforeOffsets[hunk.end] - beforeStartCount;
    const afterCount = afterOffsets[hunk.end] - afterStartCount;
    const beforeStart = beforeCount === 0 ? beforeStartCount : beforeStartCount + 1;
    const afterStart = afterCount === 0 ? afterStartCount : afterStartCount + 1;
    lines.push(`@@ -${formatDiffRange(beforeStart, beforeCount)} +${formatDiffRange(afterStart, afterCount)} @@`);

    for (const operation of operations.slice(hunk.start, hunk.end)) {
      const prefix = operation.type === "equal" ? " " : operation.type === "delete" ? "-" : "+";
      lines.push(`${prefix}${operation.line.text}`);
      if (!operation.line.ending) lines.push("\\ No newline at end of file");
    }
  }
  return lines;
}

export function createUnifiedDiff(changes, beforeByPath) {
  const lines = [];
  for (const change of changes) {
    const beforeContent = typeof beforeByPath?.[change.path] === "string" ? beforeByPath[change.path] : "";
    const afterContent = typeof change?.content === "string" ? change.content : "";
    const operations = diffOperations(splitDiffLines(beforeContent), splitDiffLines(afterContent));
    const hunks = renderDiffHunks(operations);
    if (!hunks.length) continue;
    lines.push(`--- a/${change.path}`, `+++ b/${change.path}`, ...hunks);
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
  requestTurn(_prompt, _proposalId) { throw new Error("WorkspaceBackend.requestTurn is not implemented"); }
  subscribeToEvents() { return () => {}; }
  subscribeToConnection() { return () => {}; }
  subscribeToStatus() { return () => {}; }
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

  exportFiles() { return cloneFiles(this.files); }

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

  async requestTurn(prompt, _proposalId) {
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
    this.sessionExpiresInSecs = DEFAULT_SESSION_TTL_SECS;
    this.heartbeatTimer = null;
    this.heartbeatInFlight = null;
    this.connectionListeners = new Set();
    this.statusListeners = new Set();
    this.connectionState = "disconnected";
  }

  setConnectionState(state, error = null) {
    if (this.connectionState === state && !error) return;
    this.connectionState = state;
    for (const listener of this.connectionListeners) listener({ state, error });
  }

  setStatusPayload(payload) {
    this.statusPayload = payload;
    for (const listener of this.statusListeners) listener(payload);
  }

  updateSessionLease(payload) {
    const seconds = Number(payload?.expires_in_secs);
    if (Number.isSafeInteger(seconds) && seconds >= 0) this.sessionExpiresInSecs = Math.max(1, seconds);
  }

  heartbeatIntervalMs() {
    return Math.max(MIN_HEARTBEAT_INTERVAL_MS, Math.min(MAX_HEARTBEAT_INTERVAL_MS, Math.floor(this.sessionExpiresInSecs * 1000 / 3)));
  }

  startHeartbeat() {
    this.stopHeartbeat();
    this.heartbeatTimer = setInterval(() => { void this.keepAlive(); }, this.heartbeatIntervalMs());
  }

  stopHeartbeat() {
    if (this.heartbeatTimer) clearInterval(this.heartbeatTimer);
    this.heartbeatTimer = null;
  }

  async keepAlive() {
    if (!this.token || this.heartbeatInFlight) return;
    const heartbeat = this.status();
    this.heartbeatInFlight = heartbeat;
    try {
      await heartbeat;
      if (this.connected) this.setConnectionState("connected");
    } catch (error) {
      if (error?.code === "unauthorized" || error?.code === "pairing_expired") return;
      if (error?.code === "request_in_progress") return;
      if (this.token && ["connection_closed", "connection_failed", "request_timeout"].includes(error?.code)) {
        this.disconnectSocket(error, "reconnecting");
      }
    } finally {
      if (this.heartbeatInFlight === heartbeat) this.heartbeatInFlight = null;
    }
  }

  disconnectSocket(error, state = "disconnected") {
    const socket = this.socket;
    this.socket = null;
    this.connected = false;
    this.setStatusPayload(null);
    this.rejectPending(error);
    socket?.close();
    this.setConnectionState(state, error);
  }

  invalidateSession(error) {
    const socket = this.socket;
    this.socket = null;
    this.token = null;
    this.connected = false;
    this.setStatusPayload(null);
    this.stopHeartbeat();
    this.rejectPending(error);
    socket?.close();
    this.setConnectionState("reauthorize", error);
  }

  protocolError(detail) {
    this.invalidateSession(backendError(`VT Code sent an invalid WebMCP frame: ${detail}`, "protocol_error"));
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
    this.updateSessionLease(payload);
    this.connected = true;
    this.startHeartbeat();
    this.setConnectionState("connected");
    return payload;
  }

  async resume() {
    if (!this.token) throw backendError("Pair this browser with VT Code first", "unauthorized");
    if (this.resuming) return this.resuming;
    const token = this.token;
    this.setConnectionState("reconnecting");
    const request = this.send("pair", {
      resume_token: token,
      origin: globalThis.location?.origin,
      after_sequence: this.sequence || undefined,
    }, false);
    const resuming = request.then((payload) => {
      if (this.token === token) {
        this.updateSessionLease(payload);
        this.connected = true;
        this.startHeartbeat();
        this.setConnectionState("connected");
      }
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
        const error = backendError("VT Code WebSocket disconnected", "connection_closed");
        this.disconnectSocket(error);
        fail(backendError("VT Code WebSocket closed before pairing", "connection_closed"));
      };
      socket.onmessage = (event) => this.receive(event.data);
    });
    this.opening = opening;
    try { await opening; } finally { if (this.opening === opening) this.opening = null; }
  }

  receive(raw) {
    if (typeof raw !== "string" || new TextEncoder().encode(raw).length > MAX_FRAME_BYTES) {
      this.protocolError("frame is missing or exceeds the configured limit");
      return;
    }
    let message;
    try { message = JSON.parse(raw); } catch {
      this.protocolError("frame is not valid JSON");
      return;
    }
    if (!message || typeof message !== "object") {
      this.protocolError("frame must be a JSON object");
      return;
    }
    if (message.type === "event") {
      if (!Number.isSafeInteger(message.sequence) || message.sequence < 1 || (this.sequence > 0 && message.sequence !== this.sequence + 1)) {
        this.protocolError("runtime event sequence is invalid or has a gap");
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
    else {
      const error = backendError(message.error?.message || "VT Code request failed", message.error?.code || "runtime_error");
      if (error.code === "unauthorized" || error.code === "pairing_expired") this.invalidateSession(error);
      pending.reject(error);
    }
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
      } catch {
        clearTimeout(timeout);
        this.pending.delete(requestId);
        const connectionError = backendError("VT Code WebSocket could not send the request", "connection_closed");
        this.disconnectSocket(connectionError, "reconnecting");
        reject(connectionError);
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
  async status() {
    const payload = await this.send("status");
    this.setStatusPayload(payload);
    return payload;
  }
  requestTurn(prompt, proposalId) {
    const payload = { prompt };
    if (proposalId) payload.proposal_id = proposalId;
    return this.send("turn.request", payload);
  }
  subscribeToEvents(listener) { this.listeners.add(listener); return () => this.listeners.delete(listener); }
  subscribeToConnection(listener) {
    this.connectionListeners.add(listener);
    return () => this.connectionListeners.delete(listener);
  }
  subscribeToStatus(listener) {
    this.statusListeners.add(listener);
    return () => this.statusListeners.delete(listener);
  }
  close() {
    const socket = this.socket;
    this.socket = null;
    this.stopHeartbeat();
    this.heartbeatInFlight = null;
    this.token = null;
    this.connected = false;
    this.connectionState = "closed";
    this.setStatusPayload(null);
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

export function createBackend(initialFiles = SEED_FILES) { return new InMemoryBackend(initialFiles); }

export async function connectVtCode(url, code) {
  const backend = new VtCodeBackend(url);
  try {
    await backend.pair(code);
    await backend.status();
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
