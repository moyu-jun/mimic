<script setup lang="ts">
/**
 * 底部状态栏 — DESIGN 15.3-bis / 需求 3.10
 * 圆点 + 文案，颜色与文案按 runtimeStatus 映射。
 * 右侧：实时同步当前热键（启动 / 停止），随 appStore.hotkeys 变化即时刷新。
 */
import { computed } from 'vue'
import { appStore } from '../stores/appStore'
import type { RuntimeStatus } from '../types/config'

interface StatusMeta {
  /** 圆点颜色对应的 CSS 变量名 */
  colorVar: string
  label: string
}

const STATUS_MAP: Record<RuntimeStatus, StatusMeta> = {
  Idle: { colorVar: 'var(--text-disabled)', label: '待机' },
  ReadyKeyboard: { colorVar: 'var(--text-secondary)', label: '当前可启动按键模拟' },
  ReadyMouse: { colorVar: 'var(--text-secondary)', label: '当前可启动鼠标模拟' },
  ReadyCustom: { colorVar: 'var(--text-secondary)', label: '当前可启动自定义序列' },
  RunningKeyboard: { colorVar: 'var(--success)', label: '按键模拟运行中' },
  RunningMouse: { colorVar: 'var(--success)', label: '鼠标模拟运行中' },
  RunningCustom: { colorVar: 'var(--success)', label: '自定义序列运行中' },
  PickingMouse: { colorVar: 'var(--warning)', label: '正在拾取鼠标坐标' },
  Recording: { colorVar: 'var(--warning)', label: '正在录制提示音' },
  Error: { colorVar: 'var(--danger)', label: '错误' },
}

const meta = computed(() => STATUS_MAP[appStore.runtimeStatus])
</script>

<template>
  <footer class="statusbar">
    <span class="dot" :style="{ background: meta.colorVar }"></span>
    <span class="status-text">{{ meta.label }}</span>
    <span class="hotkey-info">
      启动：<b>{{ appStore.hotkeys.start.keyLabel }}</b>
      <span class="sep">|</span>
      停止：<b>{{ appStore.hotkeys.stop.keyLabel }}</b>
    </span>
  </footer>
</template>

<style scoped>
.statusbar {
  display: flex;
  align-items: center;
  gap: 8px;
  height: var(--statusbar-height);
  padding: 0 14px;
  border-top: 1px solid var(--border-subtle);
  flex-shrink: 0;
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  transition: background var(--transition-normal) var(--ease-default);
}

.status-text {
  font-size: 12px;
  color: var(--text-secondary);
}

.hotkey-info {
  margin-left: auto;
  font-size: 12px;
  color: var(--text-secondary);
}

.hotkey-info b {
  font-weight: 600;
  color: var(--text-primary);
}

.hotkey-info .sep {
  margin: 0 6px;
  color: var(--text-disabled);
}
</style>
