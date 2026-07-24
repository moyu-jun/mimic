# Mimic 设计文档

> 本文档描述技术选型、模块划分、命令/事件协议、数据结构与线程模型。
> 功能需求见 [REQUIREMENTS.md](./REQUIREMENTS.md)，实施顺序见 [TASKS.md](./TASKS.md)。

## 1. 技术栈

- **前端框架**：Vue 3 + TypeScript + Vite
- **桌面运行时**：Tauri 2
- **后端语言**：Rust
- **底层驱动**：Interception（第三方键盘/鼠标模拟驱动）

## 2. 项目结构

```text
mimic/
├── src/                          # 前端代码
│   ├── App.vue                   # 根组件，承载标题栏、菜单、路由容器、状态栏
│   ├── main.ts                   # 前端入口
│   ├── styles/
│   │   ├── base.css              # 全局基础样式、CSS reset
│   │   └── theme.css             # 主题变量（Windows 11 暗色配色）
│   ├── types/
│   │   └── config.ts             # TypeScript 类型定义
│   ├── stores/
│   │   └── appStore.ts           # 全局状态管理（页面、配置、运行状态）
│   ├── components/
│   │   ├── AppTitleBar.vue       # 自定义标题栏
│   │   ├── AppSidebar.vue        # 左侧菜单
│   │   ├── AppStatusBar.vue      # 底部状态栏
│   │   ├── KeyCaptureInput.vue   # 按键捕获输入框组件
│   │   └── IconButton.vue        # 图标按钮（最小化/关闭/删除等）
│   └── pages/
│       ├── HomePage.vue          # 首页
│       ├── KeyboardPage.vue      # 按键模拟页
│       ├── MousePage.vue         # 鼠标模拟页
│       └── SettingsPage.vue      # 设置页
├── src-tauri/                    # Rust 后端
│   ├── src/
│   │   ├── lib.rs                # Tauri 主入口，注册命令
│   │   ├── config.rs             # INI 读写与默认配置
│   │   ├── hotkeys.rs            # 全局热键注册与管理
│   │   ├── state.rs              # 全局状态（页面、配置、运行状态、锁）
│   │   ├── input/
│   │   │   ├── mod.rs            # 输入模拟模块入口
│   │   │   ├── keyboard.rs       # 按键模拟（Interception）
│   │   │   └── mouse.rs          # 鼠标模拟（Interception）
│   │   ├── driver.rs             # Interception 驱动检测与安装
│   │   └── mouse_picker.rs       # 鼠标坐标拾取
│   ├── Cargo.toml
│   └── tauri.conf.json           # Tauri 配置
└── extra/                        # 外置资源目录（直接入仓库）
    ├── interception.dll          # 应用运行依赖的 DLL（exe 同级）
    ├── audio/                    # 提示音文件
    │   ├── 按键开启.wav
    │   └── 按键关闭.wav
    └── driver/                   # 驱动安装文件
        └── install-interception.exe
```

## 3. Tauri 配置关键点

**tauri.conf.json** 需要的关键配置：

```json
{
  "build": {
    "beforeDevCommand": "npm run dev",
    "beforeBuildCommand": "npm run build",
    "devPath": "http://localhost:5173",
    "distDir": "../dist"
  },
  "tauri": {
    "windows": [
      {
        "title": "Mimic",
        "width": 600,
        "height": 400,
        "resizable": false,
        "maximizable": false,
        "decorations": false,
        "transparent": true,
        "alwaysOnTop": false
      }
    ],
    "bundle": {
      "identifier": "com.mimic.app",
      "active": true,
      "targets": "all",
      "windows": {
        "nsis": {}
      }
    },
    "security": {
      "csp": null
    }
  }
}
```

关键说明：

- `resizable: false` → 禁止用户缩放窗口
- `maximizable: false` → 禁止最大化
- `decorations: false` → 关闭系统标题栏
- `transparent: true` → 支持 CSS 圆角和透明（需要实机验证效果）

## 4. 数据模型（前后端共享）

### 4.1 TypeScript 类型定义

**src/types/config.ts**

```typescript
export type AppPage = 'home' | 'keyboard' | 'mouse' | 'settings'

export interface KeyboardAction {
  id: string
  selected: boolean
  keyLabel: string
  scanCode: number
  intervalMs: number
}

export interface MouseAction {
  id: string
  x: number | null
  y: number | null
  intervalMs: number
}

export interface CapturedKey {
  keyLabel: string
  scanCode: number
}

export interface HotkeyConfig {
  start: CapturedKey
  stop: CapturedKey
}

export interface AppConfig {
  keyboardActions: KeyboardAction[]
  mouseActions: MouseAction[]
  hotkeys: HotkeyConfig
}

export type RuntimeStatus =
  | 'Idle'
  | 'ReadyKeyboard'
  | 'ReadyMouse'
  | 'RunningKeyboard'
  | 'RunningMouse'
  | 'PickingMouse'
  | 'Error'

export type DriverStatus =
  | 'NotInstalled'
  | 'InstalledNeedReboot'
  | 'Ready'
  | 'Error'
```

### 4.2 Rust 结构体定义

**src-tauri/src/config.rs**（部分示例）

> **命名约定（强制）**：所有跨边界（Tauri 命令、事件 payload、INI 内 JSON）共享的结构体必须标注 `#[serde(rename_all = "camelCase")]`，使 Rust snake_case 字段序列化为前端 camelCase（`key_label` → `keyLabel`）。否则 Rust↔前端的命令边界与 INI 中的 JSON 列表都会字段对不上。

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyboardAction {
    pub id: String,
    pub selected: bool,
    pub key_label: String,
    pub scan_code: u16,
    pub interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MouseAction {
    pub id: String,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturedKey {
    pub key_label: String,
    pub scan_code: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyConfig {
    pub start: CapturedKey,
    pub stop: CapturedKey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub keyboard_actions: Vec<KeyboardAction>,
    pub mouse_actions: Vec<MouseAction>,
    pub hotkeys: HotkeyConfig,
}
```

## 5. INI 配置文件格式

**mimic.ini** 结构：

```ini
[hotkeys]
start_label=F12
start_scan_code=88
stop_label=F12
stop_scan_code=88

[keyboard]
actions=[{"id":"default-keyboard-1","selected":true,"keyLabel":"F","scanCode":33,"intervalMs":20}]

[mouse]
actions=[{"id":"default-mouse-1","x":null,"y":null,"intervalMs":20}]
```

**说明**：

- `[hotkeys]` section：平铺字段，直接存取。
- `[keyboard]` 和 `[mouse]` section：`actions` 字段存储 JSON 字符串数组。
- 使用 `serde_json` 序列化/反序列化列表，避免自定义 INI 数组格式。

## 6. Tauri 命令（Rust → 前端调用）

以下命令在 `src-tauri/src/lib.rs` 中注册，前端通过 `invoke()` 调用：

```rust
#[tauri::command]
fn load_config() -> Result<AppConfig, String>

#[tauri::command]
fn save_config(config: AppConfig) -> Result<(), String>

#[tauri::command]
fn set_current_page(page: String) -> Result<(), String>

#[tauri::command]
fn update_hotkeys(hotkeys: HotkeyConfig) -> Result<HotkeyUpdateResult, String>

#[tauri::command]
fn start_pick_mouse_position(row_id: String) -> Result<(), String>

#[tauri::command]
fn stop_simulation() -> Result<(), String>

#[tauri::command]
fn get_runtime_status() -> RuntimeStatus

#[tauri::command]
fn check_driver_status() -> DriverStatus

#[tauri::command]
fn install_driver() -> Result<(), String>
```

命令设计原则：

- 读写配置：`load_config` / `save_config`
- 页面切换：`set_current_page`（后端记录当前页面以判断热键是否可触发）
- 热键更新：`update_hotkeys`（注销旧热键 → 注册新热键 → 持久化）
- 坐标拾取：`start_pick_mouse_position`（隐藏窗口 → 监听全局鼠标点击）
- 停止模拟：`stop_simulation`（设置停止标记）
- 运行状态查询：`get_runtime_status`
- 驱动管理：`check_driver_status` / `install_driver`

### 6.1 运行态命令守卫（强制，对应需求 3.9）

模拟运行（`RunningKeyboard` / `RunningMouse`）或坐标拾取（`PickingMouse`）期间，所有会改变运行上下文的命令必须在后端直接拒绝并返回 `Err`，不能仅依赖前端蒙版：

- 受守卫命令：`save_config`、`update_hotkeys`、`set_current_page`、`start_pick_mouse_position`、`install_driver`。
- 始终放行：`stop_simulation`、`get_runtime_status`、`check_driver_status`、`load_config`（只读操作，不影响运行上下文）。
- 实现方式：每个受守卫命令进入时先读 `AppState.runtime_status`，若处于上述忙状态则返回固定错误（如 `"busy: simulation running"`），前端据此忽略或提示。

### 6.2 热键更新的双重反馈（对应需求 3.3.4）

设置页保存需要分别提示"注册成功/失败"和"持久化成功/失败"，因此 `update_hotkeys` 返回结构化结果：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyUpdateResult {
    pub changed: bool,        // 与已持久化配置对比是否有变化
    pub registered: bool,     // 新热键注册是否成功
    pub persisted: bool,      // 是否成功写入 mimic.ini
    pub message: Option<String>,
}
```

- `changed == false`：无变化，前端不提示。
- `changed == true`：前端依据 `registered` / `persisted` 显示对应的小文字成功/失败提示。
- 注册失败时不应覆盖原有已注册热键（保持旧热键可用）。

## 7. Tauri 事件（Rust → 前端推送）

Rust 通过 `app.emit()` 发送事件，前端通过 `listen()` 监听：

| 事件名 | Payload | 触发时机 |
|--------|---------|---------|
| `runtime_status_changed` | `{ status: RuntimeStatus }` | 运行状态变化（Idle → Running 等） |
| `mouse_position_picked` | `{ row_id: string, x: number, y: number }` | 坐标拾取完成 |
| `config_reloaded` | `{ config: AppConfig }` | 配置重新加载（如 INI 修复后） |
| `hotkey_registration_failed` | `{ error: string }` | 热键注册失败 |
| `simulation_error` | `{ error: string }` | 模拟运行出错 |
| `driver_status_changed` | `{ status: DriverStatus }` | 驱动状态变化 |

## 8. 按键映射表

前端 `KeyboardEvent.code` 到 Interception scan code 的映射需要维护在前端或后端。

### 8.1 常用按键映射示例

| 显示名 | KeyboardEvent.code | Interception Scan Code | Tauri Hotkey Accelerator |
|--------|---------------------|------------------------|---------------------------|
| A | KeyA | 30 | A |
| F | KeyF | 33 | F |
| Space | Space | 57 | Space |
| F12 | F12 | 88 | F12 |
| Left Shift | ShiftLeft | 42 | Shift |
| Right Shift | ShiftRight | 54 | Shift |
| Left Ctrl | ControlLeft | 29 | Ctrl |
| Right Ctrl | ControlRight | 285 (0x11D) | Ctrl |
| Left Alt | AltLeft | 56 | Alt |
| Right Alt | AltRight | 312 (0x138) | Alt |
| Esc | Escape | 1 | Escape |
| Tab | Tab | 15 | Tab |

**说明**：

- 第一版必须支持字母、数字、F1-F12、Space、Tab、Esc、左右 Shift/Ctrl/Alt。**不支持方向键和组合键。**
- **E0 前缀键处理**（对应反馈 E3）：部分扩展键（如 Right Ctrl、Right Alt）scan code 在 PS/2 协议中需要 E0 前缀。Interception 通过 `stroke.state` 字段的 `INTERCEPTION_KEY_E0` 标志位表达，而非在 scan code 中编码。表中 `285 (0x11D)` 和 `312 (0x138)` 是逻辑标识，发送时需拆分为：
  ```rust
  // Right Ctrl: scan_code=29, state |= INTERCEPTION_KEY_E0
  // Right Alt: scan_code=56, state |= INTERCEPTION_KEY_E0
  ```
- Tauri global-shortcut 使用字符串加速器（如 `"F12"`, `"Ctrl+A"`），不使用 scan code。

### 8.2 映射维护建议

- 前端维护 `code → { label, scanCode, accelerator }` 映射表。
- 按键捕获时查表得到完整信息。
- 热键注册时使用 `accelerator`。
- 模拟执行时使用 `scanCode`。

### 8.3 热键实现方案演进

#### 阶段 12（当前已落地）— `tauri-plugin-global-shortcut`

- 底层 API：Windows `RegisterHotKey`（USER32.dll）。
- 集成方式:`tauri-plugin-global-shortcut` v2 注册 `Shortcut::new(None, Code::*)`,通过插件回调切换 `runtime_status`。
- **能力**：稳定支持字母/数字/F1-F12/Space/Tab/Esc 等"主键"作为独立热键;支持"修饰键 + 主键"组合。
- **限制**(阶段 12 后续修复确认):
  - `RegisterHotKey` 要求至少有一个非修饰键,**不支持单独的 Ctrl / Alt / Shift 作为热键**(注册时会因 `key_to_vk()` 找不到对应 VKCode 而报 `unknown VKCode for ControlLeft / ShiftLeft / AltLeft` 错误,导致全局热键注册失败)。
  - 因此 `keyMap.ts` 与 `scan_code_to_code()` 在阶段 12 修复中临时移除了 ShiftLeft/ShiftRight/ControlLeft/ControlRight/AltLeft/AltRight 的映射,以避免"用户在设置页设置成功但全局热键注册失败"的尴尬状态。
  - 当前可选热键收窄为字母/数字/F1-F12/Space/Tab/Esc。

#### 阶段 13（已落地）— Interception 驱动直接监听 + 按键模拟

**重要纠正**：阶段 13 验收后发现的核心缺陷修复已在"阶段 12-13 核心缺陷修复"部分（CHANGELOG.md § 阶段 12-13 修复）完整记录。此处仅说明设计原则与已验证的正确实现。

- 与按键模拟共用 Interception 驱动：阶段 13 引入 `interception` crate 作为热键监听与按键模拟通道，同一个驱动实例同时承担"输入监听（热键） + 输出发送（模拟）"职责。
- **采用动机**：
  1. **能力完整**：Interception 在内核层拦截所有按键事件，Ctrl/Alt/Shift 作为独立热键自然可识别（无需借助 `RegisterHotKey` 的修饰键语义）。
  2. **架构统一**：热键监听与按键模拟共用同一个 `interception::create_context()`，**两者为独立 context**（非同一对象），避免 "`global-shortcut` + `interception` 双驱动栈"带来的初始化重复、生命周期错配。
  3. **门控更可靠**：Interception 内核层语义清晰，可在驱动层判断按键来源（过滤掉自身模拟产生的回声）。

**核心缺陷修复（阶段 12-13）**：

| 缺陷 | 原因 | 修复 |
|------|------|------|
| **根因 #1：Ready 检测失败** | `check_driver_windows()` 仅查注册表，未验证驱动加载 | 改为先尝试 `Interception::new()`（即 `create_context()`），成功 → `Ready`；失败再查注册表 |
| **根因 #2：热键监听阻塞** | `wait()` 前未调 `set_filter()`，导致 StubKeyStroke 永不返回 | 监听线程循环前调 `set_filter(Filter::KeyFilter(...))`，设置仅监听按键事件 |
| **根因 #3：ScanCode 构造错误** | `information` 字段被误用为 ScanCode，hardcode `ScanCode::Esc` 占位 | 改为 `ScanCode::try_from(u16)` 构造真实 Code；`information` 字段恢复为 0 |
| **根因 #4：死锁** | 监听与 worker 共用同一 context + Mutex，互相争用导致阻塞 | 创建**独立 context**：监听用 `interception_ctx`（含 filter + 阻塞 wait），worker 用 `worker_ctx`（仅 send） |

**关键设计原则**（已验证）：

- **两个独立 Interception context**：
  - `interception_ctx`：监听线程用，初始化时调 `set_filter()` 过滤仅按键，循环内阻塞 `wait()`
  - `worker_ctx`：模拟 worker 用，仅调 `send()` 发送键事件，不阻塞
  - 两者都由 `Arc<Mutex<>>` 持有，线程安全但互不争用
  
- **状态机与门控**：
  - 热键回调在 `config.hotkeys` 与 `current_page` 匹配时发送 `ActionEvent` 到 worker
  - Worker 检查 `stop_flag`，从 channel 接收事件并执行模拟
  - `handle_stop_hotkey` 设置 `stop_flag` 后等待 50ms，确保 worker 收到信号后再切状态

- **驱动未安装兜底策略**：
  - 驱动 `NotInstalled` / `InstalledNeedReboot` 时不启动监听线程
  - 前端首页与设置页文案明确"安装并重启驱动后热键可用"
  - 热键功能完全禁用，无回退方案

- **风险与当前状态**：
  - ✅ **编译验证通过**：`cargo check` + `cargo clippy` 无警告
  - ⏳ **实机测试待进行**：验证热键→模拟端到端工作、修饰键识别、E0 扩展键处理

## 9. 全局状态管理

### 9.1 前端状态

使用 Vue 3 `reactive` + `provide/inject`，或轻量 composable：

```typescript
// src/stores/appStore.ts
import { reactive } from 'vue'
import type { AppConfig, AppPage, RuntimeStatus, DriverStatus } from '@/types/config'

export const appStore = reactive({
  currentPage: 'home' as AppPage,
  config: null as AppConfig | null,
  runtimeStatus: 'Idle' as RuntimeStatus,
  driverStatus: 'NotInstalled' as DriverStatus,
  isLocked: false, // 运行时锁定界面
})
```

### 9.2 Rust 后端状态

**src-tauri/src/state.rs**

```rust
use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicBool;
use serde::{Deserialize, Serialize};
use crate::config::AppConfig;

// 序列化为 PascalCase 字符串，与前端 RuntimeStatus 联合类型一致
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RuntimeStatus {
    Idle,
    ReadyKeyboard,
    ReadyMouse,
    RunningKeyboard,
    RunningMouse,
    PickingMouse,
    Error,
}

// 与前端 DriverStatus 联合类型一致
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DriverStatus {
    NotInstalled,
    InstalledNeedReboot,
    Ready,
    Error,
}

pub struct AppState {
    pub config: AppConfig,
    pub current_page: String,
    pub runtime_status: RuntimeStatus,
    pub driver_status: DriverStatus,
    pub stop_flag: Arc<AtomicBool>, // 停止标记，worker 线程检查
}

pub type SharedState = Arc<Mutex<AppState>>;
```

> serde 默认对无字段枚举变体序列化为其名字字符串（如 `"RunningKeyboard"`），正好匹配前端的 PascalCase 联合类型，无需额外 rename。

**状态同步**：

- 前端每次页面切换调用 `set_current_page(page)`，后端更新 `current_page`。
- 热键触发时检查 `current_page` 是否属于可触发页面（`keyboard` / `mouse`）。
- **热键与运行状态匹配门控**（对应反馈 L1）：
  - `Idle` 状态时，只有启动热键生效，停止热键被忽略。
  - `RunningKeyboard` / `RunningMouse` 状态时，只有停止热键生效，启动热键被忽略。
  - 状态不匹配时热键回调直接 `return`，不触发任何动作。
- **全局热键不阻塞其他键盘键**（对应反馈 L2）：
  - 全局热键通过 `tauri-plugin-global-shortcut` 注册，走系统级 `RegisterHotKey` API。
  - Interception 仅在模拟 worker 内部用作输出通道，不开启系统级输入拦截。
  - 运行期间用户可正常使用其他键盘键（如聊天、输入）。
- 运行状态变化时通过 `runtime_status_changed` 事件推送给前端。

## 10. 线程模型

- **主线程**：Tauri 命令处理、全局热键监听。
- **Worker 线程**：模拟任务循环（按键/鼠标模拟）。
- **停止机制**：主线程设置 `Arc<AtomicBool>` 停止标记，worker 线程定期检查并退出。

### 10.1 按键模拟 worker 伪代码

```rust
fn run_keyboard_simulation(
    actions: Vec<KeyboardAction>,
    stop_flag: Arc<AtomicBool>,
) {
    let ctx = interception::create_context();
    let keyboard_device = /* 选择键盘设备 */;

    loop {
        for action in &actions {
            if !action.selected { continue; }
            if stop_flag.load(Ordering::Relaxed) { return; }

            send_key_down(ctx, keyboard_device, action.scan_code);
            send_key_up(ctx, keyboard_device, action.scan_code);
            std::thread::sleep(Duration::from_millis(action.interval_ms));
        }
    }
}
```

### 10.2 鼠标模拟 worker 伪代码

```rust
fn run_mouse_simulation(
    actions: Vec<MouseAction>,
    stop_flag: Arc<AtomicBool>,
) {
    let ctx = interception::create_context();
    let mouse_device = /* 选择鼠标设备 */;

    loop {
        for action in &actions {
            if action.x.is_none() || action.y.is_none() { continue; }
            if stop_flag.load(Ordering::Relaxed) { return; }

            move_mouse_to(ctx, mouse_device, action.x.unwrap(), action.y.unwrap());
            send_mouse_click(ctx, mouse_device, MouseButton::Left);
            std::thread::sleep(Duration::from_millis(action.interval_ms));
        }
    }
}
```

## 11. 坐标拾取实现

**设计原则**：接口稳定、底层可替换。

### 11.1 接口协议

- **命令**：`start_pick_mouse_position(row_id: String)`
- **事件**：`mouse_position_picked { row_id, x, y }`

### 11.2 实现方案（✅ 已验证成功 — 2026-06-10 复用热键监听 context）

> **方案演进**：
> - **方案 A（已弃用）**：`WH_MOUSE_LL` 用户态 hook —— 被独占全屏游戏绕过，失效。
> - **方案 B（已弃用）**：worker context 单独 `wait`/`receive` 鼠标 —— 同进程双 context 竞争同一设备事件分发，worker context 此前从未设过 filter，实际收不到鼠标事件，全屏游戏内仍失败。
> - **方案 C（当前，✅ 实机验证通过，含全屏游戏）**：复用「热键监听线程」的 listener context。该 context 已被证明能正常 `receive`（键盘热键工作正常即证明）。单 context 同时监听键盘 + 鼠标，是 Interception 标准用法，避免多 context 未定义行为。

**核心经验（务必遵守，避免重蹈方案 B 覆辙）**：
> Interception 同进程内**多个 context 对同一设备的事件分发存在竞争**，一个仅用于 `send` 的 context 临时设 filter 去 `receive` 并不可靠。**监听类需求（receive）必须复用同一个已验证可 receive 的 listener context**，不要新建/借用 send-only 的 worker context 来收事件。listener 用于 receive、worker 用于 send，职责严格分离。

**实现要点**：
- listener 线程启动时**同时**设置两个 filter：键盘 `DOWN|UP`（热键）+ 鼠标 `LEFT_BUTTON_DOWN`（拾取）。
- `start_pick_mouse_position` 命令：置 `PickingMouse`、记录 `pick_row_id`、隐藏窗口（**不再开独立线程**）。
- listener 的 `wait()` 循环新增鼠标分支：收到鼠标事件 → `receive` → **透传所有 stroke**（保持目标窗口点击行为）→ 若含左键按下：
  - 读 `runtime_status`，仅 `PickingMouse` 时处理（平时左键纯透传，零影响）。
  - `GetCursorPos` 读系统光标屏幕坐标 → `drop` context 锁 → 调 `finish_pick`。
- `finish_pick`：恢复并聚焦窗口（`run_on_main_thread`）→ 状态回 `ReadyMouse` → 清 `pick_row_id` → 发 `mouse_position_picked`。
- 前端收到事件后回填对应行 X/Y 并持久化。
- **锁安全**：listener 持 listener_ctx 锁、worker 持 worker_ctx 锁（两把不同的 Mutex），与 state 锁不构成循环等待。
- **窗口恢复线程亲和性**：窗口在主线程创建，从 listener 线程直接 `show()`/`set_focus()` 受 Windows 前台锁定限制不可靠，必须 `run_on_main_thread` marshal 回主线程。
- **坐标来源**：Interception 鼠标 stroke 的 x/y 是移动量而非屏幕坐标，故用 `GetCursorPos` 读系统光标位置作为拾取结果。
- **第一版约束**（对应反馈 Q10/L13）：仅支持单显示器、标准 DPI。
- **拾取期间无取消机制**（对应反馈 Q5）：用户只能通过左键点击完成拾取。
- **异常处理**：`GetCursorPos` 失败时记 error 并仍调 `finish_pick` 恢复窗口，避免界面卡在 `PickingMouse`。
- **已知限制**：独占全屏且锁定/隐藏系统光标的游戏（部分 FPS），`GetCursorPos` 可能读不到真实准星位置；对有可见系统光标的游戏（MOBA / RTS / 窗口化全屏）已验证可用。

### 11.3 历史方案（已弃用）

- ~~方案 A：`SetWindowsHookExW` + `WH_MOUSE_LL` 用户态 hook + 消息循环~~ → 全屏游戏内失效。
- ~~方案 B：worker context 单独 wait/receive 鼠标~~ → 双 context 事件分发竞争，收不到事件。
- 备选：第三方库如 `rdev` 监听全局鼠标事件（未采用，Interception listener 方案已满足需求）。

## 12. Interception 驱动集成

### 12.1 依赖

**Cargo.toml**

```toml
[dependencies]
interception = "0.2"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
ini = "1.3"
anyhow = "1.0"
tauri-plugin-global-shortcut = "2.0.0-beta"
tauri-plugin-log = "2.0.0-beta"
windows = { version = "0.52", features = ["Win32_UI_Input_KeyboardAndMouse", "Win32_Foundation"] }
```

### 12.2 驱动检测

检测 Interception 驱动是否已安装：

```rust
pub fn check_interception_driver() -> DriverStatus {
    match interception::create_context() {
        Some(_) => DriverStatus::Ready,
        None => {
            // 检查注册表或文件系统判断是否已安装但未重启
            if is_driver_installed_but_not_loaded() {
                DriverStatus::InstalledNeedReboot
            } else {
                DriverStatus::NotInstalled
            }
        }
    }
}
```

### 12.3 驱动安装与 DLL 部署（更新：2026-06-12）

**外置资源目录结构**（`extra/` 直接入仓库）：
```text
extra/
├── interception.dll          # 应用运行依赖的 DLL
├── audio/                    # 提示音文件
│   ├── 按键开启.wav
│   └── 按键关闭.wav
└── driver/                   # 驱动安装文件
    └── install-interception.exe
```

**部署策略**（对应 DLL 加载路径问题修复）：

- `interception.dll` 必须位于 **exe 同级目录**，这样 Windows 加载器在进程启动时能通过"应用程序所在目录"这条最高优先级搜索路径找到它。
- `build.rs` 在编译时自动将 `extra/` 目录的所有内容递归复制到 `target/{debug,release}/`，保留子目录结构：
  - `extra/interception.dll` → `target/{profile}/interception.dll`（exe 同级）
  - `extra/driver/install-interception.exe` → `target/{profile}/driver/install-interception.exe`
  - `extra/audio/*.wav` → `target/{profile}/audio/*.wav`
- 不再使用 `SetDllDirectoryW` 设置子目录搜索路径（已从 `lib.rs` 移除），依赖 Windows 标准 DLL 搜索顺序。

**安装流程**（对应反馈 Q2/Q3）：

```rust
pub fn install_interception_driver() -> Result<(), String> {
    let exe_dir = std::env::current_exe()
        .map_err(|e| format!("Failed to get exe path: {}", e))?
        .parent()
        .ok_or("No parent dir")?
        .to_path_buf();

    let installer_path = exe_dir.join("driver").join("install-interception.exe");

    if !installer_path.exists() {
        return Err("Driver installer not found".to_string());
    }

    // 通过 runas 以管理员身份静默调用安装器
    // 使用 Windows ShellExecuteW with "runas" verb
    // 安装完成后弹窗提示用户必须重启系统
    // 前端显示进度提示（"正在安装驱动..."）
    // ...

    Ok(())
}
```

**调度细节（2026-06-14 加固，对应代码审查 Phase 2）**：

- **退出码校验**：`WaitForSingleObject` 等待安装器退出后，必须调 `GetExitCodeProcess` 取退出码；非 0 时返回 Err，**不能让前端拿到 Ok 走"已安装待重启"分支**，否则用户会被误导去做无效重启。`GetExitCodeProcess` 必须在 `CloseHandle` 之前调用——句柄关了就读不到退出码。
- **路径编码**：installer 路径走 `OsStrExt::encode_wide` 直接展开 PathBuf，不经 `to_string_lossy` 中转。后者遇到不能映射到 UTF-8 的 WTF-16 序列（孤立代理对、某些 emoji）会替换为 U+FFFD（�），导致 ShellExecuteExW 找不到文件。
- **错误码诊断**：`ShellExecuteExW` 失败时调 `GetLastError`，把错误码加进 err_msg。1223 = 用户拒绝 UAC、2 = 文件不存在、5 = 权限拒绝，能区分场景，便于运维诊断。
- **`SW_HIDE + INFINITE` 隐性约束**：当前实现假设 installer 是非交互控制台程序、始终能快速退出。该假设由 `oblitum/Interception` 当前版本满足。**若未来更换为带 GUI 对话框的 installer，必须同步改为 `SW_SHOWNORMAL` 或加有限超时，否则用户看不见对话框、Tauri 命令线程会永久阻塞。**

### 12.3.1 驱动卸载（与安装对称）

入口命令 `uninstall_interception_driver`，结构与 `install_interception_driver` 完全对称：

- 权限守卫：非管理员返回 `Err("permission_denied")`。
- 运行态守卫：模拟运行中（RunningKeyboard / RunningMouse / PickingMouse / Recording）返回 `busy`。
- 调用 `driver::uninstall_driver()` → 与安装复用同一 `run_installer_windows(action_param)` 实现，仅参数由 `/install` 改为 `/uninstall`（ShellExecuteExW "runas" + WaitForSingleObject 等待退出）。
- 命令成功返回即代表已调度卸载。注意：卸载后驱动仍驻留内核（`create_context()` 在重启前可能仍成功 → `check_driver_status` 不可靠），故**不依赖后端状态判断卸载结果**，由前端以命令成功返回作为「已卸载待重启」信号。

`driver.rs` 重构：原 `install_driver_windows()` 提取为参数化的 `run_installer_windows(action_param: &str)`，`install_driver()` / `uninstall_driver()` 分别传入 `/install` 与 `/uninstall`。

**前端交互（HomePage.vue）**：`Ready` / `InstalledNeedReboot` 状态在原安装按钮位置展示红色「卸载驱动」按钮（`InstalledNeedReboot` 时与「重启电脑」并排）。点击「卸载驱动」**先判管理员权限**——未授权直接提示提权、不展开确认区；管理员权限下展开内联文字确认区，须准确输入「卸载驱动」四字方可点「确认卸载」，输入不符按钮禁用。

**安装与卸载行为对齐**：二者均以命令成功返回作为「待重启」信号，不依赖 `check_driver_status`（驱动需重启才加载/卸载，检测在重启前不可靠）。前端用统一标志 `pendingReboot: 'installed' | 'uninstalled' | null` 驱动卡片展示：

- 安装成功 → `pendingReboot = 'installed'`，卡片文字「驱动已安装，需重启电脑」。
- 卸载成功 → `pendingReboot = 'uninstalled'`，卡片文字「驱动已卸载，需重启电脑」。
- 两种情况按钮均切换为「重启电脑」（复用 `onReboot`），并显示绿色 `driverMessage` 成功提示引导重启。
- 安装的权限前置由后端命令入口 `is_admin()` 守卫实现（非管理员返回 `permission_denied`、不触发 UAC），前端 catch 后提示提权，与卸载的前端预判效果一致。

页面滚动：首页 `.home` 与设置页一致采用 `overflow-y: auto` + `scrollbar-gutter: stable`，预留滚动条宽度避免内容增减（如展开卸载确认区）时页面横向抖动；滚动条样式复用各页统一的 `::-webkit-scrollbar` 规则。

### 12.4 设备选择策略

Interception 需要选择键盘/鼠标设备，但不能依赖用户先按任意键。

建议策略：

- 启动模拟前懒初始化 Interception context。
- 遍历设备 1-20，调用 `interception::is_keyboard(device)` / `interception::is_mouse(device)` 判断设备类型。
- 选择第一个检测到的键盘设备和鼠标设备。
- 如果无法检测到设备，返回错误并记录日志。

## 13. 日志系统

使用 `tauri-plugin-log` 或 `tracing` + `tracing-subscriber`。

**Cargo.toml**

```toml
[dependencies]
log = "0.4"
tauri-plugin-log = "2.0.0-beta"
```

**lib.rs**

```rust
use tauri_plugin_log::LogTarget;

fn main() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::default()
                .targets([LogTarget::Stdout, LogTarget::LogDir])
                .level(if cfg!(debug_assertions) {
                    log::LevelFilter::Info
                } else {
                    log::LevelFilter::Error
                })
                .build(),
        )
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

日志示例：

```rust
use log::{info, error};

info!("Application started, config path: {:?}", config_path);
error!("Failed to register hotkey: {}", err);
```

### 13.1 应用启动流程（强制顺序，对应需求 3.6）

Tauri `setup` 钩子中必须按以下顺序执行，热键注册依赖已加载的配置：

```rust
.setup(|app| {
    // 1. 初始化日志
    // 2. 加载/初始化 mimic.ini：存在则解析；不存在写默认；损坏用默认覆盖
    let config = config::load_or_init();          // info: config path
    // 3. 检测 Interception 驱动状态
    let driver_status = driver::check_interception_driver();
    // 4. 用加载到的配置注册全局热键（默认或用户配置）
    hotkeys::register(app.handle(), &config.hotkeys)?;   // 失败发 hotkey_registration_failed
    // 5. 将 config / driver_status / Idle 写入 SharedState
    // 6. 前端就绪后通过事件同步初始状态
    Ok(())
})
```

启动顺序约束：

- 必须先完成第 2 步（配置加载）再做第 4 步（热键注册），不得用硬编码默认热键抢先注册。
- 驱动不可用不阻塞启动；仅将 `driver_status` 标记为非 `Ready`，模拟相关命令在执行前再校验。

## 14. 权限策略与标记

代码中关键位置添加 `// ADMIN_POLICY: ...` 标记，方便后续手动调整策略。

> 当前定稿策略（2026-06-10 更新）：应用启动**不主动请求** UAC，仅驱动安装命令需要管理员；模拟运行（热键监听 + 按键/鼠标 worker）在普通权限下即可工作。下方标记用于后续若再调整策略时快速定位。

示例：

```rust
// ADMIN_POLICY: Application starts WITHOUT admin elevation (no startup UAC)
// ADMIN_POLICY: Driver installation requires admin rights (guarded in install command)
pub fn install_interception_driver() -> Result<(), String> {
    // ...
}

// ADMIN_POLICY: Simulation runtime does NOT require admin (after driver loaded)
pub fn run_keyboard_simulation() {
    // ...
}
```

### 14.1 启动提权配置（2026-06-10 更新：按需提权）

**策略变更**：启动时**不再**主动请求 UAC，应用以普通权限运行。仅「安装驱动」时按需引导提权。

- **启动行为**：
  - 应用进程以调用方权限启动（普通用户即可），不调用 `ShellExecuteW("runas")`。
  - manifest 不含 `requireAdministrator`。
  - 启动后通过 `is_admin()` 检测当前权限，仅作首页状态展示。

- **首页权限状态**：
  - 已授权 → 绿色「管理员权限已授予」。
  - 未授权 → 橙色「管理员权限受限，安装驱动需提权」+「以管理员身份重启」按钮（用户主动点击才触发 UAC）。

- **驱动安装**：
  - `install_interception_driver` 命令入口检查 `is_admin()`，未授权直接返回 `Err("permission_denied")`。
  - 前端收到 `permission_denied` → 提示「请点击上方"以管理员身份重启"按钮」。
  - 用户点击重启按钮 → `request_admin_restart` → `ShellExecuteW("runas")` 触发 UAC → 重启进程为管理员 → 再点「安装驱动」即可正常调度安装器。

- **模拟运行**：
  - 驱动安装并加载后，热键监听与按键/鼠标 worker 均通过 Interception 用户态接口工作，**不需要管理员权限**。
  - 即使应用是普通权限启动，只要驱动已就绪（`DriverStatus::Ready`），所有模拟功能可用。

**实现方式**：保留 `admin::is_admin()` / `admin::restart_as_admin()` 不变；移除 `pub fn run()` 入口处的启动期 UAC 请求逻辑；`install_interception_driver` 与 `reboot_system` 命令保留运行时权限守卫。

### 14.2 阶段 10 落地形态

- 后端模块 `src-tauri/src/admin.rs`，依赖 `windows-sys` 0.59（Win32_Foundation / Win32_Security / Win32_System_Threading / Win32_UI_Shell），仅在 `#[cfg(windows)]` 下提供真实实现。
- `is_admin()` → `OpenProcessToken(GetCurrentProcess, TOKEN_QUERY)` + `GetTokenInformation(TokenElevation)`；任一 API 失败均视为「非管理员」并 `log::warn!`。
- `restart_as_admin()` → `ShellExecuteW` 以 `runas` verb 拉起当前 exe 路径，失败返回错误字符串（用户取消 UAC 也算失败）。
- **启动期不再调用 UAC**（2026-06-10 调整）：`pub fn run()` 入口直接进入 `tauri::Builder`，不再 `is_admin()` + `restart_as_admin()`。setup 阶段仍调用 `is_admin()` 用于首页状态展示与命令守卫。
- Tauri 命令：
  - `get_admin_status() -> bool`（首页 onMounted 调用，用于判断显示绿色「已授予」还是橙色「受限」）
  - `request_admin_restart(app: AppHandle) -> Result<(), String>`：用户在拒绝首次 UAC 后改主意时点击「以管理员身份重启」触发；调度成功后由后端 spawn 一个延迟 200ms 的线程调用 `app.exit(0)`，给前端留出 UI 反馈窗口，避免双开。
- 关键点都加了 `// ADMIN_POLICY:` 标记：`admin.rs` 模块顶部、`run()` 入口启动期检测、`get_admin_status` 命令、`request_admin_restart` 命令。

## 15. 前端样式设计

### 15.1 主题变量

**src/styles/theme.css**

```css
:root {
  /* === 品牌色盘（来源：extra/color_palette.jpg） === */
  --color-graphite-900: #23262C;  /* RGB(35,38,44)   主背景 */
  --color-graphite-700: #3A3F47;  /* RGB(58,63,71)   容器/卡片 */
  --color-cloud-zinc:   #D1D5DB;  /* RGB(209,213,219) 次要文本 */
  --color-safety-orange:#FE7733;  /* RGB(254,119,51)  强调/警告 */
  --color-neon-sprout:  #B1FA63;  /* RGB(177,250,99)  成功/运行 */
  --color-paper-white:  #FFFFFF;  /* RGB(255,255,255) 主文本 */

  /* === 补充色（语义扩展） === */
  --color-graphite-elevated: #2C2F36;  /* 介于 900/700 之间，hover 用 */
  --color-border-default:    #4A4F58;  /* 由 700 提亮一档 */
  --color-border-subtle:     #2F333A;  /* 细分隔线 */
  --color-text-disabled:     #6B7079;  /* 未勾选行、禁用状态 */
  --color-error-warm:        #FF5A4A;  /* 错误状态专用（暖红） */
  --color-accent-hover:      #FF8A4D;  /* Safety Orange hover */
  --color-accent-pressed:    #E5611E;  /* Safety Orange pressed */

  /* === 语义映射 === */
  --bg-primary:     var(--color-graphite-900);
  --bg-secondary:   var(--color-graphite-700);
  --bg-elevated:    var(--color-graphite-elevated);
  --border-color:   var(--color-border-default);
  --border-subtle:  var(--color-border-subtle);
  --text-primary:   var(--color-paper-white);
  --text-secondary: var(--color-cloud-zinc);
  --text-disabled:  var(--color-text-disabled);
  --accent:         var(--color-safety-orange);
  --accent-hover:   var(--color-accent-hover);
  --accent-pressed: var(--color-accent-pressed);
  --success:        var(--color-neon-sprout);
  --warning:        var(--color-safety-orange);
  --danger:         var(--color-error-warm);

  /* Layout */
  --titlebar-height: 40px;
  --sidebar-width: 180px;
  --statusbar-height: 32px;

  /* Animation */
  --transition-fast: 120ms;
  --transition-normal: 220ms;
}
```

### 15.2 圆角与边框

```css
.app-container {
  width: 600px;
  height: 400px;
  background: var(--bg-primary);
  border-radius: 12px;
  border: 1px solid var(--border-color);
  overflow: hidden;
}
```

### 15.3 自定义标题栏

```html
<div class="titlebar" data-tauri-drag-region>
  <!-- 左：应用图标 + 应用名 -->
  <span class="title">
    <img class="app-icon" src="/icon.png" alt="" />
    <span class="app-name">Mimic</span>
  </span>
  <!-- 中：当前页面名，居中显示（需求 3.1） -->
  <span class="current-page">{{ currentPageLabel }}</span>
  <!-- 右：窗口控制按钮 -->
  <div class="titlebar-buttons">
    <button @click="minimizeWindow">−</button>
    <button @click="closeWindow">✕</button>
  </div>
</div>
```

**说明**：

- `data-tauri-drag-region` 属性标记拖拽区域，Tauri 自动处理窗口拖动；当前页面名为静态文本可纳入拖拽区，按钮需阻止拖拽（点击可用）。
- 标题栏布局自左到右：应用图标 + 应用名 `Mimic`、当前页面名（居中）、最小化 / 关闭按钮。
- `currentPageLabel` 为 `AppPage` 到中文页名的映射（如 `home` → "首页"、`keyboard` → "按键模拟"）。
- **运行状态摘要不在标题栏**，统一放在底部状态栏（见 15.4-bis / 需求 3.10）。

### 15.3-bis 底部状态栏

底部状态栏展示当前运行状态摘要（需求 3.1、3.10）：

- **形式**：彩色圆点 + 状态文案
- **颜色映射**（基于新色盘）：
  
  | 状态 | 圆点颜色 | 中文文案 |
  |------|---------|---------|
  | `Idle` | `--text-disabled` (#6B7079) | 待机 |
  | `ReadyKeyboard` | `--text-secondary` (Cloud Zinc) | 当前可启动按键模拟 |
  | `ReadyMouse` | `--text-secondary` (Cloud Zinc) | 当前可启动鼠标模拟 |
  | `RunningKeyboard` | `--success` (Neon Sprout) | 按键模拟运行中 |
  | `RunningMouse` | `--success` (Neon Sprout) | 鼠标模拟运行中 |
  | `PickingMouse` | `--warning` (Safety Orange) | 正在拾取鼠标坐标 |
  | `Error` | `--danger` (#FF5A4A) | 错误 |

- 通过 `runtime_status_changed` 事件实时更新。
- 状态栏**右侧**显示当前全局热键摘要：「启动：xx ｜ 停止：xx」，绑定到 `appStore.hotkeys`，热键变更后即时刷新（与首页「当前热键」卡片同源）。文案保持简短，不带「当前热键」前缀。

### 15.4 首页内容（状态仪表盘，对应需求 3.3.1）

首页采用紧凑状态仪表盘，展示核心状态信息，**不包含步骤引导或跳转按钮**（左侧菜单已足够明显）：

- **顶部区域**：
  - 应用名 + 版本号
  - 固定文案简介（一句话）：如"Windows 按键与鼠标模拟工具"
  
- **管理员权限状态**：
  - ✅ **已授权**：绿色图标 + "管理员权限已授予"
  - ❌ **未授权**：橙色警告图标 + "管理员权限受限，部分功能不可用"
  - 未授权时不阻止应用启动，仅在首页明确标识
  
- **驱动状态卡片**：
  - 展示 `DriverStatus`：
    - `Ready`：绿色 ✅ "Interception 驱动已加载"
    - `NotInstalled`：灰色 ⚠️ "驱动未安装" + **"安装驱动"按钮**（调用 `install_driver`）
    - `InstalledNeedReboot`：黄色 ⚠️ "驱动已安装，需重启系统"
    - `Error`：红色 ❌ "驱动错误"
  - 安装按钮点击后：
    - 后端通过 `runas` 静默调用 `install-interception.exe`
    - 显示进度提示（如"正在安装驱动..."）
    - 安装成功后弹窗提示**必须重启**
  
- **当前热键概览**：
  - 展示启动热键 / 停止热键的 `keyLabel`（只读，编辑在设置页）
  - 格式："启动：F12 | 停止：F12"

### 15.5 运行期锁定蒙版（对应需求 3.9）

```html
<!-- 蒙版仅覆盖中部主区域，绝不覆盖标题栏与状态栏 -->
<div class="main-area">
  <AppSidebar />
  <component :is="currentPageComponent" />
  <div v-if="appStore.isLocked" class="lock-overlay"></div>
</div>
```

- `lock-overlay` 为半透明灰色，绝对定位铺满 `.main-area`（菜单 + 内容），**不含任何文字、图标、按钮**。
- 通过 `pointer-events` 拦截点击，禁止菜单切换与数据编辑。
- 由 `appStore.isLocked` 控制；进入 `RunningKeyboard` / `RunningMouse` 时置真，确认 worker 停止后置假。
- 运行文案只出现在标题栏摘要与底部状态栏，不在蒙版上。

### 15.6 关键前端交互细节（对应需求 3.3.2 / 3.3.3 / 3.3.4）

- **间隔时间输入**：`type="text"` + 输入过滤（仅保留数字、剥离非数字字符），**不使用** `type="number"`（避免浏览器自带的加减步进按钮）；空值时回退到 `DEFAULT_INTERVAL_MS`。失焦时持久化（对应反馈 L4）。
- **滚动条宽度预留**：列表容器固定预留滚动条宽度（如 `scrollbar-gutter: stable;`），使无滚动条 → 有滚动条切换时列表宽度不突变。
- **设置页热键失焦回显**：捕获输入框聚焦时进入捕获并显示提示；若失焦时未捕获到新按键，则回显失焦前的原热键值（捕获前先快照原值）。
- **设置页热键捕获逻辑**（对应反馈 L3）：设置页热键输入框内**允许捕获已注册的全局热键**（如 F11/F12）。全局热键始终注册在系统级别，但输入框获得焦点时，`KeyboardEvent` 在到达 Tauri global-shortcut 监听器之前被前端 `preventDefault()` 拦截，因此可以正常捕获。前端在 `keydown` 事件处理中调用 `event.preventDefault()` + `event.stopPropagation()`，确保热键不会触发其他行为。
- **按键捕获范围外按键处理**（对应反馈 Q7）：用户按了白名单之外的按键（如方向键、PrintScreen）时，**不提示错误，继续等待**下一次按键。
- **热键与按键列表冲突校验**（对应反馈 Q6）：设置页保存热键时，后端检查启动热键和停止热键是否与 `keyboard_actions` 列表中任意 `scan_code` 冲突；冲突时返回错误并拒绝保存，前端显示"热键与按键模拟列表冲突，请更换"。
- **未勾选行视觉差异**（对应反馈 L5）：未勾选行降低透明度至 60% 或灰化背景色，**严禁使用** `text-decoration: line-through`（删除线）。
- **鼠标坐标输入框**（对应反馈 L6）：X/Y 坐标仅作为只读显示元素（`<span>` 或 `readonly` input），不允许手动输入，只能通过"坐标拾取"按钮修改。坐标为 `null` 时显示占位符（如"—"或"未设置"）。
- **按键模拟页添加按钮位置**（对应反馈 L7）：位于捕获输入框旁边；鼠标模拟页添加按钮位于列表下方。

## 16. 默认配置常量

**src-tauri/src/config.rs**

```rust
pub const DEFAULT_INTERVAL_MS: u64 = 20;

pub fn default_config() -> AppConfig {
    AppConfig {
        keyboard_actions: vec![KeyboardAction {
            id: "default-keyboard-1".to_string(),
            selected: true,
            key_label: "F".to_string(),
            scan_code: 33,
            interval_ms: DEFAULT_INTERVAL_MS,
        }],
        mouse_actions: vec![MouseAction {
            id: "default-mouse-1".to_string(),
            x: None,
            y: None,
            interval_ms: DEFAULT_INTERVAL_MS,
        }],
        hotkeys: HotkeyConfig {
            start: CapturedKey {
                key_label: "F12".to_string(),
                scan_code: 88,
            },
            stop: CapturedKey {
                key_label: "F12".to_string(),
                scan_code: 88,
            },
        },
    }
}
```

## 17. 错误处理

- 命令返回 `Result<T, String>`，前端捕获错误并展示提示。
- 关键操作失败时记录 error 级别日志。
- 驱动未安装/初始化失败时，不阻止应用启动，仅禁用模拟功能。

## 18. 热键提示音

> 对应需求 3.13。在启动/停止热键**生效**时播放提示音。

### 18.1 技术选型

- 采用 Win32 `PlaySoundW`（`windows-sys` 的 `Win32_Media_Audio` feature），不引入额外音频 crate。
- 调用 `PlaySoundW(path, NULL, SND_ASYNC | SND_FILENAME | SND_NODEFAULT)`：
  - `SND_ASYNC`：异步播放、立即返回，不阻塞热键监听 / worker 线程。
  - `SND_FILENAME`：第一参数按文件路径解析（宽字符串，含结尾 NUL）。
  - `SND_NODEFAULT`：失败时不退回系统默认提示音。
- **打断语义天然满足**：`PlaySoundW` 同一进程同时只播放一个声音，新调用会立即停止正在播放的旧声音并播放新声音 —— 正好实现"短时间连续触发优先播放后者、前者被打断"。无需自建队列 / 混音。

### 18.2 文件与路径

- 文件位于 exe 同级目录：`按键开启.wav`（启动）、`按键关闭.wav`（停止）。
- 路径由 `std::env::current_exe().parent()` 拼接；文件不存在时 `log::warn!` 后直接返回，不报错、不阻塞。

### 18.3 触发点（仅在状态切换真正生效处）

模块 [src-tauri/src/sound.rs](../src-tauri/src/sound.rs) 暴露 `play_start()` / `play_stop()`，由 `hotkeys_interception.rs` 在以下位置调用：

| 触发 | 位置 | 守卫 |
|------|------|------|
| `play_start` | `handle_start_keyboard` | 仅当有勾选动作（`!selected_actions.is_empty()`）时；空列表会即时回退 Idle，不算启动生效 |
| `play_start` | `handle_start_mouse` | 在"无有效坐标"早返回之后，确认进入循环时 |
| `play_stop` | `handle_stop_hotkey` | 该函数仅在 Running* 下按停止键时被调用，即停止真正生效 |

- 被忽略的热键（运行中再按启动、待机中按停止）不会进入上述任一 handler，故不播放，无需额外判断。
- 启动键 == 停止键的 toggle 场景：由状态机按当前 `runtime_status` 路由到 START / STOP 分支，分别播放对应声音，逻辑天然正确。

### 18.4 低延迟播放（waveOut 直接操作 — 阶段 19）

> 对应 REQUIREMENTS 3.13「响应延迟 < 10ms」要求。

**问题**：`PlaySoundW` 无论 `SND_FILENAME` 还是 `SND_MEMORY`，内部每次调用都走完整的 MME 管线（设备打开 → 格式协商 → 缓冲入队 → 设备关闭），结构性延迟 150–400ms，无法通过预热/保活消除。

**方案**：弃用 `PlaySoundW`，直接使用 `waveOut` API：
1. 启动时 `waveOutOpen` 打开设备（44100Hz, 16-bit, mono），**设备常驻不关闭**。
2. 加载 wav 后解析出纯 PCM 数据，`waveOutPrepareHeader` 预备缓冲。
3. 触发时 `waveOutReset()`（打断旧播放 ~1ms）+ `waveOutWrite()`（队列新缓冲 ~0ms），内核 → DAC 路径 ~5-10ms。
4. 无需 keepalive 线程 — 设备始终打开，无冷启动开销。
5. 录制覆盖后 `reload_cache` 卸载旧 header + 重新解析 + 预备新 header。

**数据结构**（`sound.rs::inner` 模块内）：

```rust
static DEVICE: OnceLock<Mutex<Option<WaveDevice>>> = OnceLock::new();

struct WaveDevice {
    handle: HWAVEOUT,                            // 常驻打开的设备句柄
    bufs: HashMap<&'static str, PreparedBuf>,    // 预备好的 PCM 缓冲
}

struct PreparedBuf {
    hdr: Box<WAVEHDR>,    // 已 waveOutPrepareHeader 的 header（stable 地址）
    _pcm: Arc<Vec<u8>>,   // 持有 PCM 数据生命周期
}
```

**触发路径**（`play_file`）：
```rust
waveOutReset(dev.handle);           // 打断旧播放，标记所有 buffer done
buf.hdr.dwFlags &= !WHDR_DONE;     // 清 done 标记
buf.hdr.dwFlags |= WHDR_PREPARED;  // 确保 prepared 标记在位
waveOutWrite(dev.handle, &mut buf.hdr, sizeof(WAVEHDR));  // 立即入队
```

**生命周期**：

| 时机 | 动作 |
|------|------|
| `setup` | `sound::init()` → `waveOutOpen` + 加载两个 wav + `waveOutPrepareHeader` |
| `play_start` / `play_stop` | `waveOutReset` + `waveOutWrite`（~5ms） |
| `save_trimmed_audio` 写盘成功后 | `sound::reload_cache(target)` → `waveOutReset` + `waveOutUnprepareHeader` + 重读文件 + `waveOutPrepareHeader` |
| `purge_playing` | `waveOutReset()`（录制前停止播放） |
| 进程退出 | `Drop` → `waveOutReset` + `waveOutUnprepareHeader` × N + `waveOutClose` |

**WAV 格式处理**：
- 设备格式从第一个有效 wav 文件动态推断（不硬编码 44100/16/mono），用该格式 `waveOutOpen`。
- 两个文件必须格式一致（同一录制设备产出自然一致），不匹配的文件静默跳过。
- 录制覆盖后若格式变化（如更换了麦克风），`reload_cache` 自动关闭旧设备、以新格式重新打开。
- 仅要求 PCM 格式（`wFormatTag == 1`），非 PCM 文件拒绝加载。

**预期收益**：触发延迟从 PlaySoundW 的 ~200-400ms 降至 waveOutReset + waveOutWrite + driver pipeline 的 ~5-15ms。

## 20. 提示音录制

> 对应需求 3.14。设置页录制人声 → 写 WAV → 覆盖 exe 同级提示音文件。

### 20.1 技术选型

- **采集**：`cpal` 0.15（纯 Rust，跨平台音频输入）。
- **编码**：`hound` 3.5（纯 Rust WAV 编解码）。
- 不引入 C 依赖，与现有 `PlaySoundW` 播放方案解耦。

### 20.2 数据流

```text
[Mic] → cpal input stream（采集线程）
         ├─ i16 PCM → Mutex<Vec<i16>>（累积到内存缓冲）
         └─ 窗口峰值 → Tauri event "recording_amplitude" { level: 0.0~1.0 }（~30 fps）
                       ↓ 前端 canvas 环形缓冲滚动重绘
[完成/超时] → 锁缓冲 → hound 写 *.wav.tmp → fs::rename 原子替换
```

- 录音 PCM 累积到内存，停止时一次性落盘（5s×44100×2B ≈ 440KB，瞬间完成）。
- 波形数据独立走事件通道，不读主缓冲，避免锁争用。

### 20.3 录制规格与剪裁流程

| 参数 | 值 | 说明 |
|------|----|------|
| 采样率 | 44100 Hz | 设备不支持时退回 `default_input_config()` 原生采样率 |
| 位深 | 16-bit PCM | `PlaySoundW` 必支持，文件最小 |
| 声道 | mono | 提示音无需立体声 |
| 最大时长 | 5 秒 | 到上限自动停止进入剪裁态 |
| 设备 | 系统默认输入 | 不做选择 UI |

> cpal 采集格式可能是 f32/i16/u16，统一在回调内转换为 i16；多声道设备只取第 0 声道降为 mono。

**录制 → 剪裁流程**：

1. 用户点「录制」→ 实时波形 + 倒计时。
2. 用户点「完成」或 5s 到达 → 后端停止采集，返回 base64 PCM + 采样率 + 时长（经 `recording_finished` 事件），不写盘。
3. 前端进入**剪裁态**：全长静态波形 + 双标记（起始/结束，初始覆盖 0 ~ duration）。
4. 用户拖动标记选择保留范围 → 可「试听」选区（Web Audio 不走后端）→ 点「确认」调 `save_trimmed_audio` 命令裁剪写盘，或「取消」丢弃缓冲。

### 20.4 文件覆盖策略

- 目标：exe 同级 `按键开启.wav`（target=`start`）/ `按键关闭.wav`（target=`stop`）。
- 写入前调 `PlaySoundW(NULL, NULL, SND_PURGE)` 停止所有正在播放的提示音，释放可能持有的文件句柄。
- 写 `<name>.wav.tmp` → `fs::rename` 原子替换；失败时旧文件不动，不做额外备份。

### 20.5 后端接口

模块 [src-tauri/src/sound_recorder.rs](../src-tauri/src/sound_recorder.rs)：

```rust
#[tauri::command] fn start_recording(target: String) -> Result<(), String>
#[tauri::command] fn stop_recording() -> Result<u32, String>   // 返回时长 ms
#[tauri::command] fn cancel_recording() -> Result<(), String>
#[tauri::command] fn save_trimmed_audio(target: String, start_ms: u32, end_ms: u32) -> Result<(), String>
```

- `target`: `"start"` | `"stop"`。
- 事件：`recording_amplitude { level }`、`recording_finished { cancelled, samples_base64?, sample_rate?, duration_ms }`、`recording_error { error }`。
- 采集流句柄 + 缓冲存于独立的 `RecordingState`（cpal `Stream` 非 Send，单独放在专用结构，不进 `AppState` 主锁；用一个独立 `Mutex` 守护）。
- `stop_recording` 将 PCM 存入 `AppState.recording_buffer: Arc<Mutex<Option<(Vec<i16>, u32)>>>`，前端剪裁确认后调 `save_trimmed_audio` 从缓冲裁剪写盘。

### 20.6 状态机与守卫

- 新增 `RuntimeStatus::Recording` 临时态，与 `PickingMouse` 同级。
- 现有运行态守卫命令（`save_config` / `update_hotkeys` / `set_current_page` / `start_pick_mouse_position` / `install_interception_driver`）的拒绝集加入 `Recording`。
- 反向：`start_recording` 在 `Running*` / `PickingMouse` / `Recording` 时拒绝。
- 录制仅设置页可用：前端在非设置页不渲染录制区块（后端守卫为兜底）。

### 20.7 前端 UI（设置页「提示音」区块）

- 每项一行：状态点（● 已录制 / ○ 未录制 / 🔴 面板展开中）+ 状态文字 + 「试听」（播放已存在文件）+ 「录制」（展开统一面板）。
- 点「录制」展开**统一的录制 / 剪裁面板**（上下结构），**不立即采集**：
  - 顶部：完整波形 canvas（高 60px），承担录制实时波形与停止后静态波形两种呈现。
  - 下方一行五个按钮：开始录制 / 结束录制 / 试听 / 保存 / 取消。
- **录制中**（点「开始录制」后）：canvas 走环形缓冲实时滚动（`requestAnimationFrame` 重绘，保留最近 ~150 个幅度采样），显示倒计时 `0:02 / 0:05`；可再次点「开始录制」重录（丢弃旧缓冲）。结束录制或 5s 自动停。
- **停止后**（`recording_finished` 事件带回 base64 PCM）：canvas 一次性绘制全长静态波形，叠加双标记（HTML 绝对定位，起始 / 结束，可拖动，最短 100ms，选区外半透明遮罩）+ 选区文字 `已选 1.2s ~ 3.5s（2.3秒）`。
  - 试听：前端 Web Audio 加载 base64 PCM → `AudioBufferSourceNode.start(0, startS, durS)` 播放选区，**不走后端**；播放时一条 HTML 进度线（`--success` 绿色，区别于 `--accent` 橙色标记）随 `AudioContext.currentTime` 从起始走到结束。
  - 保存：调 `save_trimmed_audio` 裁剪写盘并关闭面板。
  - 取消：丢弃缓冲并关闭面板（录制中点取消先 `cancel_recording` 停止采集，收尾在 `recording_finished` 的 cancelled 分支关闭面板）。
- 标记 / 进度线为 canvas 之上的 HTML 叠加层（百分比定位），与波形绘制解耦。
- 互斥：面板展开期间禁用另一项的录制 / 试听按钮；模拟运行（`RunningKeyboard` / `RunningMouse` / `PickingMouse`）期间整个区块禁用。

### 20.8 错误处理（均不影响主功能）

| 场景 | 处理 |
|------|------|
| 无麦克风 | 录制按钮 disabled + 提示「未检测到麦克风」 |
| 设备占用 / 权限拒绝 | `recording_error` 提示，状态回未录制 |
| 写盘失败 | 提示，旧文件不动（tmp + rename） |
| 录制中关应用 | cpal 流 drop，缓冲丢弃，无副作用 |

## 19. 方案变更记录

> 本节记录重大技术方案调整，与需求变更记录（REQUIREMENTS.md）交叉引用。

### DESIGN-CHANGE-001：热键实现从 global-shortcut 切换至 Interception

**关联需求变更**：REQ-CHANGE-001  
**变更时间**：2026-06-08  
**影响阶段**：阶段 12 → 阶段 13

#### 原方案（阶段 12）

- **技术选型**：`tauri-plugin-global-shortcut`（底层 Windows RegisterHotKey API）
- **能力范围**：支持字母/数字/F1-F12/Space/Tab/Esc 作为独立热键；支持修饰键+主键组合
- **API 限制**：RegisterHotKey 要求至少一个非修饰键，不支持纯修饰键作为独立热键
  - 尝试注册 Left Ctrl / Right Shift 等单独修饰键会失败，因为 `key_to_vk()` 无对应 VKCode
  - 阶段 12 修复中临时移除 keyMap.ts 与 scan_code_to_code() 中的修饰键映射，避免"设置成功但全局热键注册失败"的尴尬状态
- **当前可选热键**：字母/数字/F1-F12/Space/Tab/Esc（修饰键不支持）

#### 新方案（阶段 13）

- **技术选型**：Interception 驱动内核层监听
- **能力范围**：支持**所有按键**作为独立热键，包括修饰键（Left/Right Ctrl/Alt/Shift）
- **架构优势**：
  1. **统一底层**：热键监听与按键模拟共用同一 Interception context，避免"RegisterHotKey + Interception"双驱动栈的初始化重复、生命周期错配、潜在键事件干扰
  2. **完整能力**：内核层拦截所有按键，Ctrl/Alt/Shift 作为独立热键自然可识别，无需借助修饰键语义
  3. **可靠门控**：Interception 语义清晰，可过滤自身模拟产生的回声，阶段 13 worker 与监听共用 context 时此优势更明显
- **取舍与风险**：
  - 强依赖驱动：驱动未安装时热键完全不可用（阶段 12 的 global-shortcut 不依赖驱动）
  - 降级策略：驱动 `NotInstalled` / `InstalledNeedReboot` 时不启动监听线程，前端首页与设置页文案明确"安装并重启驱动后热键可用"
  - CPU 开销：监听所有按键事件相比注册特定热键略增，但 Interception 内核驱动延迟通常 <1ms，热键场景无感知影响

#### 实施细节

**代码变更**：

| 文件 | 改动 | 关键点 |
|------|------|--------|
| [src-tauri/src/hotkeys_interception.rs](../src-tauri/src/hotkeys_interception.rs) | 新建 181 行 | 完整的 Interception 热键监听实现，state_machine 门控，支持所有按键 |
| [src-tauri/src/state.rs](../src-tauri/src/state.rs) | 改 | SendInterception 包装器解决 Send/Sync 问题；AppState 新增 interception_context 字段 |
| [src-tauri/src/lib.rs](../src-tauri/src/lib.rs) | 改 | 导入 hotkeys_interception 模块；启动时初始化 Interception context 并启动监听线程；update_hotkeys 简化为仅做配置更新（无注册失败分支） |
| [src-tauri/src/hotkeys.rs](../src-tauri/src/hotkeys.rs) | 改 | 移除所有 tauri-plugin-global-shortcut API 调用；update_hotkeys 仅做冲突校验、持久化、内存更新 |
| [src-tauri/Cargo.toml](../src-tauri/Cargo.toml) | 改 | 移除 `tauri-plugin-global-shortcut` 依赖；interception 版本确认为 0.1.2 |
| [src/lib/keyMap.ts](../src/lib/keyMap.ts) | 改 | 恢复 Shift/Ctrl/Alt 修饰键映射（Left/Right），scanCode 包含 E0 前缀位 |

**监听线程架构**：

```rust
// 伪代码
loop {
  let strokes = interception::wait(context);
  for stroke in strokes {
    if is_keyboard(stroke.device) {
      let key_code = stroke.code;
      let state = stroke.state;
      
      // 状态机门控
      if check_hotkey_match(&config.hotkeys, key_code, state) {
        handle_hotkey_triggered(key_code, current_page, runtime_status)?;
      }
    }
  }
}
```

#### 验证结果

**构建验证**：
- Rust 编译：`cargo check` 通过（10.25s），无 warning，Interception 链接成功
- TypeScript 编译：`npm run build` 成功，52 模块，CSS 18.61 kB / JS 98.20 kB（gzip 35.4 kB）
- 修饰键恢复：keyMap.ts 包含 ShiftLeft/ShiftRight/ControlLeft/ControlRight/AltLeft/AltRight

**接口兼容性**：
- `update_hotkeys` 命令签名不变，前端调用无感知
- 配置文件格式不变，阶段 12 的 mimic.ini 可直接兼容
- 事件协议不变，监听线程通过 `runtime_status_changed` 推送状态变更

#### 与阶段 13 按键模拟的集成

- **共用 context**：`interception::create_context()` 仅创建一次，为 `Arc<Mutex<Option<Context>>>` 持有
- **监听线程长驻**：贯穿应用生命周期，连续监听按键事件
- **模拟 worker 启停**：仅切换 `stop_flag`，不影响监听线程，可以实现"快速停止模拟"无延迟
- **事件分发**：驱动内核层保证键事件只分发一次，Interception crate 提供的 `consume` 能力可选择性拦截或转发

#### 风险与降级

**风险 1 — Interception 驱动崩溃 / 卸载**：
- 监听线程 `wait()` 返回错误时状态切到 `Error`，推送 `simulation_error` 事件
- 前端显示"驱动异常，请重启应用"提示，用户可选择卸载驱动重新安装或重启系统

**风险 2 — 驱动版本兼容性**：
- 使用 crates.io 提供的 `interception 0.1.2` 版本，已由多个项目验证
- 如需更新版本，需实机完整测试（按键事件、模拟可靠性、修饰键识别）

**风险 3 — 与其他 Interception 客户端的冲突**：
- Interception 设计支持多个客户端同时连接
- 内核驱动通过 `predicate` 机制分发事件，多个客户端可共存但事件分发顺序不确定
- 阶段 13 实机测试需验证是否与杀软、输入法等 Interception 用户进程产生干扰

#### 后续迭代计划

阶段 13 后如需进一步优化：

1. **热键组合支持**（如 Ctrl+Shift+A）：当前实现仅支持单键，若需组合可扩展 `HotkeyConfig` 结构并在监听线程中实现组合判断
2. **热键冲突检测**：与按键模拟列表冲突校验已实现，设置页保存时给出提示
3. **监听线程崩溃恢复**：当前失败时进入 `Error` 状态，后续可考虑自动重连机制（需防止重连风暴）

---
