/**
 * 全局状态管理 — ARCHITECTURE v3.0 重构
 */

import { reactive } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { AppPage, RuntimeStatus, KeyboardConfig, MouseConfig, CustomSequence, HotkeyConfig, LogLevel } from '../types/config'

export const appStore = reactive({
  currentPage: 'home' as AppPage,
  runtimeStatus: 'Idle' as RuntimeStatus,
  configSaveError: null as string | null,
  // 自定义页内部子视图路由（不走全局 AppPage）：'list' 卡片列表 / 'detail' 序列详情
  customView: 'list' as 'list' | 'detail',
  // 当前正在编辑/激活的序列 id（detail 视图使用）
  activeSequenceId: null as string | null,
  keyboardConfigs: [
    {
      id: 'default-keyboard-1',
      enabled: true,
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
  customSequences: [] as CustomSequence[],
  logLevel: 'error' as LogLevel,
  hotkeys: {
    start: { keyLabel: 'F12', scanCode: 88 },
    stop: { keyLabel: 'F12', scanCode: 88 },
  } as HotkeyConfig,
})

let navigationQueue: Promise<void> = Promise.resolve()

export function setPage(page: AppPage): Promise<void> {
  navigationQueue = navigationQueue.then(async () => {
    try {
      // 串行等待后端活动协调器确认，避免快速点击导致响应乱序和页面分叉。
      await invoke('set_current_page', { page })
      appStore.currentPage = page
      appStore.customView = 'list'
      appStore.activeSequenceId = null
    } catch (err) {
      console.error('Failed to set current page:', err)
    }
  })
  return navigationQueue
}
