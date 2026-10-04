# ACP Registry entry

Files for listing zap in the [ACP Registry](https://github.com/agentclientprotocol/registry),
which Zed and JetBrains use for one-click agent installs. `zap/agent.json`
validates against the registry's `agent.schema.json`.

## Submitting (after the v0.16.3 GitHub release exists)

1. Check the archive URLs in `zap/agent.json` resolve (they point at the
   `v0.16.3` release assets).
2. Optionally pin checksums — add `"sha256"` to each platform entry:
   `curl -sL <archive-url> | shasum -a 256`.
3. Fork `agentclientprotocol/registry`, copy the `zap/` folder to the repo root,
   and open a PR. CI validates the JSON and the icon.

Each new zap release needs a registry PR bumping `version` and the archive URLs.

## Before submitting, verify

- **macOS Gatekeeper:** the registry downloads the release archive and runs
  `./zap` directly. On macOS 26 unsigned/ad-hoc binaries downloaded by an app
  can be killed (`Code Signature Invalid`). Install via the registry on a clean
  Mac and confirm it launches before submitting.
- **Windows** is intentionally not listed — `zap acp` is untested there.
