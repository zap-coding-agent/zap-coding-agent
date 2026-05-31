# TUI Chat — VS Code Parity Backlog

Analysis of gaps between the current TUI chat and VS Code's chat window,
with feasibility ratings and implementation notes.

---

## Current Layout

```
┌──────────────────────────────────────┬──────────┐
│  ZAP header art + git info           │          │
├──────────────────────────────────────┤ Sidebar  │
│                                      │ (22 cols)│
│  ◆ You  message text here...         │ model    │
│  ◆ zap  response + tool calls        │ context% │
│                                      │ skills   │
│  [tool blocks: collapsed/expanded]   │ todos    │
│                                      │          │
├──────────────────────────────────────┤          │
│  ❯ input box (multi-line)           │          │
├──────────────────────────────────────┤          │
│  ⌂ /path   Ctrl+F files  Ctrl+P dir │          │
└──────────────────────────────────────┴──────────┘
│  ↑↓ scroll  Tab  Ctrl+O collapse  …  Ctrl+Q    │
└─────────────────────────────────────────────────┘
```

---

## Hard TUI Limitations (can't fix)

1. **No proportional fonts** — everything is fixed-width. Message bubbles can't truly
   "float" left/right with variable width.
2. **No true right-alignment** — ratatui lays out left-to-right. Padding with spaces
   looks hacky and breaks on resize.
3. **No inline rich widgets inside paragraphs** — can't embed a "copy button" next to
   a code block. Each line is spans of styled text only.
4. **No hyperlinks** — OSC 8 links exist in some terminals (iTerm2, Kitty, WezTerm)
   but are not universally supported and ratatui doesn't wrap them.
5. **No mouse-driven interactions** — ratatui has mouse support but it's clunky for
   "hover to expand" or "click to apply diff."
6. **Screen real estate** — terminal is typically 80–120 cols. VS Code chat panel is
   ~400–600px. The current sidebar alone eats 22 of those columns.

---

## Feature Gap Matrix

| VS Code Feature | Feasible in TUI? | Notes |
|---|---|---|
| Message bubbles (distinct cards) | ✅ Yes | Wrap each message in a `Block` with borders/background |
| User/assistant asymmetry | ⚠️ Partial | Can indent user messages right, but can't truly right-align |
| Avatars/icons per sender | ✅ Yes | Unicode or Nerd Font glyphs work fine |
| Tool calls indented + dimmed | ✅ Yes | Already partially done; push further with nested borders |
| Code blocks with copy buttons | ❌ No | TUI has no clipboard integration per-block |
| Clickable file references | ❌ No | No hyperlinks in terminal |
| Context chips (@file, @folder) | ⚠️ Partial | Render as colored tags in input area; can't click-to-attach |
| Message editing + resubmit | ⚠️ Partial | Slash command for "edit last message"; inline editing is a GUI thing |
| Diff blocks with accept/reject | ✅ Yes | Already have diff viewer; add y/n per-hunk |
| Thinking section (collapsible) | ✅ Yes | Already have; could border it instead of just italics |
| Session switcher in header | ✅ Yes | Already have session picker; surface it more prominently |
| Timestamps per message | ✅ Yes | Trivial to add |
| Scroll-to-bottom indicator | ⚠️ Partial | "↓ new" indicator; already auto-scrolls |
| Stop generation | ✅ Yes | Already Ctrl+C |

---

## [C1] Message Card Borders

**What:** Wrap each user/assistant turn in a subtle border block. Assistant messages
get a faint left-border; user messages get a faint right-border (or just both bordered).

**Impact:** High — single biggest perceptual shift from "scrollable log" to "chat window."

**Effort:** Low — ~25 lines in `src/tui/render/messages.rs`.

**Details:**
- Use `Block::default().borders(Borders::LEFT).border_style(dim_gray)` on assistant
- Use `Block::default().borders(Borders::RIGHT).border_style(dim_gray)` on user (or skip — right borders on variable-width content are awkward in TUI)
- Add 1 blank line between message cards for breathing room

---

## [C2] Indent Asymmetry

**What:** User messages indented more from left, assistant messages indented less.
Creates a visual "conversation" rhythm even without bubbles.

**Impact:** High — reinforces who's speaking at a glance.

**Effort:** Low — change the indent prefix in `role_line` / `text_to_lines`.

**Details:**
- Assistant: current 2-space indent → keep at 2
- User: indent 6–8 spaces or prefix with `│   ` to create a visual right-shift
- Could also use a dim vertical bar `│` as a left gutter marker for user messages

---

## [C3] Turn Dividers

**What:** A thin horizontal line between each user→assistant exchange group.
The current/latest exchange is visually separated with a brighter border.

**Impact:** Medium — makes conversation structure immediately scannable.

**Effort:** Low — insert a `Line::from("─".repeat(width))` between message groups.

---

## [C4] Timestamps / Turn Numbers on Role Line

**What:** Current: `◆ zap` → could be `◆ zap · T4 · 2s ago`

**Impact:** Low — nice but non-essential.

**Effort:** Low — add fields to `UiMessage` for timestamp and elapsed duration.

---

## [C5] Better Tool Call Visual Nesting

**What:** Tool calls currently render as flat lines under the assistant message.
VS Code indents them with a left border, creating a tree structure.

**Impact:** High — tool calls are the most visually noisy part of the output.

**Effort:** Medium — refactor `tool_call_lines` to draw a left-border gutter.

**Proposed rendering:**
```
◆ zap
  │  ✓ read_file  src/main.rs  12ms
  │  │  [preview summary]
  │  ──
  │  ✓ edit_file  src/main.rs  8ms
  │  ──
  response text continues...
```
The `│` gutter connects all tool calls in a turn; `──` separates them.

---

## [C6] Per-Message Action Hints

**What:** Below each assistant message, a faint line showing available actions.
VS Code shows these as icon buttons; we show keybinds.

**Impact:** Medium — reduces confusion about what keys do what in context.

**Effort:** Low — append a styled `Line` after each assistant message block.

**Example:**
```
  ␣ [Tab focus · Ctrl+O collapse · /edit revise · Ctrl+Q quit]
```

---

## [C7] Context Bar Above Input

**What:** Before the `❯` prompt, show what context is currently loaded,
similar to VS Code's context chips.

**Impact:** Medium — makes context attachments visible without scrolling.

**Effort:** Medium — needs tracking of what files/context are loaded + rendering.

**Example:**
```
  [@src/auth.rs] [@README]  ❯ fix the login bug
```
Rendered as colored tags — non-interactive but informative.

---

## [C8] Collapsible History Sections

**What:** Messages older than N turns collapse into a summary line.

**Impact:** Medium — keeps the view focused on the current conversation.

**Effort:** High — needs scroll state tracking across collapse/expand boundaries.

**Example:**
```
── 12 earlier messages (Tab to expand) ──

◆ You  current message...
◆ zap  current response...
```

---

## [C9] Two-Pane Mode (Messages + Tool Output)

**What:** When a tool is running, split the view: left = conversation, right = live
tool output. Similar to VS Code's terminal panel.

**Impact:** Medium — useful for long-running builds/tests.

**Effort:** High — major layout refactor; dynamic split + live streaming to both panes.

---

## [C10] Inline Diff Apply

**What:** When a diff is shown, pressing `y` or `n` on each hunk to apply/reject.
Similar to VS Code's "Accept" button on suggested changes.

**Impact:** High — eliminates copy-paste from diff to editor.

**Effort:** Medium — needs per-hunk navigation state + git apply plumbing.

---

## Implementation Priority

| Priority | Item | Effort | Rationale |
|---|---|---|---|
| **P0** | C1 — Message borders | Low | Max perceptual shift for min code |
| **P0** | C2 — Indent asymmetry | Low | Complements C1; same code path |
| **P0** | C3 — Turn dividers | Low | Complements C1+C2 |
| **P1** | C5 — Tool call nesting | Medium | Tool output is the noisiest part |
| **P1** | C6 — Action hints | Low | Reduce keybind confusion |
| **P2** | C4 — Timestamps | Low | Nice polish |
| **P2** | C7 — Context bar | Medium | Surfaces invisible state |
| **P3** | C10 — Inline diff apply | Medium | High utility but depends on C5 |
| **P4** | C8 — Collapsible history | High | Complex state management |
| **P4** | C9 — Two-pane mode | High | Major layout refactor |

**Recommended first batch (P0):** ~50 lines in `src/tui/render/messages.rs` — message
borders + indent asymmetry + turn dividers. Do these together since they share the
same rendering path.
