---
category: domain
name: cpp
trigger: ["c++", "cpp", ".cpp", ".hpp", ".cc", ".hxx", "std::", "#include <", "cmake", "clang++", "g++", "template<", "namespace ", "virtual ", "nullptr"]
tokens: ~630
---

## C++ conventions

**Source:** C++ Core Guidelines (isocpp.github.io/CppCoreGuidelines), Google C++ Style Guide, Effective Modern C++ (Meyers).

**Resource management:** RAII always. Never raw `new`/`delete` — use `std::unique_ptr` for exclusive ownership, `std::shared_ptr` only when shared ownership is genuinely needed (it has overhead). Use `std::make_unique` / `std::make_shared`.

**Ownership & const:** Pass by `const&` for read-only access to non-trivial types. Pass by value for sink parameters (moved into). Pass by `T*` only when nullptr is a valid value. Mark every method that doesn't mutate state `const`.

**Modern C++ (17/20):**
- Prefer structured bindings: `auto [key, val] = *it;`
- Use `std::optional<T>` for nullable values, `std::variant` for sum types.
- Use range-based `for` loops and `<algorithm>` over raw index loops.
- `std::string_view` for read-only string parameters (avoids copies).
- `[[nodiscard]]` on functions whose return values must not be ignored.
- Concepts (C++20) for constrained templates instead of SFINAE.

**Error handling:** Use return values (`std::expected<T,E>` in C++23, or a project error type). Exceptions are acceptable for truly exceptional conditions but must be documented. Never let exceptions escape destructors.

**Undefined behaviour:** Enable UBSan + ASan in debug/CI builds. Use `[[assume(cond)]]` sparingly. Don't rely on signed overflow or out-of-bounds pointer arithmetic.

**Build:** CMake with `target_*` (not directory-level) commands. Use `find_package` for dependencies; vcpkg or Conan for package management. Enable `-Wall -Wextra -Wpedantic` and treat warnings as errors in CI.

**Naming:** Follow project convention. Google style: `ClassName`, `method_name()`, `member_name_`, `kConstantName`, `MACRO_NAME`. Avoid abbreviations.

**Testing:** Google Test (gtest) or Catch2. Build with sanitizers in test mode.

**Formatting:** `clang-format` with a committed `.clang-format` file. Non-negotiable.
