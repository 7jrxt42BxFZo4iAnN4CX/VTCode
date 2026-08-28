const EMPTY_INPUT_SCHEMA = Object.freeze({
  type: "object",
  properties: {},
  additionalProperties: false,
});

function abortError(signal) {
  if (signal?.reason) return signal.reason;
  if (typeof DOMException === "function") return new DOMException("The WebMCP tool call was aborted", "AbortError");
  return new Error("The WebMCP tool call was aborted");
}

function throwIfAborted(signal) {
  if (signal?.aborted) throw abortError(signal);
}

function withAbortSignal(execute) {
  return async (input = {}, options = {}) => {
    const signal = options?.signal;
    throwIfAborted(signal);
    const result = await execute(input, { signal });
    throwIfAborted(signal);
    return result;
  };
}

/**
 * Build the browser tools exposed through the standard WebMCP ModelContext.
 * The callbacks are supplied by the application so this module contains no
 * editor or filesystem state of its own.
 */
export function createWebMcpTools({
  listFiles,
  readFile,
  searchCode,
  getEditorState,
  openFile,
  reviewDraft,
  openPanel,
}) {
  return [
    {
      name: "list_project_files",
      title: "List project files",
      description: "List files visible to the current VT Code workspace without changing it.",
      inputSchema: EMPTY_INPUT_SCHEMA,
      annotations: { readOnlyHint: true, untrustedContentHint: false },
      execute: async (_input, { signal } = {}) => ({ files: await listFiles({ signal }) }),
    },
    {
      name: "read_file",
      title: "Read a project file",
      description: "Read the current browser buffer for a workspace file, or its clean backend snapshot when no draft is open.",
      inputSchema: {
        type: "object",
        properties: {
          path: {
            type: "string",
            description: "Workspace-relative file path.",
            maxLength: 4096,
          },
        },
        required: ["path"],
        additionalProperties: false,
      },
      annotations: { readOnlyHint: true, untrustedContentHint: true },
      execute: async ({ path } = {}, { signal } = {}) => readFile(path, { signal }),
    },
    {
      name: "search_code",
      title: "Search project code",
      description: "Search bounded visible workspace buffers by text without changing the project.",
      inputSchema: {
        type: "object",
        properties: {
          query: {
            type: "string",
            description: "Case-insensitive text to find.",
            maxLength: 120,
          },
        },
        required: ["query"],
        additionalProperties: false,
      },
      annotations: { readOnlyHint: true, untrustedContentHint: true },
      execute: async ({ query } = {}, { signal } = {}) => searchCode(query, { signal }),
    },
    {
      name: "get_editor_state",
      title: "Inspect editor state",
      description: "Return selected file, open tabs, draft paths, and backend state without returning file contents.",
      inputSchema: EMPTY_INPUT_SCHEMA,
      annotations: { readOnlyHint: true, untrustedContentHint: false },
      execute: async (_input, { signal } = {}) => {
        throwIfAborted(signal);
        return getEditorState();
      },
    },
    {
      name: "open_file",
      title: "Open a file in the editor",
      description: "Open a visible workspace file in the browser editor; this changes only the page view and never writes a file.",
      inputSchema: {
        type: "object",
        properties: {
          path: {
            type: "string",
            description: "Workspace-relative file path.",
            maxLength: 4096,
          },
        },
        required: ["path"],
        additionalProperties: false,
      },
      annotations: { readOnlyHint: false, untrustedContentHint: false },
      execute: async ({ path } = {}, { signal } = {}) => {
        throwIfAborted(signal);
        await openFile(path);
        return { opened: path };
      },
    },
    {
      name: "review_draft",
      title: "Review the current draft",
      description: "Create the browser's unified diff for the current draft without approving or applying a filesystem change.",
      inputSchema: EMPTY_INPUT_SCHEMA,
      annotations: { readOnlyHint: false, untrustedContentHint: true },
      execute: async (_input, { signal } = {}) => {
        throwIfAborted(signal);
        return reviewDraft();
      },
    },
    {
      name: "open_panel",
      title: "Open an editor panel",
      description: "Show the activity, changes, or VT Code panel in the browser editor.",
      inputSchema: {
        type: "object",
        properties: {
          panel: {
            type: "string",
            enum: ["activity", "changes", "turn"],
            description: "Panel to show.",
          },
        },
        required: ["panel"],
        additionalProperties: false,
      },
      annotations: { readOnlyHint: false, untrustedContentHint: false },
      execute: async ({ panel } = {}, { signal } = {}) => {
        throwIfAborted(signal);
        openPanel(panel);
        return { opened: panel };
      },
    },
  ];
}

/**
 * Register application tools with the browser's WebMCP ModelContext.
 * Aborting the returned registration unregisters every tool, as required by
 * the imperative API's AbortSignal option.
 */
export async function registerWebMcpTools(modelContext, tools, { onToolChange = () => {} } = {}) {
  if (!modelContext?.registerTool) return null;
  const controller = new AbortController();
  const names = [];
  try {
    for (const tool of tools) {
      await modelContext.registerTool({ ...tool, execute: withAbortSignal(tool.execute) }, { signal: controller.signal });
      names.push(tool.name);
    }
  } catch (error) {
    controller.abort(error);
    throw error;
  }

  const handleToolChange = () => onToolChange(names);
  modelContext.addEventListener?.("toolchange", handleToolChange);
  return {
    names,
    dispose() {
      modelContext.removeEventListener?.("toolchange", handleToolChange);
      controller.abort();
    },
  };
}
