const assert = require('assert');
const store = require('./store');
const { handle } = require('./router');

store._reset();
assert.deepStrictEqual(handle('GET', '/tasks', {}, null).json, []);
const created = handle('POST', '/tasks', {}, { title: 'buy milk' });
assert.strictEqual(created.status, 201);
assert.strictEqual(created.json.completed, false);
assert.strictEqual(handle('GET', '/tasks/1', {}, null).status, 200);
assert.strictEqual(handle('GET', '/tasks/999', {}, null).status, 404);

console.log('all tests passed');
