---
name: go
trigger: ["golang", "go mod", "goroutine", "channel", "interface{}", "go.mod", ".go", "func ", "gofmt", "golangci"]
tokens: ~500
---

## Go conventions

**Errors:** Return `error` as the last return value. Wrap with context: `fmt.Errorf("loading config: %w", err)`. Check every error — never `_` an error return from something that can fail. Create typed errors with `errors.New` or custom types for errors callers need to handle differently.

**Naming:** Short names for short-lived variables (`i`, `n`, `err`). Descriptive names for package-level declarations. Interfaces named by what they do: `Reader`, `Closer`, `Handler`. Don't stutter: `user.User` → bad, `user.Profile` → good.

**Goroutines:** Always know when a goroutine will exit. Use `context.Context` for cancellation. Protect shared state with `sync.Mutex` or channels. Use `errgroup` for concurrent work with error collection.

**Interfaces:** Define interfaces where they're used, not where the type is defined. Keep interfaces small — one or two methods is ideal. Accept interfaces, return concrete types.

**Packages:** Keep packages focused on one thing. Avoid circular imports. Don't put everything in `utils` or `helpers` — name packages by what they provide.

**Testing:** Use `testing` package. Table-driven tests for multiple inputs. Use `testify/assert` for readability. Test the public API, not internals. Benchmarks in `*_test.go` with `Benchmark` prefix.

**Formatting:** `gofmt` is non-negotiable. Run `golangci-lint` before PRs.
