const store = require('./store');

// handle(method, path, query, body) -> { status, json }
function handle(method, urlPath, query, body) {
  const idMatch = urlPath.match(/^\/tasks\/(\d+)$/);
  if (method === 'GET' && urlPath === '/tasks') {
    return { status: 200, json: store.list() };
  }
  if (method === 'POST' && urlPath === '/tasks') {
    if (!body || typeof body.title !== 'string' || body.title === '') {
      return { status: 400, json: { error: 'title required' } };
    }
    return { status: 201, json: store.add(body.title) };
  }
  if (method === 'GET' && idMatch) {
    const task = store.get(Number(idMatch[1]));
    return task ? { status: 200, json: task } : { status: 404, json: { error: 'not found' } };
  }
  return { status: 404, json: { error: 'no route' } };
}

module.exports = { handle };
