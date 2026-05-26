---
category: practice
name: code-review
trigger: ["code review", "pr review", "pull request", "lgtm", "critique", "review my", "review this", "review the pr", "review the diff", "review the code", "please review", "can you review"]
tokens: ~450
---

## Code review approach

**What to check first:**
1. Does it do what it claims? Read the PR description, then the diff
2. Are there tests? Do they cover the failure cases, not just the happy path?
3. Are there security issues? (injection, auth bypass, secrets in code, improper input validation)
4. Is it correct under concurrent/edge conditions?

**Good review comments:**
- Be specific: quote the exact line, explain the issue, suggest a fix
- Distinguish blocking from non-blocking: prefix with `nit:` for style/preference, `blocking:` for must-fix
- Explain WHY something is a problem, not just that it is
- Ask questions when unsure: "Would this fail if X is null?" not "This will fail if X is null"

**Don't block on:**
- Style preferences when there's a formatter/linter configured
- Code you'd write differently but both ways are correct
- Hypothetical future requirements

**Do block on:**
- Missing error handling for realistic failure modes
- Security vulnerabilities
- Breaking changes without migration path
- Tests that don't actually test the behaviour

**Tone:** Review the code, not the person. "This function" not "you wrote this function".
Acknowledge good work — don't only comment on problems.
