# Demo: Code Indexing

**Capability:** AST-based code index (tree-sitter → SQLite)  
**Repo:** [pallets/flask](https://github.com/pallets/flask) — Python web framework, ~14 k LOC  
**Runtime:** ~30 s per scenario (Deepseek; mostly API latency)

---

## What this demo shows

zap builds a SQLite index of every symbol (functions, classes, methods) in the
project using tree-sitter. When the LLM needs to find code, it queries this
index instead of guessing file names or reading entire directories.

### With index vs without

| Step | Without index | With index |
|------|--------------|------------|
| Find `Flask` class | `list_directory` → read 4 files | `find_definition("Flask")` → 1 result |
| Trace request flow | 8-12 tool calls, ~4k tokens | 3-4 tool calls, ~800 tokens |
| Find a method on Blueprint | grep → false positives | `code_map` → exact line range |

The key: **the LLM doesn't guess**. It asks the index, gets a file + line
number, and reads only the relevant window. Less context wasted = more tokens
available for actual reasoning.

---

## Quick start

```bash
# 1. Clone Flask and build the index
./setup.sh

# 2. Run all three scenarios
./run.sh
```

## Scenarios

| Script | Prompt | What to observe |
|--------|--------|-----------------|
| `scenarios/01_find_class.sh` | "Where is the Flask class defined?" | `find_definition` → exact file:line in 1 call |
| `scenarios/02_trace_request.sh` | "How does a request reach the route handler?" | cross-file trace using index, no blind reads |
| `scenarios/03_blueprint_api.sh` | "What public methods does Blueprint expose?" | `code_map` → structured method list without reading whole file |

---

## Understanding the output

Each scenario prints:
```
[tool calls]  ╭─ find_definition  ...
              ╭─ read_file  ...
[response]    The Flask class is defined in src/flask/app.py at line 97...
[stats]       Input: 6 421 tokens   Output: 312 tokens   Tool calls: 2
```

Compare `Tool calls` and `Input tokens` — indexed navigation consistently
uses 3-5x fewer tool calls than blind file exploration.
