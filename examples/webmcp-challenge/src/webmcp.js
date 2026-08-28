const EMPTY_INPUT_SCHEMA = Object.freeze({
  type: "object",
  properties: {},
  additionalProperties: false,
});
const MAX_TOOL_OUTPUT_CHARS = 1500;
const OUTPUT_TRUNCATION_MARKER = "\n[output truncated by WebMCP limit]";

function serializedLength(value) {
  const serialized = JSON.stringify(value);
  return typeof serialized === "string" ? serialized.length : Number.POSITIVE_INFINITY;
}

function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function toolInputError(message) {
  const error = new Error(message);
  error.code = "invalid_input";
  return error;
}

function truncateString(value, maxChars) {
  if (value.length <= maxChars) return value;
  const markerLength = OUTPUT_TRUNCATION_MARKER.length;
  return markerLength >= maxChars
    ? value.slice(0, maxChars)
    : `${value.slice(0, maxChars - markerLength)}${OUTPUT_TRUNCATION_MARKER}`;
}

function boundStringResult(value) {
  if (serializedLength(value) <= MAX_TOOL_OUTPUT_CHARS) return value;
  let low = 0;
  let high = value.length;
  let best = "";
  while (low <= high) {
    const middle = Math.floor((low + high) / 2);
    const candidate = truncateString(value, middle);
    if (serializedLength(candidate) <= MAX_TOOL_OUTPUT_CHARS) {
      best = candidate;
      low = middle + 1;
    } else {
      high = middle - 1;
    }
  }
  return best;
}

function boundTextField(result, field) {
  const truncatedField = field === "content"
    ? "content_truncated"
    : field === "unified_diff" ? "diff_truncated" : `${field}_truncated`;
  const base = { ...result, [field]: "", [truncatedField]: true };
  if (serializedLength(base) > MAX_TOOL_OUTPUT_CHARS) return { truncated: true };

  const value = result[field];
  let low = 0;
  let high = value.length;
  let best = base;
  while (low <= high) {
    const middle = Math.floor((low + high) / 2);
    const candidate = { ...base, [field]: truncateString(value, middle) };
    if (serializedLength(candidate) <= MAX_TOOL_OUTPUT_CHARS) {
      best = candidate;
      low = middle + 1;
    } else {
      high = middle - 1;
    }
  }
  return best;
}

function boundCollectionField(result, field) {
  const collection = result[field];
  const base = { ...result, [field]: [], truncated: true, omitted_count: collection.length };
  if (serializedLength(base) > MAX_TOOL_OUTPUT_CHARS) return { truncated: true };

  const included = [];
  for (const item of collection) {
    const candidate = {
      ...base,
      [field]: [...included, item],
      omitted_count: collection.length - included.length - 1,
    };
    if (serializedLength(candidate) > MAX_TOOL_OUTPUT_CHARS) break;
    included.push(item);
  }
  return {
    ...base,
    [field]: included,
    omitted_count: collection.length - included.length,
  };
}

function boundToolOutput(result) {
  if (serializedLength(result) <= MAX_TOOL_OUTPUT_CHARS) return result;
  if (typeof result === "string") return boundStringResult(result);
  if (Array.isArray(result)) return boundCollectionField({ items: result }, "items");
  if (isRecord(result)) {
    for (const field of ["content", "unified_diff", "stdout", "stderr", "text", "reason"]) {
      if (typeof result[field] === "string") return boundTextField(result, field);
    }
    for (const field of ["files", "matches", "paths", "open_tabs", "dirty_files", "items"]) {
      if (Array.isArray(result[field])) return boundCollectionField(result, field);
    }
  }
  return { truncated: true };
}

function validateToolInput(toolName, schema, input) {
  const value = input === undefined ? {} : input;
  if (!isRecord(value)) throw toolInputError(`${toolName} expects an object input matching its input schema`);
  for (const required of schema.required || []) {
    if (!(required in value)) throw toolInputError(`${toolName} requires ${required}; provide it before retrying`);
  }
  if (schema.additionalProperties === false) {
    const properties = new Set(Object.keys(schema.properties || {}));
    for (const key of Object.keys(value)) {
      if (!properties.has(key)) throw toolInputError(`${toolName} does not accept ${key}; use only fields listed in the schema`);
    }
  }
  for (const [name, property] of Object.entries(schema.properties || {})) {
    if (!(name in value)) continue;
    const current = value[name];
    if (property.type === "string" && typeof current !== "string") {
      throw toolInputError(`${toolName}.${name} must be a string; provide a string value`);
    }
    if (typeof current === "string" && property.minLength !== undefined && current.length < property.minLength) {
      throw toolInputError(`${toolName}.${name} must not be empty; provide a non-empty value`);
    }
    if (typeof current === "string" && property.maxLength !== undefined && current.length > property.maxLength) {
      throw toolInputError(`${toolName}.${name} exceeds the length limit; shorten the value and retry`);
    }
    if (property.enum && !property.enum.includes(current)) {
      throw toolInputError(`${toolName}.${name} has an unsupported value; choose one of: ${property.enum.join(", ")}`);
    }
  }
  return value;
}

function applyToolContract(tool) {
  const execute = tool.execute;
  return {
    ...tool,
    execute: async (input = {}, options = {}) => {
      const validatedInput = validateToolInput(tool.name, tool.inputSchema, input);
      return boundToolOutput(await execute(validatedInput, options));
    },
  };
}

export function replaceExactText(content, find, replace) {
  const first = content.indexOf(find);
  if (first < 0) {
    const error = new Error("Text was not found; read the current file and provide an exact non-empty match");
    error.code = "text_not_found";
    throw error;
  }
  if (content.indexOf(find, first + 1) >= 0) {
    const error = new Error("Text occurs more than once; narrow find to one exact match before retrying");
    error.code = "ambiguous_edit";
    throw error;
  }
  return `${content.slice(0, first)}${replace}${content.slice(first + find.length)}`;
}

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
  stageTextEdit,
  reviewDraft,
  openPanel,
}) {
  return [
    {
      name: "list_project_files",
      title: "List project files",
      description: "Start here to list bounded files visible to the current VT Code workspace without changing it.",
      inputSchema: EMPTY_INPUT_SCHEMA,
      annotations: { readOnlyHint: true, untrustedContentHint: true },
      execute: async (_input, { signal } = {}) => ({ files: await listFiles({ signal }) }),
    },
    {
      name: "read_file",
      title: "Read a project file",
      description: "Read a bounded file preview and fresh digest after a path is known; use it before staging an edit.",
      inputSchema: {
        type: "object",
        properties: {
          path: {
            type: "string",
            description: "Workspace-relative file path.",
            minLength: 1,
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
      description: "Search visible workspace text for a term and return bounded path, line, and text matches without changing it.",
      inputSchema: {
        type: "object",
        properties: {
          query: {
            type: "string",
            description: "Case-insensitive text to find.",
            minLength: 1,
            maxLength: 120,
          },
        },
        required: ["query"],
        additionalProperties: false,
      },
      annotations: { readOnlyHint: true, untrustedContentHint: true },
      execute: async ({ query } = {}, { signal } = {}) => {
        const result = await searchCode(query, { signal });
        return Array.isArray(result) ? { matches: result, truncated: false } : result;
      },
    },
    {
      name: "get_editor_state",
      title: "Inspect editor state",
      description: "Return selected file, open tabs, draft paths, and backend state without returning file contents.",
      inputSchema: EMPTY_INPUT_SCHEMA,
      annotations: { readOnlyHint: true, untrustedContentHint: true },
      execute: async (_input, { signal } = {}) => {
        throwIfAborted(signal);
        return getEditorState();
      },
    },
    {
      name: "open_file",
      title: "Open a file in the editor",
      description: "Open a path returned by search or file listing; this changes only the page view and never writes a file.",
      inputSchema: {
        type: "object",
        properties: {
          path: {
            type: "string",
            description: "Workspace-relative file path.",
            minLength: 1,
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
      name: "stage_text_edit",
      title: "Stage a text edit",
      description: "After read_file, stage one exact replacement in a clean browser draft using its fresh digest; review is required and disk is never written.",
      inputSchema: {
        type: "object",
        properties: {
          path: {
            type: "string",
            description: "Workspace-relative file path.",
            minLength: 1,
            maxLength: 4096,
          },
          find: {
            type: "string",
            description: "Exact non-empty text to replace once.",
            minLength: 1,
            maxLength: 65536,
          },
          replace: {
            type: "string",
            description: "Replacement text; empty text deletes the match.",
            maxLength: 65536,
          },
          expected_digest: {
            type: "string",
            description: "Fresh digest returned by read_file for this path.",
            minLength: 1,
            maxLength: 200,
          },
        },
        required: ["path", "find", "replace", "expected_digest"],
        additionalProperties: false,
      },
      annotations: { readOnlyHint: false, untrustedContentHint: true },
      execute: async (input = {}, { signal } = {}) => {
        throwIfAborted(signal);
        return stageTextEdit(input, { signal });
      },
    },
    {
      name: "review_draft",
      title: "Review the current draft",
      description: "After a draft edit, create its browser unified diff; this never approves or applies a filesystem change.",
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
  ].map(applyToolContract);
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

export { MAX_TOOL_OUTPUT_CHARS, boundToolOutput };
