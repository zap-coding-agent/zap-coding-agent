#!/usr/bin/env bash
# Scenario 2 — Code index: find_definition locates Flask class instantly
# Run from: demos/code_indexing/flask/
# Expected: INDEX hit → exact file:line for Flask class

printf '{"type":"user","text":"Where is the Flask class defined in this repo? Give me the file and line number."}\n' \
  | zap --sdk --auto 2>/dev/null
