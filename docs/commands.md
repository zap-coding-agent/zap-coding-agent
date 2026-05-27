# Commands & Tools

## Usage

```bash
zap                                        # interactive REPL
zap --goal "add tests for src/lib.rs"      # single-shot
zap --goal "..." --output-format json      # JSON output (for piping)
zap --auto --goal "..."                    # skip all permission prompts (CI)
zap --sdk                                  # JSON-lines remote control (stdin/stdout)
```

## Slash commands

| Command | Description |
|---|---|
| `/help` | Show all commands |
| `/config` | Show active provider, model, URL, sub-agent depth |
| `/cost` | Token usage and estimated cost for this session |
| `/history` | Show message count |
| `/clear` | Clear conversation history |
| `/compact` | Model summarises history in-place to free context |
| `/sessions [N]` | Browse and resume old sessions (fuzzy picker) |
| `/model <id>` | Switch model mid-session |
| `/models` | List models on your LM Studio / Ollama server |
| `/provider` | Switch provider interactively |
| `/permissions ask\|auto\|deny` | Change permission mode for this session |
| `/index [path\|stats]` | Reindex AST code symbols |
| `/index quality` | Code quality report: god objects, coupling, dead code, quality score |
| `/deploy [--check]` | Build and install zap with live streaming output, no timeout |
| `/undo [file]` | Undo the last file edit |
| `/init` | Create a `ZAP.md` for this project (auto-filled by the agent) |
| `/run <workflow>` | Run a `.zap/workflows/<name>.yaml` pipeline |
| `/workflow new <name>` | Scaffold a new workflow file |
| `/tasks` | Browse and execute structured task sessions from `.zap/tasks/` |
| `/attach <path>` | Stage an image for the next message |
| `/paste` | Paste an image from the clipboard |
| `/memory list\|get\|set\|del` | Manage persistent key-value memory |
| `/skill list\|show\|create\|log` | Manage skills; `log` shows which skills fired per turn |
| `/skill scope` | Show or change which domain skills are active for this session |
| `/hooks` | List all configured hooks and their trigger events |
| `/branch <name>` | Fork the current conversation |
| `/branches` | List all conversation branches |
| `/switch <name>` | Switch to a different branch |
| `/audit [N]` | Show last N audit log lines |
| `/think [on\|off\|N]` | Toggle Anthropic extended thinking with configurable token budget |
| `/goal <condition>` | Run turns automatically until the model signals done |
| `/remote` | Start a local HTTP server + public tunnel for remote control |
| `/exit` | Quit |

## Built-in tools

| Tool | What it does |
|---|---|
| `read_file` | Read with optional offset/limit, output prefixed with line numbers |
| `edit_file` | Surgical find-and-replace (rejects ambiguous matches) |
| `batch_edit` | Multiple edits to one file in a single validated call |
| `write_file` | Write or overwrite a file |
| `undo_edit` | Restore a file to its pre-edit snapshot |
| `shell` | Run a shell command (approval required in `ask` mode) |
| `git_status` | Git status + recent log |
| `search_code` | Ripgrep (falls back to grep) with file-type filter and context lines |
| `list_directory` | `ls -la` |
| `glob_read` | List/preview files matching a glob pattern |
| `code_map` | AST-backed structural outline — functions, structs, classes, line numbers |
| `find_definition` | Jump to where a symbol is defined (AST index → ripgrep fallback) |
| `find_references` | All call sites of a symbol across the codebase |
| `web_fetch` | Fetch a URL, strip HTML, return readable text |
| `web_search` | DuckDuckGo search — no API key required |
| `spawn_agent` | Spawn a parallel sub-agent with its own tool loop |
