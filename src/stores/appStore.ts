/**
 * 全局状态管理 — ARCHITECTURE v3.0 重构
 */

import { reactive } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { AppPage, RuntimeStatus, KeyboardConfig, MouseConfig, HotkeyConfig } from '../types/config'

export const appStore = reactive({
  currentPage: 'home' as AppPage,
  runtimeStatus: 'Idle' as RuntimeStatus,
  isLocked: false,
  keyboardConfigs: [
    {
      id: 'default-keyboard-1',
      enabled: true,
      actionType: 'press' as const,
      keyLabel: 'F',
      scanCode: 33,
      intervalMs: 20,
    },
  ] as KeyboardConfig[],
  mouseConfigs: [
    {
      id: 'default-mouse-1',
      enabled: true,
      actionType: 'click_left' as const,
      x: null,
      y: null,
      intervalMs: 20,
    },
  ] as MouseConfig[],
  hotkeys: {
    start: { keyLabel: 'F12', scanCode: 88 },
    stop: { keyLabel: 'F12', scanCode: 88 },
  } as HotkeyConfig,
})

export function setPage(page: AppPage): void {
  appStore.currentPage = page
  invoke('set_current_page', { page }).catch((err) => {
    console.error('Failed to set current page:', err)
  })
}
