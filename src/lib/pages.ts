/**
 * 页面元数据 — 中文名映射与左侧菜单顺序。
 * 标题栏（当前页名）与侧边栏（菜单项）共用，避免标签重复定义。
 */

import type { AppPage } from '../types/config'

/** AppPage → 中文显示名（标题栏居中显示用） */
export const PAGE_LABELS: Record<AppPage, string> = {
  home: '首页',
  keyboard: '按键模拟',
  mouse: '鼠标模拟',
  custom: '自定义',
  settings: '设置',
}

/** 主菜单顺序（不含「设置」，设置固定置于侧边栏底部） */
export const MAIN_PAGES: AppPage[] = ['home', 'keyboard', 'mouse', 'custom']
