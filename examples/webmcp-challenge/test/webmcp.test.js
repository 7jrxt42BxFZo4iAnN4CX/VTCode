import test from "node:test";
import assert from "node:assert/strict";
import { createWebMcpTools, registerWebMcpTools } from "../src/webmcp.js";

class FakeModelContext {
  tools = [];
  listeners = new Map();

  async registerTool(tool, options) {
    this.tools.push({ tool, options });
  }

  addEventListener(type, listener) {
    this.listeners.set(type, listener);
  }

  removeEventListener(type, listener) {
    if (this.listeners.get(type) === listener) this.listeners.delete(type);
  }

  dispatch(type) {
    this.listeners.get(type)?.();
  }
}

function toolsForTest(overrides = {}) {
  return createWebMcpTools({
    listFiles: async () => ["README.md"],
    readFile: async (path) => ({ path, content: "draft\n" }),
    searchCode: async (query) => [{ path: "README.md", line: 1, text: query }],
    getEditorState: () => ({ selected: "README.md" }),
    openFile: async (path) => path,
    reviewDraft: async () => ({ reviewed: true }),
    openPanel: (panel) => panel,
    ...overrides,
  });
}

test("registers spec-shaped WebMCP tools with titles, schemas, annotations, and abort signals", async () => {
  const context = new FakeModelContext();
  const registration = await registerWebMcpTools(context, toolsForTest());

  assert.equal(registration.names.length, 7);
  assert.deepEqual(registration.names, [
    "list_project_files",
    "read_file",
    "search_code",
    "get_editor_state",
    "open_file",
    "review_draft",
    "open_panel",
  ]);
  assert.equal(context.tools[0].tool.title, "List project files");
  assert.equal(context.tools[0].tool.annotations.readOnlyHint, true);
  assert.equal(context.tools[4].tool.annotations.readOnlyHint, false);
  assert.equal(context.tools[5].tool.annotations.untrustedContentHint, true);
  assert.equal(context.tools[1].tool.inputSchema.required[0], "path");
  assert.equal(context.tools[6].tool.inputSchema.properties.panel.enum.length, 3);

  const state = await context.tools[3].tool.execute({}, { signal: new AbortController().signal });
  assert.deepEqual(state, { selected: "README.md" });
  assert.equal(context.tools[0].options.signal.aborted, false);

  registration.dispose();
  assert.equal(context.tools[0].options.signal.aborted, true);
});

test("aborted WebMCP calls reject before application callbacks run", async () => {
  let called = false;
  const context = new FakeModelContext();
  await registerWebMcpTools(context, toolsForTest({
    listFiles: async () => {
      called = true;
      return [];
    },
  }));
  const controller = new AbortController();
  controller.abort();

  await assert.rejects(
    context.tools[0].tool.execute({}, { signal: controller.signal }),
    { name: "AbortError" },
  );
  assert.equal(called, false);
});

test("toolchange notifications reach the registration owner", async () => {
  const context = new FakeModelContext();
  let changed = null;
  const registration = await registerWebMcpTools(context, toolsForTest(), {
    onToolChange: (names) => { changed = names; },
  });

  context.dispatch("toolchange");
  assert.deepEqual(changed, registration.names);
  registration.dispose();
  changed = null;
  context.dispatch("toolchange");
  assert.equal(changed, null);
});
