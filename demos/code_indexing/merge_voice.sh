#!/usr/bin/env bash
# Merge your recorded voiceover into demo_tui.mp4
#
# Usage:
#   ./merge_voice.sh <voiceover.mp3|.m4a|.wav|.aiff>
#
# Output:
#   demo_tui_voiced.mp4
#
# Tips:
#   - Record your voice in any tool (QuickTime, Voice Memos, Audacity, etc.)
#   - The voiceover can be shorter or longer than the video:
#       shorter → silence at the end
#       longer  → audio gets trimmed to video length
#   - To delay audio by N seconds: add -itsoffset N before -i <voiceover>

set -euo pipefail

VIDEO="demo_tui.mp4"
VOICE="${1:-}"
OUT="demo_tui_voiced.mp4"

if [[ -z "$VOICE" ]]; then
  echo "Usage: ./merge_voice.sh <voiceover.mp3|.m4a|.wav|.aiff>"
  exit 1
fi

if [[ ! -f "$VIDEO" ]]; then
  echo "Error: $VIDEO not found. Run: VHS_NO_SANDBOX=1 vhs demo_tui.tape first."
  exit 1
fi

if [[ ! -f "$VOICE" ]]; then
  echo "Error: $VOICE not found."
  exit 1
fi

VIDEO_DURATION=$(ffprobe -v quiet -show_entries format=duration -of csv="p=0" "$VIDEO")
echo "  Video: $VIDEO  (${VIDEO_DURATION}s)"
echo "  Voice: $VOICE"
echo "  Out:   $OUT"
echo ""

ffmpeg -y \
  -i "$VIDEO" \
  -i "$VOICE" \
  -map 0:v \
  -map 1:a \
  -c:v copy \
  -c:a aac -b:a 192k \
  -shortest \
  "$OUT"

echo ""
echo "  ✓ done → $OUT"
