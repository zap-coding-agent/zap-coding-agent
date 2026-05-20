---
category: domain
name: kotlin
trigger: ["kotlin", ".kt", ".kts", "fun ", "data class", "coroutine", "kotlinx", "gradle.kts", "jetpack", "compose", "flow", "suspend fun", "sealed class"]
tokens: ~580
---

## Kotlin conventions

**Source:** Kotlin coding conventions (kotlinlang.org/docs/coding-conventions.html), Android developers guides.

**Null safety:** Never use `!!` — it is a crash waiting to happen. Use `?.let`, `?:`, `requireNotNull`, or `checkNotNull` with a meaningful message. Design APIs to avoid nullable return types where possible.

**Immutability:** Prefer `val` over `var`. Use immutable collections (`listOf`, `mapOf`, `setOf`) by default; `mutableListOf` only when mutation is needed.

**Data classes:** Use `data class` for DTOs and value holders. Implement `copy()` instead of mutation. Prefer `object` for singletons, `companion object` for factory methods.

**Sealed classes:** Use `sealed class` / `sealed interface` for exhaustive state modelling (e.g. `Result`, `UiState`). The `when` expression on a sealed type is checked exhaustively by the compiler.

**Coroutines:**
- Launch from a `CoroutineScope` — never `GlobalScope` in production.
- Use `viewModelScope` / `lifecycleScope` in Android.
- `Flow` for streams, `suspend fun` for single async values.
- `withContext(Dispatchers.IO)` for blocking I/O; `Dispatchers.Default` for CPU work.
- Handle exceptions with `CoroutineExceptionHandler` or `runCatching`.

**Extension functions:** Use to add utility to existing types without inheritance. Keep them in a file named after the receiver type (`StringExt.kt`). Don't overuse — they pollute autocomplete.

**Scope functions:** `let` for nullable transformation, `apply` for object configuration, `run` for scope with result, `with` for multiple calls on an object, `also` for side effects. Don't chain more than two.

**Naming:** `camelCase` for functions/properties, `PascalCase` for classes, `SCREAMING_SNAKE_CASE` for constants in `companion object`.

**Testing:** JUnit 5 + Kotest or kotlinx-coroutines-test. Use `runTest` for coroutine tests.
