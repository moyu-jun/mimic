<script setup lang="ts">
/**
 * 鼠标模拟页 — 需求 3.3.3 / DESIGN 15.6
 * 表格六列：启用 / 类型 / X坐标 / Y坐标 / 时间间隔 / 操作（坐标拾取 + 删除）。
 * 表头固定（sticky），数据行滚动。
 * 阶段 12：移除 onMounted/onBeforeUnmount 中的状态切换，由 set_current_page 统一管理。
 * 阶段 14：坐标拾取接真实命令 start_pick_mouse_position，监听 mouse_position_picked 回填并持久化。
 * 维护迭代：数据模型新增 actionType（左/右/中键单击）与 enabled 开关，列表页按新模型组织；
 *           界面按 800×600 窗口重新布局。
 */
import { onMounted, onBeforeUnmount } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { appStore } from '../stores/appStore'
import type { MouseActionType, MouseConfig } from '../types/config'
import { persistConfig } from '../lib/configUtil'

const DEFAULT_INTERVAL_MS = 20

// 列表页支持的动作类型（三种单击）。
const ACTION_TYPE_OPTIONS: { value: MouseActionType; label: string }[] = [
  { value: 'click_left', label: '左键单击' },
  { value: 'click_right', label: '右键单击' },
  { value: 'click_middle', label: '中键单击' },
]

let unlistenPicked: UnlistenFn | null = null

// 阶段 14：监听坐标拾取完成事件，回填对应行 X/Y 并持久化
onMounted(async () => {
  unlistenPicked = await listen<{ rowId: string; x: number; y: number }>(
    'mouse_position_picked',
    (event) => {
      const { rowId, x, y } = event.payload
      const action = appStore.mouseConfigs.find(a => a.id === rowId)
      if (!action) return
      action.x = x
      action.y = y
      persistConfig().catch(() => {
        // 错误已在 configUtil 中记录，不阻塞用户操作
      })
    },
  )
})

onBeforeUnmount(() => {
  if (unlistenPicked) unlistenPicked()
})

function addAction(): void {
  const newConfig: MouseConfig = {
    id: `mouse-${Date.now()}`,
    enabled: true,
    actionType: 'click_left',
    x: null,
    y: null,
    intervalMs: DEFAULT_INTERVAL_MS,
  }

  appStore.mouseConfigs.push(newConfig)

  persistConfig().catch(() => {})
}

function deleteAction(id: string): void {
  const idx = appStore.mouseConfigs.findIndex(a => a.id === id)
  if (idx !== -1) {
    appStore.mouseConfigs.splice(idx, 1)

    // 结构性变更：立即持久化
    persistConfig().catch(() => {
      // 错误已在 configUtil 中记录，不阻塞用户操作
    })
  }
}

function toggleEnabled(action: MouseConfig): void {
  action.enabled = !action.enabled
  persistConfig().catch(() => {
    // 错误已在 configUtil 中记录，不阻塞用户操作
  })
}

function onActionTypeChange(action: MouseConfig, e: Event): void {
  action.actionType = (e.target as HTMLSelectElement).value as MouseActionType
  persistConfig().catch(() => {
    // 错误已在 configUtil 中记录，不阻塞用户操作
  })
}

function onIntervalInput(action: MouseConfig, e: Event): void {
  const target = e.target as HTMLInputElement
  // 仅剥离非数字字符；允许中间态为空（用户清空后准备重新输入）
  const sanitized = target.value.replace(/[^0-9]/g, '')
  if (target.value !== sanitized) target.value = sanitized
  const num = parseInt(sanitized, 10)
  if (!isNaN(num) && num > 0) action.intervalMs = num
}

function onIntervalCommit(action: MouseConfig, e: Event): void {
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

function startPickPosition(id: string): void {
  // 阶段 14：进入拾取 — 后端隐藏窗口并监听一次全局左键点击
  invoke('start_pick_mouse_position', { rowId: id }).catch((err) => {
    console.error('[MousePage] 坐标拾取启动失败:', err)
  })
}
</script>

<template>
  <section class="mouse-page">
    <div class="table-scroll">
      <div class="table-header">
        <div class="th">启用</div>
        <div class="th">操作类型</div>
        <div class="th">X 坐标</div>
        <div class="th">Y 坐标</div>
        <div class="th">时间间隔</div>
        <div class="th">操作</div>
      </div>

      <div v-if="!appStore.mouseConfigs.length" class="empty-hint">
        暂无鼠标动作
      </div>
      <div
        v-for="action in appStore.mouseConfigs"
        v-else
        :key="action.id"
        class="table-row"
        :class="{ disabled: !action.enabled }"
      >
        <div class="td enabled-cell">
          <input
            type="checkbox"
            class="checkbox"
            :checked="action.enabled"
            aria-label="启用此动作"
            @change="toggleEnabled(action)"
          />
        </div>
        <div class="td type-cell">
          <select
            class="type-select"
            :value="action.actionType"
            aria-label="动作类型"
            @change="onActionTypeChange(action, $event)"
          >
            <option
              v-for="opt in ACTION_TYPE_OPTIONS"
              :key="opt.value"
              :value="opt.value"
            >
              {{ opt.label }}
            </option>
          </select>
        </div>
        <div class="td coord-cell">
          {{ action.x !== null ? action.x : '—' }}
        </div>
        <div class="td coord-cell">
          {{ action.y !== null ? action.y : '—' }}
        </div>
        <div class="td interval-cell">
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
        <div class="td actions-cell">
          <button
            type="button"
            class="pick-btn"
            @click="startPickPosition(action.id)"
          >
            坐标拾取
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

    <footer class="bottom-bar">
      <button type="button" class="add-btn" @click="addAction">添加</button>
    </footer>
  </section>
</template>

<style scoped>
.mouse-page {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: 16px 18px;
  gap: 14px;
  overflow: hidden;
}

.table-scroll {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  scrollbar-gutter: stable;
  border: 1px solid var(--border-subtle);
  border-radius: 7px;
}

/* 表头与数据行共用网格列宽，保证对齐 */
.table-header,
.table-row {
  display: grid;
  grid-template-columns: 48px 104px 1fr 1fr 128px 108px;
  gap: 10px;
  align-items: center;
  padding: 0 16px;
}

.table-header {
  position: sticky;
  top: 0;
  z-index: 1;
  height: 36px;
  background: var(--bg-elevated);
  border-bottom: 1px solid var(--border-subtle);
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
}

.th {
  text-align: center;
  letter-spacing: 0.3px;
}

.th:last-child {
  text-align: left;
}

.table-row {
  height: 44px;
  min-height: 44px;
  border-bottom: 1px solid var(--border-subtle);
  background: var(--bg-secondary);
  transition: opacity var(--transition-fast) var(--ease-default);
}

.table-row:last-child {
  border-bottom: none;
}

.table-row.disabled {
  opacity: 0.5;
}

.td {
  font-size: 13px;
  color: var(--text-primary);
}

.enabled-cell {
  display: flex;
  align-items: center;
  justify-content: center;
}

.checkbox {
  width: 18px;
  height: 18px;
  cursor: pointer;
  accent-color: var(--accent);
}

.type-cell {
  display: flex;
  align-items: center;
}

.type-select {
  width: 100%;
  height: 28px;
  padding: 0 8px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-subtle);
  border-radius: 5px;
  font-size: 12px;
  color: var(--text-primary);
  cursor: pointer;
  transition: border-color var(--transition-fast) var(--ease-default);
}

.type-select:focus {
  outline: none;
  border-color: var(--accent);
}

.coord-cell {
  text-align: center;
  font-family: 'Consolas', 'Courier New', monospace;
}

.interval-cell {
  display: flex;
  align-items: center;
  gap: 4px;
  justify-content: center;
}

.actions-cell {
  display: flex;
  align-items: center;
  gap: 8px;
}

.empty-hint {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  font-size: 13px;
  color: var(--text-disabled);
}

.interval-input {
  width: 60px;
  height: 26px;
  padding: 0 8px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-subtle);
  border-radius: 5px;
  font-size: 13px;
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
}

.pick-btn {
  height: 26px;
  padding: 0 12px;
  border-radius: 5px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-color);
  color: var(--text-primary);
  font-size: 12px;
  font-weight: 500;
  white-space: nowrap;
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

.delete-btn:hover {
  background: var(--bg-elevated);
  color: var(--danger);
}

.delete-btn:active {
  background: var(--bg-primary);
}

.bottom-bar {
  flex-shrink: 0;
  display: flex;
  justify-content: center;
}

.add-btn {
  height: 34px;
  min-width: 180px;
  padding: 0 40px;
  border-radius: 6px;
  background: var(--accent);
  color: var(--color-paper-white);
  font-size: 13px;
  font-weight: 500;
  letter-spacing: 1px;
  transition: background var(--transition-fast) var(--ease-default);
}

.add-btn:hover {
  background: var(--accent-hover);
}

.add-btn:active {
  background: var(--accent-pressed);
}

/* 滚动条样式 */
.table-scroll::-webkit-scrollbar {
  width: 8px;
}

.table-scroll::-webkit-scrollbar-track {
  background: transparent;
}

.table-scroll::-webkit-scrollbar-thumb {
  background: var(--border-color);
  border-radius: 4px;
}

.table-scroll::-webkit-scrollbar-thumb:hover {
  background: var(--text-disabled);
}
</style>
