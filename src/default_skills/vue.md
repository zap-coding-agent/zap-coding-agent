---
name: vue
trigger: ["vue", "vuejs", "vue.js", "nuxt", "pinia", "vuex", "vite", ".vue", "<template>", "<script setup>", "composable", "ref(", "reactive(", "computed(", "defineProps"]
tokens: ~560
---

## Vue.js conventions

**Source:** Vue.js Style Guide (vuejs.org/style-guide), Nuxt docs, Vue 3 Composition API guide.

**Composition API (Vue 3):** Always use `<script setup>` — it is terser and has better TypeScript support than the Options API. The Options API is only appropriate for migrating Vue 2 code.

**Reactivity:**
- `ref()` for primitives and when you need `.value` access. `reactive()` for objects (avoid destructuring — it loses reactivity).
- `computed()` for derived state — never recompute manually.
- `watch` for side effects on reactive changes; `watchEffect` for automatic dependency tracking.
- Never mutate props — emit events up (`defineEmits`).

**Component design:**
- One component per file (`.vue`). Name components in `PascalCase` in `<script>`, `kebab-case` in templates.
- Keep components small. Extract reusable logic into composables (`use*.ts` files).
- `defineProps` with TypeScript types — no untyped prop objects.
- Use `defineExpose` sparingly — prefer event-driven communication.

**State management:**
- Local state: `ref`/`reactive` inside the component.
- Shared state: Pinia stores (preferred over Vuex in Vue 3). One store per domain.
- Avoid mutating store state outside store actions.

**Composables:** Extract reusable stateful logic into `composables/useXxx.ts`. Always return refs (not raw values) so reactivity is preserved at the call site. Name with `use` prefix.

**Performance:**
- `v-once` for truly static content. `v-memo` for expensive list items.
- Use `shallowRef`/`shallowReactive` for large objects you control mutation of.
- Key `v-for` items with stable unique ids, never array index.

**Nuxt (if applicable):** Use `useFetch` / `useAsyncData` for data fetching (SSR-safe). Leverage auto-imports for composables and components. Use `server/api/` for API routes.

**Formatting:** Prettier + ESLint with `eslint-plugin-vue`. `vue/recommended` ruleset. 2-space indent.
