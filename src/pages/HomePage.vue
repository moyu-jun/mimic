<script setup lang="ts">
/**
 * 首页 — 状态仪表盘（DESIGN 15.4 / 需求 3.3.1）
 * 阶段 9：onMounted 调用 get_init_warning，有写盘失败时显示小字提示。
 * 主程序保持普通权限；安装、卸载和重启仅对具体系统进程请求 UAC。
 *   - 驱动状态：阶段 11 接 check_driver_status / install_driver
 *   - 热键概览：由 load_config 提供（阶段 8 已接入）
 * 阶段 12：移除「模拟运行（mock）」临时切换按钮。
 */
import { ref, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { appStore } from '../stores/appStore'
import { commandError } from '../lib/errorUtil'
import type { DriverStatus } from '../types/config'

const APP_VERSION = '0.1.0'
const APP_TAGLINE = 'Windows 按键与鼠标模拟工具'

// 启动时配置写盘失败的警告（极小概率，默认为空）
const configWarning = ref<string | null>(null)
const isAdmin = ref<boolean | null>(null)

// 阶段 11：驱动状态（来自后端 check_driver_status）
const driverStatus = ref<DriverStatus>('NotInstalled')
const isInstalling = ref(false)
const installError = ref<string | null>(null)
const isRebooting = ref(false)

// 卸载驱动：内联文字确认，防止误触
const showUninstallConfirm = ref(false)
const uninstallConfirmText = ref('')
const isUninstalling = ref(false)
// 安装 / 卸载成功后置位：驱动卡片进入「待重启」展示，按钮变「重启电脑」。
// 'installed' = 安装成功待重启；'uninstalled' = 卸载成功待重启。
const pendingReboot = ref<'installed' | 'uninstalled' | null>(null)
const driverMessage = ref<string | null>(null)
const UNINSTALL_PHRASE = '卸载驱动'

async function onInstallDriver(): Promise<void> {
  if (isInstalling.value) return
  isInstalling.value = true
  installError.value = null
  driverMessage.value = null
  try {
    await invoke('install_interception_driver')
    // 安装已执行：驱动文件与注册表服务项已写入，但驱动需重启系统才会加载。
    // 与卸载对称——以命令成功返回作为「已安装待重启」信号，不依赖 check_driver_status。
    pendingReboot.value = 'installed'
    driverMessage.value = '驱动已安装，需重启电脑后才会加载。'
  } catch (err) {
    installError.value = formatDriverError(err, '安装')
    log_error('[HomePage] install driver failed:', err)
  } finally {
    isInstalling.value = false
  }
}

function formatDriverError(error: unknown, operation: '安装' | '卸载'): string {
  const detail = commandError(error)
  if (detail.code === 'resource_integrity_failed') return '驱动安装资源校验失败，已拒绝提权执行'
  if (detail.code === 'elevation_cancelled') {
    return `已取消${operation}`
  }
  if (detail.code === 'busy') return `当前有任务正在运行，请停止后再${operation}`
  if (detail.code === 'not_found') return '安装程序不存在，请检查 driver 目录'
  return `${operation}失败，请稍后重试`
}

function log_error(msg: string, err: unknown): void {
  console.error(msg, err)
}

async function onReboot(): Promise<void> {
  if (isRebooting.value) return
  isRebooting.value = true
  installError.value = null
  try {
    await invoke('reboot_system')
    // 系统即将重启，前端无需后续处理
  } catch (err) {
    isRebooting.value = false
    installError.value = commandError(err).code === 'elevation_cancelled'
      ? '已取消重启'
      : '重启失败，请手动重启电脑'
    log_error('[HomePage] reboot failed:', err)
  }
}

/** 点击「卸载驱动」后展开文字确认区，确认后仅受限维护 helper 进程请求 UAC。 */
function onUninstallClick(): void {
  installError.value = null
  driverMessage.value = null
  showUninstallConfirm.value = true
}

/** 取消卸载，收起确认区并清空输入 */
function cancelUninstall(): void {
  showUninstallConfirm.value = false
  uninstallConfirmText.value = ''
  installError.value = null
}

/** 确认卸载：校验文字 → 调后端卸载 → 进入「已卸载待重启」 */
async function onConfirmUninstall(): Promise<void> {
  if (isUninstalling.value) return
  if (uninstallConfirmText.value.trim() !== UNINSTALL_PHRASE) {
    installError.value = `请在输入框中准确输入「${UNINSTALL_PHRASE}」以确认`
    return
  }
  isUninstalling.value = true
  installError.value = null
  try {
    await invoke('uninstall_interception_driver')
    // 卸载已执行：注册表服务项已移除，但驱动仍驻留内核，必须重启系统才彻底卸载。
    // check_driver_status 在重启前可能仍返回 Ready（context 尚可创建），不可靠，
    // 故以命令成功返回作为「已卸载待重启」信号，引导用户重启。
    cancelUninstall()
    pendingReboot.value = 'uninstalled'
    driverMessage.value = '驱动已卸载，需重启电脑后彻底生效。'
  } catch (err) {
    installError.value = formatDriverError(err, '卸载')
    log_error('[HomePage] uninstall driver failed:', err)
  } finally {
    isUninstalling.value = false
  }
}

onMounted(async () => {
  // 并行触发，任一失败不阻塞其他状态检测。
  const warningPromise = invoke<string | null>('get_init_warning').catch(() => null)
  const adminPromise = invoke<boolean>('get_admin_status').catch(() => null)
  const driverPromise = invoke<string>('check_driver_status').catch(() => '"NotInstalled"')
  const [warning, admin, driverRaw] = await Promise.all([
    warningPromise,
    adminPromise,
    driverPromise,
  ])
  configWarning.value = warning
  isAdmin.value = admin
  try {
    driverStatus.value = JSON.parse(driverRaw) as DriverStatus
  } catch {
    driverStatus.value = 'NotInstalled'
  }
})
</script>

<template>
  <section class="home">
    <!-- 顶部：应用名 + 版本 + 简介 -->
    <header class="hero">
      <div class="hero-title">
        <span class="app-name">Mimic</span>
        <span class="app-version">v{{ APP_VERSION }}</span>
      </div>
      <p class="tagline">{{ APP_TAGLINE }}</p>
    </header>

    <div class="status-line" :class="isAdmin ? 'warn' : 'ok'">
      <span class="status-icon">{{ isAdmin ? '!' : '✓' }}</span>
      <span class="status-text">
        {{ isAdmin === true
          ? '主程序当前以管理员权限运行；日常使用无需此权限'
          : isAdmin === false
            ? '主程序以普通权限运行；驱动维护时 Windows 会单独请求授权'
            : '无法读取当前权限状态；驱动维护仍会单独请求授权' }}
      </span>
    </div>

    <!-- 驱动状态卡片 -->
    <div class="card driver-card">
      <div class="driver-info">
        <span
          class="driver-dot"
          :class="{
            'dot-ready': driverStatus === 'Ready' && !pendingReboot,
            'dot-warn': driverStatus === 'InstalledNeedReboot' || pendingReboot !== null,
            'dot-error': driverStatus === 'Error',
          }"
        ></span>
        <span class="driver-text">
          {{ pendingReboot === 'installed' ? '驱动已安装，需重启电脑'
           : pendingReboot === 'uninstalled' ? '驱动已卸载，需重启电脑'
           : driverStatus === 'Ready' ? 'Interception 驱动已加载'
           : driverStatus === 'InstalledNeedReboot' ? '驱动已安装，需重启系统'
           : driverStatus === 'Error' ? '驱动错误'
           : '驱动未安装' }}
        </span>
      </div>
      <!-- 安装 / 卸载成功后：仅显示「重启电脑」 -->
      <button
        v-if="pendingReboot"
        type="button"
        class="install-btn reboot-btn"
        :disabled="isRebooting"
        @click="onReboot"
      >
        {{ isRebooting ? '正在重启...' : '重启电脑' }}
      </button>
      <button
        v-else-if="driverStatus === 'NotInstalled'"
        type="button"
        class="install-btn"
        :disabled="isInstalling"
        @click="onInstallDriver"
      >
        {{ isInstalling ? '正在安装...' : '安装驱动' }}
      </button>
      <div v-else-if="driverStatus === 'Ready' || driverStatus === 'InstalledNeedReboot'" class="driver-actions">
        <button
          v-if="driverStatus === 'InstalledNeedReboot'"
          type="button"
          class="install-btn reboot-btn"
          :disabled="isRebooting"
          @click="onReboot"
        >
          {{ isRebooting ? '正在重启...' : '重启电脑' }}
        </button>
        <button
          type="button"
          class="install-btn uninstall-btn"
          :disabled="isUninstalling || showUninstallConfirm"
          @click="onUninstallClick"
        >
          卸载驱动
        </button>
      </div>
    </div>

    <!-- 卸载二次确认（内联展开，防误触）— 仅管理员且点击卸载后出现 -->
    <div v-if="showUninstallConfirm" class="uninstall-confirm card">
      <p class="uninstall-warn">
        ⚠ 卸载 Interception 驱动是<b>高风险操作</b>，将移除按键与鼠标模拟能力，且通常需要重启系统才能彻底生效。请谨慎确认。
      </p>
      <p class="uninstall-hint">如确需卸载，请在下方输入框输入「{{ UNINSTALL_PHRASE }}」后点击「确认卸载」。</p>
      <input
        v-model="uninstallConfirmText"
        type="text"
        class="uninstall-input"
        :placeholder="`输入 ${UNINSTALL_PHRASE} 以确认`"
        :disabled="isUninstalling"
        @keyup.enter="onConfirmUninstall"
      />
      <div class="uninstall-confirm-actions">
        <button
          type="button"
          class="install-btn"
          :disabled="isUninstalling"
          @click="cancelUninstall"
        >
          取消
        </button>
        <button
          type="button"
          class="install-btn uninstall-btn"
          :disabled="isUninstalling || uninstallConfirmText.trim() !== UNINSTALL_PHRASE"
          @click="onConfirmUninstall"
        >
          {{ isUninstalling ? '正在卸载...' : '确认卸载' }}
        </button>
      </div>
    </div>
    <p v-if="installError" class="install-error">{{ installError }}</p>
    <p v-if="driverMessage" class="driver-message">{{ driverMessage }}</p>

    <!-- 当前热键概览 -->
    <div class="card hotkey-card">
      <span class="hotkey-label">当前热键</span>
      <span class="hotkey-value">
        启动：<b>{{ appStore.hotkeys.start.keyLabel }}</b>
        <span class="sep">|</span>
        停止：<b>{{ appStore.hotkeys.stop.keyLabel }}</b>
      </span>
    </div>

    <!-- 配置写盘失败警告（极小概率，默认不显示） -->
    <p v-if="configWarning" class="config-warning">
      ⚠ 配置文件无法写入，本次使用默认配置运行，修改不会保存。
    </p>
  </section>
</template>

<style scoped>
.home {
  display: flex;
  flex-direction: column;
  gap: 12px;
  height: 100%;
  padding: 16px 18px;
  overflow-x: hidden;
  overflow-y: auto;
  scrollbar-gutter: stable;
}

/* 滚动条样式 — 同 SettingsPage / KeyboardPage / MousePage */
.home::-webkit-scrollbar {
  width: 8px;
}

.home::-webkit-scrollbar-track {
  background: transparent;
}

.home::-webkit-scrollbar-thumb {
  background: var(--border-color);
  border-radius: 4px;
}

.home::-webkit-scrollbar-thumb:hover {
  background: var(--text-disabled);
}

.hero {
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.hero-title {
  display: flex;
  align-items: baseline;
  gap: 8px;
}

.app-name {
  font-size: 20px;
  font-weight: 700;
  color: var(--text-primary);
}

.app-version {
  font-size: 12px;
  color: var(--text-disabled);
}

.tagline {
  margin: 0;
  font-size: 12px;
  color: var(--text-secondary);
}

/* 权限状态行 */
.status-line {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  border-radius: 8px;
  font-size: 12px;
}

.status-line.ok {
  background: color-mix(in srgb, var(--success) 14%, transparent);
  color: var(--success);
}

.status-line.warn {
  background: color-mix(in srgb, var(--warning) 16%, transparent);
  color: var(--warning);
}

.status-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: currentColor;
  color: var(--bg-primary);
  font-size: 10px;
  font-weight: 700;
  flex-shrink: 0;
}

/* 阶段 10：以管理员身份重启按钮 — 仅未授权时出现 */
.restart-btn {
  margin-left: auto;
  padding: 3px 10px;
  border-radius: 6px;
  background: var(--warning);
  color: var(--color-paper-white);
  font-size: 11px;
  font-weight: 600;
  flex-shrink: 0;
  transition:
    background var(--transition-fast) var(--ease-default),
    transform var(--transition-fast) var(--ease-default);
}

.restart-btn:hover:not(:disabled) {
  background: var(--accent-hover);
}

.restart-btn:active:not(:disabled) {
  background: var(--accent-pressed);
  transform: translateY(1px);
}

.restart-btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.restart-error {
  margin: -4px 0 0;
  font-size: 11px;
  color: var(--danger);
}

/* 卡片通用 */
.card {
  display: flex;
  align-items: center;
  padding: 10px 12px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-subtle);
  border-radius: 8px;
}

.driver-card {
  justify-content: space-between;
}

.driver-info {
  display: flex;
  align-items: center;
  gap: 8px;
}

.driver-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--text-disabled);
}

.driver-dot.dot-ready {
  background: var(--success);
}

.driver-dot.dot-warn {
  background: var(--warning);
}

.driver-dot.dot-error {
  background: var(--danger);
}

.install-error {
  margin: -4px 0 0;
  font-size: 11px;
  color: var(--danger);
}

.driver-message {
  margin: -4px 0 0;
  font-size: 11px;
  color: var(--success);
}

.driver-text {
  font-size: 13px;
  color: var(--text-secondary);
}

.install-btn {
  padding: 5px 14px;
  border-radius: 6px;
  background: var(--accent);
  color: var(--color-paper-white);
  font-size: 12px;
  font-weight: 500;
  transition:
    background var(--transition-fast) var(--ease-default),
    transform var(--transition-fast) var(--ease-default);
}

.install-btn:hover {
  background: var(--accent-hover);
}

.install-btn:active {
  background: var(--accent-pressed);
  transform: translateY(1px);
}

.install-btn:focus-visible {
  outline: 1px solid var(--accent);
  outline-offset: 2px;
}

.install-btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

/* 重启电脑按钮 — 警告色，提示这是系统级操作 */
.install-btn.reboot-btn {
  background: var(--warning);
  color: var(--color-paper-white);
}

.install-btn.reboot-btn:hover:not(:disabled) {
  background: color-mix(in srgb, var(--warning) 85%, white);
}

/* 卸载驱动按钮 — 鲜明红色警告色 */
.driver-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.install-btn.uninstall-btn {
  background: var(--danger);
  color: var(--color-paper-white);
}

.install-btn.uninstall-btn:hover:not(:disabled) {
  background: color-mix(in srgb, var(--danger) 85%, black);
}

/* 卸载二次确认区 — 内联展开 */
.uninstall-confirm {
  flex-direction: column;
  align-items: stretch;
  gap: 8px;
  border-color: color-mix(in srgb, var(--danger) 50%, var(--border-subtle));
  background: color-mix(in srgb, var(--danger) 8%, var(--bg-secondary));
}

.uninstall-warn {
  margin: 0;
  font-size: 12px;
  line-height: 1.5;
  color: var(--danger);
}

.uninstall-warn b {
  font-weight: 700;
}

.uninstall-hint {
  margin: 0;
  font-size: 12px;
  color: var(--text-secondary);
}

.uninstall-input {
  padding: 6px 10px;
  border: 1px solid var(--border-subtle);
  border-radius: 6px;
  background: var(--bg-primary);
  color: var(--text-primary);
  font-size: 13px;
}

.uninstall-input:focus {
  outline: none;
  border-color: var(--danger);
}

.uninstall-confirm-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}

/* 热键概览 */
.hotkey-card {
  justify-content: space-between;
}

.hotkey-label {
  font-size: 13px;
  color: var(--text-secondary);
}

.hotkey-value {
  font-size: 13px;
  color: var(--text-primary);
}

.hotkey-value b {
  font-weight: 600;
}

.hotkey-value .sep {
  margin: 0 6px;
  color: var(--text-disabled);
}

/* 配置写盘失败警告 — 正常情况不可见 */
.config-warning {
  margin: 0;
  font-size: 11px;
  color: var(--warning);
  opacity: 0.85;
}
</style>
