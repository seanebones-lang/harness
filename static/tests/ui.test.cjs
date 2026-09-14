const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { JSDOM, VirtualConsole } = require('jsdom');
const html = fs.readFileSync(path.join(__dirname, '..', 'index.html'), 'utf8');

async function boot(storage = {}) {
  const errors = [];
  const requests = [];
  const console = new VirtualConsole();
  console.on('jsdomError', error => errors.push(error));
  const dom = new JSDOM(html, {
    url: 'http://127.0.0.1:8787/', runScripts: 'dangerously', pretendToBeVisual: true,
    virtualConsole: console,
    beforeParse(window) {
      for (const [key, value] of Object.entries(storage)) window.localStorage.setItem(key, value);
      window.TextDecoder = TextDecoder;
      window.HTMLElement.prototype.scrollIntoView = () => {};
      window.fetch = async (url, options = {}) => {
        requests.push({ url, options });
        if (url === '/api/health') return Response.json({ status: 'ok', model: 'fixture-model',
          provider_model: 'fixture-model', auth_token: 'synthetic-token', key_env: { detected: [] } });
        if (url === '/api/projects' || url === '/api/sessions') return Response.json([]);
        if (url === '/api/chat') {
          const events = [{ type: 'session_id', id: 'fixture-session' },
            { type: 'text_chunk', content: 'BROWSER_TEST_OK' }, { type: 'done' }];
          return new Response(events.map(event => 'data: ' + JSON.stringify(event) + '\n\n').join(''),
            { headers: { 'Content-Type': 'text/event-stream' } });
        }
        throw new Error(`Unexpected request: ${url}`);
      };
    },
  });
  await waitFor(() => errors.length || dom.window.document.getElementById('status').textContent.includes('ready'));
  assert.deepEqual(errors, [], 'The actual embedded script must initialize without exceptions');
  return { dom, errors, requests };
}

async function waitFor(predicate) {
  for (let i = 0; i < 100; i++) {
    if (predicate()) return;
    await new Promise(resolve => setTimeout(resolve, 10));
  }
  assert.fail('UI did not reach its expected state');
}

test('fresh profile initializes and sends authenticated streaming chat', async () => {
  const { dom, requests, errors } = await boot();
  try {
    const { document, Event } = dom.window;
    assert.match(document.getElementById('status').textContent, /ready.*fixture-model/);
    assert.equal(document.getElementById('resume-last').checked, true);
    const prompt = document.getElementById('prompt');
    prompt.value = 'Synthetic chat test';
    prompt.dispatchEvent(new Event('input', { bubbles: true }));
    assert.equal(document.getElementById('send').disabled, false);
    document.getElementById('send').click();
    await waitFor(() => document.getElementById('chat').textContent.includes('BROWSER_TEST_OK'));
    assert.deepEqual(errors, []);
    const chat = requests.find(request => request.url === '/api/chat');
    assert.equal(chat.options.headers.Authorization, 'Bearer synthetic-token');
    assert.equal(JSON.parse(chat.options.body).prompt, 'Synthetic chat test');
  } finally { dom.window.close(); }
});

test('disabled resume preference avoids loading a stale stored session', async () => {
  const { dom, requests } = await boot({ 'harness.resume_last_session': 'false', 'harness.session_id': 'stale-id' });
  try {
    assert.equal(dom.window.document.getElementById('resume-last').checked, false);
    assert.equal(requests.some(request => request.url === '/api/sessions/stale-id'), false);
  } finally { dom.window.close(); }
});
