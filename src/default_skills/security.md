---
category: practice
name: security
description: Security best practices for auth, secrets, input validation, and crypto.
trigger: ["auth", "authentication", "authorization", "password", "api key", "jwt", "oauth", "xss", "sql injection", "csrf", "encrypt", "sanitize", "validate input", "access token", "bearer token", "api token", "secret key", "auth token", "login flow", "session token", "session cookie", "hash password", "password hash"]
tokens: ~500
---

## Security guidelines

**Secrets — never in code:**
- Use environment variables or a secrets manager, never hardcoded values
- Never log secrets — scrub them before any log statement
- Rotate compromised secrets immediately; treat them as burned

**User input — validate at every boundary:**
- SQL: always use parameterized queries / prepared statements — never string-interpolate
- HTML output: escape or use a templating engine that auto-escapes; never `innerHTML` with user data
- File paths: reject `..` traversal; validate against an allowlist of allowed prefixes
- Shell commands: never interpolate user input — pass arguments as an array, not a string

**Passwords:**
- Hash with bcrypt, Argon2, or scrypt — never MD5, SHA1, or bare SHA256
- Compare hashes with constant-time equality to prevent timing attacks

**Tokens / sessions:**
- Use short expiry for JWTs; validate signature on every request — don't trust claims alone
- Store session tokens in HttpOnly cookies, not localStorage or sessionStorage
- Rotate tokens after privilege escalation (login, password change, role change)

**Least privilege:**
- Validate on the server even if the client already validated
- Give each component only the permissions it actually needs
- Fail closed: deny by default, allow explicitly
