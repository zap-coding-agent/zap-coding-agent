// Verification tests for the Express app. DO NOT MODIFY THIS FILE.
// This script starts and stops its own server, so it always runs against
// the latest version of app.js.

const http = require('http');
const { spawn } = require('child_process');

const PORT = 3456;

function get(path) {
  return new Promise((resolve, reject) => {
    http.get('http://localhost:' + PORT + path, (res) => {
      let data = '';
      res.on('data', (c) => data += c);
      res.on('end', () => {
        try { resolve({ status: res.statusCode, body: JSON.parse(data) }); }
        catch (e) { resolve({ status: res.statusCode, body: data }); }
      });
    }).on('error', reject);
  });
}

async function run() {
  const assert = require('assert');

  // Start the server
  const server = spawn('node', ['app.js'], {
    env: { ...process.env, PORT: String(PORT) },
    stdio: 'pipe',
  });

  // Wait for server to be ready
  await new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error('server start timeout')), 5000);
    server.stdout.on('data', (data) => {
      if (data.toString().includes('listening')) { clearTimeout(timeout); resolve(); }
    });
  });

  try {
    // Test 1: /health still works
    const health = await get('/health');
    assert.strictEqual(health.status, 200, '/health should return 200');
    assert.deepStrictEqual(health.body, { status: 'ok' }, '/health should return status ok');

    // Test 2: /todos returns an array
    const todos = await get('/todos');
    assert.strictEqual(todos.status, 200, '/todos should return 200');
    assert.ok(Array.isArray(todos.body), '/todos should return an array');
    assert.ok(todos.body.length >= 3, '/todos should have at least 3 items');

    // Test 3: each todo has correct shape
    todos.body.forEach((t, i) => {
      assert.ok(typeof t.id === 'number', 'todo[' + i + '] missing id');
      assert.ok(typeof t.title === 'string', 'todo[' + i + '] missing title');
      assert.ok(typeof t.done === 'boolean', 'todo[' + i + '] missing done');
    });

    console.log('ok');
  } finally {
    server.kill();
  }
}

run().catch((e) => { console.error(e.message); process.exit(1); });
