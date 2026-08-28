import test from "node:test";
import assert from "node:assert/strict";
import { MAX_TOOL_OUTPUT_CHARS, createWebMcpTools, registerWebMcpTools, replaceExactText } from "../src/webmcp.js";

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

  async getTools() {
    return this.tools.map(({ tool }) => tool).sort((left, right) => left.name.localeCompare(right.name));
  }

  async executeTool(tool, serializedInput, options = {}) {
    return tool.execute(JSON.parse(serializedInput), options);
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
    stageTextEdit: async ({ path }) => ({ staged: true, path, requires_review: true }),
    reviewDraft: async () => ({ reviewed: true }),
    openPanel: (panel) => panel,
    ...overrides,
  });
}

test("registers spec-shaped WebMCP tools with titles, schemas, annotations, and abort signals", async () => {
  const context = new FakeModelContext();
  const registration = await registerWebMcpTools(context, toolsForTest());

  assert.equal(registration.names.length, 8);
  assert.deepEqual(registration.names, [
    "list_project_files",
    "read_file",
    "search_code",
    "get_editor_state",
    "open_file",
    "stage_text_edit",
    "review_draft",
    "open_panel",
  ]);
  assert.equal(context.tools[0].tool.title, "List project files");
  assert.equal(context.tools[0].tool.annotations.readOnlyHint, true);
  assert.equal(context.tools[0].tool.annotations.untrustedContentHint, true);
  assert.equal(context.tools[4].tool.annotations.readOnlyHint, false);
  assert.equal(context.tools[3].tool.annotations.untrustedContentHint, true);
  assert.equal(context.tools[6].tool.annotations.untrustedContentHint, true);
  assert.equal(context.tools[1].tool.inputSchema.required[0], "path");
  assert.equal(context.tools[7].tool.inputSchema.properties.panel.enum.length, 3);

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

test("tools are discoverable and executable through the browser ModelContext shape", async () => {
  const calls = [];
  const context = new FakeModelContext();
  const registration = await registerWebMcpTools(context, toolsForTest({
    openFile: async (path) => { calls.push(["open_file", path]); },
    stageTextEdit: async ({ path }) => { calls.push(["stage_text_edit", path]); return { staged: true, path, requires_review: true }; },
    reviewDraft: async () => { calls.push(["review_draft"]); return { reviewed: true }; },
    openPanel: (panel) => { calls.push(["open_panel", panel]); },
  }));
  const tools = await context.getTools();
  const byName = new Map(tools.map((tool) => [tool.name, tool]));

  assert.deepEqual(await context.executeTool(byName.get("list_project_files"), "{}"), { files: ["README.md"] });
  assert.deepEqual(
    await context.executeTool(byName.get("read_file"), JSON.stringify({ path: "README.md" })),
    { path: "README.md", content: "draft\n" },
  );
  assert.deepEqual(await context.executeTool(byName.get("open_file"), JSON.stringify({ path: "README.md" })), { opened: "README.md" });
  assert.deepEqual(
    await context.executeTool(byName.get("stage_text_edit"), JSON.stringify({
      path: "README.md",
      find: "draft",
      replace: "updated",
      expected_digest: "sha256:test",
    })),
    { staged: true, path: "README.md", requires_review: true },
  );
  assert.deepEqual(await context.executeTool(byName.get("review_draft"), "{}"), { reviewed: true });
  assert.deepEqual(await context.executeTool(byName.get("open_panel"), JSON.stringify({ panel: "changes" })), { opened: "changes" });
  assert.deepEqual(calls, [["open_file", "README.md"], ["stage_text_edit", "README.md"], ["review_draft"], ["open_panel", "changes"]]);

  registration.dispose();
});

test("tool inputs fail clearly before application callbacks run", async () => {
  let called = false;
  const context = new FakeModelContext();
  await registerWebMcpTools(context, toolsForTest({
    readFile: async () => { called = true; return {}; },
  }));

  await assert.rejects(context.tools[1].tool.execute({}, {}), /read_file requires path/);
  await assert.rejects(context.tools[2].tool.execute({ query: "" }, {}), /search_code.query must not be empty/);
  await assert.rejects(
    context.tools[7].tool.execute({ panel: "unknown" }, {}),
    /open_panel\.panel has an unsupported value; choose one of: activity, changes, turn/,
  );
  await assert.rejects(context.tools[1].tool.execute({ path: "README.md", extra: true }, {}), /read_file does not accept extra/);
  assert.equal(called, false);
});

test("browser draft edits require exactly one matching text span", () => {
  assert.equal(replaceExactText("Hello WebMCP", "Hello", "Hi"), "Hi WebMCP");
  assert.equal(replaceExactText("remove me", "remove me", ""), "");
  assert.throws(() => replaceExactText("Hello", "missing", "new"), {
    code: "text_not_found",
    message: /read the current file/,
  });
  assert.throws(() => replaceExactText("same same", "same", "new"), {
    code: "ambiguous_edit",
    message: /narrow find/,
  });
});

test("tool results stay within the WebMCP character budget and report truncation", async () => {
  const context = new FakeModelContext();
  await registerWebMcpTools(context, toolsForTest({
    listFiles: async () => Array.from({ length: 200 }, (_, index) => `src/${"x".repeat(24)}-${index}.js`),
    readFile: async (path) => ({ path, content: "content ".repeat(3000), digest: "sha256:test", size_bytes: 24000 }),
    searchCode: async () => Array.from({ length: 200 }, (_, index) => ({ path: `src/${index}.js`, line: index + 1, text: "match ".repeat(30) })),
    reviewDraft: async () => ({ reviewed: true, unified_diff: "+change\n".repeat(3000), diff_truncated: false }),
  }));
  const tool = (name) => context.tools.find(({ tool: candidate }) => candidate.name === name).tool;
  const outputs = await Promise.all([
    tool("list_project_files").execute({}, {}),
    tool("read_file").execute({ path: "README.md" }, {}),
    tool("search_code").execute({ query: "match" }, {}),
    tool("review_draft").execute({}, {}),
  ]);

  for (const output of outputs) assert.ok(JSON.stringify(output).length <= MAX_TOOL_OUTPUT_CHARS);
  assert.equal(outputs[0].truncated, true);
  assert.equal(outputs[1].content_truncated, true);
  assert.equal(outputs[2].truncated, true);
  assert.equal(outputs[3].diff_truncated, true);
  assert.ok(outputs[0].omitted_count > 0);
  assert.ok(outputs[2].omitted_count > 0);
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
