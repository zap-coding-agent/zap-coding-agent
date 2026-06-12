#!/usr/bin/env node
// Test 2 — realistic multi-file feature, executed by an SLM from a frontier-authored plan.
//
// Premise (the zap thesis): a frontier model decomposes a real feature into scoped,
// verifiable steps (an impl plan written so an SLM can execute it). Each step is then
// run as its own scoped agent loop against the SAME persistent project — exactly how zap
// would dispatch a tasks.md. We verify each step objectively by running the code.
//
// Usage:  node test2.mjs <model> [--verbose]

import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { execSync } from "node:child_process";

const BASE = process.env.LMSTUDIO_URL || "http://localhost:1234/v1/chat/completions";
const MAX_TURNS = 16;
const SANDBOX = path.join(os.tmpdir(), "slm_eval_test2");

// ── seed project: a zero-dep Node "tasks" REST API ──────────────────────────
const SEED = {
  "store.js":
    "let tasks = [];\n" +
    "let nextId = 1;\n\n" +
    "function list() { return tasks; }\n" +
    "function get(id) { return tasks.find((t) => t.id === id) || null; }\n" +
    "function add(title) {\n" +
    "  const task = { id: nextId++, title, completed: false };\n" +
    "  tasks.push(task);\n" +
    "  return task;\n" +
    "}\n" +
    "function _reset() { tasks = []; nextId = 1; }\n\n" +
    "module.exports = { list, get, add, _reset };\n",
  "router.js":
    "const store = require('./store');\n\n" +
    "// handle(method, path, query, body) -> { status, json }\n" +
    "function handle(method, urlPath, query, body) {\n" +
    "  const idMatch = urlPath.match(/^\\/tasks\\/(\\d+)$/);\n" +
    "  if (method === 'GET' && urlPath === '/tasks') {\n" +
    "    return { status: 200, json: store.list() };\n" +
    "  }\n" +
    "  if (method === 'POST' && urlPath === '/tasks') {\n" +
    "    if (!body || typeof body.title !== 'string' || body.title === '') {\n" +
    "      return { status: 400, json: { error: 'title required' } };\n" +
    "    }\n" +
    "    return { status: 201, json: store.add(body.title) };\n" +
    "  }\n" +
    "  if (method === 'GET' && idMatch) {\n" +
    "    const task = store.get(Number(idMatch[1]));\n" +
    "    return task ? { status: 200, json: task } : { status: 404, json: { error: 'not found' } };\n" +
    "  }\n" +
    "  return { status: 404, json: { error: 'no route' } };\n" +
    "}\n\n" +
    "module.exports = { handle };\n",
  "server.js":
    "const http = require('http');\n" +
    "const { handle } = require('./router');\n\n" +
    "const server = http.createServer((req, res) => {\n" +
    "  const u = new URL(req.url, 'http://localhost');\n" +
    "  const query = Object.fromEntries(u.searchParams);\n" +
    "  let raw = '';\n" +
    "  req.on('data', (c) => (raw += c));\n" +
    "  req.on('end', () => {\n" +
    "    const body = raw ? JSON.parse(raw) : null;\n" +
    "    const { status, json } = handle(req.method, u.pathname, query, body);\n" +
    "    res.writeHead(status, { 'Content-Type': 'application/json' });\n" +
    "    res.end(JSON.stringify(json));\n" +
    "  });\n" +
    "});\n\n" +
    "if (require.main === module) server.listen(3000);\n" +
    "module.exports = { server };\n",
  "test.js":
    "const assert = require('assert');\n" +
    "const store = require('./store');\n" +
    "const { handle } = require('./router');\n\n" +
    "store._reset();\n" +
    "assert.deepStrictEqual(handle('GET', '/tasks', {}, null).json, []);\n" +
    "const created = handle('POST', '/tasks', {}, { title: 'buy milk' });\n" +
    "assert.strictEqual(created.status, 201);\n" +
    "assert.strictEqual(created.json.completed, false);\n" +
    "assert.strictEqual(handle('GET', '/tasks/1', {}, null).status, 200);\n" +
    "assert.strictEqual(handle('GET', '/tasks/999', {}, null).status, 404);\n\n" +
    "console.log('all tests passed');\n",
};

// ── the frontier-authored plan (SLM-sized, each step names file+signature+behavior) ──
const PLAN = [
  {
    id: 1, title: "store.update(id, patch)",
    instruction:
      "In store.js, add and export a new function `update(id, patch)`. It must find the task " +
      "whose `id` equals the given id, merge the key/values from the `patch` object into that " +
      "task (mutating it in place), and return the updated task object. If no task has that id, " +
      "return null. Add `update` to the module.exports. Do not change the other functions.",
    verify: (sb) => nodeEval(sb,
      "const s=require('./store'); s._reset(); s.add('x'); " +
      "const u=s.update(1,{completed:true}); const miss=s.update(99,{completed:true}); " +
      "console.log(u && u.completed===true, miss===null)",
      "true true"),
  },
  {
    id: 2, title: "PATCH /tasks/:id",
    instruction:
      "In router.js, add handling for `PATCH /tasks/:id`. Parse the numeric id from the path " +
      "(same /^\\/tasks\\/(\\d+)$/ shape already used for GET). If the request body contains a " +
      "boolean `completed`, call store.update(id, { completed: body.completed }). If a task was " +
      "found, return { status: 200, json: updatedTask }. If not found, return " +
      "{ status: 404, json: { error: 'not found' } }. Keep every existing route working.",
    verify: (sb) => nodeEval(sb,
      "const s=require('./store'); const {handle}=require('./router'); s._reset(); s.add('x'); " +
      "const ok=handle('PATCH','/tasks/1',{},{completed:true}); " +
      "const miss=handle('PATCH','/tasks/9',{},{completed:true}); " +
      "console.log(ok.status, ok.json.completed, miss.status)",
      "200 true 404"),
  },
  {
    id: 3, title: "GET /tasks?status=open|done filter",
    instruction:
      "Extend the existing `GET /tasks` handler in router.js so it honors a `query.status` filter. " +
      "When query.status === 'open', return only tasks whose completed is false. When " +
      "query.status === 'done', return only tasks whose completed is true. When status is absent " +
      "or anything else, return all tasks (current behavior). Status code stays 200.",
    verify: (sb) => nodeEval(sb,
      "const s=require('./store'); const {handle}=require('./router'); s._reset(); " +
      "s.add('a'); s.add('b'); s.update(1,{completed:true}); " +
      "const done=handle('GET','/tasks',{status:'done'},null).json.length; " +
      "const open=handle('GET','/tasks',{status:'open'},null).json.length; " +
      "const all=handle('GET','/tasks',{},null).json.length; " +
      "console.log(done, open, all)",
      "1 1 2"),
  },
  {
    id: 4, title: "tests for the new behavior",
    instruction:
      // NOTE: this step was REFINED after Test-2 round 1, where an under-specified version " +
      // ('add assertions covering ...') caused the model to break shared test state and thrash.
      // The fix is a tighter, state-explicit plan — the 'frontier writes SLM-friendly steps' principle.
      "Add NEW assertions to the END of test.js. Do NOT modify or reorder the existing assertions, " +
      "and keep `console.log('all tests passed')` as the very last line. Immediately before your " +
      "new assertions, call store._reset() to start from a clean state, then create exactly one " +
      "task with `handle('POST','/tasks',{},{title:'t'})` (it will have id 1). Then add these " +
      "assertions: (1) handle('PATCH','/tasks/1',{},{completed:true}).status === 200 AND its " +
      ".json.completed === true; (2) handle('PATCH','/tasks/999',{},{completed:true}).status === 404; " +
      "(3) handle('GET','/tasks',{status:'done'},null).json.length === 1. Then run `node test.js` " +
      "and confirm it prints 'all tests passed'.",
    verify: (sb) => {
      try {
        const out = execSync("node test.js", { cwd: sb, encoding: "utf8" }).trim();
        const t = read(sb, "test.js");
        if (!out.includes("all tests passed")) return [false, `suite did not pass: ${out}`];
        if (!/PATCH/.test(t) || !/status=done|status':\s*'done'|status:\s*'done'/.test(t))
          return [false, "new assertions (PATCH / status=done) not found in test.js"];
        return [true, "suite green with new PATCH + filter assertions"];
      } catch (e) { return [false, `suite failed: ${(e.stderr || e.stdout || "").toString().trim()}`]; }
    },
  },
];

// ── shared helpers ──────────────────────────────────────────────────────────
const read = (sb, f) => { try { return fs.readFileSync(path.join(sb, f), "utf8"); } catch { return ""; } };
function nodeEval(sb, code, want) {
  try {
    const out = execSync(`node -e ${JSON.stringify(code)}`, { cwd: sb, encoding: "utf8" }).trim();
    return out === want ? [true, `output '${out}'`] : [false, `got '${out}' want '${want}'`];
  } catch (e) { return [false, `crashed: ${(e.stderr || e.stdout || e.message).toString().trim().slice(0, 200)}`]; }
}

const TOOL_DEFS = [
  { type: "function", function: { name: "list_dir", description: "List files in the project root.",
    parameters: { type: "object", properties: { path: { type: "string" } }, required: ["path"] } } },
  { type: "function", function: { name: "read_file", description: "Read a file's full contents.",
    parameters: { type: "object", properties: { path: { type: "string" } }, required: ["path"] } } },
  { type: "function", function: { name: "edit_file",
    description: "Replace an exact substring in a file. old_string must occur exactly once.",
    parameters: { type: "object", properties: { path: { type: "string" }, old_string: { type: "string" }, new_string: { type: "string" } }, required: ["path", "old_string", "new_string"] } } },
  { type: "function", function: { name: "write_file", description: "Overwrite a file with full new contents.",
    parameters: { type: "object", properties: { path: { type: "string" }, content: { type: "string" } }, required: ["path", "content"] } } },
  { type: "function", function: { name: "run_cmd", description: "Run a shell command in the project root.",
    parameters: { type: "object", properties: { command: { type: "string" } }, required: ["command"] } } },
];

function safe(p) { const full = path.resolve(SANDBOX, p || "."); if (!full.startsWith(path.resolve(SANDBOX))) throw new Error("escape"); return full; }
function runTool(name, args) {
  try {
    if (name === "list_dir") return fs.readdirSync(safe(args.path)).sort().join("\n");
    if (name === "read_file") return fs.readFileSync(safe(args.path), "utf8");
    if (name === "edit_file") {
      const p = safe(args.path); const c = fs.readFileSync(p, "utf8");
      const { old_string: o, new_string: n } = args;
      if (o === undefined || n === undefined) return "ERROR: missing old_string/new_string";
      const cnt = c.split(o).length - 1;
      if (cnt === 0) return "ERROR: old_string not found — re-read the file to see its exact current text before editing";
      if (cnt > 1) return `ERROR: old_string appears ${cnt} times, must be unique`;
      fs.writeFileSync(p, c.replace(o, n)); return "OK: edited";
    }
    if (name === "write_file") { fs.writeFileSync(safe(args.path), args.content ?? ""); return "OK: wrote file"; }
    if (name === "run_cmd") {
      try { return "exit=0\n" + execSync(args.command, { cwd: SANDBOX, timeout: 30000, encoding: "utf8" }); }
      catch (e) { return `exit=${e.status ?? 1}\n${e.stdout || ""}${e.stderr || ""}`; }
    }
  } catch (e) { return `ERROR: ${e.message}`; }
  return "ERROR: unknown tool";
}

async function post(payload) {
  const r = await fetch(BASE, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(payload) });
  if (!r.ok) throw new Error(`HTTP ${r.status}: ${(await r.text()).slice(0, 200)}`);
  return r.json();
}

// ── run one plan step as its own scoped agent loop ──────────────────────────
async function runStep(model, step, verbose) {
  const system =
    "You are a coding agent working in a small Node.js project (zero dependencies). You are " +
    "implementing ONE step of a larger implementation plan. Use the tools to read and edit files " +
    "and run commands. ALWAYS read a file before editing it. If an edit fails, re-read the file " +
    "and try again with the exact current text. When the step is done and verified, reply with a " +
    "short final summary. Do not ask questions.";
  const messages = [{ role: "system", content: system },
                    { role: "user", content: `Plan step ${step.id}: ${step.title}\n\n${step.instruction}` }];
  const log = [];
  let toolCalls = 0, turn = 0, genMs = 0, compTokens = 0, promptTokens = 0;
  const failSig = new Map(); // loop-breaker: identical failing tool calls
  const t0task = performance.now();

  for (turn = 0; turn < MAX_TURNS; turn++) {
    let resp;
    try {
      const t0 = performance.now();
      resp = await post({ model, messages, tools: TOOL_DEFS, temperature: 0 });
      genMs += performance.now() - t0;
      const u = resp.usage || {}; compTokens += u.completion_tokens || 0; promptTokens += u.prompt_tokens || 0;
    } catch (e) { log.push(`[t${turn}] API ERROR ${e.message}`); break; }
    const msg = resp.choices[0].message; const fr = resp.choices[0].finish_reason;
    const tcs = msg.tool_calls || [];
    messages.push({ role: "assistant", content: msg.content, tool_calls: tcs.length ? tcs : undefined });
    if (!tcs.length) { log.push(`[t${turn}] FINAL (${fr}): ${(msg.content || "").slice(0, 120)}`); break; }
    let broke = false;
    for (const tc of tcs) {
      toolCalls++;
      const fn = tc.function.name; let a = {};
      try { a = JSON.parse(tc.function.arguments || "{}"); } catch {}
      const result = runTool(fn, a);
      const shown = Object.fromEntries(Object.entries(a).map(([k, v]) => [k, typeof v === "string" && v.length > 32 ? v.slice(0, 32) + "…" : v]));
      log.push(`[t${turn}] ${fn}(${JSON.stringify(shown)}) -> ${(result.split("\n")[0] || "").slice(0, 70)}`);
      // loop-breaker (a tuning lever from Test 1): identical failing call 3x => abort step
      if (result.startsWith("ERROR")) {
        const sig = fn + JSON.stringify(a);
        failSig.set(sig, (failSig.get(sig) || 0) + 1);
        if (failSig.get(sig) >= 3) { log.push(`[t${turn}] LOOP-BREAKER: identical failing call x3, aborting step`); broke = true; }
      }
      messages.push({ role: "tool", tool_call_id: tc.id, content: result });
    }
    if (broke) break;
  }
  const wallSec = (performance.now() - t0task) / 1000;
  const tokps = genMs > 0 ? compTokens / (genMs / 1000) : 0;
  const [ok, why] = step.verify(SANDBOX);
  if (verbose) log.forEach((l) => console.log("      " + l));
  return { ok, why, turns: Math.min(turn + 1, MAX_TURNS), toolCalls, wallSec, tokps, compTokens };
}

// ── main ────────────────────────────────────────────────────────────────────
const model = process.argv[2];
const verbose = process.argv.includes("--verbose");
const onlyArg = process.argv.find((a) => a.startsWith("--only="));
const only = onlyArg ? Number(onlyArg.split("=")[1]) : null;

if (only) {
  // Re-run a single step against the project as a prior run left it (preserves the
  // model's feature code in store.js/router.js). Reset test.js to a clean baseline so
  // the test-authoring step starts from a passing suite.
  if (!fs.existsSync(SANDBOX)) { console.error("no prior sandbox; run a full pass first"); process.exit(1); }
  fs.writeFileSync(path.join(SANDBOX, "test.js"), SEED["test.js"]);
} else {
  fs.rmSync(SANDBOX, { recursive: true, force: true });
  fs.mkdirSync(SANDBOX, { recursive: true });
  for (const [f, c] of Object.entries(SEED)) fs.writeFileSync(path.join(SANDBOX, f), c);
}

console.log(`\nTest 2 — tasks REST API feature · model: ${model}${only ? ` · only step ${only}` : ""}\n`);
let passed = 0, totWall = 0, totTok = 0;
const steps = only ? PLAN.filter((s) => s.id === only) : PLAN;
const rows = [];
for (const step of steps) {
  const r = await runStep(model, step, verbose);
  const status = r.ok ? "✅" : "❌";
  console.log(`  ${status} step ${step.id} (${step.title})  —  ${r.turns} turns, ${r.toolCalls} tools, ${r.wallSec.toFixed(1)}s, ${r.tokps.toFixed(0)} tok/s  —  ${r.why}`);
  rows.push({ step: step.id, title: step.title, ok: r.ok, ...r });
  if (r.ok) passed++;
  totWall += r.wallSec; totTok += r.compTokens;
}

// final: full regression — does the whole feature hang together?
let finalOk = false, finalMsg = "";
try {
  const out = execSync("node test.js", { cwd: SANDBOX, encoding: "utf8" }).trim();
  finalOk = out.includes("all tests passed"); finalMsg = out.split("\n").pop();
} catch (e) { finalMsg = (e.stderr || "").toString().trim().slice(0, 160); }

console.log(`\n  ── summary ──`);
console.log(`  steps passed:   ${passed}/${steps.length}`);
console.log(`  full suite:     ${finalOk ? "✅ green" : "❌ " + finalMsg}`);
console.log(`  total LLM time: ${totWall.toFixed(1)}s   ·   total output tokens: ${totTok}`);
console.log(`  avg throughput: ${totWall > 0 ? (totTok / totWall).toFixed(0) : 0} tok/s (incl. prompt processing)\n`);
