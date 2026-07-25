# Mimic 后端架构设计

> Mimic 是仅面向 Windows 的按键/鼠标模拟工具。本文档描述其 Rust 后端（`src-tauri/`）的整体架构。
>
> 后端经过两轮重构，均已实施并校验：
> - **模拟核心**：统一键鼠事件模型 + 驱动抽象，双线程统一延迟模型，时序精确。
> - **应用层**：命令 / 运行器 / 监听三层拆分，配置→序列转换可插拔，面向「混合序列 + 更丰富操作类型」可扩展。
>
> **平台**：Windows only，依赖第三方 Interception 驱动。
> **注**：代码是最终事实来源；文档与代码不符时以代码为准。

---

## 目录

1. [设计目标与约束](#设计目标与约束)
2. [分层架构](#分层架构)
3. [目录结构](#目录结构)
4. [核心数据模型](#核心数据模型)
5. [核心接口](#核心接口)
6. [线程模型与执行流程](#线程模型与执行流程)
7. [时序模型](#时序模型)
8. [扩展性](#扩展性)

---

## 设计目标与约束

**目标**
1. **统一核心**：键盘与鼠标共用同一套事件、动作、调度与消费链路。
2. **驱动抽象**：模拟逻辑通过 `InputDriver` trait 与具体驱动解耦，便于替换/测试。
3. **消除重复**：键盘/鼠标/未来混合三种启动分支收敛为一条参数化链路。
4. **单一归属**：配置→序列的转换集中为可独立单测的纯逻辑。
5. **时序精确**：所有延迟（动作内部 + 步骤间隔）串行执行，长按不与后续间隔并行。

**约束（对前端冻结的契约）**
- `AppConfig` 序列化形状：`keyboardConfigs` / `mouseConfigs` / `hotkeys`（camelCase）。
- `RuntimeStatus` 枚举字符串：`Idle` / `ReadyKeyboard` / `RunningKeyboard` / … 前端逐字符匹配。
- 全部 Tauri 命令名不变。
- 平台 Windows only；Interception 依赖不变。

---

## 分层架构

```
┌───────────────────────────────────────────────────────────────┐
│ 命令层 commands/    Tauri 命令实现（薄，仅参数校验 + 转发）        │
├───────────────────────────────────────────────────────────────┤
│ 运行器层 runner/    一次运行的生命周期：选数据→构建序列→起停       │
│   ├─ SimulationRunner   启动→循环→停止的生命周期封装（键鼠共用）    │
│   └─ SequenceBuilder    配置 → ActionSequence 的可插拔转换器       │
├───────────────────────────────────────────────────────────────┤
│ 监听层 listener/    Interception wait/receive 主循环 + 事件路由    │
│   ├─ filter   过滤器设置                                          │
│   ├─ hotkey   热键匹配 + 状态机门控 → 调 SimulationRunner          │
│   └─ mod      主循环：鼠标透传/坐标拾取分派、键盘转 hotkey          │
├───────────────────────────────────────────────────────────────┤
│ 模拟核心 simulation/    统一事件/动作/驱动/调度                     │
│   event / action / keyboard / mouse / driver / executor          │
│   simulation_worker（事件消费者，常驻线程）                        │
├───────────────────────────────────────────────────────────────┤
│ 基础设施   config / state / sound / driver(安装) / admin          │
└───────────────────────────────────────────────────────────────┘
```

**关键分层原则**：监听层只「识别事件 + 决定调哪个编排动作」，不构建序列、不 spawn；
运行器层统一编排一次运行；模拟核心不感知配置与页面，只消费 `ActionSequence`。

---

## 目录结构

```
src-tauri/src/
├── lib.rs                  # setup 装配 + invoke_handler 注册
├── commands/               # Tauri 命令实现（按主题分文件）
│   ├── config_cmd.rs       #   load/save 配置、init warning
│   ├── driver_cmd.rs       #   驱动 check/install/uninstall/reboot
│   ├── runtime_cmd.rs      #   set_current_page / stop / get_runtime_status
│   ├── pick_cmd.rs         #   start_pick_mouse_position
│   └── sound_cmd.rs        #   提示音录制/预览
├── runner/                 # 运行器层
│   ├── mod.rs              #   SimulationRunner
│   └── builder.rs          #   SequenceBuilder trait + 键盘/鼠标 builder
├── listener/               # 监听层
│   ├── mod.rs              #   start_listener 主循环 + 路由
│   ├── filter.rs           #   filter 设置
│   └── hotkey.rs           #   热键匹配 + 状态机门控
├── simulation/             # 模拟核心
│   ├── event.rs            #   SimulationEvent / MouseButton
│   ├── action/             #   Action / ActionSequence / ActionStep
│   ├── keyboard/           #   KeyAction + to_events()
│   ├── mouse/              #   MouseAction + CoordinateMapper
│   ├── driver/             #   InputDriver trait / Interception 实现 / 设备缓存
│   ├── executor/           #   Scheduler
│   └── timing.rs           #   动作内部延迟常量
├── simulation_worker.rs    # 事件消费线程（驱动通信 + sleep）
├── config.rs               # 配置模型与持久化（INI）
├── state.rs                # AppState / RuntimeStatus / DriverStatus
├── hotkeys.rs              # 热键配置校验/持久化
├── mouse_picker.rs         # 坐标拾取（被 listener 复用）
└── sound*.rs / admin.rs / driver.rs   # 提示音 / 提权 / 驱动安装
```

---

## 核心数据模型

数据自上而下经历三次形态转换：**配置**（对外冻结，跟页面走）→ **业务动作**（对内统一）→ **原子事件**（喂驱动）。

### 配置层 — `config.rs`（对前端冻结）

```rust
struct AppConfig {
    keyboard_configs: Vec<KeyboardConfig>,   // 序列化为 keyboardConfigs
    mouse_configs:    Vec<MouseConfig>,       // 序列化为 mouseConfigs
    hotkeys:          HotkeyConfig,
}

enum KeyActionType  { Press, Hold, Combo }
enum MouseActionType { ClickLeft, ClickRight, ClickMiddle, ScrollUp, ScrollDown, Drag }
// KeyboardConfig / MouseConfig：enabled + action_type + 参数（scan_code / x,y / interval_ms 等）
```

键盘页与鼠标页各自独立存储（满足「数据跟页面走」），前端契约零改动；未来混合页新增 `mixedConfigs` 字段即可，不动现有两个字段。

### 业务层 — `simulation/action` + `keyboard` / `mouse`

```rust
struct ActionSequence { steps: Vec<ActionStep> }
struct ActionStep     { action: Action, interval_ms: u64 }   // interval_ms = 执行后等待

enum Action { Keyboard(KeyAction), Mouse(MouseAction), Delay(u64) }

enum KeyAction  { Press{scan_code}, Down{scan_code}, Up{scan_code},
                  Hold{scan_code, duration_ms}, Combo{modifiers, key} }
enum MouseAction { MoveTo{x,y}, Click{button,x,y}, Down{button}, Up{button},
                   Hold{button,duration_ms}, Scroll{delta}, Drag{button,from,to} }
```

`Action::to_events()` 把一个业务动作展开为原子事件序列（含动作内部延迟）。

### 事件层 — `simulation/event.rs`（驱动原子事件）

```rust
enum SimulationEvent {
    KeyDown{scan_code}, KeyUp{scan_code},              // 键盘
    MouseMove{x,y}, MouseButtonDown{button},
    MouseButtonUp{button}, MouseWheel{delta},          // 鼠标
    Delay{ms},                                         // 控制：唯一的延迟表达
    Stop,                                              // 保留，当前停止走 stop_flag
}

enum MouseButton { Left, Right, Middle, Side1, Side2 } // Side* 预留
```

**要点**：每个事件对应一次驱动调用，不含业务逻辑；**所有延迟统一为 `Delay` 事件**，由 worker 单线程 sleep 执行——这是时序精确的关键。

---

## 核心接口

### 驱动抽象 — `simulation/driver`

```rust
trait InputDriver: Send + Sync {
    fn send_keyboard(&self, scan_code: u16, is_press: bool) -> Result<(), DriverError>;
    fn send_mouse_move(&self, x: i32, y: i32) -> Result<(), DriverError>;
    fn send_mouse_button(&self, button: MouseButton, is_press: bool) -> Result<(), DriverError>;
    fn send_mouse_wheel(&self, delta: i32) -> Result<(), DriverError>;
    fn is_ready(&self) -> bool;
}

enum DriverError { NotReady, DeviceNotFound(String), SendFailed(String) }
```

当前唯一实现 `InterceptionDriver`：持有共享 Interception context，内含设备编号缓存（`device.rs`，启动扫描一次）与坐标转换器（`mouse/coordinate.rs`，屏幕坐标 → 0–65535 归一化）。未来可加 `SendInputDriver` / `MockDriver` 而不动上层。

### 序列构建器 — `runner/builder.rs`

```rust
trait SequenceBuilder {
    fn build(&self, config: &AppConfig) -> Option<ActionSequence>; // None = 无有效动作，忽略启动
    fn running_status(&self) -> RuntimeStatus;                      // 该模式对应运行态
}
```

`KeyboardSequenceBuilder` / `MouseSequenceBuilder` 各自把本页配置映射为 `ActionSequence`（无勾选/坐标全空返回 `None`）。这是配置→序列的**单一归属**，纯逻辑，已有单元测试覆盖。

### 运行生命周期 — `runner/mod.rs`

```rust
struct SimulationRunner;
impl SimulationRunner {
    fn start(app, state, builder: &dyn SequenceBuilder); // build→None 则忽略；否则置态+播音+emit+spawn 生产者
    fn stop(app, state);                                 // 置 stop_flag→短等待→回 Idle+播音+emit
}
```

`start`/`stop` 统一了原先键盘/鼠标各一份的重复启动逻辑；监听层按当前页选一个 builder 传入即可。

### 调度器 — `simulation/executor/scheduler.rs`

```rust
struct Scheduler;  // Scheduler::new(event_tx).execute_loop(&sequence, &stop_flag)
```

循环展开序列：逐步 `action.to_events()` 发送，再把 `interval_ms` 作为 `Delay` 事件发送；每个事件发送前检查 `stop_flag`，保证停止响应及时（生产者不自己 sleep）。

---

## 线程模型与执行流程

三条线程，职责清晰：

```
应用启动 (lib.rs::run / setup)
  ├─ 建 channel(event_tx, event_rx)，event_tx 存入 AppState
  ├─ simulation_worker::start(event_rx, state, worker_ctx)   ← 常驻【消费线程】
  └─ listener::start_listener(app, state, listener_ctx)      ← 常驻【监听线程】

用户按启动热键
  └─【监听线程】listener/hotkey：命中启动 + 状态机门控
        └─ 按 current_page 选 builder → SimulationRunner::start
              ├─ builder.build(config) → Option<ActionSequence>
              ├─ 置 running_status + 清 stop_flag + play_start + emit
              └─ spawn【生产者线程】Scheduler::execute_loop → event_tx.send()
                                                    └─►【消费线程】串行执行事件/延迟

用户按停止热键
  └─ SimulationRunner::stop → stop_flag=true → 回 Idle + play_stop + emit
```

- **监听线程**：阻塞在 Interception `wait()`；鼠标事件透传（拾取态捕获左键回填坐标），键盘事件转 `hotkey` 做热键匹配与门控。
- **生产者线程**：每次运行临时 spawn，只展开序列、发事件、查 `stop_flag`，不阻塞。
- **消费线程**（`simulation_worker`）：常驻，持有 `InputDriver`，从 channel 取事件，做状态门控后调用驱动或 `sleep`。

**并发要点**：两个独立 Interception context（监听阻塞 `wait` / 消费仅 `send`）；`event_tx` 在 `AppState`；`stop_flag` 为 `Arc<AtomicBool>`。context 由 `Arc<Mutex<Option<SendInterception>>>` 持有，Mutex 串行化访问（见 `state.rs` SAFETY 注释）。

---

## 时序模型

**问题**：若生产者用 `sleep(interval)` 自己等待步骤间隔，长按（`Hold`）动作会与后续间隔并行，导致时序错乱。

**方案**：步骤间隔也转换为 `Delay` 事件发给消费线程，与动作内部延迟一样在**同一线程串行 sleep**。生产者只发事件、不 sleep。

示例序列「按 W → 等 100ms；点击(500,300) → 等 50ms」的实际时序：

```
T0      KeyDown{W}
T0+10   KeyUp{W}          (动作内部 Delay 10)
T0+110  MouseMove{500,300} (步骤间隔 Delay 100)
T0+115  MouseButtonDown
T0+125  MouseButtonUp
T0+175  回到步骤1          (步骤间隔 Delay 50)
```

动作内部延迟常量集中在 `simulation/timing.rs`（如按键按下→释放、点击稳定/保持延迟），便于统一调优「人类感」。

---

## 扩展性

| 演进项 | 改动范围 | 是否动既有链路 |
|--------|---------|--------------|
| 新增操作类型（Hold/Scroll/Drag/Combo 等） | `KeyAction`/`MouseAction` 加变体 + `to_events()` + 对应 builder 映射 | 否 |
| 新增混合序列页面 | `AppConfig` 加 `mixedConfigs` + `MixedSequenceBuilder` + hotkey 分支 | 否（纯新增） |
| 替换底层驱动（SendInput/Mock） | 实现 `InputDriver` trait | 否 |
| 多序列并发 / 宏录制回放 | 扩展 runner（未来议题） | 是（届时评估） |

**核心扩展点**：操作类型隔离在动作层 + builder；模式/页面隔离在 builder + hotkey 分支。监听、调度、消费三条主链路对二者均免疫。

---

## 修订历史

| 版本 | 日期 | 说明 |
|------|------|------|
| 模拟核心重构 | 2026-07-24 | 统一 `SimulationEvent`/`KeyAction`/`MouseAction`；`InputDriver` 抽象；双线程统一延迟模型。 |
| 应用层重构 | 2026-07-25 | commands/ 抽离；runner/（SequenceBuilder + SimulationRunner）；hotkeys_interception 拆为 listener/{mod,filter,hotkey}。静态检查 + builder 单测通过。 |
| 整合归档 | 2026-07-25 | 两轮方案整合为本文档；均已实施并校验。 |
