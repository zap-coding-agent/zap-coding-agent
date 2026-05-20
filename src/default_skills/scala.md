---
category: domain
name: scala
trigger: ["scala", ".scala", "sbt", "akka", "spark", "pekko", "cats", "zio", "case class", "object ", "trait ", "implicit ", "given ", "for comprehension", "val ", "def "]
tokens: ~560
---

## Scala conventions

**Source:** Scala Style Guide (docs.scala-lang.org/style), Scala 3 docs, Effective Scala (Twitter).

**Immutability:** Always `val` over `var`. Immutable collections by default (`List`, `Map`, `Set` from `scala.collection.immutable`). Only reach for `var` and mutable collections in tight performance-critical loops.

**Case classes:** Use for data modelling — they get `equals`, `hashCode`, `copy`, `unapply` for free. Use `sealed trait` + `case class` / `case object` for ADTs (exhaustive pattern matching).

**Pattern matching:** Exhaustive `match` expressions over chains of `if`. Use guards (`case x if x > 0`). Prefer `match` over `isInstanceOf` / `asInstanceOf`.

**Functional style:**
- Chain `map`/`flatMap`/`filter`/`fold` over collections instead of imperative loops.
- Use `Option` for nullable values — never `null` in Scala code.
- `Either[Error, Value]` for error-carrying computations. `Try` for wrapping Java exceptions.
- Use `for` comprehensions to flatten nested `flatMap` chains.

**Scala 3 specifics:**
- Use `given`/`using` over `implicit` (clearer intent).
- `enum` for ADTs instead of `sealed trait` + case classes where appropriate.
- Extension methods replace `implicit class`.
- Opaque types for type-safe wrappers with zero runtime cost.

**Effects (ZIO / Cats Effect):** If using an effect system, never mix raw `Future` — pick one and be consistent. Avoid blocking — wrap with `ZIO.blocking` / `IO.blocking`. Model errors in the type system, not exceptions.

**Concurrency:** Prefer `ZIO` / `Cats Effect IO` over `scala.concurrent.Future` (better error handling, resource safety). If using Akka/Pekko, keep actors thin — put logic in plain functions.

**Build:** SBT. Keep `build.sbt` clean. Use `libraryDependencies` scoped correctly (`% Test` for test deps).

**Naming:** `camelCase` for methods/values, `PascalCase` for types/objects. Avoid single-letter names except in truly local lambda args (`x`, `xs`, `f`).

**Formatting:** `scalafmt` with a committed `.scalafmt.conf`. Non-negotiable.
