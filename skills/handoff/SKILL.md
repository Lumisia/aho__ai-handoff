---
name: handoff
description: Use /handoff to fetch and consume the latest pending ai-handoff capsule for the current project from the other agent.
disable-model-invocation: true
---

# handoff

Fetch the pending handoff capsule for this project and continue from it.

## Usage

    ai-handoff handoff --agent <self>
    aho handoff --agent <self>

Always pass `--agent` set to the agent you are: `claude-code` if you are Claude
Code, `codex` if you are Codex. Run the command from the current project
directory so ai-handoff uses the right project fingerprint.

If stdout contains `hookSpecificOutput.additionalContext`, read that context,
continue from the capsule, and mention the consumed capsule context briefly. If
stdout is `{}`, report that there is no pending handoff capsule for this
project/agent.

This command consumes a pending capsule by marking it consumed in the local
store. Capsules are never consumed automatically at session start — a session
only receives a short notice that one is pending, and this command is the only
way to consume it. Do not run `checkpoint` from `/handoff`; checkpoint creates
a new handoff instead of receiving one.
