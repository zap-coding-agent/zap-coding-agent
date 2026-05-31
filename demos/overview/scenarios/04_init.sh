#!/usr/bin/env bash
# Scenario 4 — /init: bootstrap project knowledge
# Run from: demos/code_indexing/flask/
# This shows /init auto-generating ZAP.md and .zap/understanding.md

printf '{"type":"user","text":"/init"}\n' \
  | zap --sdk --auto 2>/dev/null
