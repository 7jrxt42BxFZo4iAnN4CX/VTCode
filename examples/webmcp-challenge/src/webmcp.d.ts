export {};

declare global {
  interface WebMcpToolAnnotations {
    readonly readOnlyHint?: boolean;
    readonly destructiveHint?: boolean;
    readonly idempotentHint?: boolean;
    readonly openWorldHint?: boolean;
    readonly untrustedContentHint?: boolean;
  }

  interface WebMcpToolRegistrationOptions {
    readonly signal?: AbortSignal;
  }

  interface WebMcpToolExecutionOptions {
    readonly signal?: AbortSignal;
  }

  interface WebMcpTool {
    readonly name: string;
    readonly title: string;
    readonly description: string;
    readonly inputSchema: Record<string, unknown>;
    readonly annotations?: WebMcpToolAnnotations;
    readonly execute: (input?: unknown, options?: WebMcpToolExecutionOptions) => Promise<unknown> | unknown;
  }

  interface ModelContext extends EventTarget {
    readonly registerTool?: (
      tool: WebMcpTool,
      options?: WebMcpToolRegistrationOptions,
    ) => Promise<void> | void;
  }

  interface Document {
    readonly modelContext?: ModelContext;
  }
}
