<script setup lang="ts">
/**
 * 自定义序列详情页 — 编辑单个序列的有序动作列表。
 *
 * 功能：
 *   - 顶部：返回、序列名就地重命名、删除整个序列。
 *   - 动作列表：每行勾选 / 类型专属编辑（键盘捕获 / 鼠标坐标拾取）/ 间隔 / 上移下移 / 删除。
 *   - 底部：+ 添加按键 / + 添加鼠标。
 *
 * 热键门控：onMounted 调 enter_custom_sequence(id) 激活本序列（进入 ReadyCustom）；
 * 返回列表时调 set_current_page('custom') 复位为 Idle 并清空激活序列。
 */
import { computed, onMounted, onBeforeUnmount, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { appStore } from '../stores/appStore'
import KeyCaptureInput from './KeyCaptureInput.vue'
import type { CapturedKey, CustomAction, CustomSequence, MouseActionType } from '../types/config'
import { persistConfig } from '../lib/configUtil'

const DEFAULT_INTERVAL_MS = 20

// 当前编辑的序列（activeSequenceId 由 List 视图设置）。
const sequence = computed<CustomSequence | undefined>(() =>
  appStore.customSequences.find(s => s.id === appStore.activeSequenceId),
)

const capturedKey = ref<CapturedKey | null>(null)
let unlistenPicked: UnlistenFn | null = null
let unlistenStartFailed: UnlistenFn | null = null
const conflictActionId = ref<string | null>(null)
const isDeletingSequence = ref(false)

onMounted(async () => {
  unlistenPicked = await listen<{ rowId: string; x: number; y: number }>(
    'mouse_position_picked',
    (event) => {
      const { rowId, x, y } = event.payload
      const action = sequence.value?.actions.find(
        a => a.kind === 'mouse' && a.id === rowId,
      )
      if (!action || action.kind !== 'mouse') return
      action.x = x
      action.y = y
      persist()
    },
  )

  unlistenStartFailed = await listen<{ error: string }>('simulation_start_failed', (event) => {
    const [code, actionId] = event.payload.error.split(':')
    if (code !== 'hotkey_conflict' || !actionId) return
    conflictActionId.value = actionId
    window.setTimeout(() => {
      if (conflictActionId.value === actionId) conflictActionId.value = null
    }, 4000)
  })
})
onBeforeUnmount(() => {
  if (unlistenPicked) unlistenPicked()
  if (unlistenStartFailed) unlistenStartFailed()
})

function persist(): void {
  void persistConfig().catch(() => {
    // 事务工具已恢复最后成功快照并显示可重试提示。
  })
}

async function backToList(): Promise<void> {
  appStore.customView = 'list'
  appStore.activeSequenceId = null
  // 复位后端：清空激活序列 + 回 Idle（列表页热键无效）
  try {
    await invoke('set_current_page', { page: 'custom' })
  } catch (err) {
    console.error('[CustomDetailView] set_current_page 失败:', err)
  }
}

// ---- 序列名重命名 ----
function onNameInput(e: Event): void {
  if (!sequence.value) return
  sequence.value.name = (e.target as HTMLInputElement).value
}

function onNameCommit(): void {
  if (!sequence.value) return
  const trimmed = sequence.value.name.trim()
  sequence.value.name = trimmed.length ? trimmed : '未命名序列'
  persist()
}

// ---- 删除整个序列 ----
async function deleteSequence(): Promise<void> {
  const id = appStore.activeSequenceId
  if (
    !id ||
    isDeletingSequence.value ||
    !window.confirm('确认删除整个自定义序列？删除后无法撤销。')
  ) return

  const idx = appStore.customSequences.findIndex(s => s.id === id)
  if (idx === -1) return
  isDeletingSequence.value = true
  appStore.customSequences.splice(idx, 1)
  try {
    await persistConfig()
    await backToList()
  } catch {
    // 配置事务已把被删除序列恢复到最后成功快照，停留详情页便于重试。
  } finally {
    isDeletingSequence.value = false
  }
}

// ---- 添加动作 ----
function addKeyboardAction(): void {
  if (!sequence.value || !capturedKey.value) return
  const action: CustomAction = {
    kind: 'keyboard',
    id: `ck-${Date.now()}`,
    enabled: true,
    keyLabel: capturedKey.value.keyLabel,
    scanCode: capturedKey.value.scanCode,
    intervalMs: DEFAULT_INTERVAL_MS,
  }
  sequence.value.actions.push(action)
  capturedKey.value = null
  persist()
}

function addMouseAction(): void {
  if (!sequence.value) return
  const action: CustomAction = {
    kind: 'mouse',
    id: `cm-${Date.now()}`,
    enabled: true,
    actionType: 'click_left',
    x: null,
    y: null,
    intervalMs: DEFAULT_INTERVAL_MS,
  }
  sequence.value.actions.push(action)
  persist()
}

function onMouseButtonChange(action: CustomAction, e: Event): void {
  if (action.kind !== 'mouse') return
  const value = (e.target as HTMLSelectElement).value as MouseActionType
  action.actionType = value
  persist()
}

// ---- 行操作 ----
function deleteAction(id: string): void {
  if (!sequence.value) return
  const idx = sequence.value.actions.findIndex(a => a.id === id)
  if (idx !== -1) sequence.value.actions.splice(idx, 1)
  persist()
}

function moveUp(index: number): void {
  if (!sequence.value || index <= 0) return
  const arr = sequence.value.actions
  ;[arr[index - 1], arr[index]] = [arr[index], arr[index - 1]]
  persist()
}

function moveDown(index: number): void {
  if (!sequence.value || index >= sequence.value.actions.length - 1) return
  const arr = sequence.value.actions
  ;[arr[index + 1], arr[index]] = [arr[index], arr[index + 1]]
  persist()
}

function startPickPosition(id: string): void {
  invoke('start_pick_mouse_position', { rowId: id }).catch((err) => {
    console.error('[CustomDetailView] 坐标拾取启动失败:', err)
  })
}

function onIntervalInput(action: CustomAction, e: Event): void {
  const target = e.target as HTMLInputElement
  const sanitized = target.value.replace(/[^0-9]/g, '')
  if (target.value !== sanitized) target.value = sanitized
  const num = parseInt(sanitized, 10)
  if (!isNaN(num) && num > 0) action.intervalMs = num
}

function onIntervalCommit(action: CustomAction, e: Event): void {
  const target = e.target as HTMLInputElement
  const num = parseInt(target.value, 10)
  if (isNaN(num) || num <= 0) {
    action.intervalMs = DEFAULT_INTERVAL_MS
    target.value = String(DEFAULT_INTERVAL_MS)
  } else {
    target.value = String(num)
  }
  persist()
}
</script>

<template>
  <section class="detail-page">
    <header class="detail-header">
      <button type="button" class="icon-btn" aria-label="返回" @click="backToList">
        <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
          <path d="M10 3 L5 8 L10 13" stroke="currentColor" stroke-width="1.6"
            stroke-linecap="round" stroke-linejoin="round" fill="none" />
        </svg>
      </button>
      <input
        v-if="sequence"
        type="text"
        class="name-input"
        :value="sequence.name"
        aria-label="序列名称"
        @input="onNameInput"
        @blur="onNameCommit"
        @keydown.enter="onNameCommit"
      />
      <button
        type="button"
        class="delete-seq-btn"
        :disabled="isDeletingSequence"
        @click="deleteSequence"
      >
        {{ isDeletingSequence ? '正在删除…' : '删除序列' }}
      </button>
    </header>

    <div class="top-bar">
      <KeyCaptureInput v-model="capturedKey" placeholder="点击捕获按键" />
      <button type="button" class="add-btn" :disabled="!capturedKey" @click="addKeyboardAction">
        + 添加按键
      </button>
      <button type="button" class="add-btn secondary" @click="addMouseAction">
        + 添加鼠标
      </button>
    </div>

    <p v-if="conflictActionId" class="conflict-message">该动作与全局热键冲突，请修改后再启动。</p>

    <div class="list-container">
      <div v-if="!sequence || !sequence.actions.length" class="empty-hint">
        暂无动作，添加按键或鼠标动作
      </div>
      <div v-else class="list-scroll">
        <div
          v-for="(action, index) in sequence.actions"
          :key="action.id"
          class="list-row"
          :class="{ disabled: !action.enabled, conflict: conflictActionId === action.id }"
        >
          <label class="action-switch" :title="action.enabled ? '已启用' : '已停用'">
            <input v-model="action.enabled" type="checkbox" @change="persist" />
            <span class="switch-track"><span class="switch-thumb"></span></span>
          </label>

          <!-- 键盘行：显示键位徽标 -->
          <span v-if="action.kind === 'keyboard'" class="key-badge">
            {{ action.keyLabel }}
          </span>
          <!-- 鼠标行：按钮类型下拉 + 坐标区 + 拾取按钮 -->
          <div v-else class="mouse-row-content">
            <select
              class="mouse-button-select"
              :value="action.actionType"
              @change="onMouseButtonChange(action, $event)"
            >
              <option value="click_left">左键</option>
              <option value="click_middle">中键</option>
              <option value="click_right">右键</option>
            </select>
            <span class="coord-text">
              <template v-if="action.x !== null && action.y !== null">
                {{ action.x }} - {{ action.y }}
              </template>
              <template v-else>
                X - Y
              </template>
            </span>
            <button
              type="button"
              class="pick-btn"
              @click="startPickPosition(action.id)"
            >
              拾取
            </button>
          </div>

          <div class="interval-group">
            <span class="interval-label">间隔</span>
            <input
              type="text"
              inputmode="numeric"
              class="interval-input"
              :value="action.intervalMs"
              @input="onIntervalInput(action, $event)"
              @blur="onIntervalCommit(action, $event)"
              @keydown.enter="onIntervalCommit(action, $event)"
            />
            <span class="unit">ms</span>
          </div>

          <button
            type="button"
            class="move-btn"
            aria-label="上移"
            :disabled="index === 0"
            @click="moveUp(index)"
          >
            <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
              <path d="M3 7 L6 4 L9 7" stroke="currentColor" stroke-width="1.5"
                stroke-linecap="round" stroke-linejoin="round" fill="none" />
            </svg>
          </button>
          <button
            type="button"
            class="move-btn"
            aria-label="下移"
            :disabled="index === sequence.actions.length - 1"
            @click="moveDown(index)"
          >
            <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
              <path d="M3 5 L6 8 L9 5" stroke="currentColor" stroke-width="1.5"
                stroke-linecap="round" stroke-linejoin="round" fill="none" />
            </svg>
          </button>
          <button type="button" class="delete-btn" aria-label="删除" @click="deleteAction(action.id)">
            <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
              <path d="M3 3 L11 11 M11 3 L3 11" stroke="currentColor"
                stroke-width="1.5" stroke-linecap="round" />
            </svg>
          </button>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
.detail-page {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: 14px 16px;
  gap: 12px;
  overflow: hidden;
}

.detail-header {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-shrink: 0;
}

.icon-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  border-radius: 6px;
  color: var(--text-secondary);
  transition: background var(--transition-fast) var(--ease-default);
}

.icon-btn:hover {
  background: var(--bg-elevated);
  color: var(--text-primary);
}

.name-input {
  flex: 1;
  height: 30px;
  padding: 0 10px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-subtle);
  border-radius: 6px;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
  transition: border-color var(--transition-fast) var(--ease-default);
}

.name-input:focus {
  outline: none;
  border-color: var(--accent);
}

.delete-seq-btn {
  height: 30px;
  padding: 0 12px;
  border-radius: 6px;
  font-size: 12px;
  color: var(--danger);
  border: 1px solid var(--border-subtle);
  transition: background var(--transition-fast) var(--ease-default);
}

.delete-seq-btn:hover {
  background: var(--bg-elevated);
}

.top-bar {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-shrink: 0;
}

.add-btn {
  height: 30px;
  padding: 0 14px;
  border-radius: 6px;
  background: var(--accent);
  color: var(--color-paper-white);
  font-size: 12px;
  font-weight: 500;
  transition:
    background var(--transition-fast) var(--ease-default),
    opacity var(--transition-fast) var(--ease-default);
}

.add-btn:hover:not(:disabled) {
  background: var(--accent-hover);
}

.add-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.add-btn.secondary {
  background: var(--bg-elevated);
  color: var(--text-primary);
  border: 1px solid var(--border-subtle);
}

.add-btn.secondary:hover:not(:disabled) {
  background: var(--bg-primary);
}

.list-container {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.empty-hint {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  font-size: 12px;
  color: var(--text-disabled);
}

.list-scroll {
  display: flex;
  flex-direction: column;
  gap: 8px;
  overflow-y: auto;
  scrollbar-gutter: stable;
}

.list-row {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 40px;
  min-height: 40px;
  flex-shrink: 0;
  padding: 0 14px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-subtle);
  border-radius: 8px;
  transition: opacity var(--transition-fast) var(--ease-default);
}

.action-switch {
  position: relative;
  width: 30px;
  height: 18px;
  flex-shrink: 0;
  cursor: pointer;
}

.action-switch input {
  position: absolute;
  opacity: 0;
  pointer-events: none;
}

.switch-track {
  display: block;
  width: 30px;
  height: 18px;
  padding: 2px;
  background: var(--border-color);
  border-radius: 9px;
  transition: background var(--transition-fast) var(--ease-default);
}

.switch-thumb {
  display: block;
  width: 14px;
  height: 14px;
  background: var(--color-paper-white);
  border-radius: 50%;
  transition: transform var(--transition-fast) var(--ease-default);
}

.action-switch input:checked + .switch-track {
  background: var(--accent);
}

.action-switch input:checked + .switch-track .switch-thumb {
  transform: translateX(12px);
}

.list-row.disabled > :not(.action-switch) {
  opacity: 0.48;
}

.key-badge {
  flex: 1;
  min-width: 0;
  height: 30px;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0 16px;
  border-radius: 6px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-subtle);
  font-size: 15px;
  font-weight: 600;
  color: var(--text-primary);
  letter-spacing: 0.5px;
}

.coord-area {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 12px;
}

.mouse-row-content {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 8px;
}

.mouse-button-select {
  width: 70px;
  height: 28px;
  padding: 0 6px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-subtle);
  border-radius: 5px;
  font-size: 12px;
  font-weight: 500;
  color: var(--text-primary);
  cursor: pointer;
  transition: border-color var(--transition-fast) var(--ease-default);
  flex-shrink: 0;
}

.mouse-button-select:hover {
  border-color: var(--accent);
}

.mouse-button-select:focus {
  outline: none;
  border-color: var(--accent);
}

.coord-text {
  flex: 1;
  min-width: 0;
  font-family: 'Consolas', 'Courier New', monospace;
  font-size: 12px;
  color: var(--text-primary);
  font-weight: 500;
  text-align: center;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.pick-btn {
  width: 70px;
  height: 28px;
  padding: 0 10px;
  border-radius: 5px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-color);
  color: var(--text-primary);
  font-size: 12px;
  font-weight: 500;
  white-space: nowrap;
  flex-shrink: 0;
  transition:
    background var(--transition-fast) var(--ease-default),
    border-color var(--transition-fast) var(--ease-default);
}

.pick-btn:hover {
  background: var(--bg-secondary);
  border-color: var(--accent);
}

.pick-btn:active {
  background: var(--bg-primary);
}

.interval-group {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-left: auto;
  padding: 4px 10px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-subtle);
  border-radius: 14px;
  flex-shrink: 0;
}

.interval-label {
  font-size: 11px;
  color: var(--text-secondary);
}

.interval-input {
  width: 60px;
  height: 24px;
  padding: 0 6px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-subtle);
  border-radius: 4px;
  font-size: 12px;
  font-weight: 500;
  color: var(--text-primary);
  text-align: center;
  transition: border-color var(--transition-fast) var(--ease-default);
}

.interval-input:focus {
  outline: none;
  border-color: var(--accent);
}

.unit {
  font-size: 11px;
  color: var(--text-disabled);
  font-weight: 500;
}

.move-btn,
.delete-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border-radius: 5px;
  color: var(--text-secondary);
  flex-shrink: 0;
  transition:
    background var(--transition-fast) var(--ease-default),
    color var(--transition-fast) var(--ease-default);
}

.move-btn:hover:not(:disabled) {
  background: var(--bg-elevated);
  color: var(--text-primary);
}

.move-btn:disabled {
  opacity: 0.3;
  cursor: not-allowed;
}

.delete-btn:hover {
  background: var(--bg-elevated);
  color: var(--danger);
}

.list-scroll::-webkit-scrollbar {
  width: 8px;
}

.list-scroll::-webkit-scrollbar-track {
  background: transparent;
}

.list-scroll::-webkit-scrollbar-thumb {
  background: var(--border-color);
  border-radius: 4px;
}

.list-scroll::-webkit-scrollbar-thumb:hover {
  background: var(--text-disabled);
}

.conflict-message {
  margin: 0;
  color: var(--warning);
  font-size: 11px;
}

.list-row.conflict {
  border-color: var(--warning);
  box-shadow: 0 0 0 1px color-mix(in srgb, var(--warning) 45%, transparent);
}</style>
