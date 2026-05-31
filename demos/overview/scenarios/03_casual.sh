#!/usr/bin/env bash
# Scenario 3 — Casual turn: greeting, no skills injected
# Run from: demos/code_indexing/flask/
# Expected: no ↳ skills line — zap treats this as casual (~31 tokens)

printf '{"type":"user","text":"Nice, thanks!"}\n' \
  | zap --sdk --auto 2>/dev/null
