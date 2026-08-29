import { defineConfig, type Plugin } from "vite";

interface OriginTrialResult {
  readonly html: string;
  readonly tags: readonly [{
    readonly tag: "meta";
    readonly attrs: { readonly "http-equiv": "origin-trial"; readonly content: string };
    readonly injectTo: "head-prepend";
  }];
}

interface OriginTrialPlugin {
  readonly name: string;
  readonly transformIndexHtml: (html: string) => string | OriginTrialResult;
}

export function webmcpOriginTrialPlugin(token = process.env.VITE_WEBMCP_ORIGIN_TRIAL_TOKEN): OriginTrialPlugin {
  const normalizedToken = typeof token === "string" ? token.trim() : "";
  return {
    name: "webmcp-origin-trial",
    transformIndexHtml(html: string) {
      if (!normalizedToken) return html;
      return {
        html,
        tags: [{
          tag: "meta",
          attrs: {
            "http-equiv": "origin-trial",
            content: normalizedToken,
          },
          injectTo: "head-prepend",
        }],
      };
    },
  };
}

export default defineConfig({
  // GitHub Pages serves this WebMCP app beneath the repository path
  // (/VTCode/), so generated assets must remain relative to the project page.
  base: "./",
  // Keep fallback state scoped to this deployed WebMCP app version. Without
  // an explicit instance, every origin falls back to the development key and a
  // stale/empty browser snapshot can hide the deterministic seed workspace.
  define: {
    __VTCODE_APP_INSTANCE__: JSON.stringify("vtcode-webmcp-app-v2"),
  },
  plugins: [webmcpOriginTrialPlugin() as Plugin],
});
