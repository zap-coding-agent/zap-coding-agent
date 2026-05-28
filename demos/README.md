# zap — Capability Demos

Runnable demos showing what separates zap from generic "chat + tools" agents.
Each folder is a self-contained capability showcase using a real public repo.

## Prerequisites

- `zap` built and on `$PATH` (`cargo install --path .` from the repo root)
- An API key configured in `~/.agent.toml` (Deepseek, Anthropic, or any OpenAI-compatible provider)
- `git`, `python3` (for output formatting)

## Demos

| Folder | Capability | Repo used | What it shows |
|--------|-----------|-----------|---------------|
| [code_indexing/](code_indexing/) | AST code index | pallets/flask (Python) | LLM finds symbols in 1-2 tool calls instead of 6-10 blind file reads |

## How the demos work

Each demo runs `zap --sdk --auto`, which means:
- `--sdk`: reads JSON prompts from stdin, writes JSON to stdout — fully scriptable
- `--auto`: no permission prompts — tool calls execute immediately
- Tool call traces appear on stdout so you can see exactly what the LLM did

The scripts time each scenario and print the tool navigation path so you can
see the difference between indexed and unindexed code navigation.
