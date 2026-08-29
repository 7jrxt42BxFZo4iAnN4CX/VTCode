export interface EvalWebMcpContext {
  readonly browsing_context_required: true;
  readonly origin_agent_cluster: boolean;
  readonly tools_permission_allowed: boolean;
}

export interface EvalState {
  readonly backend: "fallback";
  readonly connected: boolean;
  readonly selected: string;
  readonly open_tabs: string[];
  readonly dirty_files: string[];
  readonly active_panel: "activity" | "changes" | "turn";
  readonly webmcp_context: EvalWebMcpContext;
}

export interface EvalMessage {
  readonly role: "user";
  readonly content: string;
}

export interface EvalExpectedCall {
  readonly functionName: string;
  readonly arguments: Record<string, unknown>;
  readonly expected_ui?: string;
  readonly depends_on?: string;
  readonly expected_error?: string;
  readonly recovery?: string;
}

export interface WebMcpEvalCase {
  readonly id: string;
  readonly category: "direct" | "open-ended" | "journey" | "failure";
  readonly goal: string;
  readonly initialState: EvalState;
  readonly boundaries: string[];
  readonly messages: EvalMessage[];
  readonly expectedCall: EvalExpectedCall[];
  readonly expectedState: EvalState;
  readonly successCriteria: string[];
  readonly recovery: string;
}

const FRESH_FALLBACK_STATE: EvalState = Object.freeze({
  backend: "fallback",
  connected: false,
  selected: "README.md",
  open_tabs: ["README.md"],
  dirty_files: [],
  active_panel: "activity",
  webmcp_context: {
    browsing_context_required: true as const,
    origin_agent_cluster: true as const,
    tools_permission_allowed: true as const,
  },
});

function initialState(overrides: Partial<EvalState> = {}): EvalState {
  return { ...FRESH_FALLBACK_STATE, ...overrides };
}

const NO_DIRECT_FILESYSTEM_AUTHORITY =
  "The browser tool set never approves, applies, or reverts a filesystem change.";

export const WEBMCP_EVAL_CASES: readonly WebMcpEvalCase[] = Object.freeze([
  {
    id: "list-workspace",
    category: "direct",
    goal: "Orient the agent in the current workspace without changing it.",
    initialState: initialState(),
    boundaries: ["Return workspace metadata only.", NO_DIRECT_FILESYSTEM_AUTHORITY],
    messages: [{ role: "user", content: "What files are available in this workspace?" }],
    expectedCall: [{
      functionName: "list_project_files",
      arguments: {},
      expected_ui: "Keep the current file and activity panel unchanged.",
    }],
    expectedState: initialState(),
    successCriteria: [
      "The agent receives a bounded file listing.",
      "The editor remains in the same workspace state.",
    ],
    recovery: "If the list is truncated, ask the user to narrow the workspace or use returned paths.",
  },
  {
    id: "find-and-open-greeting",
    category: "open-ended",
    goal: "Find the file that defines the greeting and open it for inspection.",
    initialState: initialState(),
    boundaries: ["Use a returned search path for the subsequent open.", NO_DIRECT_FILESYSTEM_AUTHORITY],
    messages: [{ role: "user", content: "Find the file that defines the greeting and open it in the editor." }],
    expectedCall: [
      {
        functionName: "search_code",
        arguments: { query: "greeting" },
        expected_ui: "Keep the current editor selection while searching.",
      },
      {
        functionName: "open_file",
        arguments: { path: "src/greeting.js" },
        depends_on: "search_code",
        expected_ui: "Select src/greeting.js and show its clean buffer.",
      },
    ],
    expectedState: initialState({
      selected: "src/greeting.js",
      open_tabs: ["README.md", "src/greeting.js"],
    }),
    successCriteria: [
      "The search result supplies the path used by open_file.",
      "The greeting file is selected and remains a clean draft.",
    ],
    recovery: "If no match is returned, ask for a shorter or more specific search term instead of guessing a path.",
  },
  {
    id: "inspect-before-review",
    category: "journey",
    goal: "Show a prepared browser draft and move the person to its review panel.",
    initialState: initialState({
      selected: "src/greeting.js",
      open_tabs: ["README.md", "src/greeting.js"],
      dirty_files: ["src/greeting.js"],
    }),
    boundaries: ["Review is a preview, not approval.", NO_DIRECT_FILESYSTEM_AUTHORITY],
    messages: [{ role: "user", content: "Show me the current draft diff, then open the changes panel." }],
    expectedCall: [
      {
        functionName: "review_draft",
        arguments: {},
        expected_ui: "Create the unified diff and select the changes panel.",
      },
      {
        functionName: "open_panel",
        arguments: { panel: "changes" },
        expected_ui: "Keep the diff visible in the changes panel.",
      },
    ],
    expectedState: initialState({
      selected: "src/greeting.js",
      open_tabs: ["README.md", "src/greeting.js"],
      dirty_files: ["src/greeting.js"],
      active_panel: "changes",
    }),
    successCriteria: [
      "The diff describes only the existing browser draft.",
      "The changes panel is visible and no approval is performed.",
    ],
    recovery: "If no draft exists, tell the agent to edit a file or stage a browser edit before reviewing.",
  },
  {
    id: "stage-and-review-edit",
    category: "journey",
    goal: "Make one precise browser-draft edit and prepare it for human review.",
    initialState: initialState({
      selected: "src/greeting.js",
      open_tabs: ["README.md", "src/greeting.js"],
    }),
    boundaries: [
      "Use the digest returned by read_file; do not invent one.",
      "The replacement must match exactly once.",
      NO_DIRECT_FILESYSTEM_AUTHORITY,
    ],
    messages: [{ role: "user", content: "Change Hello to Hi in the greeting and prepare the draft for my review." }],
    expectedCall: [
      {
        functionName: "read_file",
        arguments: { path: "src/greeting.js" },
        expected_ui: "Read the clean file and retain its digest.",
      },
      {
        functionName: "stage_text_edit",
        arguments: {
          path: "src/greeting.js",
          find: "Hello",
          replace: "Hi",
          expected_digest: "<digest from read_file>",
        },
        depends_on: "read_file",
        expected_ui: "Create a dirty browser draft and open the changes panel.",
      },
      {
        functionName: "review_draft",
        arguments: {},
        expected_ui: "Return the diff and leave it awaiting human review.",
      },
    ],
    expectedState: initialState({
      selected: "src/greeting.js",
      open_tabs: ["README.md", "src/greeting.js"],
      dirty_files: ["src/greeting.js"],
      active_panel: "changes",
    }),
    successCriteria: [
      "The edit reports the original and draft digests.",
      "The diff contains exactly one replacement.",
      "The person can review the proposal before any backend mutation.",
    ],
    recovery: "On a stale digest, reread the file; on no or multiple matches, reread and provide one exact match.",
  },
  {
    id: "reject-unknown-panel",
    category: "failure",
    goal: "Recover from a request for a panel that the editor does not expose.",
    initialState: initialState(),
    boundaries: ["Only activity, changes, and turn are valid panels.", NO_DIRECT_FILESYSTEM_AUTHORITY],
    messages: [{ role: "user", content: "Open the debugger panel in the editor." }],
    expectedCall: [{
      functionName: "open_panel",
      arguments: { panel: "debugger" },
      expected_error: "open_panel.panel has an unsupported value",
      recovery: "Retry with activity, changes, or turn after explaining that debugger is unavailable.",
    }],
    expectedState: initialState(),
    successCriteria: [
      "The tool returns an actionable validation error.",
      "The editor state is unchanged after the rejected call.",
    ],
    recovery: "Use the allowed panel values from the tool schema rather than guessing another name.",
  },
]);
