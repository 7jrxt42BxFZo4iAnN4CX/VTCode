import test from "node:test";
import assert from "node:assert/strict";
import { webmcpOriginTrialPlugin } from "../vite.config.js";

test("origin-trial injection is disabled without an explicit token", () => {
  const html = "<head></head>";
  assert.equal(webmcpOriginTrialPlugin("").transformIndexHtml(html), html);
  assert.equal(webmcpOriginTrialPlugin(undefined).transformIndexHtml(html), html);
});

test("origin-trial injection prepends a token to the document head", () => {
  const result = webmcpOriginTrialPlugin("trial-token").transformIndexHtml("<head></head>");
  assert.deepEqual(result, {
    html: "<head></head>",
    tags: [{
      tag: "meta",
      attrs: {
        "http-equiv": "origin-trial",
        content: "trial-token",
      },
      injectTo: "head-prepend",
    }],
  });
});

test("origin-trial tokens are trimmed before injection", () => {
  const result = webmcpOriginTrialPlugin("  trial-token  ").transformIndexHtml("<head></head>");
  assert.equal(result.tags[0].attrs.content, "trial-token");
});
