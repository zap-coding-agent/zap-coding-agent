---
category: practice
name: git
trigger: ["commit", "git", "branch", "merge", "rebase", "pull request", "pr", "git push", "git stage", "staged changes", "changelog", "git release", "git tag", "git log", "merge conflict", "push to"]
tokens: ~400
---

## Git conventions

**Commits:** Use Conventional Commits format: `<type>(<scope>): <description>`

Types: `feat` (new feature), `fix` (bug fix), `docs`, `style` (formatting), `refactor`, `perf`, `test`, `chore` (build/tooling), `ci`, `revert`

Examples:
- `feat(auth): add OAuth2 login flow`
- `fix(api): handle null response from payment provider`
- `chore(deps): bump tokio to 1.37`

**Rules:**
- Subject line ≤ 72 characters, imperative mood ("add" not "added")
- Body explains WHY, not what (the diff shows what)
- Never force-push to `main`/`master`
- One logical change per commit — don't bundle unrelated fixes
- Reference issues: `Fixes #123` or `Closes #456` in the body

**Branches:** `feat/description`, `fix/description`, `chore/description`. Delete after merge.

**PRs:** Keep small (< 400 lines changed ideally). Write a description that explains what changed and why. Include a test plan. Link related issues.

**Before committing:** Run tests, linter, and formatter. Review your own diff first.
