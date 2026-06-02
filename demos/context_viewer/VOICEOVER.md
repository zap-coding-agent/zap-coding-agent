# /context Voiceover Script
# Video length: ~5:00  |  PlaybackSpeed 1.5x
# Format: [timestamp] — what's on screen, then your words

---

## [0:00] — Title card  (40 seconds — plenty of room)

[breathe, let the card land for 3 seconds before speaking]

"Every AI coding tool shows you a number.
... Twenty-two percent.
... That's all you get."

[pause 3s]

"You have no idea which question caused it.
... Which tool call bloated it.
... Or what you can actually do about it."

[pause 3s]

"You're flying blind.
... With your own context.
... In your own session."

[pause 3s]

"zap changes that."

[let the silence hold to the end of the title card]

---

## [0:40] — Phase card appears: "Building real context"

"Four questions. Real codebase. Real tool calls.
... Then we look inside."

---

## [0:50] — TUI launches

[wait for TUI to load]

"Let's go."

---

## [0:57] — Turn 1: "Give me a quick overview..."  (~17s video)

"First question — project overview.
... See those tool calls? File tree, source files — all going into context."

---

## [1:14] — Turn 2: "Show me all the route handlers..."  (~23s video)

"Second question. Watch this one — multiple files, multiple tool calls.
... This is the heaviest turn.
... That single question just consumed the biggest chunk of context."

---

## [1:37] — Turn 3: "What does the User model look like?"  (~17s video)

"Third — data model. Lighter, a file or two.
... Context growing, but slower."

---

## [1:54] — Turn 4: "Got it, thanks."  (~7s video)

"And a greeting.
... Zero tool calls. ... Near-zero cost.
... That's the contrast.
... Four turns — completely different footprints."

---

## [2:42] — Pre-context pause, then /context typed

[brief pause]

"Now let's look inside.
... /context."

---

## [2:44] — /context overlay opens

[overlay appears with all 4 turns listed]

"There it is.
... Every turn ... with its exact token cost ...
... and its percentage of the total context window.
... This is what twenty-two percent actually looks like — broken down."

---

## [2:49] — Navigate down to Turn 2

[j j pressed, Turn 2 highlighted in yellow]

"Turn Two.
... The route handler question.
... Look at that number.
... Over half the context ... from a single question."

---

## [2:53] — Detail panel opens (press l / →)

[right panel slides in with real content]

"Now let's go inside.
... This is the detail panel.
... Real content — nothing hidden."

---

## [2:57] — Scrolling through detail: user text → tool calls

[scrolling down, tool call section appears with JSON]

"The user message at the top.
... Then the tool calls — actual function names,
... actual JSON input that was sent.
... This is exactly what the LLM requested."

---

## [3:03] — Pause on tool results

[tool result content visible]

"And the results — the full file contents that came back.
... Every byte of this ... is in your context.
... Every byte is costing you."

---

## [3:08] — Scroll to assistant response

[assistant text section]

"And finally the assistant's response.
... The whole turn ... laid out in front of you."

---

## [3:12] — Return to list panel (press h)

[left panel gets focus back]

"Back to the list.
... Turn Two is selected.
... We've read it. We've seen it.
... And honestly ... we don't need it anymore.
... The route handlers are understood. That context is done."

---

## [3:17] — Long pause before drop

[Turn 2 highlighted, user can narrate calmly]

"So let's drop it.
... In any other tool you'd have to clear everything,
... or compact everything,
... or just ... live with it.
... In zap — you pick the turn. ... You drop the turn."

---

## [3:18] — d pressed → red confirmation banner

[row turns red, footer shows: "⚠ Drop Turn 2 (X tokens, 52%)? [Enter] confirm [Esc] cancel"]

"There's the confirmation.
... Turn Two. ... Its token cost. ... Its exact share of the window.
... One more press to confirm."

---

## [3:22] — Enter pressed → turn drops

[list shrinks, header token count drops live]

"Gone.
... Look at the header.
... Token count dropped ... instantly.
... Not an estimate. Not a rough figure.
... That turn is out of the context window. ... Right now."

---

## [3:26] — Navigate remaining turns after drop

[navigating remaining turns — Turn 1, Turn 3, Turn 4, opening detail briefly]

"The other turns are still here — complete, intact.
... Nothing else was touched.
... Three turns remain. ... Twenty-two percent is now ten."

[open a detail panel, scroll briefly]

"You can inspect any of them the same way.
... Same visibility. ... Same control."

[close detail, return to list]

---

## [3:40] — Close context viewer

[q pressed, TUI chat visible]

"That's /context in zap.
... You've seen every byte.
... You dropped exactly what you didn't need."

---

## [3:50] — Summary card appears

[summary card visible with the story]

[read the card or just let it breathe]

"Four questions.
... Different costs.
... Full visibility into every one of them.
... Drop what you don't need."

[pause]

"Your context. Your control.
... This is /context in zap."

[let summary card hold to end]

---

## Delivery notes

- Speak at **a relaxed pace** — the video is 1.5x speed so rushing will sound frantic
- The **long pauses** marked above are real silences — let the screen do the work
- Emphasis words: "instantly", "exactly", "every byte", "right now", "your control"
- Tone: calm confidence, not hype — let the feature speak
- Turn 2 tool calls section (~3:00): this is the **money shot**, slow down here

