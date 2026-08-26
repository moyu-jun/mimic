/**
 * 配置持久化事务。
 *
 * 所有写入按调用顺序串行执行。每次写入携带不可变候选快照；最新写入失败时，
 * 前端状态会恢复为最后一次后端确认成功的快照。
 */
import { invoke } from '@tauri-apps/api/core'
import { appStore } from '../stores/appStore'
import type { AppConfig } from '../types/config'

let committedConfig: AppConfig | null = null
let persistQueue: Promise<void> = Promise.resolve()
let latestRevision = 0

function cloneConfig(config: AppConfig): AppConfig {
  return JSON.parse(JSON.stringify(config)) as AppConfig
}

function currentConfig(): AppConfig {
  return cloneConfig({
    keyboardConfigs: appStore.keyboardConfigs,
    mouseConfigs: appStore.mouseConfigs,
    customSequences: appStore.customSequences,
    hotkeys: appStore.hotkeys,
    logLevel: appStore.logLevel,
  })
}

function restoreConfig(config: AppConfig): void {
  const restored = cloneConfig(config)
  appStore.keyboardConfigs = restored.keyboardConfigs
  appStore.mouseConfigs = restored.mouseConfigs
  appStore.customSequences = restored.customSequences
  appStore.hotkeys = restored.hotkeys
  appStore.logLevel = restored.logLevel
}

/** 在启动配置载入后建立前端的已提交基线。 */
export function initializePersistedConfig(config: AppConfig): void {
  committedConfig = cloneConfig(config)
  appStore.configSaveError = null
}

/** 专用后端命令成功保存日志或热键后，同步推进前端已提交基线。 */
export function markCurrentConfigPersisted(): void {
  committedConfig = currentConfig()
  appStore.configSaveError = null
}

/**
 * 串行持久化当前候选配置。
 *
 * 后端只有在原子写盘成功后才更新其内存配置；前端只有在收到成功响应后才推进
 * committedConfig。最新候选失败时恢复最后成功快照并向界面公开可重试错误。
 */
export function persistConfig(): Promise<void> {
  const candidate = currentConfig()
  const revision = ++latestRevision

  const operation = persistQueue.then(async () => {
    try {
      await invoke('persist_config', { config: candidate })
      committedConfig = cloneConfig(candidate)
      if (revision === latestRevision) appStore.configSaveError = null
    } catch (error) {
      if (revision === latestRevision) {
        if (committedConfig) restoreConfig(committedConfig)
        appStore.configSaveError = '配置保存失败，本次修改已回滚，请重试。'
      }
      console.error('[persistConfig] 保存配置失败:', error)
      throw error
    }
  })

  persistQueue = operation.then(
    () => undefined,
    () => undefined,
  )
  return operation
}