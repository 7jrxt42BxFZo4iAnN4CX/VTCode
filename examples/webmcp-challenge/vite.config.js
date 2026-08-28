import { defineConfig } from "vite";

export function webmcpOriginTrialPlugin(token = process.env.VITE_WEBMCP_ORIGIN_TRIAL_TOKEN) {
  const normalizedToken = typeof token === "string" ? token.trim() : "";
  return {
    name: "webmcp-origin-trial",
    transformIndexHtml(html) {
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
  plugins: [webmcpOriginTrialPlugin()],
});
