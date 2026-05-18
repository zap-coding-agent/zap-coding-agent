---
name: debugging
description: Systematic approach to diagnosing and fixing bugs.
trigger: ["debug", "error", "bug", "crash", "panic", "not working", "broken", "trace", "exception", "stacktrace", "segfault", "undefined", "wrong output"]
tokens: ~450
---

## Debugging approach

**Before changing anything:** Reproduce the bug with a minimal case. Understand what SHOULD happen vs. what IS happening.

**Systematic process:**
1. Read the full error message — the root cause is usually in the last few lines
2. Find which stage produces wrong data: narrow the blast radius before touching code
3. Check recent changes first: `git log --oneline -10` and `git diff HEAD~1`
4. Add targeted logging at the suspected point, not everywhere
5. Check if the failure is environment-specific (local vs CI, debug vs release build)

**For panics / crashes:**
- Find your stack frame, not the library internals
- Check: out-of-bounds, null/None unwrap, concurrent mutation, resource exhaustion

**For logic bugs:**
- Write a failing test that reproduces it before touching the fix
- The test is your proof that the fix works and a regression guard

**Don't:**
- Scatter random changes and see what sticks
- Fix symptoms rather than root causes
- Fix unrelated issues while debugging — log them, address them separately
