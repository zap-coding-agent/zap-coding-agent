---
category: domain
name: dart
trigger: ["dart", "flutter", ".dart", "widget", "stateful", "stateless", "pubspec", "pub.dev", "riverpod", "bloc", "provider", "async/await", "future<", "stream<", "buildcontext"]
tokens: ~570
---

## Dart / Flutter conventions

**Source:** Effective Dart (dart.dev/effective-dart), Flutter docs, Very Good Ventures style guide.

**Dart language:**
- Use `final` for variables that aren't reassigned, `const` for compile-time constants.
- Sound null safety — never use `!` (null assertion) except when you can prove non-null. Use `??`, `?.`, `late` where appropriate.
- Prefer `=>` for one-line functions. Use `async`/`await` over raw `Future.then()`.
- Named parameters for clarity: `createUser(name: 'Alice', age: 30)`. Required named params with `required`.
- Use `sealed class` (Dart 3) for exhaustive pattern matching.

**Flutter widgets:**
- Prefer `StatelessWidget` — extract state only when needed.
- `const` constructors everywhere possible (enables widget caching and hot reload optimisation).
- Keep `build()` pure — no side effects, no async calls directly inside it.
- Extract large `build` methods into private methods or sub-widgets.
- Use `Builder` widget to get a fresh `BuildContext` below a provider.

**State management:**
- Riverpod (recommended): Providers are globally accessible, testable, composable. Use `AsyncNotifierProvider` for async state.
- BLoC: clear event/state separation, good for complex flows. One BLoC per feature.
- Avoid `setState` beyond single-widget local state (e.g. text field focus).

**Navigation:** Use `go_router` or Navigator 2.0 for named/nested routing. Avoid anonymous `MaterialPageRoute` pushes in large apps.

**Performance:**
- `ListView.builder` / `GridView.builder` over `ListView(children: [...])` for long lists.
- Avoid rebuilding large widget trees — use `select` (Riverpod) or `BlocSelector` to narrow rebuilds.
- `RepaintBoundary` around independently animating widgets.

**Testing:** Unit tests with `flutter_test`. Widget tests with `tester.pumpWidget`. Integration tests with `patrol` or `integration_test`. Aim for high coverage on business logic; widget tests for critical UI flows.

**Formatting:** `dart format` (built-in). `flutter analyze` clean before PR.
