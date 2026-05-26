---
category: practice
name: deploy
trigger: ["deploy", "deployment", "release version", "ship it", "publish package", "publish release", "install locally", "update zap", "build zap", "cut a release", "release build"]
tokens: ~200
---

## Deploy zap

Deploy = `cargo build --release` + install to `~/.cargo/bin/zap` and `~/.local/bin/zap` + codesign on macOS.

The build takes 2-3 minutes and exceeds the shell timeout — **always run as a background process**:

### Step 1 — pre-flight (fast, run inline)

```bash
git status && grep '^version' Cargo.toml
```

Confirm: version bumped, changes committed.

### Step 2 — launch deploy in background

```bash
nohup bash scripts/deploy.sh > /tmp/zap-deploy.log 2>&1 & echo "deploy PID: $!"
```

Returns instantly. Build runs in background.

### Step 3 — show initial output

```bash
sleep 3 && tail -40 /tmp/zap-deploy.log
```

### Monitor until done

```bash
tail -f /tmp/zap-deploy.log
```

### Check installed versions (no build)

```bash
bash scripts/deploy.sh --check
```
