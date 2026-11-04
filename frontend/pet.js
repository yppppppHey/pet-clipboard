'use strict';

const { invoke } = window.__TAURI__.core;
const bubble = document.getElementById('bubble');
const pet = document.getElementById('pet');
const toggle = document.getElementById('toggleBtn');
const pin = document.getElementById('petPinBtn');
const unlisten = [];
let bounceTimer;
let disposed = false;

// Windows 没有「辅助功能」授权：监听中断时展示恢复文案（Mac 保留授权引导原文）。
if (typeof navigator !== 'undefined' && /Windows/i.test(navigator.userAgent)) {
  bubble.title = '恢复复制和粘贴监听';
  bubble.textContent = '复制/粘贴监听已中断，正在自动恢复；点击重试';
}

function report(error) {
  console.error(error);
}

function updatePin(pinned) {
  pin.classList.toggle('pinned', pinned);
  pin.setAttribute('aria-pressed', String(pinned));
  pin.title = pinned ? '取消桌宠置顶' : '置顶桌宠';
  pin.setAttribute('aria-label', pin.title);
}

async function syncPermission() {
  try {
    bubble.classList.toggle('show', !await invoke('permission_status'));
  } catch (error) {
    report(error);
  }
}

async function subscribe(event, handler) {
  try {
    const stop = await window.__TAURI__.event.listen(event, handler);
    if (disposed) stop();
    else unlisten.push(stop);
  } catch (error) {
    report(error);
  }
}

void subscribe('no-permission', () => bubble.classList.add('show'));
void subscribe('permission-granted', () => bubble.classList.remove('show'));
void subscribe('v-pressed', () => {
  clearTimeout(bounceTimer);
  pet.classList.add('bouncing');
  bounceTimer = setTimeout(() => pet.classList.remove('bouncing'), 200);
});

bubble.addEventListener('click', () => invoke('open_accessibility_settings').catch(report));
pet.addEventListener('mousedown', event => {
  if (event.button === 0) invoke('start_drag').catch(report);
});
toggle.addEventListener('click', async () => {
  if (toggle.disabled) return;
  toggle.disabled = true;
  try {
    await invoke('toggle_panel');
  } catch (error) {
    report(error);
  } finally {
    toggle.disabled = false;
  }
});

pin.addEventListener('click', async () => {
  if (pin.disabled) return;
  pin.disabled = true;
  try {
    updatePin(await invoke('toggle_pet_pin'));
  } catch (error) {
    pin.title = '切换置顶失败，点击重试';
    report(error);
  } finally {
    pin.disabled = false;
  }
});

window.addEventListener('focus', () => { void syncPermission(); });
window.addEventListener('pagehide', () => {
  disposed = true;
  clearTimeout(bounceTimer);
  unlisten.splice(0).forEach(stop => stop());
});
pin.disabled = true;
invoke('is_pet_pinned').then(updatePin).catch(error => {
  pin.title = '读取置顶状态失败，点击重试';
  report(error);
}).finally(() => {
  pin.disabled = false;
});
void syncPermission();
