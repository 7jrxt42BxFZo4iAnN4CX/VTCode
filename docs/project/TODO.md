openai just published the five rules that keep their own agent harness cheap:

- deferred discovery: mcp tools, skills and plugins don't all load at the start. the model only sees them when they're needed
- tool output is capped at 10,000 tokens by default. the model has to ask for a different limit
- everything the model can see is 𝗮𝗽𝗽𝗲𝗻𝗱 𝗼𝗻𝗹𝘆. new messages, tool results and environment updates go on the end, never back into earlier context
- tools are presented in a fixed order
- runtime settings like approval policy get applied at execution time instead of being written into the tool definitions

the last three all keep the prompt prefix 𝗯𝘆𝘁𝗲 𝗳𝗼𝗿 𝗯𝘆𝘁𝗲 𝗶𝗱𝗲𝗻𝘁𝗶𝗰𝗮𝗹, which is what keeps the cache hitting. they credit codex and chatgpt work's high cache hit rate to that design.

if you want to check your own, 𝗽𝗮𝘀𝘁𝗲 𝘁𝗵𝗶𝘀 𝗶𝗻𝘁𝗼 𝘄𝗵𝗶𝗰𝗵𝗲𝘃𝗲𝗿 𝗮𝗴𝗲𝗻𝘁 𝘆𝗼𝘂 𝘂𝘀𝗲. it works on any of them:

read through this project's agent config. that includes mcp settings, skills, plugins, hooks and rules files. judge it against these five, one at a time. keep the results separate:

1. how many tools and skills load at the start, and which of those have never been called in this project
2. is there a cap on tool output. which tool has returned the biggest payload, and roughly how many tokens
3. does anything rewrite or inject into context that has already been sent, like stamping the current time every turn, re-reading a rules file mid session, or editing the system prompt on the fly
4. is the tool list in the same order across two starts
5. is any permission or approval policy written into a tool description

trace each one: pass or fail, the exact file or entry, and the one thing to change. if you can't find it, say you can't find it. don't make it up.

If you’re an API developer trying to maximize performance, we recommend using the same settings that we deploy in our own products:

- Use our Responses API, not our legacy Chat Completions API
- Retain reasoning
- Use compaction

===

check and fix Preflight validation circuit breaker reached after 3 consecutive failures for tool 'code_search'.

on plan mode, after confirmation approval to build. the agent can't continue and build the plan.

/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/checkpoints/turn_874.json

====

IMPORTANT: change keyboard short to switch mode/agent by single tab to shift+TAB. IMPORTANT

===

check CRITICAL: check Plan mode tool call output raw tool call xml.

/Users/vinhnguyenxuan/.vtcode/sessions/debug-session-vtcode-20260731t033432z_426387-26942.log /Users/vinhnguyenxuan/.vtcode/sessions/session-vtcode-20260731T033432Z_426387-26942.json /Users/vinhnguyenxuan/.vtcode/sessions/atif-trajectory-session-vtcode-20260731T033432Z_426387-26942-20260731T033433Z.json /Users/vinhnguyenxuan/.vtcode/sessions/harness-session-vtcode-20260731T033432Z_426387-26942-20260731T033433Z.jsonl

```
Diagnosis confirmed: raw <tool_call> XML leaked as plain text in Plan mode

Session: session-vtcode-20260731T033432Z_426387-26942 — model minimax-m3:cloud via ollama-cloud (custom OpenAI-compatible provider), planning workflow enabled.

What the session files show

Both assistant turns returned raw XML as text, and it propagated everywhere:

File
Evidence
session-*.json
Assistant messages (lines 62, 73) contain verbatim <tool_call><tools:call name="exec_command">…</tool_call>, marked "phase": "final_answer"
atif-trajectory-*.json
Steps 1 and 4 (source: "agent") carry the raw XML as the agent message
harness-*.jsonl
Lines 5, 11: item.completed agent_message text = raw XML
debug log
llm_response_parsed kind="text" tool_calls=0 interpreted_textual_call=false → text_response_decision should_continue=false reason="planning_active"

Root cause chain

1. Zero tool schemas on the wire. Turn metrics: on_wire_tools=0, tool_schema_tokens=0, client_local_deferral=true. The provider resolves to the ClientLocal deferral policy ([snapshot.rs:112](src/agent/runloop/unified/turn/turn_processing/llm_request/snapshot.rs#L112)), so [tool_shaping.rs:123 client_local_wire_tools](src/agent/runloop/unified/turn/turn_processing/llm_request/tool_shaping.rs#L123) strips all deferred definitions. The system prompt still names exec_command/code_search via the [Runtime Tool Catalog]/[Deferred Tools] sections, so the model knows the tools exist but has no native calling format.
2. The model improvised a dialect no parser recognizes. It emitted <tool_call><tools:call name="X"><parameter name="k">v</parameter></tools:call></tool_call> — the string tools:call appears nowhere in this repo; it's from MiniMax's training data. All 8 parsers in [detect.rs:25](src/agent/runloop/text_tools/detect.rs#L25) miss it:
- [parse_tagged.rs:19](src/agent/runloop/text_tools/parse_tagged.rs#L19) standard <tool_call> parser requires the tool name immediately after the tag — here it hits <tools:call → empty name → None
- [parse_tagged.rs:153](src/agent/runloop/text_tools/parse_tagged.rs#L153) MiniMax parser requires <invoke name=" — model used <tools:call name=" → None
- <function= parser → None; the rest (channel/DSML/structured/YAML/bracketed/prefix/alias) → no match
3. Plan mode treats any text as terminal. [continuation.rs:99](src/agent/runloop/unified/turn/context/continuation.rs#L99): if planning_active { return d(false, "planning_active") }. The turn ends with the XML as the "final answer" instead of recovering. The pseudo-marker cleanup guard ([result_handler.rs:195](src/agent/runloop/unified/turn/turn_processing/result_handler.rs#L195), which would catch <tool_call) only fires during a tool-free recovery pass — not active here.
4. Leak path: handle_text_response stores the text verbatim → session archive, ATIF export, harness event log, and the user-facing transcript.

Recommended fix

Primary (small, follows precedent): generalize parse_minimax_tool_call to also accept <tools:call name="…">…</tools:call> as an alias of <invoke name="…">…</invoke> — the <parameter name="…"> child format is already parsed there. This mirrors how the <function= dialect was added for checkpoint turn_856. Add a regression test in text_tools/tests.rs using the exact payload from this session.

Defense-in-depth (optional): in the plan-mode text path, when contains_pseudo_tool_call_markers(text) is true and no <proposed_plan> was extracted, strip the region or re-prompt instead of ending the turn with raw markup.

Want me to implement the parser fix with tests?
```
