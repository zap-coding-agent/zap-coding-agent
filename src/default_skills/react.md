---
category: domain
name: react
trigger: ["react", "component", "jsx", "tsx", "hook", "usestate", "useeffect", "useref", "usememo", "usecallback", "props", "next.js", "nextjs", "vite"]
tokens: ~650
---

## React conventions

**Components:** Always functional components — no class components. One component per file. File name matches component name (`UserCard.tsx`, not `user-card.tsx`).

**TypeScript:** Define a `Props` interface above the component. Export both named and default: `export function Button(...)` + `export default Button`. Never use `any` — use `unknown` and narrow it.

**Hooks:** Custom hooks go in `hooks/` and start with `use`. Keep `useEffect` dependencies honest — don't suppress the lint rule. Prefer `useMemo`/`useCallback` only when you have a measured perf problem, not pre-emptively.

**State:** Lift state to the lowest common ancestor. Prefer `useState` for local UI state, context for cross-tree state, and a dedicated store (Zustand/Jotai) for global app state.

**Styling:** Use the project's existing approach (Tailwind, CSS modules, or styled-components). Don't mix approaches in a single PR. No inline `style={{}}` objects except for dynamic values that can't be expressed in CSS.

**Performance:** Don't wrap every component in `React.memo`. Profile first, optimize second.

**Fetching:** Use React Query or SWR for server state — not `useEffect` + `useState` for data fetching. Keep components free of fetch logic; put it in hooks.

**File structure:**
```
src/
  components/       # Shared UI components
  features/         # Feature-specific components + logic
  hooks/            # Custom hooks
  lib/              # Pure utilities, no React
```
