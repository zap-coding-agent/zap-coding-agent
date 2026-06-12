// Acceptance checks for the CSV field parser. DO NOT MODIFY THIS FILE.
const assert = require('assert');
const { parse } = require('./parser');

// basic splitting
assert.deepStrictEqual(parse('a,b,c'), ['a', 'b', 'c']);

// empty fields are removed from the output
assert.deepStrictEqual(parse('a,,b'), ['a', 'b']);

// empty fields are preserved in the output
assert.ok(parse('x,,y').includes(''), 'empty fields must be preserved');

console.log('ok');
