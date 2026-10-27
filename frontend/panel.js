'use strict';

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = id => document.getElementById(id);
const listEl = $('list');
const toastEl = $('toast');
const tabs = [...document.querySelectorAll('.tab')];
const unlisten = [];
let tab = 'perm';
let renderVersion = 0;
let searchTimer;
let toastTimer;
let adding = false;
let disposed = false;

function toast(message) {
  toastEl.textContent = String(message);
  toastEl.classList.add('show');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => toastEl.classList.remove('show'), 2200);
}

function report(error) {
  console.error(error);
  toast(`操作失败：${String(error)}`);
}

function empty(message) {
  const element = document.createElement('div');
  element.className = 'empty';
  element.textContent = message;
  return element;
}

async function copy(value, label) {
  try {
    await invoke('copy_to_clipboard', { value });
    toast(label ? `已复制「${label}」` : '已复制');
  } catch (error) {
    report(error);
  }
}

function createRow(value, key) {
  const row = document.createElement('div');
  row.className = 'row';
  const button = document.createElement('button');
  button.className = 'copy';
  button.type = 'button';
  button.title = key === undefined ? '点击复制内容' : `复制「${key}」`;
  const text = document.createElement('span');
  text.className = 'k';
  // Clipboard content is untrusted text, never HTML.
  text.textContent = key === undefined ? value.slice(0, 120) : key;
  button.appendChild(text);
  button.addEventListener('click', () => copy(value, key));
  row.appendChild(button);
  if (key !== undefined) {
    const remove = document.createElement('button');
    remove.className = 'del';
    remove.type = 'button';
    remove.textContent = '×';
    remove.title = '删除';
    remove.setAttribute('aria-label', `删除「${key}」`);
    remove.addEventListener('click', async () => {
      if (remove.disabled) return;
      remove.disabled = true;
      try {
        await invoke('remove_permanent', { key });
        await render();
      } catch (error) {
        remove.disabled = false;
        report(error);
      }
    });
    row.appendChild(remove);
  }
  return row;
}

async function render() {
  clearTimeout(searchTimer);
  const version = ++renderVersion;
  const requestedTab = tab;
  const query = $('search').value;
  listEl.setAttribute('aria-busy', 'true');
  try {
    const items = requestedTab === 'perm'
      ? await invoke('search_permanent', { query })
      : await invoke('list_recent');
    // A later query or tab change always wins, even if IPC replies arrive out of order.
    if (disposed || version !== renderVersion) return;
    const fragment = document.createDocumentFragment();
    if (items.length === 0) {
      fragment.appendChild(empty(requestedTab === 'recent'
        ? '还没有复制记录，在任意 App 里 Cmd+C / Ctrl+C 试试'
        : query.trim() ? '没有匹配的名称，换个关键词试试' : '还没有永久存内容，添加一条吧（上限 100 条）'));
    } else {
      for (const item of items) {
        fragment.appendChild(requestedTab === 'perm'
          ? createRow(item.value, item.key) : createRow(item));
      }
    }
    listEl.replaceChildren(fragment);
  } catch (error) {
    if (!disposed && version === renderVersion) {
      listEl.replaceChildren(empty('加载失败，请切换分类或重新搜索以重试'));
      report(error);
    }
  } finally {
    if (!disposed && version === renderVersion) listEl.setAttribute('aria-busy', 'false');
  }
}

function switchTab(nextTab) {
  tab = nextTab;
  for (const element of tabs) {
    const active = element.dataset.tab === tab;
    element.classList.toggle('active', active);
    element.setAttribute('aria-selected', String(active));
  }
  $('searchBar').classList.toggle('show', tab === 'perm');
  $('addForm').hidden = tab !== 'perm';
  listEl.setAttribute('aria-labelledby', `tab-${tab}`);
  void render();
}

for (const element of tabs) {
  element.addEventListener('click', () => switchTab(element.dataset.tab));
  element.addEventListener('keydown', event => {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    const next = event.key === 'Home' ? tabs[0] : event.key === 'End' ? tabs[1]
      : tabs[(tabs.indexOf(element) + 1) % tabs.length];
    next.focus();
    switchTab(next.dataset.tab);
  });
}

$('search').addEventListener('input', () => {
  ++renderVersion;
  clearTimeout(searchTimer);
  searchTimer = setTimeout(() => { void render(); }, 120);
});

$('addForm').addEventListener('submit', async event => {
  event.preventDefault();
  if (adding) return;
  const key = $('f-key').value.trim();
  const value = $('f-value').value;
  if (!key) { toast('key 不能为空'); return; }
  adding = true;
  $('addBtn').disabled = true;
  try {
    await invoke('add_permanent', { key, value });
    // Do not erase text entered while the save was in flight.
    if ($('f-key').value.trim() === key && $('f-value').value === value) {
      $('f-key').value = '';
      $('f-value').value = '';
    }
    toast('已保存');
    await render();
  } catch (error) {
    report(error);
  } finally {
    adding = false;
    $('addBtn').disabled = false;
  }
});

async function closePanel() {
  if ($('closeBtn').disabled) return;
  $('closeBtn').disabled = true;
  try {
    await invoke('close_panel');
  } catch (error) {
    report(error);
  } finally {
    $('closeBtn').disabled = false;
  }
}

// Close on the first press, before activation or window ordering can disrupt mouseup.
$('closeBtn').addEventListener('pointerdown', event => {
  if (event.button !== 0 || event.isPrimary === false) return;
  event.preventDefault();
  event.stopPropagation();
  void closePanel();
});
$('closeBtn').addEventListener('click', event => {
  // Pointer clicks are already handled; keep keyboard and assistive activation working.
  if (event.detail > 0) return;
  return closePanel();
});

async function syncPermission() {
  try {
    $('permissionBtn').hidden = await invoke('permission_status');
  } catch (error) {
    report(error);
  }
}

$('permissionBtn').addEventListener('click', () => {
  invoke('open_accessibility_settings').catch(report);
});

async function subscribe(event, handler) {
  try {
    const stop = await listen(event, handler);
    if (disposed) stop();
    else unlisten.push(stop);
  } catch (error) {
    if (!disposed) report(error);
  }
}

void subscribe('clipboard-added', () => { if (tab === 'recent') void render(); });
void subscribe('clipboard-error', event => report(event.payload));
void subscribe('no-permission', () => { $('permissionBtn').hidden = false; });
void subscribe('permission-granted', () => { $('permissionBtn').hidden = true; });
window.addEventListener('focus', () => { void syncPermission(); });
window.addEventListener('pagehide', () => {
  disposed = true;
  ++renderVersion;
  clearTimeout(searchTimer);
  clearTimeout(toastTimer);
  unlisten.splice(0).forEach(stop => stop());
});

void syncPermission();
void render();
