---
category: domain
name: java
trigger: ["java", "maven", "gradle", "spring", "jvm", ".java", "public class", "implements ", "extends ", "junit", "lombok", "jakarta", "record ", "interface "]
tokens: ~650
---

## Java conventions

**Source:** Effective Java (Bloch), Google Java Style Guide, JDK 17+ idioms.

**Error handling:** Throw checked exceptions only when callers can reasonably recover. Prefer unchecked (`RuntimeException`) for programming errors. Never swallow exceptions with an empty catch. Include meaningful messages.

**Immutability:** Prefer immutable classes — `final` fields, no setters. Use Java 16+ `record` for pure data carriers. For mutable builders use the Builder pattern (Effective Java Item 2).

**Nulls:** Prefer `Optional<T>` as a return type when absence is expected. Never pass `Optional` as a parameter. Avoid `null` in collections — use empty collections instead.

**Modern Java (17+):**
- Use `var` where the type is obvious from the right-hand side.
- Use `switch` expressions and pattern matching (`instanceof Pattern p`).
- Use sealed classes + records for algebraic data types.
- Prefer `List.of()`, `Map.of()`, `Set.of()` for immutable collections.

**Collections & Streams:** Use the Streams API for transformation pipelines; keep terminal operations simple. Avoid parallel streams unless you've profiled and confirmed a benefit — they have hidden costs.

**Naming:** `camelCase` for methods/fields, `PascalCase` for classes, `ALL_CAPS` for constants, `camelCase` for packages (no underscores). Interfaces may omit `I` prefix — name by capability (`Readable`, `Comparator`).

**Dependency injection:** Use constructor injection over field injection (easier to test). Define dependencies through interfaces, not concrete classes.

**Testing:** JUnit 5 + AssertJ for assertions. Mockito for mocking. Name tests `methodName_scenario_expectedResult`. One logical assertion per test.

**Build:** Maven or Gradle. Keep `pom.xml` / `build.gradle` dependency scope correct (`test` for test-only deps). Use `mvn versions:display-dependency-updates` / `./gradlew dependencyUpdates` regularly.

**Formatting:** Enforce with Google Java Format or Checkstyle. 4-space indent, 100-char line limit.
