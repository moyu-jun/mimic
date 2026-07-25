/**
 * 全局状态管理 — ARCHITECTURE v3.0 重构
 */

import { reactive } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { AppPage, RuntimeStatus, KeyboardConfig, MouseConfig, CustomSequence, HotkeyConfig } from '../types/config'

export const appStore = reactive({
  currentPage: 'home' as AppPage,
  runtimeStatus: 'Idle' as RuntimeStatus,
  isLocked: false,
  // 自定义页内部子视图路由（不走全局 AppPage）：'list' 卡片列表 / 'detail' 序列详情
  customView: 'list' as 'list' | 'detail',
  // 当前正在编辑/激活的序列 id（detail 视图使用）
  activeSequenceId: null as string | null,
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
  customSequences: [] as CustomSequence[],
  hotkeys: {
    start: { keyLabel: 'F12', scanCode: 88 },
    stop: { keyLabel: 'F12', scanCode: 88 },
  } as HotkeyConfig,
})

export function setPage(page: AppPage): void {
  appStore.currentPage = page
  // 侧边栏导航离开或重进自定义页时，子视图始终复位到卡片列表；
  // 后端 set_current_page 同步清空激活序列（详情页热键随之失效）。
  appStore.customView = 'list'
  appStore.activeSequenceId = null
  invoke('set_current_page', { page }).catch((err) => {
    console.error('Failed to set current page:', err)
  })
}
