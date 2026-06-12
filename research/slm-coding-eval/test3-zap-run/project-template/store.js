let tasks = [];
let nextId = 1;

function list() { return tasks; }
function get(id) { return tasks.find((t) => t.id === id) || null; }
function add(title) {
  const task = { id: nextId++, title, completed: false };
  tasks.push(task);
  return task;
}
function _reset() { tasks = []; nextId = 1; }

module.exports = { list, get, add, _reset };
