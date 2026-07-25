<script setup lang="ts">
/**
 * 菜单图标 — 侧边栏 4 个入口的统一线性图标。
 *
 * 设计取向：
 *   - 16×16 viewBox，stroke-width 1.5，line/round caps，与 Lucide / Tabler 系
 *     线性图标语言一致；颜色继承 `currentColor`，由 .menu-item 控制。
 *   - 按 AppPage 取图标，集中维护避免在 sidebar 模板里堆 SVG。
 *   - 仅装饰用，`aria-hidden="true"`；菜单项的可访问名由按钮文字承载。
 */
import type { AppPage } from '../types/config'

defineProps<{ name: AppPage }>()
</script>

<template>
  <svg
    class="menu-icon"
    viewBox="0 0 16 16"
    fill="none"
    stroke="currentColor"
    stroke-width="1.5"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <!-- 首页：屋顶 + 屋身 + 居中门洞 -->
    <template v-if="name === 'home'">
      <path d="M2.5 7.6 L8 2.5 L13.5 7.6 V13 a0.5 0.5 0 0 1 -0.5 0.5 H3 a0.5 0.5 0 0 1 -0.5 -0.5 Z" />
      <path d="M6.5 13.5 V9.5 H9.5 V13.5" />
    </template>

    <!-- 按键模拟：键盘外框 + 三排按键点 + 底部空格条 -->
    <template v-else-if="name === 'keyboard'">
      <rect x="1.5" y="4" width="13" height="8" rx="1.5" />
      <path d="M4 6.8 h0.01 M7 6.8 h0.01 M10 6.8 h0.01 M13 6.8 h0.01" />
      <path d="M4 9.2 h0.01 M7 9.2 h0.01 M10 9.2 h0.01 M13 9.2 h0.01" />
      <path d="M5 11 h6" />
    </template>

    <!-- 鼠标模拟：圆角矩形外形 + 顶部到中部的滚轮分线 -->
    <template v-else-if="name === 'mouse'">
      <rect x="4" y="1.5" width="8" height="13" rx="4" />
      <path d="M8 1.5 V7" />
    </template>

    <!-- 自定义：三条带前导圆点的序列行（表示可编排的有序动作列表） -->
    <template v-else-if="name === 'custom'">
      <path d="M3 4 h0.01 M3 8 h0.01 M3 12 h0.01" />
      <path d="M6 4 H13 M6 8 H13 M6 12 H13" />
    </template>

    <!-- 设置：齿轮（中心圆 + 8 个齿） -->
    <template v-else-if="name === 'settings'">
      <circle cx="8" cy="8" r="2.2" />
      <path
        d="M8 1.5 V3.3 M8 12.7 V14.5 M14.5 8 H12.7 M3.3 8 H1.5 M12.6 3.4 L11.3 4.7 M4.7 11.3 L3.4 12.6 M12.6 12.6 L11.3 11.3 M4.7 4.7 L3.4 3.4"
      />
    </template>
  </svg>
</template>

<style scoped>
.menu-icon {
  width: 16px;
  height: 16px;
  flex-shrink: 0;
}
</style>
