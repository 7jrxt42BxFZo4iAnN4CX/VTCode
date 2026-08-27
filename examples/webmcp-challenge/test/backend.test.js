import test from "node:test";
import assert from "node:assert/strict";
import { InMemoryBackend, MAX_TURN_PROMPT_BYTES, buildTurnPrompt, createUnifiedDiff, digest } from "../src/backend.js";

test("fallback backend is deterministic and memory-only", async () => {
  const backend = new InMemoryBackend({ "src/main.js": "export const answer = 42;\n" });
  assert.deepEqual(await backend.listFiles(), ["src/main.js"]);
  const file = await backend.readFile("src/main.js");
  assert.equal(file.content, "export const answer = 42;\n");
  assert.equal(file.digest, await digest(file.content));
  await backend.writeFile({ path: file.path, content: "export const answer = 43;\n", baseDigest: file.digest });
  assert.equal((await backend.readFile(file.path)).content, "export const answer = 43;\n");
});

test("fallback backend rejects stale proposals", async () => {
  const backend = new InMemoryBackend({ "README.md": "base\n" });
  const base = await backend.readFile("README.md");
  await backend.writeFile({ path: "README.md", content: "new base\n", baseDigest: base.digest });
  await assert.rejects(
    backend.writeFile({ path: "README.md", content: "stale patch\n", baseDigest: base.digest }),
    /Stale patch/,
  );
});

test("fallback backend validates malformed proposal entries", async () => {
  const backend = new InMemoryBackend({ "README.md": "base\n" });
  await assert.rejects(backend.proposeChanges([null]), /Workspace path is invalid/);
});

test("fallback structured proposal supports diff, apply, checks, and guarded revert", async () => {
  const backend = new InMemoryBackend();
  const base = await backend.readFile("src/greeting.js");
  const changes = [{ path: base.path, base_digest: base.digest, content: base.content.replace("Hello", "Hi") }];
  const proposal = await backend.proposeChanges(changes);
  assert.match(proposal.unified_diff, /--- a\/src\/greeting\.js/);
  assert.match(createUnifiedDiff(changes, { "src/greeting.js": base.content }), /\+.*Hi/);
  const applied = await backend.applyProposal(proposal.proposal_id);
  assert.match((await backend.readFile(base.path)).content, /Hi/);
  assert.equal((await backend.runChecks()).exit_code, 1);
  await backend.revertLastChange(applied.change_id);
  assert.equal((await backend.readFile(base.path)).content, base.content);
});

test("fallback multi-file apply validates every file before mutating any file", async () => {
  const backend = new InMemoryBackend({ "a.txt": "a\n", "b.txt": "b\n" });
  const first = await backend.readFile("a.txt");
  const second = await backend.readFile("b.txt");
  const proposal = await backend.proposeChanges([
    { path: first.path, base_digest: first.digest, content: "new a\n" },
    { path: second.path, base_digest: second.digest, content: "new b\n" },
  ]);
  await backend.writeFile({ path: second.path, content: "external\n", baseDigest: second.digest });
  await assert.rejects(backend.applyProposal(proposal.proposal_id), /Stale patch/);
  assert.equal((await backend.readFile(first.path)).content, first.content);
  assert.equal((await backend.readFile(second.path)).content, "external\n");
});

test("fallback turn explains that no VT Code runtime is connected", async () => {
  const result = await new InMemoryBackend().requestTurn("review this draft");
  assert.equal(result.accepted, false);
  assert.match(result.reason, /agent turns require an active VT Code runtime/i);
});

test("turn prompt includes a bounded draft diff", () => {
  const prompt = buildTurnPrompt("Review the change", "--- a/file.js\n+++ b/file.js\n+updated\n".repeat(2000));
  assert.match(prompt, /Review the change/);
  assert.match(prompt, /browser draft unified diff/);
  assert.match(prompt, /diff truncated/);
  assert.ok(new TextEncoder().encode(prompt).length <= MAX_TURN_PROMPT_BYTES);
});

test("turn prompt stays bounded for multi-byte prompt text", () => {
  const prompt = buildTurnPrompt("🙂".repeat(MAX_TURN_PROMPT_BYTES), "diff");
  assert.ok(new TextEncoder().encode(prompt).length <= MAX_TURN_PROMPT_BYTES);
});
