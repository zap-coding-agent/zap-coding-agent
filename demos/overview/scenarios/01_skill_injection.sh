#!/usr/bin/env bash
# Scenario 1 — Skill injection: Python question triggers python skill
# Run from: demos/code_indexing/flask/
# Expected: ↳ skills: python  (amber line before the response)

printf '{"type":"user","text":"What is the idiomatic Python pattern for handling request lifecycle in Flask? Show me the key classes."}\n' \
  | zap --sdk --auto 2>/dev/null
