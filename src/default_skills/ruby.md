---
name: ruby
trigger: ["ruby", ".rb", "rails", "gem", "bundler", "rspec", "rake", "rubocop", "gemfile", "minitest", "erb", "sinatra", "activerecord", "def ", "end\n", "do |"]
tokens: ~560
---

## Ruby conventions

**Source:** Ruby Style Guide (rubocop/ruby-style-guide), The Well-Grounded Rubyist, Rails Guides.

**Idiomatic Ruby:**
- Use `||=` for memoisation, `&&=` for conditional assignment.
- Prefer `map`/`select`/`reject`/`reduce` over `each` + mutation.
- Use `tap` for debugging and building, `then`/`yield_self` for pipelines.
- Prefer `Symbol#to_proc`: `users.map(&:name)` over `users.map { |u| u.name }`.
- Use string interpolation `"#{expr}"` over concatenation.

**Classes & modules:**
- Single Responsibility — keep classes small.
- Use modules for mixins (`include`) and namespacing.
- `attr_accessor`/`attr_reader` over manual getters/setters.
- `private` methods at the bottom, below `protected`.

**Errors:** Raise `StandardError` subclasses (not `Exception`). Rescue specifically — never bare `rescue`. Include context in the message. Use `ensure` for cleanup.

**Rails (if applicable):**
- Fat models, skinny controllers — business logic in models/service objects.
- Use service objects (`app/services/`) for complex operations.
- N+1 queries: always `includes` / `eager_load` for associations rendered in views.
- Validate at the model layer. Use strong parameters in controllers.
- Write database indexes for every foreign key and queried column.

**Gems:** Use `Bundler`. Pin exact versions in `Gemfile.lock` (commit it for apps, not for libraries). Run `bundle audit` for security advisories.

**Testing:** RSpec for BDD-style, Minitest for unit tests. Factory Bot for test data. VCR or WebMock for HTTP. Avoid `let!` unless ordering matters.

**Naming:** `snake_case` everywhere. `PascalCase` for classes/modules. `SCREAMING_SNAKE_CASE` for constants. Predicate methods end with `?`, bang methods with `!`.

**Formatting:** RuboCop with `.rubocop.yml`. 2-space indent. Max line 120 chars.
