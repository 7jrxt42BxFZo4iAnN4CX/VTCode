export const WEBMCP_EVAL_CASES = Object.freeze([
  {
    id: "list-workspace",
    category: "direct",
    messages: [{ role: "user", content: "What files are available in this workspace?" }],
    expectedCall: [{ functionName: "list_project_files", arguments: {} }],
  },
  {
    id: "find-and-open-greeting",
    category: "open-ended",
    messages: [{ role: "user", content: "Find the file that defines the greeting and open it in the editor." }],
    expectedCall: [
      { functionName: "search_code", arguments: { query: "greeting" } },
      { functionName: "open_file", arguments: { path: "src/greeting.js" }, depends_on: "search_code" },
    ],
  },
  {
    id: "inspect-before-review",
    category: "journey",
    messages: [{ role: "user", content: "Show me the current draft diff, then open the changes panel." }],
    expectedCall: [
      { functionName: "review_draft", arguments: {} },
      { functionName: "open_panel", arguments: { panel: "changes" } },
    ],
  },
  {
    id: "stage-and-review-edit",
    category: "journey",
    messages: [{ role: "user", content: "Change Hello to Hi in the greeting and prepare the draft for my review." }],
    expectedCall: [
      { functionName: "read_file", arguments: { path: "src/greeting.js" } },
      {
        functionName: "stage_text_edit",
        arguments: {
          path: "src/greeting.js",
          find: "Hello",
          replace: "Hi",
          expected_digest: "<digest from read_file>",
        },
        depends_on: "read_file",
      },
      { functionName: "review_draft", arguments: {} },
    ],
  },
  {
    id: "reject-unknown-panel",
    category: "failure",
    messages: [{ role: "user", content: "Open the debugger panel in the editor." }],
    expectedCall: [{
      functionName: "open_panel",
      arguments: { panel: "debugger" },
      expected_error: "open_panel.panel has an unsupported value",
    }],
  },
]);
