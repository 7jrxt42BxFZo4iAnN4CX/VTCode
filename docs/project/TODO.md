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
