---
name: php
trigger: ["php", ".php", "composer", "laravel", "symfony", "wordpress", "<?php", "artisan", "phpunit", "pest", "eloquent", "blade", "namespace ", "use "]
tokens: ~560
---

## PHP conventions

**Source:** PSR-12 coding standard (php-fig.org), PHP The Right Way (phptherightway.com), Laravel best practices.

**Modern PHP (8.1+):**
- Use strict types: `declare(strict_types=1);` at the top of every file.
- Use typed properties, union types, intersection types, and `never` return type.
- Use `enum` for sets of values instead of constants.
- Named arguments for clarity at call sites: `array_slice(array: $arr, offset: 2)`.
- `readonly` properties for immutable value objects.
- `match` expression over `switch` (strict comparison, expression syntax).
- Fibers for cooperative concurrency (PHP 8.1+).

**Namespaces & autoloading:** PSR-4 autoloading via Composer. One class/interface/trait per file. Namespace mirrors directory structure.

**Error handling:** Throw exceptions — never return `false`/`null`/`0` to signal failure. Use typed exception hierarchy. Set `error_reporting(E_ALL)` and convert errors to exceptions with a custom handler in development.

**Dependency management:** Composer for everything. Pin in `composer.lock` (commit it). Use `composer audit` for security vulnerabilities. Never `require` files manually if autoloading covers it.

**Security:**
- Parameterised queries always (PDO/prepared statements). Never string interpolation in SQL.
- `htmlspecialchars($output, ENT_QUOTES, 'UTF-8')` before any user content in HTML.
- Use `password_hash()`/`password_verify()` for passwords — never MD5/SHA1.
- `random_bytes()` / `random_int()` for cryptographic randomness.

**Laravel (if applicable):**
- Service container for DI. Constructor injection over facades in library code.
- Eloquent: use relationships, avoid N+1 with `with()` / `load()`.
- Jobs for async work, queues for deferral. Never sleep in web requests.
- Use `FormRequest` for validation; keep controllers thin.

**Testing:** Pest (modern) or PHPUnit. Feature tests for HTTP endpoints, unit tests for services.

**Formatting:** PHP-CS-Fixer with PSR-12. 4-space indent.
