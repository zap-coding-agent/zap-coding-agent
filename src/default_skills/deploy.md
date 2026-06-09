---
category: practice
name: deploy
trigger: ["deploy", "deployment", "release version", "ship it", "publish package", "publish release", "install locally", "update zap", "build zap", "cut a release", "release build", "deploy website", "update website", "deploy to cloudflare", "push website"]
tokens: ~300
---

## Deploy zap

Two deployment targets: **binary** (local install) and **website** (Cloudflare Pages).

---

### A) Binary — local install

Deploy = `cargo build --release` + install to `~/.cargo/bin/zap` and `~/.local/bin/zap` + codesign on macOS.

The build takes 2-3 minutes and exceeds the shell timeout — **always run as a background process**:

#### Step 1 — pre-flight (fast, run inline)

```bash
git status && grep '^version' Cargo.toml
```

Confirm: version bumped, changes committed.

#### Step 2 — launch deploy in background

```bash
nohup bash scripts/deploy.sh > /tmp/zap-deploy.log 2>&1 & echo "deploy PID: $!"
```

Returns instantly. Build runs in background.

#### Step 3 — show initial output

```bash
sleep 3 && tail -40 /tmp/zap-deploy.log
```

#### Monitor until done

```bash
tail -f /tmp/zap-deploy.log
```

#### Check installed versions (no build)

```bash
bash scripts/deploy.sh --check
```

---

### B) Website — Cloudflare Pages

Site lives in `website/` and deploys to `zap.justpush.cloud`.

```bash
CLOUDFLARE_API_TOKEN=<pages-token> \
CLOUDFLARE_ACCOUNT_ID=72dde18fd67721aed830b1e138963cb2 \
npx wrangler pages deploy website --project-name=zap --commit-dirty=true
```

Always commit website changes to git first, then deploy.

Account ID: `72dde18fd67721aed830b1e138963cb2`
Project: `zap` → `zap-93s.pages.dev` → `zap.justpush.cloud`
Token: stored in local env / password manager (not committed)
