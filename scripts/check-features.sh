#!/bin/bash
# Claude Code PostToolUse hook.
# Reads tool call JSON from stdin; prints a reminder if a src/*.rs file was just edited.

input=$(cat)

# Extract file path from Edit or Write tool input.
file_path=$(echo "$input" | python3 -c "
import sys, json
try:
    d = json.load(sys.stdin)
    inp = d.get('tool_input', {})
    print(inp.get('file_path', '') or inp.get('path', ''))
except Exception:
    print('')
" 2>/dev/null)

# Only fire for src/*.rs files.
if echo "$file_path" | grep -qE 'src/.*\.rs$'; then
    # Check if FEATURES.md has been touched in the working tree this session.
    features_dirty=$(git diff --name-only 2>/dev/null | grep 'FEATURES\.md')
    features_staged=$(git diff --cached --name-only 2>/dev/null | grep 'FEATURES\.md')

    if [ -z "$features_dirty" ] && [ -z "$features_staged" ]; then
        echo "📋 FEATURES.md: $(basename "$file_path") was edited — update the Implemented table if a feature shipped."
    fi
fi
