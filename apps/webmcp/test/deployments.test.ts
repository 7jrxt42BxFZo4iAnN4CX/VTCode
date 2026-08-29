import assert from "node:assert/strict";
import test from "node:test";

import { WEBMCP_DEPLOYMENTS, browserOrigin, webmcpDeploymentForOrigin } from "../src/deployments.ts";

test("published WebMCP deployments keep exact URLs and origins", () => {
  assert.deepEqual(
    WEBMCP_DEPLOYMENTS.map(({ id, url, origin }) => ({ id, url, origin })),
    [
      {
        id: "chatgpt-site",
        url: "https://vtcode.vinhnx.chatgpt.site/",
        origin: "https://vtcode.vinhnx.chatgpt.site",
      },
      {
        id: "github-pages",
        url: "https://vinhnx.github.io/VTCode/",
        origin: "https://vinhnx.github.io",
      },
    ],
  );
  assert.equal(webmcpDeploymentForOrigin("https://vtcode.vinhnx.chatgpt.site")?.id, "chatgpt-site");
  assert.equal(webmcpDeploymentForOrigin("https://vinhnx.github.io")?.id, "github-pages");
  assert.equal(webmcpDeploymentForOrigin("https://example.test"), null);
});

test("browser origin preserves deployed hosts and falls back for non-browser execution", () => {
  assert.equal(browserOrigin({ origin: "https://vtcode.vinhnx.chatgpt.site" }), "https://vtcode.vinhnx.chatgpt.site");
  assert.equal(browserOrigin({ origin: "https://vinhnx.github.io" }), "https://vinhnx.github.io");
  assert.equal(browserOrigin({ origin: "" }), "http://localhost:5173");
  assert.equal(browserOrigin({ origin: "null" }), "http://localhost:5173");
  assert.equal(browserOrigin(null), "http://localhost:5173");
});
