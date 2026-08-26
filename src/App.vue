<script setup lang="ts">
/**
 * 根组件 — 组合标题栏 + (侧边栏 + 路由内容) + 状态栏。
 * 路由用 currentPage 映射组件，无需引入 vue-router（页面固定四个）。
 *
 * 阶段 7：在 .main-area 内追加 lock-overlay（DESIGN 15.5 / 需求 3.9）。
 *   - 仅覆盖菜单 + 内容区，不覆盖标题栏与状态栏。
 *   - 半透明灰色，pointer-events 拦截点击。
 *   - 内部无任何文字 / 图标 / 按钮，运行文案由状态栏承载。
 *   - 阶段 12：由 runtime_status_changed 事件驱动 isLocked 状态。
 */
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { appStore } from './stores/appStore'
import { initializePersistedConfig } from './lib/configUtil'
import AppTitleBar from './components/AppTitleBar.vue'
import AppSidebar from './components/AppSidebar.vue'
import AppStatusBar from './components/AppStatusBar.vue'
import HomePage from './pages/HomePage.vue'
import KeyboardPage from './pages/KeyboardPage.vue'
import MousePage from './pages/MousePage.vue'
import CustomPage from './pages/CustomPage.vue'
import SettingsPage from './pages/SettingsPage.vue'
import type { AppConfig, AppPage, RuntimeStatus } from './types/config'

const PAGE_COMPONENTS = {
  home: HomePage,
  keyboard: KeyboardPage,
  mouse: MousePage,
  custom: CustomPage,
  settings: SettingsPage,
} satisfies Record<AppPage, unknown>

const currentPageComponent = computed(() => PAGE_COMPONENTS[appStore.currentPage])
const isInteractionLocked = computed(() =>
  ['RunningKeyboard', 'RunningMouse', 'RunningCustom', 'PickingMouse'].includes(
    appStore.runtimeStatus,
  ),
)
const isCancelling = ref(false)

let unlisten: UnlistenFn | null = null

async function cancelCurrentOperation(): Promise<void> {
  if (isCancelling.value) return
  isCancelling.value = true
  try {
    if (appStore.runtimeStatus === 'PickingMouse') {
      await invoke('cancel_pick_mouse_position')
    } else {
      await invoke('stop_simulation')
    }
  } catch (error) {
    console.error('Failed to cancel current operation:', error)
  } finally {
    isCancelling.value = false
  }
}

// 启动时加载配置 — ARCHITECTURE v3.0 重构
onMounted(async () => {
  try {
    const config = await invoke<AppConfig>('load_config')

    // 注入到 appStore
    appStore.keyboardConfigs = config.keyboardConfigs
    appStore.mouseConfigs = config.mouseConfigs
    appStore.customSequences = config.customSequences ?? []
    appStore.hotkeys = config.hotkeys
    appStore.logLevel = config.logLevel
    initializePersistedConfig(config)
  } catch (error) {
    console.error('Failed to load config:', error)
  }

  // 监听 runtime_status_changed 事件
  unlisten = await listen<{ status: RuntimeStatus }>('runtime_status_changed', (event) => {
    appStore.runtimeStatus = event.payload.status
  })
})

onUnmounted(() => {
  if (unlisten) unlisten()
})
</script>

<template>
  <div class="app-container">
    <AppTitleBar />
    <div v-if="appStore.configSaveError" class="config-error-banner" role="alert">
      {{ appStore.configSaveError }}
    </div>
    <div class="main-area">
      <AppSidebar />
      <main class="content">
        <component :is="currentPageComponent" />
      </main>
      <!-- 运行期锁定蒙版：阶段 12 由 runtime_status_changed 事件驱动 -->
      <div v-if="isInteractionLocked" class="lock-overlay">
        <button type="button" class="cancel-operation-btn" :disabled="isCancelling" @click="cancelCurrentOperation">
          {{ isCancelling ? '正在取消…' : '取消' }}
        </button>
      </div>
    </div>
    <AppStatusBar />
  </div>
</template>

<style scoped>
.config-error-banner {
  position: absolute;
  top: 38px;
  left: 50%;
  z-index: 30;
  transform: translateX(-50%);
  padding: 6px 12px;
  border: 1px solid color-mix(in srgb, var(--danger) 50%, var(--border-subtle));
  border-radius: 6px;
  background: color-mix(in srgb, var(--danger) 14%, var(--bg-elevated));
  color: var(--danger);
  font-size: 11px;
  box-shadow: var(--shadow-md);
}
.app-container {
  display: flex;
  flex-direction: column;
  width: var(--window-width);
  height: var(--window-height);
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: var(--window-radius);
  overflow: hidden;
}

.main-area {
  position: relative;
  display: flex;
  flex: 1;
  min-height: 0;
}

.content {
  flex: 1;
  min-width: 0;
  overflow: hidden;
}

/**
 * lock-overlay — DESIGN 15.5
 * 绝对定位铺满 .main-area，拦截底层交互并在中央提供取消操作。
 * 半透明灰色取主背景为基底叠加 60% 不透明度，避免硬编码 RGBA。
 */
.lock-overlay {
  display: flex;
  align-items: center;
  justify-content: center;
  position: absolute;
  inset: 0;
  background: color-mix(in srgb, var(--bg-primary) 65%, transparent);
  pointer-events: auto;
  cursor: not-allowed;
  z-index: 10;
}

.cancel-operation-btn {
  min-width: 112px;
  height: 36px;
  padding: 0 20px;
  color: var(--color-paper-white);
  background: var(--danger);
  border-radius: 6px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
}

.cancel-operation-btn:disabled {
  opacity: 0.6;
  cursor: wait;
}
</style>
