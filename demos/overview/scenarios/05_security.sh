#!/usr/bin/env bash
# Scenario 5 — Security: show permission model and features list
# Run from: demos/code_indexing/flask/

printf '{"type":"user","text":"Show me the zap permission modes and explain the security model."}\n' \
  | zap --sdk --auto 2>/dev/null
