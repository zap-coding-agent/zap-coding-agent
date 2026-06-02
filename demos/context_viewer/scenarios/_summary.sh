#!/usr/bin/env bash
sleep 1

Y='\033[38;5;220m'
G='\033[38;5;114m'
C='\033[38;5;81m'
D='\033[38;5;240m'
R='\033[38;5;203m'
B='\033[1m'
N='\033[0m'
S='\033[38;5;246m'
M='\033[38;5;183m'

printf '\n'
printf "  ${Y}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${N}\n"
printf '\n'
printf "  ${B}${Y}⚡ zap /context  —  what happened in this video${N}\n"
printf '\n'

# ── The 4 turns ──────────────────────────────────────────────────────────────

printf "  ${D}Step 1  Build context${N}\n"
printf '\n'
printf "  ${C}Turn 1${N}  ${S}\"Give me a quick overview of this Flask project\"${N}\n"
printf "  ${S}        → LLM read the file tree + source files${N}\n"
printf "  ${S}        → total context: ${N}${G}~5%%  of model window${N}\n"
printf '\n'
printf "  ${C}Turn 2${N}  ${B}\"Show me all the route handlers\"${N}  ${Y}← heaviest${N}\n"
printf "  ${S}        → LLM opened 6+ files with multiple tool calls${N}\n"
printf "  ${S}        → total context: ${N}${Y}~14%%  of model window${N}\n"
printf "  ${S}        → Turn 2 alone = ${N}${Y}52%% of the context used so far${N}\n"
printf '\n'
printf "  ${C}Turn 3${N}  ${S}\"What does the User model look like?\"${N}\n"
printf "  ${S}        → LLM read the models file${N}\n"
printf "  ${S}        → total context: ${N}${M}~19%%  of model window${N}\n"
printf '\n'
printf "  ${C}Turn 4${N}  ${S}\"Got it, thanks.\"${N}  ${D}← the contrast${N}\n"
printf "  ${S}        → simple reply, zero tool calls, nothing read${N}\n"
printf "  ${S}        → total context: ${N}${D}~22%%  — barely moved${N}\n"
printf '\n'
printf "  ${D}────────────────────────────────────────────────────────────────────${N}\n"
printf '\n'

# ── What /context revealed ───────────────────────────────────────────────────

printf "  ${D}Step 2  Open /context${N}\n"
printf '\n'
printf "  ${Y}Overlay opened${N}  ${S}→ all 4 turns listed with exact token cost${N}\n"
printf "  ${S}                   and each turn's %% share of the 22%% used${N}\n"
printf '\n'
printf "  ${Y}Detail panel${N}   ${S}→ Turn 2 opened — real user message,${N}\n"
printf "  ${S}                   tool call names, full JSON input,${N}\n"
printf "  ${S}                   complete file contents, assistant response${N}\n"
printf '\n'
printf "  ${D}────────────────────────────────────────────────────────────────────${N}\n"
printf '\n'

# ── The drop ─────────────────────────────────────────────────────────────────

printf "  ${D}Step 3  Drop Turn 2${N}\n"
printf '\n'
printf "  ${R}Pressed d${N}  ${S}→ red confirmation: \"Drop Turn 2 (52%% of context)?\"${N}\n"
printf "  ${R}Confirmed${N}  ${S}→ token count dropped instantly in the header${N}\n"
printf "  ${S}             22%%  →  ~10%%  — that single turn was half of everything${N}\n"
printf '\n'
printf "  ${D}────────────────────────────────────────────────────────────────────${N}\n"
printf '\n'

printf "  ${B}See every byte.  Drop exactly what you don't need.${N}\n"
printf "  ${D}No other AI coding agent gives you this.${N}\n"
printf '\n'
printf "  ${Y}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${N}\n"
printf '\n'
