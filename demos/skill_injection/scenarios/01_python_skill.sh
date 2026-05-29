#!/usr/bin/env bash
# Scenario 1 — Python skill auto-injection
# Run from: demos/skill_injection/flask/
# Expected: ↳ skills: python  (amber line before the response)

printf '{"type":"user","text":"How should I handle database connection errors in a Flask route? Show me the idiomatic Python pattern."}\n' \
  | zap --sdk --auto 2>/dev/null
