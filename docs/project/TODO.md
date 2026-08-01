check and fix Preflight validation circuit breaker reached after 3 consecutive failures for tool 'code_search'.

on plan mode, after confirmation approval to build. the agent can't continue and build the plan.

/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/checkpoints/turn_874.json

====

improve loading state observation for bottom status view when the agent is planning (plan, build, execute). the status view should show the current state of the agent and any relevant information about the plan, build, or execution process. this will help users understand what the agent is doing and when it is ready for the next step.

also, add a config to toggle show/hide plan mode's bottom TODOs task tracking view. default to off. this will allow users to customize their experience and focus on the information that is most relevant to them.

---

and then the global loading state is not reflected. also refine the message and guide the user how to proceed with recomendation. also instruct the agent. the goal is to preserve long horizontal context and avoid hitting the tool call limit. the agent should be able to summarize the plan and provide a clear path forward then execute it.

logs: /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/checkpoints/turn_891.json /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/checkpoints/turn_890.json
