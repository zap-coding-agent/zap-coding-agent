# Skill: Add a REST Endpoint to Express App

## Task
Add a `GET /todos` endpoint to `app.js` that returns a JSON array of todo items.

## Step-by-Step Plan

### Step 1 — Define the todo data
Add this array ABOVE the `module.exports = app;` line:
```js
const todos = [
  { id: 1, title: 'Learn Express', done: false },
  { id: 2, title: 'Build an API', done: true },
  { id: 3, title: 'Write tests', done: false },
];
```

### Step 2 — Add the route
Add this route BELOW the `// TODO: follow skill.md instructions` comment and ABOVE `module.exports = app;`:
```js
app.get('/todos', (req, res) => {
  res.json(todos);
});
```

### Step 3 — Verify
Run: `node test.js`
Expected: all tests pass with "ok".

## Rules
- Do NOT modify the /health route or the server startup code.
- Only add code between the `// TODO` comment and `module.exports = app;`.
- After each code change, run `node test.js` to verify.
- Stop when all tests pass.
