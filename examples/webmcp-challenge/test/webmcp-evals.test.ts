import test from "node:test";
import assert from "node:assert/strict";
import { WEBMCP_EVAL_CASES } from "../evals/webmcp-evals.ts";
import { createWebMcpTools, type WebMcpTool } from "../src/webmcp.ts";

function toolsForEval(): WebMcpTool[] {
  return createWebMcpTools({
    listFiles: async () => [],
    readFile: async (path) => ({ path, content: "draft\n" }),
    searchCode: async () => ({ matches: [], truncated: false }),
    getEditorState: () => ({}),
    openFile: async () => {},
    stageTextEdit: async () => ({ staged: true, requires_review: true }),
    reviewDraft: async () => ({ reviewed: true }),
    openPanel: () => {},
  });
}

test("WebMCP eval corpus covers direct, open-ended, journey, and failure cases", () => {
  const toolNames = new Set(toolsForEval().map((tool) => tool.name));
  const categories = new Set(WEBMCP_EVAL_CASES.map((testCase) => testCase.category));
  assert.deepEqual(categories, new Set(["direct", "open-ended", "journey", "failure"]));

  for (const testCase of WEBMCP_EVAL_CASES) {
    assert.match(testCase.id, /^[a-z0-9-]+$/);
    assert.ok(testCase.goal.length > 0);
    assert.deepEqual(Object.keys(testCase.initialState).sort(), [
      "active_panel",
      "backend",
      "connected",
      "dirty_files",
      "open_tabs",
      "selected",
      "webmcp_context",
    ]);
    assert.deepEqual(testCase.initialState.webmcp_context, {
      browsing_context_required: true,
      origin_agent_cluster: true,
      tools_permission_allowed: true,
    });
    assert.ok(testCase.boundaries.length > 0);
    assert.ok(testCase.messages.length > 0);
    const message = testCase.messages[0];
    assert.ok(message);
    assert.equal(message.role, "user");
    assert.ok(message.content.length > 0);
    assert.ok(testCase.successCriteria.length > 0);
    assert.ok(testCase.recovery.length > 0);
    for (const expectedCall of testCase.expectedCall) {
      assert.ok(toolNames.has(expectedCall.functionName), `${testCase.id}: unknown tool ${expectedCall.functionName}`);
      assert.equal(typeof expectedCall.arguments, "object");
      assert.ok(expectedCall.expected_ui || expectedCall.recovery);
      if (expectedCall.expected_error) assert.equal(typeof expectedCall.expected_error, "string");
    }
  }
});

test("WebMCP eval corpus keeps write authority outside the browser tool set", () => {
  const names = toolsForEval().map((tool) => tool.name);
  assert.equal(names.some((name) => /apply|write|revert/i.test(name)), false);
  const readFileTool = toolsForEval().find((tool) => tool.name === "read_file");
  assert.ok(readFileTool);
  assert.equal(readFileTool.annotations.untrustedContentHint, true);
});

test("WebMCP metadata stays within the recommended discoverability budgets", () => {
  for (const tool of toolsForEval()) {
    assert.ok(tool.name.length <= 30, `${tool.name} name is too long`);
    assert.ok(tool.description.length <= 500, `${tool.name} description is too long`);
    for (const property of Object.values(tool.inputSchema.properties)) {
      assert.ok((property.description ?? "").length <= 150, `${tool.name} parameter description is too long`);
    }
  }
});

test("failure evals exercise clear runtime errors", async () => {
  const tool = toolsForEval().find((candidate) => candidate.name === "open_panel");
  assert.ok(tool);
  await assert.rejects(tool.execute({ panel: "debugger" }, {}), /open_panel\.panel has an unsupported value/);
});
