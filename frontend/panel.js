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
let editing = false;
let idleTimer;
const IDLE_TIMEOUT_MS = 8000;
let disposed = false;

// Windows 没有「辅助功能」授权：监听中断时展示恢复文案（Mac 保留授权引导原文）。
if (typeof navigator !== 'undefined' && /Windows/i.test(navigator.userAgent)) {
  $('permissionBtn').textContent = '全局复制/粘贴监听已中断，正在自动恢复，点击重试';
}

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
    const edit = document.createElement('button');
    edit.className = 'edit';
    edit.type = 'button';
    edit.textContent = '✎';
    edit.title = '编辑';
    edit.setAttribute('aria-label', `编辑「${key}」`);
    edit.addEventListener('click', () => enterEdit(row, key, value));
    row.appendChild(edit);
  }
  return row;
}

function enterEdit(row, oldKey, value) {
  editing = true;
  row.classList.add('editing');
  const keyInput = document.createElement('input');
  keyInput.type = 'text';
  keyInput.className = 'edit-input key-input';
  keyInput.setAttribute('aria-label', '名称');
  keyInput.value = oldKey;
  const valueInput = document.createElement('input');
  valueInput.type = 'text';
  valueInput.className = 'edit-input value-input';
  valueInput.setAttribute('aria-label', '内容');
  valueInput.value = value;
  const save = document.createElement('button');
  save.className = 'save-btn';
  save.type = 'button';
  save.textContent = '保存';
  const cancel = document.createElement('button');
  cancel.className = 'cancel-btn';
  cancel.type = 'button';
  cancel.textContent = '取消';
  const submit = () => { void saveEdit(row, oldKey, keyInput, valueInput, save); };
  save.addEventListener('click', submit);
  cancel.addEventListener('click', () => { void exitEdit(); });
  const keydown = event => {
    if (event.key === 'Escape') {
      event.preventDefault();
      void exitEdit();
    } else if (event.key === 'Enter') {
      event.preventDefault();
      submit();
    }
  };
  keyInput.addEventListener('keydown', keydown);
  valueInput.addEventListener('keydown', keydown);
  row.replaceChildren(keyInput, valueInput, save, cancel);
}

async function saveEdit(row, oldKey, keyInput, valueInput, save) {
  if (!editing || save.disabled) return;
  const newKey = keyInput.value.trim();
  if (!newKey) { toast('key 不能为空'); return; }
  save.disabled = true;
  try {
    await invoke('update_permanent', { oldKey, newKey, value: valueInput.value });
    editing = false;
    toast('已保存');
    await render();
  } catch (error) {
    // 失败保留行内输入，用户可修改后重试；不做任何重渲染。
    save.disabled = false;
    report(error);
  }
}

async function exitEdit() {
  editing = false;
  await render();
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

// Cmd+X 唤出的面板 8 秒无操作自动隐藏；📋 打开的面板不武装，保持现状。
function armIdle() {
  clearTimeout(idleTimer);
  idleTimer = setTimeout(() => {
    // 编辑进行中绝不打断：到期改为重新武装，待编辑结束后继续计时。
    if (editing) {
      armIdle();
      return;
    }
    idleTimer = undefined;
    invoke('close_panel').catch(report);
  }, IDLE_TIMEOUT_MS);
}

function disarmIdle() {
  clearTimeout(idleTimer);
  idleTimer = undefined;
}

// 交互续期：仅当计时器已武装（Cmd+X 唤出）才顺延，绝不新武装。
for (const type of ['pointerdown', 'keydown', 'input']) {
  window.addEventListener(type, () => { if (idleTimer !== undefined) armIdle(); });
}

void subscribe('panel-shown-by-shortcut', () => armIdle());
void subscribe('panel-hidden', () => disarmIdle());

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
  clearTimeout(idleTimer);
  unlisten.splice(0).forEach(stop => stop());
});

void syncPermission();
void render();
