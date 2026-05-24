---
category: practice
name: deploy
trigger: ["deploy", "deployment", "release", "ship", "publish", "install locally", "update zap", "build zap"]
tokens: ~300
---

## Deploy zap

Deploy means: build a release binary and install it to both `~/.cargo/bin/zap` and `~/.local/bin/zap` (codesigned on macOS).

### Pre-deploy checklist (before running deploy.sh)

1. **Tests pass:** `cargo test` (all 83 tests)
2. **Build passes:** `cargo build` (no errors)
3. **Version bumped:** `Cargo.toml` → `version = "x.y.z"` (semver)
4. **FEATURES.md updated:** any new feature entries added under the relevant section
5. **Committed + pushed:** `git push` (pre-push hook runs `cargo test`)

### Deploy command

```bash
bash scripts/deploy.sh
```

This runs from the repo root. It does:
1. `cargo build --release` → `target/release/zap`
2. Copies to `~/.cargo/bin/zap` and `~/.local/bin/zap`
3. Codesigns both copies (macOS only, non-fatal on failure)
4. Smoke test: `zap --help` exits 0

### Check installed versions (no build)

```bash
bash scripts/deploy.sh --check
```

### Typical deploy workflow

```
# 1. Bump version in Cargo.toml
# 2. Add entry to FEATURES.md
# 3. git add -A && git commit -m "feat: short description (vX.Y.Z)"
# 4. git push  # triggers pre-push cargo test
# 5. bash scripts/deploy.sh  # release build + install
```

### Locations

| Path | Purpose |
|---|---|
| `scripts/deploy.sh` | The deploy script |
| `~/.cargo/bin/zap` | Primary install (cargo PATH) |
| `~/.local/bin/zap` | Secondary install (user PATH) |
| `target/release/zap` | Build output (~19MB) |
