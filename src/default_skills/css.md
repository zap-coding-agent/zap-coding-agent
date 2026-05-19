---
name: css
trigger: ["css", "scss", "sass", "less", "stylesheet", ".css", ".scss", "tailwind", "flexbox", "grid", "media query", "animation", "keyframe", "styled-components", "css modules", "postcss"]
tokens: ~540
---

## CSS / SCSS conventions

**Source:** MDN Web Docs, Tailwind CSS docs, Every Layout (every-layout.dev), Defensive CSS (defensivecss.dev).

**Layout:**
- CSS Grid for two-dimensional layout; Flexbox for one-dimensional (row or column).
- Prefer `gap` over margins for spacing between flex/grid children.
- Use `min-content`, `max-content`, `minmax()`, `clamp()` for intrinsically responsive layouts.
- Avoid fixed heights — let content determine height. Use `min-height`.
- `aspect-ratio` for preserving proportions instead of the padding-hack.

**Sizing & spacing:**
- Use relative units: `rem` for font sizes (respects user preferences), `em` for component-relative spacing, `%` and `vw`/`vh`/`svh` for container-relative sizing.
- Avoid magic numbers — use CSS custom properties (`--spacing-md: 1rem`).
- Define a spacing scale as custom properties on `:root`.

**Modern CSS (no preprocessor needed for many patterns):**
- CSS custom properties for theming and dynamic values.
- `@layer` for cascade management (base, components, utilities).
- Container queries (`@container`) for component-level responsiveness.
- `:is()`, `:where()`, `:has()` for concise selectors.
- `@starting-style` for enter animations without JavaScript.

**SCSS (if used):**
- Use `@use` / `@forward` (not `@import` — deprecated).
- One partial per component (`_button.scss`). Flat structure over deep nesting.
- Max 3 levels of nesting. Avoid `&` chains that produce unreadable selectors.
- Variables → custom properties for runtime values; SCSS variables for build-time (breakpoints, tool config).

**Tailwind (if used):**
- Extract repeating utility patterns to components, not `@apply` classes.
- Use `@layer components` for truly reusable UI patterns.
- Customise via `tailwind.config` — don't override with arbitrary values in markup unless necessary.

**Performance:** Avoid `*` universal selectors in deep trees. Use `will-change` sparingly. Prefer CSS animations over JS for simple transitions (compositor thread).

**Accessibility:** Sufficient colour contrast (WCAG AA: 4.5:1 normal, 3:1 large text). Never remove `:focus` outline without a visible replacement. Use `prefers-reduced-motion` media query.
