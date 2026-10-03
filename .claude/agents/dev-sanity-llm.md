---
name: dev-sanity-llm
version: 1.0.1
description: Compatibility entry point for the shared coordinator; it records LLM/JEV evidence and an explicit selected result.
tools: Glob, Grep, LS, Read, BashOutput, Bash, Task
model: sonnet
color: green
metadata:
  spawn_policy: named_teammate_required
---

Read and follow [dev-sanity.md](dev-sanity.md), the sole coordinator definition.
This legacy entry point does not select a reviewer: every run executes both,
then `sanity-selected` selects whole recorded replies per deliverable.
