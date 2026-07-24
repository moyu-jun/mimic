## 阶段 9 修复：设置页热键保存持久化

**完成时间**：2026-06-07

### 问题描述

用户报告阶段 9 完成后两个问题：
1. 设置页热键修改后点击保存，实际没有数据持久化到 INI 文件
2. 保存成功提示中仍有 "（mock）" 字样

### 改动摘要

| 文件 | 改动类型 | 关键点 |
|------|---------|--------|
| [src/pages/SettingsPage.vue](../src/pages/SettingsPage.vue) | 改 | `onSave` 改为异步函数，添加 `invoke('save_config')` 调用；从 `appStore` 获取完整配置构造 `AppConfig` 对象；持久化成功后才更新内存状态；错误处理显示详细失败原因；移除 "(mock)" 字样 |
| [src/types/config.ts](../src/types/config.ts) | 改 | 新增 `AppConfig` 接口定义（由 build-error-resolver 代理完成） |

### 关键决策

- **持久化优先原则**：先调用后端 `save_config` 持久化，只有在 `await` 成功后才更新 `appStore` 和 `persistedSnapshot`，确保内存状态与磁盘状态一致性。
- **完整配置对象**：构造的 `AppConfig` 包含所有必需字段（`keyboardActions`、`mouseActions`、`hotkeys`），从 `appStore` 获取 actions 字段，避免部分字段丢失。
- **详细错误提示**：采纳 code-reviewer 建议，从错误对象提取具体消息展示给用户（`保存失败: ${errorMsg}`），而非简单的"保存失败"。
- **类型安全**：添加 `AppConfig` 接口定义，与后端 Rust 的 `AppConfig` 结构保持一致（字段名使用 camelCase）。

### 验证结果

- `npm run build`（vue-tsc + Vite）— 通过：52 模块，CSS 17.42 kB / JS 95.92 kB（gzip 34.64 kB），无 TS 错误。
- `cargo build`（src-tauri）— 通过：3.58s，无 warning。
- `npx tsc --noEmit` — 通过：TypeScript 类型检查无错误。
- **code-reviewer** 审查 — 批准：核心逻辑正确，状态同步顺序正确，数据完整性正确，错误处理存在。

### 文档回写

- 本次为 bug 修复，REQUIREMENTS / DESIGN / TASKS 无改动。

### 偏差与遗留

- 无。修复符合阶段 9 原有设计意图，仅补足了遗漏的持久化调用和用户体验细节。
