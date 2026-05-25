#!/bin/sh
# PostToolUse hook: warn when an edited .rs file grows past 500 lines.
# Receives Claude Code hook JSON on stdin; exits 0 always (warning only).

input=$(cat)
file=$(printf '%s' "$input" | python3 -c \
    "import sys,json; d=json.load(sys.stdin); print(d.get('tool_input',{}).get('file_path',''))" \
    2>/dev/null)

case "$file" in
  *.rs) ;;
  *) exit 0 ;;
esac

[ -f "$file" ] || exit 0

lines=$(wc -l < "$file")
if [ "$lines" -gt 500 ]; then
    printf "\n  ⚠  %s has %d lines — consider splitting into smaller modules.\n\n" "$file" "$lines" >&2
fi
exit 0
