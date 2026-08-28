import test from "node:test";
import assert from "node:assert/strict";
import { InMemoryBackend, MAX_TURN_PROMPT_BYTES, VtCodeBackend, buildTurnPrompt, createUnifiedDiff, digest } from "../src/backend.js";
import {
  BROWSER_SETTINGS_STORAGE_KEY,
  BROWSER_WORKSPACE_STORAGE_KEY,
  loadBrowserSettings,
  loadBrowserState,
  saveBrowserSettings,
  saveBrowserState,
} from "../src/persistence.js";

class MemoryStorage {
  #values = new Map();

  getItem(key) { return this.#values.get(key) ?? null; }
  setItem(key, value) { this.#values.set(key, value); }
  removeItem(key) { this.#values.delete(key); }
}

class LoopbackWebSocket {
  static OPEN = 1;
  static CLOSED = 3;
  static instances = [];
  static requests = [];

  constructor() {
    this.readyState = 0;
    LoopbackWebSocket.instances.push(this);
    queueMicrotask(() => {
      this.readyState = LoopbackWebSocket.OPEN;
      this.onopen?.();
    });
  }

  send(serialized) {
    const request = JSON.parse(serialized);
    LoopbackWebSocket.requests.push(request);
    const payload = request.type === "pair"
      ? { token: "session-token", protocol_version: "1", expires_in_secs: 300 }
      : {
        protocol_version: "1",
        connected: true,
        authenticated_origin: "http://localhost:5173",
        runtime: { turns_available: true },
        settings: {
          host: "127.0.0.1",
          port: 4321,
          pairing_ttl_secs: 300,
          max_frame_bytes: 1048576,
          max_in_flight_requests: 8,
          remote_enabled: false,
        },
        latest_sequence: 0,
      };
    queueMicrotask(() => {
      if (this.readyState === LoopbackWebSocket.OPEN) {
        this.onmessage?.({ data: JSON.stringify({ type: "response", request_id: request.request_id, ok: true, payload }) });
      }
    });
  }

  close() {
    if (this.readyState === LoopbackWebSocket.CLOSED) return;
    this.readyState = LoopbackWebSocket.CLOSED;
    this.onclose?.();
  }
}

class MalformedResponseWebSocket extends LoopbackWebSocket {
  send(serialized) {
    const request = JSON.parse(serialized);
    if (request.type === "pair") {
      super.send(serialized);
      return;
    }
    queueMicrotask(() => {
      if (this.readyState === LoopbackWebSocket.OPEN) this.onmessage?.({ data: "{" });
    });
  }
}

test("fallback backend is deterministic and memory-only", async () => {
  const backend = new InMemoryBackend({ "src/main.js": "export const answer = 42;\n" });
  assert.deepEqual(await backend.listFiles(), ["src/main.js"]);
  const file = await backend.readFile("src/main.js");
  assert.equal(file.content, "export const answer = 42;\n");
  assert.equal(file.digest, await digest(file.content));
  await backend.writeFile({ path: file.path, content: "export const answer = 43;\n", baseDigest: file.digest });
  assert.equal((await backend.readFile(file.path)).content, "export const answer = 43;\n");
});

test("fallback backend rejects stale proposals", async () => {
  const backend = new InMemoryBackend({ "README.md": "base\n" });
  const base = await backend.readFile("README.md");
  await backend.writeFile({ path: "README.md", content: "new base\n", baseDigest: base.digest });
  await assert.rejects(
    backend.writeFile({ path: "README.md", content: "stale patch\n", baseDigest: base.digest }),
    /Stale patch/,
  );
});

test("fallback backend validates malformed proposal entries", async () => {
  const backend = new InMemoryBackend({ "README.md": "base\n" });
  await assert.rejects(backend.proposeChanges([null]), /Workspace path is invalid/);
});

test("fallback structured proposal supports diff, apply, checks, and guarded revert", async () => {
  const backend = new InMemoryBackend();
  const base = await backend.readFile("src/greeting.js");
  const changes = [{ path: base.path, base_digest: base.digest, content: base.content.replace("Hello", "Hi") }];
  const proposal = await backend.proposeChanges(changes);
  assert.match(proposal.unified_diff, /--- a\/src\/greeting\.js/);
  assert.match(createUnifiedDiff(changes, { "src/greeting.js": base.content }), /\+.*Hi/);
  const applied = await backend.applyProposal(proposal.proposal_id);
  assert.match((await backend.readFile(base.path)).content, /Hi/);
  assert.equal((await backend.runChecks()).exit_code, 1);
  await backend.revertLastChange(applied.change_id);
  assert.equal((await backend.readFile(base.path)).content, base.content);
});

test("unified diff preserves context and separates distant edits", () => {
  const before = Array.from({ length: 12 }, (_, index) => `line-${index + 1}`).join("\n") + "\n";
  const after = before.replace("line-2", "changed-2").replace("line-10", "changed-10");
  const diff = createUnifiedDiff([{ path: "file.txt", content: after }], { "file.txt": before });
  assert.equal(diff, [
    "--- a/file.txt",
    "+++ b/file.txt",
    "@@ -1,5 +1,5 @@",
    " line-1",
    "-line-2",
    "+changed-2",
    " line-3",
    " line-4",
    " line-5",
    "@@ -7,6 +7,6 @@",
    " line-7",
    " line-8",
    " line-9",
    "-line-10",
    "+changed-10",
    " line-11",
    " line-12",
  ].join("\n"));
});

test("unified diff reports additions, deletions, and missing final newlines", () => {
  const before = "one\ntwo\nthree\n";
  assert.equal(
    createUnifiedDiff([{ path: "middle.txt", content: "one\ninserted\ntwo\nthree\n" }], { "middle.txt": before }),
    [
      "--- a/middle.txt",
      "+++ b/middle.txt",
      "@@ -1,3 +1,4 @@",
      " one",
      "+inserted",
      " two",
      " three",
    ].join("\n"),
  );
  assert.equal(
    createUnifiedDiff([{ path: "middle.txt", content: "one\nthree\n" }], { "middle.txt": before }),
    [
      "--- a/middle.txt",
      "+++ b/middle.txt",
      "@@ -1,3 +1,2 @@",
      " one",
      "-two",
      " three",
    ].join("\n"),
  );
  assert.equal(
    createUnifiedDiff([{ path: "new.txt", content: "first\nsecond\n" }], { "new.txt": "" }),
    ["--- a/new.txt", "+++ b/new.txt", "@@ -0,0 +1,2 @@", "+first", "+second"].join("\n"),
  );
  assert.equal(
    createUnifiedDiff([{ path: "old.txt", content: "" }], { "old.txt": "first\nsecond\n" }),
    ["--- a/old.txt", "+++ b/old.txt", "@@ -1,2 +0,0 @@", "-first", "-second"].join("\n"),
  );
  assert.equal(
    createUnifiedDiff([{ path: "line.txt", content: "new" }], { "line.txt": "old" }),
    [
      "--- a/line.txt",
      "+++ b/line.txt",
      "@@ -1 +1 @@",
      "-old",
      "\\ No newline at end of file",
      "+new",
      "\\ No newline at end of file",
    ].join("\n"),
  );
});

test("fallback multi-file apply validates every file before mutating any file", async () => {
  const backend = new InMemoryBackend({ "a.txt": "a\n", "b.txt": "b\n" });
  const first = await backend.readFile("a.txt");
  const second = await backend.readFile("b.txt");
  const proposal = await backend.proposeChanges([
    { path: first.path, base_digest: first.digest, content: "new a\n" },
    { path: second.path, base_digest: second.digest, content: "new b\n" },
  ]);
  await backend.writeFile({ path: second.path, content: "external\n", baseDigest: second.digest });
  await assert.rejects(backend.applyProposal(proposal.proposal_id), /Stale patch/);
  assert.equal((await backend.readFile(first.path)).content, first.content);
  assert.equal((await backend.readFile(second.path)).content, "external\n");
});

test("fallback turn explains that no VT Code runtime is connected", async () => {
  const result = await new InMemoryBackend().requestTurn("review this draft");
  assert.equal(result.accepted, false);
  assert.match(result.reason, /agent turns require an active VT Code runtime/i);
});

test("turn prompt includes a bounded draft diff", () => {
  const prompt = buildTurnPrompt("Review the change", "--- a/file.js\n+++ b/file.js\n+updated\n".repeat(2000));
  assert.match(prompt, /Review the change/);
  assert.match(prompt, /browser draft unified diff/);
  assert.match(prompt, /diff truncated/);
  assert.ok(new TextEncoder().encode(prompt).length <= MAX_TURN_PROMPT_BYTES);
});

test("turn prompt stays bounded for multi-byte prompt text", () => {
  const prompt = buildTurnPrompt("🙂".repeat(MAX_TURN_PROMPT_BYTES), "diff");
  assert.ok(new TextEncoder().encode(prompt).length <= MAX_TURN_PROMPT_BYTES);
});

test("browser workspace state survives reloads within one Vite app instance", () => {
  const storage = new MemoryStorage();
  const state = {
    fallback_files: { "src/app.js": "export const ready = true;\n" },
    drafts: { "src/app.js": "export const ready = false;\n" },
    open_tabs: ["src/app.js"],
    selected: "src/app.js",
    expanded_dirs: ["src"],
    filter: "app",
    workspace_path: "/tmp/demo",
  };
  assert.equal(saveBrowserState(storage, "vite-1", state), true);
  assert.deepEqual(loadBrowserState(storage, "vite-1"), {
    version: 1,
    app_instance: "vite-1",
    ...state,
  });
  assert.equal(loadBrowserState(storage, "vite-2"), null);
  assert.equal(storage.getItem(BROWSER_WORKSPACE_STORAGE_KEY), null);
});

test("browser settings persist setup values but never pairing credentials", () => {
  const storage = new MemoryStorage();
  assert.equal(saveBrowserSettings(storage, "vite-1", {
    workspace_path: "/tmp/demo",
    bridge_url: "ws://127.0.0.1:4321/webmcp",
    pairing_code: "SECRET-CODE",
  }), true);
  assert.deepEqual(loadBrowserSettings(storage, "vite-1"), {
    version: 1,
    app_instance: "vite-1",
    workspace_path: "/tmp/demo",
    bridge_url: "ws://127.0.0.1:4321/webmcp",
  });
  assert.equal(storage.getItem(BROWSER_SETTINGS_STORAGE_KEY).includes("SECRET-CODE"), false);
  assert.equal(loadBrowserSettings(storage, "vite-2"), null);
  assert.equal(storage.getItem(BROWSER_SETTINGS_STORAGE_KEY), null);
});

test("websocket backend resumes the in-memory session after a dropped socket", async () => {
  const previousWebSocket = globalThis.WebSocket;
  globalThis.WebSocket = LoopbackWebSocket;
  LoopbackWebSocket.instances = [];
  LoopbackWebSocket.requests = [];
  const backend = new VtCodeBackend("ws://127.0.0.1:4321/webmcp");
  try {
    await backend.pair("ABCD");
    assert.equal(backend.connected, true);
    assert.equal(backend.heartbeatIntervalMs(), 30_000);
    const statusUpdates = [];
    const stopStatus = backend.subscribeToStatus((payload) => statusUpdates.push(payload));
    const status = await backend.status();
    assert.equal(status.settings.port, 4321);
    assert.equal(statusUpdates.at(-1).authenticated_origin, "http://localhost:5173");
    stopStatus();

    LoopbackWebSocket.instances[0].close();
    assert.equal(backend.connected, false);

    const resumedStatus = await backend.status();
    assert.equal(resumedStatus.connected, true);
    assert.equal(backend.connected, true);
    assert.equal(LoopbackWebSocket.instances.length, 2);

    await backend.requestTurn("Implement the staged change", "proposal-1");
    const request = LoopbackWebSocket.requests.at(-1);
    assert.equal(request.type, "turn.request");
    assert.equal(request.token, "session-token");
    assert.equal(request.prompt, "Implement the staged change");
    assert.equal(request.proposal_id, "proposal-1");
  } finally {
    backend.close();
    if (previousWebSocket === undefined) delete globalThis.WebSocket;
    else globalThis.WebSocket = previousWebSocket;
  }
});

test("protocol errors invalidate the session and notify connection listeners", async () => {
  const previousWebSocket = globalThis.WebSocket;
  globalThis.WebSocket = MalformedResponseWebSocket;
  const backend = new VtCodeBackend("ws://127.0.0.1:4321/webmcp");
  const connectionStates = [];
  const stopConnection = backend.subscribeToConnection((event) => connectionStates.push(event));
  try {
    await backend.pair("ABCD");
    await assert.rejects(backend.status(), { code: "protocol_error" });
    assert.equal(backend.connected, false);
    assert.equal(backend.token, null);
    assert.equal(backend.connectionState, "reauthorize");
    assert.equal(connectionStates.at(-1).state, "reauthorize");
  } finally {
    stopConnection();
    backend.close();
    if (previousWebSocket === undefined) delete globalThis.WebSocket;
    else globalThis.WebSocket = previousWebSocket;
  }
});
