---
name: swift
trigger: ["swift", ".swift", "swiftui", "xcode", "ios", "macos", "watchos", "tvos", "func ", "guard let", "if let", "struct ", "protocol ", "actor ", "async throws", "combine"]
tokens: ~600
---

## Swift conventions

**Source:** Swift API Design Guidelines (swift.org/documentation/api-design-guidelines), Apple Human Interface Guidelines.

**Optionals:** Never force-unwrap (`!`) except in tests or IBOutlets. Use `guard let` for early exit, `if let` for local scope. Use `?? defaultValue` for fallbacks. `map`/`flatMap` for optional transformation.

**Value vs reference types:** Prefer `struct` for data (value semantics, stack allocation, no retain cycles). Use `class` only when identity matters or inheritance is required. Use `enum` with associated values for sum types.

**Concurrency (Swift 5.5+):**
- Use `async`/`await` over completion handlers — no new callback-based APIs.
- `actor` for protecting shared mutable state (replaces serial queues).
- `Task { }` to bridge sync → async. `@MainActor` for UI updates.
- `AsyncStream` / `AsyncThrowingStream` for event sequences.
- Never `Task.sleep` in production loops — use proper timers.

**Protocol-oriented:** Define capabilities as protocols. Use protocol extensions for default implementations. Prefer composition over class inheritance.

**Error handling:** Use typed `throws` (Swift 6 `typed throws`). Define `enum MyError: Error` for domain errors. `do/try/catch` at the boundary; propagate with `throws` internally.

**SwiftUI:**
- Keep `View` structs small and focused — extract sub-views liberally.
- Put business logic in `@Observable` / `ObservableObject` view models, not in views.
- Prefer `@State` for local view state, `@Binding` for child-writable parent state.
- Use `.task { }` for async work tied to view lifecycle.

**Naming:** `camelCase` for functions/variables/properties, `UpperCamelCase` for types/protocols. Omit needless words (`removeElement(at:)` not `removeElementAt(index:)`). Label parameters clearly at call site.

**Formatting:** `swift-format` or SwiftLint with a committed `.swiftlint.yml`. 4-space indent.
