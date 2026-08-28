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
  plugins: [webmcpOriginTrialPlugin() as Plugin],
});
