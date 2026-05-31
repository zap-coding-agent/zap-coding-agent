#!/usr/bin/env bash
# explore-db.sh — interactive explorer for zap's code index (.zap/code.db)
# Requires: sqlite3 (ships with macOS; `brew install sqlite` on Linux)
# Usage: ./scripts/explore-db.sh [path/to/.zap/code.db]

set -euo pipefail

# ── colours ──────────────────────────────────────────────────────────────────
CYAN='\033[36m' YELLOW='\033[33m' GREEN='\033[32m' DIM='\033[2m' BOLD='\033[1m' RESET='\033[0m'
c()  { printf "${CYAN}%s${RESET}"   "$*"; }
y()  { printf "${YELLOW}%s${RESET}" "$*"; }
g()  { printf "${GREEN}%s${RESET}"  "$*"; }
d()  { printf "${DIM}%s${RESET}"    "$*"; }
b()  { printf "${BOLD}%s${RESET}"   "$*"; }
sep(){ printf "${DIM}%s${RESET}\n"  "────────────────────────────────────────────────────"; }

# ── locate DB ────────────────────────────────────────────────────────────────
DB="${1:-}"
if [[ -z "$DB" ]]; then
    # walk up from CWD looking for .zap/code.db
    dir="$PWD"
    while [[ "$dir" != "/" ]]; do
        [[ -f "$dir/.zap/code.db" ]] && { DB="$dir/.zap/code.db"; break; }
        dir="$(dirname "$dir")"
    done
fi

if [[ -z "$DB" || ! -f "$DB" ]]; then
    printf "\n  ${YELLOW}!${RESET} No .zap/code.db found in this directory tree.\n"
    printf "  Run $(c 'zap') then $(c '/init') inside your project to build the index.\n\n"
    exit 1
fi

q() { sqlite3 "$DB" "$1"; }   # run a query
qf(){ sqlite3 -column -header "$DB" "$1"; }  # run a query with formatted output

# ── header ───────────────────────────────────────────────────────────────────
clear
printf "\n"
b "  zap code index explorer"
printf "\n"
d "  $DB"
printf "\n\n"

FILES=$(q "SELECT COUNT(*) FROM indexed_files;")
SYMS=$(q  "SELECT COUNT(*) FROM symbols;")
DB_KB=$(du -k "$DB" | cut -f1)

printf "  $(c "$FILES") files   $(c "$SYMS") symbols   $(d "${DB_KB} KB")\n\n"
sep

# ── menu ─────────────────────────────────────────────────────────────────────
MENU=(
    "Overview — symbols by kind"
    "Top 15 files by symbol count"
    "All functions"
    "All structs"
    "All enums & traits"
    "Search symbol by name"
    "All symbols in a file"
    "Find where a symbol is referenced"
    "Biggest files (most symbols)"
    "Raw SQL — type your own query"
    "Quit"
)

while true; do
    printf "\n"
    b "  Choose a query:"
    printf "\n\n"
    for i in "${!MENU[@]}"; do
        printf "  $(y "$((i+1))")  ${MENU[$i]}\n"
    done
    printf "\n  $(d "> ")"
    read -r choice

    printf "\n"
    sep

    case "$choice" in

    1)  # Overview — symbols by kind
        printf "\n$(b "  Symbols by kind")\n\n"
        q "SELECT kind, COUNT(*) as count
           FROM symbols
           GROUP BY kind
           ORDER BY count DESC;" \
        | while IFS='|' read -r kind count; do
            bar=$(printf '█%.0s' $(seq 1 $((count / 20 + 1))))
            printf "  $(c "$kind")  $(d "$count")  $bar\n"
          done
        printf "\n"
        ;;

    2)  # Top 15 files by symbol count
        printf "\n$(b "  Top 15 files by symbol count")\n\n"
        q "SELECT symbol_count, path FROM indexed_files ORDER BY symbol_count DESC LIMIT 15;" \
        | while IFS='|' read -r count path; do
            short="${path##*/}"
            printf "  $(c "$count")  $(d "$path")\n"
          done
        printf "\n"
        ;;

    3)  # All functions
        printf "\n$(b "  Functions")\n\n"
        q "SELECT name, path, line FROM symbols
           WHERE kind IN ('fn','function','def')
           ORDER BY path, line
           LIMIT 80;" \
        | while IFS='|' read -r name path line; do
            short="${path##"${PWD}/"}"
            printf "  $(g "$name")  $(d "$short:$line")\n"
          done
        count=$(q "SELECT COUNT(*) FROM symbols WHERE kind IN ('fn','function','def');")
        printf "\n  $(d "($count total functions — showing first 80)")\n\n"
        ;;

    4)  # All structs
        printf "\n$(b "  Structs")\n\n"
        q "SELECT name, path, line, signature FROM symbols
           WHERE kind = 'struct'
           ORDER BY name
           LIMIT 60;" \
        | while IFS='|' read -r name path line sig; do
            short="${path##"${PWD}/"}"
            printf "  $(c "$name")  $(d "$short:$line")\n"
            [[ -n "$sig" ]] && printf "    $(d "$sig")\n"
          done
        printf "\n"
        ;;

    5)  # Enums & traits
        printf "\n$(b "  Enums & Traits")\n\n"
        q "SELECT kind, name, path, line FROM symbols
           WHERE kind IN ('enum','trait')
           ORDER BY kind, name
           LIMIT 60;" \
        | while IFS='|' read -r kind name path line; do
            short="${path##"${PWD}/"}"
            tag=$( [[ "$kind" == "enum" ]] && printf "E" || printf "T" )
            printf "  $(y "[$tag]") $(c "$name")  $(d "$short:$line")\n"
          done
        printf "\n"
        ;;

    6)  # Search symbol by name
        printf "  Symbol name (partial ok): "
        read -r term
        printf "\n$(b "  Results for: '$term'")\n\n"
        q "SELECT name, kind, path, line, signature FROM symbols
           WHERE name LIKE '%${term}%' COLLATE NOCASE
           ORDER BY kind, name
           LIMIT 40;" \
        | while IFS='|' read -r name kind path line sig; do
            short="${path##"${PWD}/"}"
            printf "  $(c "$name")  $(y "$kind")  $(d "$short:$line")\n"
            [[ -n "$sig" ]] && printf "    $(d "$sig")\n"
          done
        printf "\n"
        ;;

    7)  # All symbols in a file
        printf "  File name (partial ok, e.g. provider): "
        read -r term
        printf "\n$(b "  Symbols in files matching '$term'")\n\n"
        q "SELECT name, kind, line, signature FROM symbols
           WHERE path LIKE '%${term}%'
           ORDER BY line
           LIMIT 80;" \
        | while IFS='|' read -r name kind line sig; do
            printf "  $(d "$line")  $(c "$name")  $(y "$kind")\n"
            [[ -n "$sig" ]] && printf "    $(d "$sig")\n"
          done
        printf "\n"
        ;;

    8)  # Find where a symbol is referenced
        printf "  Symbol name (exact): "
        read -r term
        printf "\n$(b "  Files that mention '$term'")\n\n"
        # grep the raw source files for the symbol name
        if command -v rg &>/dev/null; then
            rg --no-heading -n "$term" src/ 2>/dev/null | head -40 \
            | while IFS=: read -r file line rest; do
                printf "  $(d "$file:$line")  $(d "$rest")\n"
              done
        else
            grep -rn "$term" src/ 2>/dev/null | head -40 \
            | while IFS=: read -r file line rest; do
                printf "  $(d "$file:$line")  $(d "$rest")\n"
              done
        fi
        printf "\n"
        ;;

    9)  # Biggest files
        printf "\n$(b "  All indexed files — by symbol count")\n\n"
        q "SELECT symbol_count, path FROM indexed_files ORDER BY symbol_count DESC LIMIT 30;" \
        | while IFS='|' read -r count path; do
            short="${path##"${PWD}/"}"
            printf "  $(c "$count")  $short\n"
          done
        printf "\n"
        ;;

    10) # Raw SQL
        printf "  SQL: "
        read -r sql
        printf "\n$(b "  Result")\n\n"
        qf "$sql" 2>&1 | head -60 | while IFS= read -r row; do
            printf "  $(d "$row")\n"
        done
        printf "\n"
        ;;

    11|q|Q|quit|exit)
        printf "\n  $(g "bye")\n\n"
        exit 0
        ;;

    *)  printf "  $(d "pick 1–11")\n" ;;

    esac

    sep
done
