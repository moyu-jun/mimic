# Mimic

一款**仅面向 Windows** 的桌面按键 / 鼠标模拟工具，主要用于游戏场景。在界面中配置按键序列、点击坐标与全局热键，按下启动热键后由后台循环执行模拟动作，按下停止热键退出循环；并支持热键提示音与自定义提示音录制。

底层的键盘 / 鼠标模拟通过第三方 [Interception](https://github.com/oblitum/Interception) 驱动完成。

## 功能

- 按键模拟：配置按键序列与每个动作的间隔时间，循环执行。
- 鼠标模拟：配置点击坐标与间隔，支持屏幕坐标拾取。
- 全局热键：自定义启动 / 停止热键（借助 Interception listener，支持任意按键，含独立修饰键）。
- 热键提示音：启动 / 停止生效时播放提示音，支持在设置页录制并替换自定义音效。
- 驱动管理：首页展示 Interception 驱动状态，提供安装 / 卸载引导。

## 技术栈

| 层 | 选型 |
|----|------|
| 前端 | Vue 3 + TypeScript + Vite |
| 桌面运行时 | Tauri 2 |
| 后端 | Rust |
| 底层驱动 | Interception |

## 运行要求

- Windows（不支持 macOS / Linux）。
- 模拟功能依赖 Interception 驱动：驱动未安装或安装后未重启时模拟不可用，但配置界面可正常打开。
- 采用「按需提权」：启动不主动请求 UAC，仅在安装 / 卸载驱动时引导提权。

## 使用说明

详细的功能介绍、快速开始指南、常见问题与注意事项，请查看：

**[📖 使用说明文档](extra/使用说明.html)**（双击打开或在浏览器中查看）

使用说明文档随应用一起分发，打包后位于 exe 同级目录。

## 开发

```bash
npm install                # 安装前端依赖
npm run tauri dev          # 开发运行（自动启动 vite）
npm run tauri build        # 打包
npm run build              # 前端类型检查 + 构建（vue-tsc --noEmit && vite build）
```

后端检查（在 `src-tauri/` 目录下）：

```bash
cargo fmt
cargo clippy -- -D warnings
cargo check
```

## 项目结构

```text
mimic/
├── src/          # 前端（Vue）：pages / components / stores / styles / lib
├── src-tauri/    # Rust 后端：Tauri 命令、热键、Interception 模拟、驱动管理、提示音
├── extra/        # 运行期外置资源：interception.dll、audio/、driver/
│                 # 由 build.rs 编译时复制到 target/{debug,release}/
└── docs/         # 需求 / 设计 / 任务 / 变更记录文档
```

## 架构

后端已完成以生命周期安全和最小权限为核心的重构：

- 单线程 Runtime Actor 独占 `InputDriver`、动作游标、可中断计时和按下账本；Stop/Shutdown 通过有界通道确认，释放失败不会伪装成功。
- `commands/` 是薄适配层，`runner/` 负责序列构建，监听、录音、拾取和音频预热均由可停止、可 Join 的 Handle 管理。
- 主应用保持普通权限；驱动安装、卸载和重启仅通过独立、哈希固定、白名单协议的最小 helper 执行。
- 配置与 WAV 使用候选写入、校验和原子发布，可写数据隔离在 `data/`，固定资源拒绝链接/重解析点。

详见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

## 文档

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — 后端架构设计（模拟核心 + 应用层，已完成）
- [docs/RELEASE.md](docs/RELEASE.md) — Windows 发布、独立提权 helper 与签名流程
- [docs/WINDOWS_ACCEPTANCE.md](docs/WINDOWS_ACCEPTANCE.md) — Stop 延迟、发布预检与真机验收矩阵
- [docs/REQUIREMENTS.md](docs/REQUIREMENTS.md) — 功能需求与行为约束
- [docs/DESIGN.md](docs/DESIGN.md) — 技术设计与模块划分
- [docs/TASKS.md](docs/TASKS.md) — 实施顺序与阶段验收
- [docs/CHANGELOG.md](docs/CHANGELOG.md) — 阶段执行日志

## 推荐 IDE

[VS Code](https://code.visualstudio.com/) + [Vue - Official](https://marketplace.visualstudio.com/items?itemName=Vue.volar) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
