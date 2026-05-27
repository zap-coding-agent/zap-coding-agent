# MCP Support — Lazy-Loaded, Cross-Agent Compatible

MCP (Model Context Protocol) is an open standard. **Any MCP server you configure in zap also works in Claude Code, Cursor, Kiro, and other agents** — the config format is shared. zap adds two optional fields (`description`, `toolsHint`) that other agents silently ignore, so your config file is fully portable.

## Config file locations

| File | Scope |
|---|---|
| `~/.zap/mcp.json` | Global — applies to every session |
| `.mcp.json` (project root) | Project-local — checked into git, takes precedence |

## How lazy loading works

Most agents connect to every configured server at startup and dump all tool schemas into the LLM's context on every turn. Ten servers × five tools each = 10,000+ wasted tokens per request, whether you use them or not.

zap keeps every server in a **pending** state at startup. Instead of their tool schemas, the LLM gets one lightweight stub:

```
mcp_connect(server)
  - filesystem: Read/write files in /tmp and the project  [tools: read_file, write_file, list_directory…]
  - fetch: Fetch web pages as markdown                    [tools: fetch]
  - memory: Persistent knowledge graph                    [tools: create_entities, search_nodes…]
```

When the LLM decides it needs a server, it calls `mcp_connect("filesystem")`. zap spawns the process, runs the handshake, fetches the real `tools/list`, and registers those tools — all within the same agentic turn.

| Stage | Other agents | zap |
|---|---|---|
| Startup | Spawns all server processes | Reads config only — zero processes |
| LLM tool list per turn | All tool schemas, always | One `mcp_connect` stub until needed |
| First use of a server | Already connected | Spawns process on demand, ~200 ms |
| After first use | — | Real schemas in context, `mcp_connect` gone |

## Sample `~/.zap/mcp.json`

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp", "/home/user/project"],
      "description": "Read and write files inside /tmp and the project directory",
      "toolsHint": "read_file, write_file, edit_file, list_directory, search_files"
    },
    "fetch": {
      "command": "uvx",
      "args": ["mcp-server-fetch"],
      "description": "Fetch any URL and return it as clean markdown",
      "toolsHint": "fetch"
    },
    "memory": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-memory"],
      "description": "Persistent knowledge graph — entities and relations survive across sessions",
      "toolsHint": "create_entities, create_relations, search_nodes, read_graph"
    },
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": { "GITHUB_TOKEN": "ghp_your_token_here" },
      "description": "GitHub repos, issues, pull requests, and code search",
      "toolsHint": "create_issue, create_pull_request, search_code, list_commits"
    }
  }
}
```

## Fields

| Field | Required | Portable | Description |
|---|---|---|---|
| `command` | yes | yes | Executable to spawn (`npx`, `uvx`, `node`, absolute path) |
| `args` | yes | yes | Arguments passed to the command |
| `env` | no | yes | Extra environment variables (API keys, tokens) |
| `description` | no | yes* | What this server does — shown in `mcp_connect` stub so the LLM knows when to connect it |
| `toolsHint` | no | zap-only | Comma-separated key tool names — lets the LLM plan without connecting |
| `disabled` | no | yes | Set `true` to skip this server entirely |
| `autoApprove` | no | yes | Tool names to auto-approve (Claude Code convention, parsed but not yet enforced) |

\* `description` is a zap extension. Other agents that don't know the field simply ignore it.

## Commands

```
/mcp list              list all servers — connected, pending, or failed
/mcp edit              open ~/.zap/mcp.json in $EDITOR
/mcp edit project      open .mcp.json (project-level config)
/mcp path              print both config file paths
```

## Installing public MCP servers

The two most useful zero-config servers:

```bash
# filesystem — read/write local files (requires Node)
# add to mcp.json: "command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "/your/allowed/path"]

# fetch — fetch any URL as markdown (requires Python + uv)
# add to mcp.json: "command": "uvx", "args": ["mcp-server-fetch"]
```

Both install automatically on first connect via `npx -y` / `uvx` — no manual `npm install` needed.
