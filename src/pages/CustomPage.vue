<script setup lang="ts">
/**
 * 自定义页 — 容器，按 appStore.customView 在两个子视图间切换：
 *   - list：已保存序列的卡片列表（含固定的「+ 新建」卡片），此页热键无效。
 *   - detail：单个序列的详情编辑页，可就地编辑动作、重命名、删除，并支持热键启动。
 *
 * 全局 AppPage 保持为 'custom'；子视图路由是本页内部状态，不进入全局路由。
 * 热键门控靠后端「激活序列」机制：进入 detail 调 enter_custom_sequence(id)，
 * 返回 list 调 set_current_page('custom') 复位（清空激活序列 + 回 Idle）。
 */
import { appStore } from '../stores/appStore'
import CustomListView from '../components/CustomListView.vue'
import CustomDetailView from '../components/CustomDetailView.vue'
</script>

<template>
  <CustomDetailView v-if="appStore.customView === 'detail' && appStore.activeSequenceId" />
  <CustomListView v-else />
</template>
