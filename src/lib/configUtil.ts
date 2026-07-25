/**
 * 配置持久化工具 — ARCHITECTURE v3.0 重构
 *
 * 封装 persist_config 命令调用，统一错误处理。
 */
import { invoke } from '@tauri-apps/api/core'
import { appStore } from '../stores/appStore'

/**
 * 持久化当前配置到 mimic.ini
 *
 * @throws 持久化失败时抛出错误
 */
export async function persistConfig(): Promise<void> {
  try {
    await invoke('persist_config', {
      config: {
        keyboardConfigs: appStore.keyboardConfigs,
        mouseConfigs: appStore.mouseConfigs,
        hotkeys: appStore.hotkeys,
      }
    })
  } catch (err) {
    if (String(err).includes('busy')) {
      console.warn('[persistConfig] 模拟运行中，跳过持久化')
      return
    }
    console.error('[persistConfig] 保存配置失败:', err)
    throw err
  }
}
