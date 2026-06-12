#!/usr/bin/env node
// SLM agentic-coding stress harness.
//
// Runs a REAL multi-turn agent loop against a local LM Studio model, with real
// filesystem + shell tools operating in an isolated sandbox, on realistic JS
// coding tasks — then verifies the result OBJECTIVELY (harness-side, never
// trusting the model's self-report).
//
// Usage:  node harness.mjs <model> <A|B|C|all> [--verbose]
//
// Why this exists: single-shot tool-call curls prove a model can emit one
// tool call. They do NOT prove it can complete a task — drive a multi-step
// loop, recover from a failed edit, and terminate. This measures completion.

import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { execSync } from "node:child_process";

const BASE = process.env.LMSTUDIO_URL || "http://localhost:1234/v1/chat/completions";
const MAX_TURNS = 14;
const ROOT = path.join(os.tmpdir(), "slm_eval_sandboxes");

// ── sandbox project (fresh copy per run) ────────────────────────────────────
const FILES = {
  "calc.js":
    "function add(a, b) { return a + b; }\n" +
    "function subtract(a, b) { return a - b; }\n" +
    "function divide(a, b) { return a / b; }\n" +
    "module.exports = { add, subtract, divide };\n",
  "greet.js":
    "function greet(name) {\n  return `Hello, ${name}!`;\n}\n" +
    "module.exports = { greet };\n",
  "main.js":
    "const { greet } = require('./greet');\n" +
    "console.log(greet('world'));\n",
  "test_calc.js":
    "const assert = require('assert');\n" +
    "const { add, subtract } = require('./calc');\n" +
    "assert.strictEqual(add(2, 3), 5);\n" +
    "assert.strictEqual(subtract(5, 2), 3);\n" +
    "console.log('all tests passed');\n",
};

const TASKS = {
  A: "Add a function `multiply(a, b)` to calc.js that returns a * b, and make sure it " +
     "is exported from the module. Then add an assertion to test_calc.js that checks " +
     "multiply(3, 4) === 12 (import multiply too). Finally run `node test_calc.js` and " +
     "confirm it prints 'all tests passed'.",
  B: "There is a bug: divide(a, b) in calc.js returns Infinity when b is 0. Fix divide " +
     "so that when b === 0 it returns null instead. Normal division must still work. " +
     "Verify by running: node -e \"const {divide}=require('./calc'); console.log(divide(6,2), divide(1,0))\" " +
     "which should print: 3 null",
  C: "Rename the function `greet` to `welcome` everywhere in the project. It is defined " +
     "and exported in greet.js, and imported + called in main.js. After renaming, run " +
     "`node main.js` and confirm it still prints 'Hello, world!'. No reference to the " +
     "name `greet` may remain in any file.",
};

// ── tools (real, sandbox-jailed) ────────────────────────────────────────────
const TOOL_DEFS = [
  { type: "function", function: { name: "list_dir", description: "List files in a directory.",
    parameters: { type: "object", properties: { path: { type: "string", description: "'.' for project root" } }, required: ["path"] } } },
  { type: "function", function: { name: "read_file", description: "Read a text file's full contents.",
    parameters: { type: "object", properties: { path: { type: "string" } }, required: ["path"] } } },
  { type: "function", function: { name: "edit_file",
    description: "Replace an exact substring in a file. old_string must occur exactly once.",
    parameters: { type: "object", properties: { path: { type: "string" }, old_string: { type: "string" }, new_string: { type: "string" } }, required: ["path", "old_string", "new_string"] } } },
  { type: "function", function: { name: "write_file", description: "Overwrite a file with full new contents.",
    parameters: { type: "object", properties: { path: { type: "string" }, content: { type: "string" } }, required: ["path", "content"] } } },
  { type: "function", function: { name: "run_cmd",
    description: "Run a shell command in the project root; returns stdout+stderr+exit code.",
    parameters: { type: "object", properties: { command: { type: "string" } }, required: ["command"] } } },
];

function safe(sandbox, p) {
  const full = path.resolve(sandbox, p || ".");
  if (!full.startsWith(path.resolve(sandbox))) throw new Error("path escapes sandbox");
  return full;
}

function runTool(sandbox, name, args) {
  try {
    if (name === "list_dir") return fs.readdirSync(safe(sandbox, args.path)).sort().join("\n") || "(empty)";
    if (name === "read_file") return fs.readFileSync(safe(sandbox, args.path), "utf8");
    if (name === "edit_file") {
      const p = safe(sandbox, args.path);
      const c = fs.readFileSync(p, "utf8");
      const { old_string: o, new_string: n } = args;
      if (o === undefined || n === undefined) return "ERROR: missing old_string/new_string";
      const count = c.split(o).length - 1;
      if (count === 0) return "ERROR: old_string not found";
      if (count > 1) return `ERROR: old_string appears ${count} times, must be unique`;
      fs.writeFileSync(p, c.replace(o, n));
      return "OK: edited";
    }
    if (name === "write_file") { fs.writeFileSync(safe(sandbox, args.path), args.content ?? ""); return "OK: wrote file"; }
    if (name === "run_cmd") {
      try {
        const out = execSync(args.command, { cwd: sandbox, timeout: 30000, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
        return `exit=0\n${out}`;
      } catch (e) {
        return `exit=${e.status ?? 1}\nstdout:\n${e.stdout || ""}\nstderr:\n${e.stderr || ""}`;
      }
    }
  } catch (e) { return `ERROR: ${e.message}`; }
  return "ERROR: unknown tool";
}

// ── objective verification (does NOT trust the model) ───────────────────────
function nodeEval(sandbox, code) {
  try { return { ok: true, out: execSync(`node -e ${JSON.stringify(code)}`, { cwd: sandbox, encoding: "utf8" }).trim() }; }
  catch (e) { return { ok: false, out: (e.stderr || e.stdout || e.message).toString().trim() }; }
}
function rd(sandbox, f) { try { return fs.readFileSync(path.join(sandbox, f), "utf8"); } catch { return ""; } }

function verify(task, sandbox) {
  if (task === "A") {
    if (!/multiply/.test(rd(sandbox, "calc.js"))) return [false, "multiply not in calc.js"];
    try {
      const out = execSync("node test_calc.js", { cwd: sandbox, encoding: "utf8" }).trim();
      if (!out.includes("all tests passed")) return [false, `test output: ${out}`];
    } catch (e) { return [false, `tests failed: ${(e.stderr || e.stdout || "").toString().trim()}`]; }
    // A passing run that imports+exercises multiply is the real bar; don't over-fit
    // on the exact assertion spelling.
    if (!/multiply/.test(rd(sandbox, "test_calc.js"))) return [false, "multiply not tested in test_calc.js"];
    return [true, "multiply added + exported, tested, all tests pass"];
  }
  if (task === "B") {
    const r = nodeEval(sandbox, "const {divide}=require('./calc'); console.log(divide(6,2), divide(1,0))");
    if (!r.ok) return [false, `crashed: ${r.out}`];
    if (r.out !== "3 null") return [false, `wrong output: ${JSON.stringify(r.out)} (want '3 null')`];
    return [true, "divide(6,2)=3, divide(1,0)=null"];
  }
  if (task === "C") {
    let out;
    try { out = execSync("node main.js", { cwd: sandbox, encoding: "utf8" }).trim(); }
    catch (e) { return [false, `main.js crashes: ${(e.stderr || "").toString().trim()}`]; }
    if (out !== "Hello, world!") return [false, `wrong output: ${JSON.stringify(out)}`];
    // The task renames the FUNCTION, not the file. `require('./greet')` legitimately
    // keeps the filename — strip the module-path string before checking for leftovers.
    const stripPath = (s) => s.replace(/(['"])\.\/greet\1/g, "''");
    const leftover = ["greet.js", "main.js"].filter((f) => /\bgreet\b/.test(stripPath(rd(sandbox, f))));
    if (leftover.length) return [false, `function 'greet' still referenced in ${leftover.join(", ")}`];
    return [true, "renamed to welcome, main.js prints correctly, no 'greet' left"];
  }
  return [false, "unknown task"];
}

// ── agent loop ──────────────────────────────────────────────────────────────
async function post(payload) {
  const r = await fetch(BASE, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(payload) });
  if (!r.ok) throw new Error(`HTTP ${r.status}: ${(await r.text()).slice(0, 200)}`);
  return r.json();
}

async function run(model, task, verbose) {
  const sandbox = path.join(ROOT, `sb_${task}_${model.replace(/[^a-z0-9]/gi, "_")}`);
  fs.rmSync(sandbox, { recursive: true, force: true });
  fs.mkdirSync(sandbox, { recursive: true });
  for (const [fn, c] of Object.entries(FILES)) fs.writeFileSync(path.join(sandbox, fn), c);

  const system = "You are a coding agent working in a small Node.js project. Use the provided " +
    "tools to inspect and edit files and run commands. Make the change, verify it by running it, " +
    "then reply with a short final summary. Do not ask the user questions.";
  const messages = [{ role: "system", content: system }, { role: "user", content: TASKS[task] }];
  const log = [];
  let toolCalls = 0, turn = 0, leak = false;
  let genMs = 0, compTokens = 0, promptTokens = 0; // perf accounting
  const taskStart = performance.now();

  for (turn = 0; turn < MAX_TURNS; turn++) {
    let resp;
    try {
      const t0 = performance.now();
      resp = await post({ model, messages, tools: TOOL_DEFS, temperature: 0 });
      genMs += performance.now() - t0;
      const u = resp.usage || {};
      compTokens += u.completion_tokens || 0;
      promptTokens += u.prompt_tokens || 0;
    }
    catch (e) { log.push(`[turn ${turn}] API ERROR: ${e.message}`); break; }
    const msg = resp.choices[0].message;
    const fr = resp.choices[0].finish_reason;
    const tcs = msg.tool_calls || [];
    const content = msg.content || "";
    if (!tcs.length && /"(name|arguments|tool_call)"\s*:/.test(content)) { leak = true; log.push(`[turn ${turn}] ⚠ PLAINTEXT tool-call leak`); }
    messages.push({ role: "assistant", content: msg.content, tool_calls: tcs.length ? tcs : undefined });
    if (!tcs.length) { log.push(`[turn ${turn}] FINAL (${fr}): ${content.slice(0, 140)}`); break; }
    for (const tc of tcs) {
      toolCalls++;
      const fname = tc.function.name;
      let fargs = {};
      try { fargs = JSON.parse(tc.function.arguments || "{}"); } catch (e) { log.push(`[turn ${turn}] BAD JSON args for ${fname}`); }
      const result = runTool(sandbox, fname, fargs);
      const shown = Object.fromEntries(Object.entries(fargs).map(([k, v]) => [k, typeof v === "string" && v.length > 36 ? v.slice(0, 36) + "…" : v]));
      log.push(`[turn ${turn}] ${fname}(${JSON.stringify(shown)}) -> ${(result.split("\n")[0] || "").slice(0, 76)}`);
      messages.push({ role: "tool", tool_call_id: tc.id, content: result });
    }
  }
  if (turn === MAX_TURNS) log.push(`[turn cap ${MAX_TURNS} hit — no final answer]`);

  const wallSec = (performance.now() - taskStart) / 1000;
  const tokps = genMs > 0 ? compTokens / (genMs / 1000) : 0; // effective output tok/s
  const [ok, why] = verify(task, sandbox);
  if (verbose) log.forEach((l) => console.log("    " + l));
  return { ok, why, turns: Math.min(turn + 1, MAX_TURNS), toolCalls, leak, log,
           wallSec, tokps, compTokens, promptTokens };
}

// ── main ────────────────────────────────────────────────────────────────────
const model = process.argv[2];
const which = process.argv[3] || "all";
const verbose = process.argv.includes("--verbose");
const tasks = which === "all" ? ["A", "B", "C"] : [which];

for (const t of tasks) {
  const r = await run(model, t, verbose);
  const status = r.ok ? "✅ PASS" : "❌ FAIL";
  console.log(`${status}  task ${t}  ${model}  (${r.turns} turns, ${r.toolCalls} tool calls, ` +
    `${r.wallSec.toFixed(1)}s, ${r.tokps.toFixed(0)} tok/s, ${r.compTokens} out-tok` +
    `${r.leak ? ", PLAINTEXT-LEAK" : ""})  — ${r.why}\n`);
}
