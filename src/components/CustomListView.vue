<script setup lang="ts">
/**
 * 自定义序列卡片列表 — 自定义页的初始视图。
 * 第一张固定为「+」卡片，点击新建序列（自动默认名）并进入详情页；
 * 其余卡片显示序列名称，点击进入对应详情页。
 * 本视图不触发热键（后端处于 Idle，热键被状态机门控挡下）。
 */
import { invoke } from '@tauri-apps/api/core'
import { appStore } from '../stores/appStore'
import type { CustomSequence } from '../types/config'
import { persistConfig } from '../lib/configUtil'

/** 生成不与现有序列重名的默认名「未命名序列 N」。 */
function nextDefaultName(): string {
  let n = appStore.customSequences.length + 1
  const names = new Set(appStore.customSequences.map(s => s.name))
  while (names.has(`未命名序列 ${n}`)) n++
  return `未命名序列 ${n}`
}

async function enterDetail(id: string): Promise<void> {
  appStore.activeSequenceId = id
  appStore.customView = 'detail'
  // 通知后端进入详情页 → 激活该序列 + ReadyCustom（热键在详情页生效）
  try {
    await invoke('enter_custom_sequence', { id })
  } catch (err) {
    console.error('[CustomListView] enter_custom_sequence 失败:', err)
  }
}

async function createSequence(): Promise<void> {
  const seq: CustomSequence = {
    id: `seq-${Date.now()}`,
    name: nextDefaultName(),
    actions: [],
  }
  appStore.customSequences.push(seq)
  // 结构性变更只有在后端原子写盘确认后才进入详情页；失败由事务工具回滚。
  try {
    await persistConfig()
  } catch {
    return
  }
  await enterDetail(seq.id)
}
</script>

<template>
  <section class="custom-list">
    <div class="card-grid">
      <!-- 固定的新建卡片 -->
      <button type="button" class="card card-add" @click="createSequence">
        <svg width="28" height="28" viewBox="0 0 28 28" aria-hidden="true">
          <path
            d="M14 6 V22 M6 14 H22"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
          />
        </svg>
        <span class="card-add-label">新建序列</span>
      </button>

      <!-- 已保存序列卡片 -->
      <button
        v-for="seq in appStore.customSequences"
        :key="seq.id"
        type="button"
        class="card card-seq"
        @click="enterDetail(seq.id)"
      >
        <span class="seq-name" :title="seq.name">{{ seq.name }}</span>
        <span class="seq-meta">{{ seq.actions.length }} 个动作</span>
      </button>
    </div>
  </section>
</template>

<style scoped>
.custom-list {
  height: 100%;
  padding: 16px;
  overflow-y: auto;
  scrollbar-gutter: stable;
}

.card-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(130px, 1fr));
  gap: 12px;
}

.card {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  height: 92px;
  padding: 10px;
  border-radius: 9px;
  border: 1px solid var(--border-subtle);
  background: var(--bg-secondary);
  color: var(--text-primary);
  cursor: pointer;
  transition:
    border-color var(--transition-fast) var(--ease-default),
    background var(--transition-fast) var(--ease-default),
    transform var(--transition-fast) var(--ease-default);
}

.card:hover {
  border-color: var(--accent);
  background: var(--bg-elevated);
}

.card:active {
  transform: scale(0.98);
}

.card-add {
  color: var(--text-secondary);
  border-style: dashed;
}

.card-add:hover {
  color: var(--accent);
}

.card-add-label {
  font-size: 12px;
}

.card-seq {
  align-items: flex-start;
  justify-content: space-between;
  text-align: left;
}

.seq-name {
  font-size: 13px;
  font-weight: 600;
  width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.seq-meta {
  font-size: 11px;
  color: var(--text-disabled);
}
</style>
