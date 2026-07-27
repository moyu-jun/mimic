<script setup lang="ts">
/**
 * 按键模拟页 — 需求 3.3.2 / DESIGN 15.6
 * 列表：勾选 / 键位 / 间隔 / 删除；顶部：捕获框 + 添加按钮。
 * 数据通过 load_config 加载，persist_config 持久化。
 * 阶段 12：移除 onMounted/onBeforeUnmount 中的状态切换，由 set_current_page 统一管理。
 */
import { ref, onBeforeUnmount } from 'vue'
import { appStore } from '../stores/appStore'
import KeyCaptureInput from '../components/KeyCaptureInput.vue'
import type { CapturedKey, KeyboardConfig } from '../types/config'
import { persistConfig } from '../lib/configUtil'

const DEFAULT_INTERVAL_MS = 20

const capturedKey = ref<CapturedKey | null>(null)
const duplicateHint = ref('')
let duplicateTimer: number | null = null

function showDuplicateHint(msg: string): void {
  duplicateHint.value = msg
  if (duplicateTimer !== null) window.clearTimeout(duplicateTimer)
  duplicateTimer = window.setTimeout(() => {
    duplicateHint.value = ''
    duplicateTimer = null
  }, 2000)
}

function addAction(): void {
  if (!capturedKey.value) return

  // 已存在相同键位则拒绝添加并提示
  const exists = appStore.keyboardConfigs.some(
    a => a.scanCode === capturedKey.value!.scanCode
  )
  if (exists) {
    showDuplicateHint(`按键「${capturedKey.value.keyLabel}」已存在`)
    capturedKey.value = null
    return
  }

  const newConfig: KeyboardConfig = {
    id: `kb-${Date.now()}`,
    enabled: false,
    keyLabel: capturedKey.value.keyLabel,
    scanCode: capturedKey.value.scanCode,
    intervalMs: DEFAULT_INTERVAL_MS,
  }

  appStore.keyboardConfigs.push(newConfig)
  capturedKey.value = null

  // 结构性变更：立即持久化
  persistConfig().catch(() => {
    // 错误已在 configUtil 中记录，不阻塞用户操作
  })
}

function deleteAction(id: string): void {
  const idx = appStore.keyboardConfigs.findIndex(a => a.id === id)
  if (idx !== -1) {
    appStore.keyboardConfigs.splice(idx, 1)

    // 结构性变更：立即持久化
    persistConfig().catch(() => {
      // 错误已在 configUtil 中记录，不阻塞用户操作
    })
  }
}

function toggleSelected(id: string): void {
  const action = appStore.keyboardConfigs.find(a => a.id === id)
  if (action) {
    action.enabled = !action.enabled

    // 结构性变更：立即持久化
    persistConfig().catch(() => {
      // 错误已在 configUtil 中记录，不阻塞用户操作
    })
  }
}

function onIntervalInput(action: KeyboardConfig, e: Event): void {
  const target = e.target as HTMLInputElement
  // 仅剥离非数字字符；允许中间态为空（用户清空后准备重新输入）
  const sanitized = target.value.replace(/[^0-9]/g, '')
  if (target.value !== sanitized) target.value = sanitized
  const num = parseInt(sanitized, 10)
  if (!isNaN(num) && num > 0) action.intervalMs = num
}

function onIntervalCommit(action: KeyboardConfig, e: Event): void {
  const target = e.target as HTMLInputElement
  const num = parseInt(target.value, 10)
  if (isNaN(num) || num <= 0) {
    action.intervalMs = DEFAULT_INTERVAL_MS
    target.value = String(DEFAULT_INTERVAL_MS)
  } else {
    target.value = String(num)
  }

  // 数字输入提交：失焦/回车时持久化
  persistConfig().catch(() => {
    // 错误已在 configUtil 中记录，不阻塞用户操作
  })
}

function moveUp(index: number): void {
  if (index <= 0) return
  const arr = appStore.keyboardConfigs
  ;[arr[index - 1], arr[index]] = [arr[index], arr[index - 1]]
  persistConfig().catch(() => {
    // 错误已在 configUtil 中记录，不阻塞用户操作
  })
}

function moveDown(index: number): void {
  if (index >= appStore.keyboardConfigs.length - 1) return
  const arr = appStore.keyboardConfigs
  ;[arr[index + 1], arr[index]] = [arr[index], arr[index + 1]]
  persistConfig().catch(() => {
    // 错误已在 configUtil 中记录，不阻塞用户操作
  })
}

onBeforeUnmount(() => {
  if (duplicateTimer !== null) window.clearTimeout(duplicateTimer)
})
</script>
<template>
  <section class="keyboard-page">
    <header class="top-bar">
      <KeyCaptureInput v-model="capturedKey" placeholder="点击捕获按键" />
      <button
        type="button"
        class="add-btn"
        :disabled="!capturedKey"
        @click="addAction"
      >
        添加
      </button>
      <span v-if="duplicateHint" class="duplicate-hint">{{ duplicateHint }}</span>
    </header>

    <div class="list-container">
      <div v-if="!appStore.keyboardConfigs.length" class="empty-hint">
        暂无按键动作
      </div>
      <div v-else class="list-scroll">
        <div
          v-for="(action, index) in appStore.keyboardConfigs"
          :key="action.id"
          class="list-row"
          :class="{ disabled: !action.enabled }"
        >
          <label class="checkbox-wrapper">
            <input
              type="checkbox"
              class="checkbox"
              :checked="action.enabled"
              :aria-label="`选择 ${action.keyLabel} 按键动作`"
              @change="toggleSelected(action.id)"
            />
          </label>
          <span class="key-badge">{{ action.keyLabel }}</span>
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
              <path
                d="M3 7 L6 4 L9 7"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
                stroke-linejoin="round"
                fill="none"
              />
            </svg>
          </button>
          <button
            type="button"
            class="move-btn"
            aria-label="下移"
            :disabled="index === appStore.keyboardConfigs.length - 1"
            @click="moveDown(index)"
          >
            <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
              <path
                d="M3 5 L6 8 L9 5"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
                stroke-linejoin="round"
                fill="none"
              />
            </svg>
          </button>
          <button
            type="button"
            class="delete-btn"
            aria-label="删除"
            @click="deleteAction(action.id)"
          >
            <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
              <path
                d="M3 3 L11 11 M11 3 L3 11"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
              />
            </svg>
          </button>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
.keyboard-page {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: 14px 16px;
  gap: 12px;
  overflow: hidden;
}

.top-bar {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-shrink: 0;
}

.add-btn {
  height: 30px;
  padding: 0 16px;
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

.add-btn:active:not(:disabled) {
  background: var(--accent-pressed);
}

.add-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.duplicate-hint {
  font-size: 11px;
  color: var(--warning);
  white-space: nowrap;
  animation: fade-in var(--transition-normal) var(--ease-default);
}

@keyframes fade-in {
  from { opacity: 0; transform: translateY(2px); }
  to { opacity: 1; transform: translateY(0); }
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
  gap: 16px;
  height: 44px;
  min-height: 44px;
  flex-shrink: 0;
  padding: 0 18px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-subtle);
  border-radius: 8px;
  transition: opacity var(--transition-fast) var(--ease-default);
}

.list-row.disabled {
  opacity: 0.5;
}

.checkbox-wrapper {
  display: flex;
  align-items: center;
  cursor: pointer;
}

.checkbox {
  width: 18px;
  height: 18px;
  cursor: pointer;
  accent-color: var(--accent);
}

.key-badge {
  min-width: 100px;
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

.interval-group {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-left: auto;
  padding: 5px 14px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-subtle);
  border-radius: 16px;
}

.interval-label {
  font-size: 12px;
  color: var(--text-secondary);
}

.interval-input {
  width: 84px;
  height: 26px;
  padding: 0 8px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-subtle);
  border-radius: 5px;
  font-size: 13px;
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
  font-size: 12px;
  color: var(--text-disabled);
  font-weight: 500;
}

.move-btn,
.delete-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border-radius: 6px;
  color: var(--text-secondary);
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

.delete-btn:active {
  background: var(--bg-primary);
}

/* 滚动条样式 */
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
</style>
