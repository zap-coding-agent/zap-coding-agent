#!/usr/bin/env bash
# Scenario 3 — Casual message: no skills, minimal system prompt
# Run from: demos/skill_injection/flask/
# Expected: no ↳ skills line — zap detects this is conversational and skips injection entirely

printf '{"type":"user","text":"Nice, thanks!"}\n' \
  | zap --sdk --auto 2>/dev/null
