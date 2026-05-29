#!/usr/bin/env bash
# Scenario 2 — Two skills fire on the same turn: python + git
# Run from: demos/skill_injection/flask/
# Expected: ↳ skills: python, git  (both injected, ~950 tokens total)

printf '{"type":"user","text":"Write a git commit message for adding request-id tracing middleware to Flask"}\n' \
  | zap --sdk --auto 2>/dev/null
