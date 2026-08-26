<script setup lang="ts">
/**
 * 左侧菜单 — 需求 3.2
 * 主菜单（首页 / 按键模拟 / 鼠标模拟）置顶，「设置」固定底部。
 * 激活项使用强调色左条 + 高亮背景；菜单项左侧带线性图标。
 */
import { computed } from 'vue'
import { appStore, setPage } from '../stores/appStore'
import { PAGE_LABELS, MAIN_PAGES } from '../lib/pages'
import type { AppPage } from '../types/config'
import MenuIcon from './MenuIcon.vue'

const navigationBlocked = computed(() =>
  ['RunningKeyboard', 'RunningMouse', 'RunningCustom', 'PickingMouse', 'Recording'].includes(
    appStore.runtimeStatus,
  ),
)

function isActive(page: AppPage): boolean {
  return appStore.currentPage === page
}
</script>

<template>
  <nav class="sidebar" aria-label="主菜单">
    <div class="menu-top">
      <button
        v-for="page in MAIN_PAGES"
        :key="page"
        type="button"
        class="menu-item"
        :class="{ active: isActive(page) }"
        :aria-current="isActive(page) ? 'page' : undefined"
        :disabled="navigationBlocked"
        @click="setPage(page)"
      >
        <MenuIcon :name="page" />
        <span class="menu-label">{{ PAGE_LABELS[page] }}</span>
      </button>
    </div>
    <div class="menu-bottom">
      <button
        type="button"
        class="menu-item"
        :class="{ active: isActive('settings') }"
        :aria-current="isActive('settings') ? 'page' : undefined"
        :disabled="navigationBlocked"
        @click="setPage('settings')"
      >
        <MenuIcon name="settings" />
        <span class="menu-label">{{ PAGE_LABELS.settings }}</span>
      </button>
    </div>
  </nav>
</template>

<style scoped>
.sidebar {
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  width: var(--sidebar-width);
  flex-shrink: 0;
  padding: 10px 8px;
  border-right: 1px solid var(--border-subtle);
}

.menu-top,
.menu-bottom {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.menu-item {
  position: relative;
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  height: 34px;
  padding: 0 12px;
  border-radius: 7px;
  font-size: 13px;
  color: var(--text-secondary);
  text-align: left;
  transition:
    background var(--transition-fast) var(--ease-default),
    color var(--transition-fast) var(--ease-default);
}

.menu-label {
  flex: 1;
  min-width: 0;
}

.menu-item:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.menu-item:hover:not(:disabled) {
  background: var(--bg-elevated);
  color: var(--text-primary);
}

.menu-item:active {
  background: var(--bg-secondary);
}

.menu-item.active {
  background: var(--bg-secondary);
  color: var(--text-primary);
}

.menu-item.active::before {
  content: '';
  position: absolute;
  left: 0;
  top: 50%;
  transform: translateY(-50%);
  width: 3px;
  height: 16px;
  border-radius: 0 2px 2px 0;
  background: var(--accent);
}

.menu-item:focus-visible {
  outline: 1px solid var(--accent);
  outline-offset: 1px;
}
</style>
