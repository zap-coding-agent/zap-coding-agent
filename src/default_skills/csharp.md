---
category: domain
name: csharp
trigger: ["csharp", "c#", ".cs", "dotnet", ".net", "nuget", "using System", "namespace ", "async Task", "linq", "entity framework", "asp.net", "blazor", "maui", "xamarin"]
tokens: ~620
---

## C# / .NET conventions

**Source:** Microsoft .NET coding conventions, C# Language Reference, ASP.NET Core docs.

**Async:** Always `async`/`await` — never `.Result` or `.Wait()` (deadlock risk in sync contexts). Suffix async methods with `Async`. Use `CancellationToken` in every async public API. `ConfigureAwait(false)` in library code, not application code.

**Nullability:** Enable `<Nullable>enable</Nullable>` in every project. Use `?` suffix for nullable reference types. Prefer `??` and `?.` operators over explicit null checks. Use `ArgumentNullException.ThrowIfNull()`.

**Modern C# (10+):**
- Use `record` / `record struct` for immutable data.
- Use file-scoped `namespace MyApp;` over block-scoped.
- Use primary constructors (C# 12) for simple classes.
- Target-typed `new()` where type is clear from context.
- Use `required` properties instead of constructor-heavy DTO patterns.

**LINQ:** Prefer method syntax for pipelines. Avoid complex query syntax — readability suffers. Never call `.ToList()` prematurely inside `IQueryable` chains (forces DB round-trips).

**Dependency Injection:** Constructor injection always. Register services with the correct lifetime (`Singleton`, `Scoped`, `Transient`). Never resolve services directly from `IServiceProvider` except in factory methods.

**Error handling:** Use `Result<T>` pattern (e.g. via OneOf/ErrorOr) for expected failures in domain code. `Exception` for truly unexpected errors. Global middleware (`IExceptionHandler` in ASP.NET Core 8+) for HTTP error responses.

**Naming:** `PascalCase` for types/methods/properties, `_camelCase` for private fields, `camelCase` for locals/parameters. Prefix interfaces with `I` (`IUserRepository`).

**Testing:** xUnit + FluentAssertions + Moq/NSubstitute. One class per system-under-test. Use `[Theory]` with `[InlineData]` for parameterised cases.

**Formatting:** Use `.editorconfig` with C# rules. `dotnet format` enforces style. 4-space indent.
