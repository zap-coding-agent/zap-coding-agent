const http = require('http');
const { handle } = require('./router');

const server = http.createServer((req, res) => {
  const u = new URL(req.url, 'http://localhost');
  const query = Object.fromEntries(u.searchParams);
  let raw = '';
  req.on('data', (c) => (raw += c));
  req.on('end', () => {
    const body = raw ? JSON.parse(raw) : null;
    const { status, json } = handle(req.method, u.pathname, query, body);
    res.writeHead(status, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify(json));
  });
});

if (require.main === module) server.listen(3000);
module.exports = { server };
