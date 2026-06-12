# Task

## IMPORTANT: Read this entire plan before taking any action.

## Instructions (follow exactly, in order)

### Step 1 — Read the current code
Read app.js to understand the existing structure.

### Step 2 — Add todo data
Add this constant ABOVE the `module.exports = app;` line in app.js:
```js
const todos = [
  { id: 1, title: 'Learn Express', done: false },
  { id: 2, title: 'Build an API', done: true },
  { id: 3, title: 'Write tests', done: false },
];
```

### Step 3 — Add the /todos route
Add this route ABOVE the `module.exports = app;` line in app.js:
```js
app.get('/todos', (req, res) => {
  res.json(todos);
});
```

### Step 4 — Verify
Run: `node test.js`
Expected: prints "ok"
If it doesn't pass, re-check steps 2 and 3, fix any mistakes, then verify again.

## Rules
- Do NOT modify the /health route or the server startup code
- Only add code between the `// TODO` comment and `module.exports = app;`
- Run `node test.js` ONLY after you have made all the code changes in steps 2 and 3

## When Finished
Once `node test.js` prints "ok", reply with EXACTLY this text and NOTHING else:
```
Task completed. /exit
```
Do not write summaries, explanations, or suggestions. Just the above line.
