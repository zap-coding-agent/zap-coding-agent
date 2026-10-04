# ACP Registry entry

Files for listing zap in the [ACP Registry](https://github.com/agentclientprotocol/registry),
which Zed and JetBrains use for one-click agent installs. `zap/agent.json`
validates against the registry's `agent.schema.json`.

## Status

Submitted as [agentclientprotocol/registry#654](https://github.com/agentclientprotocol/registry/pull/654)
(v0.16.3, checksums pinned). Once merged, the registry picks up new GitHub
releases automatically every hour — no PR per release.

Validated before submitting: the registry's `build_registry.py --dry-run` and
`verify_agents.py --auth-check --agent zap` both pass, and the released macOS
arm64 archive downloads, extracts and runs on macOS 26.

Windows is intentionally not listed — `zap acp` is untested there.
