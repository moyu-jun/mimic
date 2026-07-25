/**
 * 前后端共享类型定义 — ARCHITECTURE v3.0 重构
 *
 * 重构要点：
 * - 配置层使用 XxxConfig 命名（与模拟层 XxxAction 区分）
 * - 增加动作类型枚举
 * - selected 改名为 enabled（更语义化）
 */

/** 左侧菜单的四个页面标识 */
export type AppPage = 'home' | 'keyboard' | 'mouse' | 'settings'

/** 运行状态机 */
export type RuntimeStatus =
  | 'Idle'
  | 'ReadyKeyboard'
  | 'ReadyMouse'
  | 'RunningKeyboard'
  | 'RunningMouse'
  | 'PickingMouse'
  | 'Recording'
  | 'Error'

/** Interception 驱动状态 */
export type DriverStatus =
  | 'NotInstalled'
  | 'InstalledNeedReboot'
  | 'Ready'
  | 'Error'

/** 键盘动作类型 */
export type KeyActionType = 'press' | 'hold' | 'combo'

/** 鼠标动作类型 */
export type MouseActionType =
  | 'click_left'
  | 'click_right'
  | 'click_middle'
  | 'scroll_up'
  | 'scroll_down'
  | 'drag'

/** 按键捕获结果 */
export interface CapturedKey {
  keyLabel: string
  scanCode: number
}

/** 键盘配置项 */
export interface KeyboardConfig {
  id: string
  enabled: boolean
  actionType: KeyActionType
  keyLabel: string
  scanCode: number
  holdDurationMs?: number
  modifiers?: number[]
  intervalMs: number
}

/** 鼠标配置项 */
export interface MouseConfig {
  id: string
  enabled: boolean
  actionType: MouseActionType
  x: number | null
  y: number | null
  scrollDelta?: number
  dragToX?: number
  dragToY?: number
  intervalMs: number
}

/** 热键配置 */
export interface HotkeyConfig {
  start: CapturedKey
  stop: CapturedKey
}

/** 应用完整配置 */
export interface AppConfig {
  keyboardConfigs: KeyboardConfig[]
  mouseConfigs: MouseConfig[]
  hotkeys: HotkeyConfig
}

/** 热键更新结果 */
export interface HotkeyUpdateResult {
  changed: boolean
  registered: boolean
  persisted: boolean
  message: string | null
}
