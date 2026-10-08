import type { KeyValueStorage } from './api/client';
import type { DeviceInfo } from './api/types';

export const DEVICE_ID_KEY = 'forge-admin.deviceId';

function safeLocalStorage(): KeyValueStorage | null {
  try {
    return typeof window !== 'undefined' && window.localStorage ? window.localStorage : null;
  } catch {
    return null;
  }
}

/** randomUUID 只在安全上下文可用（http 局域网地址访问时没有），回落到 getRandomValues。 */
function randomUuid(): string {
  const c: Crypto | undefined = globalThis.crypto;
  if (c && typeof c.randomUUID === 'function') return c.randomUUID();
  const bytes = new Uint8Array(16);
  if (c && typeof c.getRandomValues === 'function') c.getRandomValues(bytes);
  else for (let i = 0; i < bytes.length; i++) bytes[i] = Math.floor(Math.random() * 256);
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

/** 本浏览器的持久设备 ID：同一 device.id 再次登录时服务端会吊销旧会话，避免会话堆积。 */
export function getDeviceId(storage: KeyValueStorage | null = safeLocalStorage()): string {
  try {
    const existing = storage?.getItem(DEVICE_ID_KEY);
    if (existing) return existing;
  } catch {
    // 忽略，生成新的
  }
  const id = randomUuid();
  try {
    storage?.setItem(DEVICE_ID_KEY, id);
  } catch {
    // 存储不可用：本次会话内有效
  }
  return id;
}

export function adminDeviceInfo(storage?: KeyValueStorage | null): DeviceInfo {
  return { id: getDeviceId(storage), name: '管理后台', platform: 'web', appVersion: 'admin' };
}
