'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

class Element {
  constructor(tag = 'div') {
    this.tagName = tag;
    this.children = [];
    this.listeners = new Map();
    this.attributes = {};
    this.dataset = {};
    this.value = '';
    this.disabled = false;
    this.hidden = false;
    this.text = '';
    const classes = new Set();
    this.classList = {
      add: name => classes.add(name),
      remove: name => classes.delete(name),
      contains: name => classes.has(name),
      toggle(name, force = !classes.has(name)) {
        if (force) classes.add(name);
        else classes.delete(name);
        return force;
      },
    };
  }
  set textContent(value) { this.text = String(value); this.children = []; }
  get textContent() { return this.text + this.children.map(child => child.textContent).join(''); }
  set innerHTML(_) { throw new Error('Untrusted HTML must not enter the DOM'); }
  setAttribute(name, value) { this.attributes[name] = String(value); }
  appendChild(child) { this.children.push(child); }
  replaceChildren(...children) {
    this.text = '';
    this.children = children.flatMap(child => child.tagName === '#fragment' ? child.children : [child]);
  }
  addEventListener(type, callback) {
    if (!this.listeners.has(type)) this.listeners.set(type, []);
    this.listeners.get(type).push(callback);
  }
  fire(type, event = {}) {
    return Promise.all((this.listeners.get(type) || []).map(callback => callback({
      preventDefault() {}, stopPropagation() {}, ...event,
    })));
  }
  focus() { this.focused = true; }
}

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

const flush = () => new Promise(resolve => setImmediate(resolve));

function fixture(overrides = {}, script = 'panel.js') {
  const elements = new Map();
  const get = id => {
    if (!elements.has(id)) elements.set(id, new Element());
    return elements.get(id);
  };
  const tabs = ['perm', 'recent'].map(name => {
    const element = get(`tab-${name}`);
    element.dataset.tab = name;
    return element;
  });
  const calls = [];
  const events = new Map();
  const timers = new Map();
  let timerId = 0;
  let stopped = 0;
  const defaults = {
    search_permanent: () => [], list_recent: () => [],
    permission_status: () => true, is_pet_pinned: () => false,
  };
  const window = new Element();
  window.__TAURI__ = {
    core: {
      invoke(command, args) {
        calls.push({ command, args });
        return Promise.resolve().then(() => (overrides[command] || defaults[command] || (() => undefined))(args));
      },
    },
    event: {
      async listen(name, handler) {
        events.set(name, handler);
        return () => { stopped++; events.delete(name); };
      },
    },
  };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../frontend', script), 'utf8'), {
    window,
    document: {
      getElementById: get, querySelectorAll: () => tabs,
      createElement: tag => new Element(tag), createDocumentFragment: () => new Element('#fragment'),
    },
    console: { error() {} },
    setTimeout(callback, delay) { timers.set(++timerId, { callback, delay }); return timerId; },
    clearTimeout(id) { timers.delete(id); },
  });
  return {
    get, tabs, calls, events, window,
    get stopped() { return stopped; },
    runTimers(delay) {
      for (const [id, timer] of [...timers]) {
        if (timer.delay === delay) { timers.delete(id); timer.callback(); }
      }
    },
    count(command) { return calls.filter(call => call.command === command).length; },
  };
}

test('late search replies cannot overwrite a newer result', async () => {
  const old = deferred();
  const app = fixture({ search_permanent: ({ query }) => query ? [{ key: 'new', value: 'value' }] : old.promise });
  await flush();
  app.get('search').value = 'new';
  await app.get('search').fire('input');
  app.runTimers(120);
  await flush();
  assert.match(app.get('list').textContent, /new/);
  old.resolve([{ key: 'old', value: 'value' }]);
  await flush();
  assert.doesNotMatch(app.get('list').textContent, /old/);
  assert.equal(app.get('list').attributes['aria-busy'], 'false');
});

test('tab changes invalidate in-flight results', async () => {
  const old = deferred();
  const app = fixture({ search_permanent: () => old.promise, list_recent: () => ['recent-text'] });
  await app.tabs[1].fire('click');
  await flush();
  old.resolve([{ key: 'old-permanent', value: 'value' }]);
  await flush();
  assert.equal(app.get('list').textContent, 'recent-text');
  assert.equal(app.get('addForm').hidden, true);
  assert.equal(app.tabs[1].attributes['aria-selected'], 'true');
});

test('rapid search input is debounced into one IPC request', async () => {
  const app = fixture();
  await flush();
  for (const text of ['a', 'ab', 'abc']) {
    app.get('search').value = text;
    await app.get('search').fire('input');
  }
  assert.equal(app.count('search_permanent'), 1);
  app.runTimers(120);
  await flush();
  assert.equal(app.count('search_permanent'), 2);
  assert.equal(app.calls.filter(call => call.command === 'search_permanent').at(-1).args.query, 'abc');
});

test('clipboard updates do not re-query the permanent tab', async () => {
  const app = fixture();
  await flush();
  app.events.get('clipboard-added')();
  await flush();
  assert.equal(app.count('search_permanent'), 1);
  await app.tabs[1].fire('click');
  await flush();
  app.events.get('clipboard-added')();
  await flush();
  assert.equal(app.count('list_recent'), 2);
});

test('HTML-looking clipboard content remains literal text', async () => {
  const payload = '<img src=x onerror=alert(1)>';
  const app = fixture({ list_recent: () => [payload] });
  await app.tabs[1].fire('click');
  await flush();
  const row = app.get('list').children[0];
  assert.equal(row.children[0].children[0].tagName, 'span');
  assert.equal(row.textContent, payload);
  await row.children[0].fire('click');
  assert.equal(app.calls.find(call => call.command === 'copy_to_clipboard').args.value, payload);
});

test('failed copy and delete show errors rather than success', async () => {
  const app = fixture({
    search_permanent: () => [{ key: 'key', value: 'value' }],
    copy_to_clipboard: () => Promise.reject('clipboard unavailable'),
    remove_permanent: () => Promise.reject('disk full'),
  });
  await flush();
  const row = app.get('list').children[0];
  await row.children[0].fire('click');
  assert.match(app.get('toast').textContent, /操作失败.*clipboard unavailable/);
  await row.children[1].fire('click');
  assert.match(app.get('toast').textContent, /操作失败.*disk full/);
  assert.equal(row.children[1].disabled, false);
  assert.equal(app.get('list').children.length, 1);
});

test('duplicate submits are blocked and failed saves preserve the form', async () => {
  const save = deferred();
  const app = fixture({ add_permanent: () => save.promise });
  await flush();
  app.get('f-key').value = 'key';
  app.get('f-value').value = 'value';
  const first = app.get('addForm').fire('submit');
  await app.get('addForm').fire('submit');
  assert.equal(app.count('add_permanent'), 1);
  assert.equal(app.get('addBtn').disabled, true);
  save.reject('disk full');
  await first;
  assert.equal(app.get('f-key').value, 'key');
  assert.equal(app.get('f-value').value, 'value');
  assert.equal(app.get('addBtn').disabled, false);
});

test('successful saves do not erase a newly edited form', async () => {
  const save = deferred();
  const app = fixture({ add_permanent: () => save.promise });
  await flush();
  app.get('f-key').value = 'key';
  app.get('f-value').value = 'old';
  const first = app.get('addForm').fire('submit');
  app.get('f-value').value = 'new';
  save.resolve();
  await first;
  assert.equal(app.get('f-value').value, 'new');
});

test('pet pin toggles use the returned state without opening the panel', async () => {
  let pinned = true;
  const app = fixture({ is_pet_pinned: () => true, toggle_pet_pin: () => (pinned = !pinned) }, 'pet.js');
  await flush();
  await app.get('petPinBtn').fire('click');
  assert.equal(app.get('petPinBtn').attributes['aria-pressed'], 'false');
  assert.equal(app.get('petPinBtn').classList.contains('pinned'), false);
  assert.equal(app.get('petPinBtn').title, '置顶桌宠');
  await app.get('petPinBtn').fire('click');
  assert.equal(app.get('petPinBtn').attributes['aria-pressed'], 'true');
  assert.equal(app.get('petPinBtn').classList.contains('pinned'), true);
  assert.equal(app.get('petPinBtn').title, '取消桌宠置顶');
  assert.equal(app.count('is_pet_pinned'), 1);
  assert.equal(app.get('petPinBtn').disabled, false);
  assert.equal(app.count('toggle_panel'), 0);
  assert.equal(app.count('start_drag'), 0);
});

test('pet pin button waits for its initial state before accepting toggles', async () => {
  const initial = deferred();
  const app = fixture({ is_pet_pinned: () => initial.promise, toggle_pet_pin: () => false }, 'pet.js');
  await app.get('petPinBtn').fire('click');
  assert.equal(app.count('toggle_pet_pin'), 0);
  initial.resolve(true);
  await flush();
  assert.equal(app.get('petPinBtn').attributes['aria-pressed'], 'true');
  await app.get('petPinBtn').fire('click');
  assert.equal(app.get('petPinBtn').attributes['aria-pressed'], 'false');
});

test('pet pin failures preserve the state and allow retry without duplicate requests', async () => {
  const pending = deferred();
  let attempts = 0;
  const app = fixture({
    is_pet_pinned: () => true,
    toggle_pet_pin: () => attempts++ === 0 ? pending.promise : false,
  }, 'pet.js');
  await flush();
  const first = app.get('petPinBtn').fire('click');
  await app.get('petPinBtn').fire('click');
  assert.equal(app.count('toggle_pet_pin'), 1);
  assert.equal(app.get('petPinBtn').disabled, true);
  pending.reject('window unavailable');
  await first;
  assert.equal(app.get('petPinBtn').attributes['aria-pressed'], 'true');
  assert.equal(app.get('petPinBtn').disabled, false);
  assert.match(app.get('petPinBtn').title, /失败/);
  await app.get('petPinBtn').fire('click');
  assert.equal(app.get('petPinBtn').attributes['aria-pressed'], 'false');
});

test('failed pet pin initialization does not leave the control disabled', async () => {
  const app = fixture({
    is_pet_pinned: () => Promise.reject('failed'), toggle_pet_pin: () => false,
  }, 'pet.js');
  await flush();
  assert.equal(app.get('petPinBtn').disabled, false);
  await app.get('petPinBtn').fire('click');
  assert.equal(app.get('petPinBtn').attributes['aria-pressed'], 'false');
});

test('pet starts unpinned and only an explicit toggle enables pinning', async () => {
  const app = fixture({ toggle_pet_pin: () => true }, 'pet.js');
  await flush();
  assert.equal(app.get('petPinBtn').attributes['aria-pressed'], 'false');
  assert.equal(app.get('petPinBtn').classList.contains('pinned'), false);
  assert.equal(app.count('toggle_pet_pin'), 0);
  await app.get('petPinBtn').fire('click');
  assert.equal(app.get('petPinBtn').attributes['aria-pressed'], 'true');
});

test('first close press hides immediately and its subsequent click cannot close twice', async () => {
  const app = fixture();
  await flush();
  let prevented = false;
  let stopped = false;
  await app.get('closeBtn').fire('pointerdown', {
    button: 0, isPrimary: true,
    preventDefault() { prevented = true; },
    stopPropagation() { stopped = true; },
  });
  await flush();
  assert.equal(app.count('close_panel'), 1);
  assert.equal(prevented, true);
  assert.equal(stopped, true);
  await app.get('closeBtn').fire('click', { detail: 1 });
  assert.equal(app.count('close_panel'), 1);
  await app.get('closeBtn').fire('click', { detail: 0 });
  assert.equal(app.count('close_panel'), 2); // Keyboard activation is still supported.
});

test('close press ignores right clicks and secondary pointers', async () => {
  const app = fixture();
  await flush();
  await app.get('closeBtn').fire('pointerdown', { button: 2, isPrimary: true });
  await app.get('closeBtn').fire('pointerdown', { button: 0, isPrimary: false });
  assert.equal(app.count('close_panel'), 0);
});

test('panel close only requests hiding and preserves the form and rendered data', async () => {
  const pending = deferred();
  const app = fixture({
    search_permanent: () => [{ key: 'saved', value: 'text' }],
    close_panel: () => pending.promise,
  });
  await flush();
  app.get('f-key').value = 'draft';
  app.get('f-value').value = 'unsaved text';
  const first = app.get('closeBtn').fire('click');
  await app.get('closeBtn').fire('click');
  assert.equal(app.count('close_panel'), 1);
  assert.equal(app.get('closeBtn').disabled, true);
  pending.resolve();
  await first;
  assert.equal(app.get('closeBtn').disabled, false);
  assert.equal(app.get('f-key').value, 'draft');
  assert.equal(app.get('f-value').value, 'unsaved text');
  assert.match(app.get('list').textContent, /saved/);
  assert.equal(app.count('toggle_panel'), 0);
  assert.equal(app.count('remove_permanent'), 0);
});

test('failed panel close shows an error and can be retried', async () => {
  let attempts = 0;
  const app = fixture({
    close_panel: () => attempts++ === 0 ? Promise.reject('window unavailable') : undefined,
  });
  await flush();
  await app.get('closeBtn').fire('click');
  assert.match(app.get('toast').textContent, /操作失败.*window unavailable/);
  assert.equal(app.get('closeBtn').disabled, false);
  await app.get('closeBtn').fire('click');
  assert.equal(app.count('close_panel'), 2);
});

test('panel initialization, clipboard changes, and permissions never open the panel', async () => {
  const app = fixture();
  await flush();
  await app.tabs[1].fire('click');
  app.events.get('clipboard-added')();
  app.events.get('clipboard-error')({ payload: 'failed' });
  app.events.get('no-permission')();
  app.events.get('permission-granted')();
  await app.window.fire('focus');
  await flush();
  assert.equal(app.count('toggle_panel'), 0);
  assert.equal(app.count('close_panel'), 0);
});

test('permission notifications and page teardown clean up correctly', async () => {
  const app = fixture({ permission_status: () => false });
  await flush();
  assert.equal(app.get('permissionBtn').hidden, false);
  app.events.get('permission-granted')();
  assert.equal(app.get('permissionBtn').hidden, true);
  app.events.get('no-permission')();
  assert.equal(app.get('permissionBtn').hidden, false);
  await app.window.fire('pagehide');
  assert.equal(app.stopped, 4);
});

test('failed loads and empty search results are distinct states', async () => {
  const app = fixture({ search_permanent: ({ query }) => query ? [] : Promise.reject('failed') });
  await flush();
  assert.match(app.get('list').textContent, /加载失败/);
  app.get('search').value = 'missing';
  await app.get('search').fire('input');
  app.runTimers(120);
  await flush();
  assert.match(app.get('list').textContent, /没有匹配/);
});

test('pet exposes permission guidance, ignores right-drag, and serializes panel toggles', async () => {
  const toggle = deferred();
  const app = fixture({ permission_status: () => false, toggle_panel: () => toggle.promise }, 'pet.js');
  await flush();
  assert.equal(app.get('bubble').classList.contains('show'), true);
  await app.get('bubble').fire('click');
  assert.equal(app.count('open_accessibility_settings'), 1);
  await app.get('pet').fire('mousedown', { button: 2 });
  assert.equal(app.count('start_drag'), 0);
  await app.get('pet').fire('mousedown', { button: 0 });
  assert.equal(app.count('start_drag'), 1);
  const first = app.get('toggleBtn').fire('click');
  await app.get('toggleBtn').fire('click');
  assert.equal(app.count('toggle_panel'), 1);
  toggle.resolve();
  await first;
  assert.equal(app.get('toggleBtn').disabled, false);
  app.events.get('permission-granted')();
  assert.equal(app.get('bubble').classList.contains('show'), false);
  await app.window.fire('pagehide');
  assert.equal(app.stopped, 3);
});

test('pet startup, repeated paste, and permission events never open the panel or change pet pin state', async () => {
  const app = fixture({}, 'pet.js');
  await flush();
  app.events.get('no-permission')();
  app.events.get('permission-granted')();
  for (let index = 0; index < 10; index++) app.events.get('v-pressed')();
  assert.equal(app.get('pet').classList.contains('bouncing'), true);
  app.runTimers(200);
  assert.equal(app.get('pet').classList.contains('bouncing'), false);
  await app.window.fire('focus');
  await flush();
  assert.equal(app.count('toggle_panel'), 0);
  assert.equal(app.count('toggle_pet_pin'), 0);
  assert.equal(app.get('petPinBtn').attributes['aria-pressed'], 'false');
  assert.equal(app.get('petPinBtn').classList.contains('pinned'), false);
  assert.equal(app.count('copy_to_clipboard'), 0);
});
