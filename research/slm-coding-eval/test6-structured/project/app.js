// Base Express app — the /health endpoint already exists.
// Your task: add the /todos GET endpoint by following the plan in skill.md.

const express = require('express');
const app = express();

// ---- existing route (do not modify) ----
app.get('/health', (req, res) => {
  res.json({ status: 'ok' });
});

// ---- add your /todos route below this line ----
// TODO: follow skill.md instructions

const todos = [
  { id: 1, title: 'Learn Express', done: false },
  { id: 2, title: 'Build an API', done: true },
  { id: 3, title: 'Write tests', done: false },
];

app.get('/todos', (req, res) => {
  res.json(todos);
});

module.exports = app;

// Start server if run directly
if (require.main === module) {
  const port = process.env.PORT || 3456;
  const server = app.listen(port, () => {
    console.log('listening on ' + port);
    // signal ready to test runner
    if (process.send) process.send('ready');
  });
  // shutdown gracefully
  process.on('SIGTERM', () => { server.close(); process.exit(0); });
}
