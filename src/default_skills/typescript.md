---
category: domain
name: typescript
trigger: ["typescript", "javascript", "node", "npm", "yarn", "pnpm", "package.json", ".ts", ".js", "express", "fastify", "bun"]
tokens: ~500
---

## TypeScript / JavaScript conventions

**Types:** Prefer `interface` for object shapes, `type` for unions/intersections/aliases. Never use `any` — use `unknown` and narrow with type guards. Enable `strict: true` in `tsconfig.json`.

**Null safety:** Use optional chaining `?.` and nullish coalescing `??`. Avoid non-null assertions `!` except where you've just done an explicit check.

**Imports:** Use named imports over default imports where possible (better tree-shaking, better refactoring). Keep imports grouped: Node builtins → external packages → internal modules. Use path aliases (`@/`) over deep relative paths.

**Async:** Prefer `async/await` over `.then()` chains. Always handle promise rejections — either with `try/catch` or explicit `.catch()`. Never `async function` without `await` inside.

**Functions:** Prefer arrow functions for callbacks and short utilities. Use named `function` declarations for top-level functions (better stack traces). Keep functions under ~30 lines.

**Error handling:** Create typed error classes that extend `Error`. Always include a `message`. Log errors at the boundary where you handle them, not where you throw.

**Testing:** Use Vitest or Jest. Co-locate test files as `*.test.ts`. Mock at the module boundary, not deep inside implementations.

**Node:** Use `process.env` with a validated config object at startup — never read `process.env` scattered through the codebase. Use `zod` for runtime validation of external data.
