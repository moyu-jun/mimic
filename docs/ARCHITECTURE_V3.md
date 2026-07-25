# Mimic 整体架构重构设计文档（v3.0 · 应用层）

> 承接 v2.0（`ARCHITECTURE.md`，模拟核心 `simulation/` 已落地），本轮把重构从
> **底层核心**扩展到**核心之上的整个应用层**：监听、调度生命周期、配置→序列转换、
> Tauri 命令层。目标是低耦合、高内聚、面向「混合序列 + 更丰富操作类型」可扩展。
>
> **设计日期**: 2026-07-25
> **架构版本**: v3.0（应用层）
> **状态**: 已实施完成（阶段 A–D 全部落地）
> **范围**: 仅 `src-tauri/` 后端；前端保持现状
>
> ---

## 目录

1. [背景与问题诊断](#背景与问题诊断)
2. [设计目标与约束](#设计目标与约束)
3. [关键决策（已与用户确认）](#关键决策已与用户确认)
4. [目标架构总览](#目标架构总览)
5. [模块设计](#模块设计)
6. [数据模型与序列构建](#数据模型与序列构建)
7. [生命周期与线程模型](#生命周期与线程模型)
8. [扩展性演进路径](#扩展性演进路径)
9. [实施计划](#实施计划)
10. [验证策略](#验证策略)

---

## 背景与问题诊断

v2.0 已把**模拟核心**重构为清晰的分层：`event`（原子事件）/ `action`（业务动作）/
`keyboard` / `mouse` / `driver`（驱动抽象）/ `executor`（调度器）/ `simulation_worker`
（消费端）。这一层已经在**类型层面**支持键鼠混合的 `ActionSequence`。

问题集中在**核心之上的应用层**——它们仍编码着「键盘 vs 鼠标」的旧二分，与核心的统一能力错位：

| 模块 | 现状问题 | 影响 |
|------|---------|------|
| `hotkeys_interception.rs`（556 行） | 单文件混杂 7 个职责：filter 设置、wait/receive 循环、鼠标透传、坐标拾取、热键匹配、状态机、**配置→序列构建 + 线程 spawn** | 高耦合，难测试，难扩展 |
| `handle_start_keyboard` / `handle_start_mouse` | 近乎重复：都在「读配置→构建序列→播放音→emit→spawn→execute_loop」 | 加第三种模式（混合）要再复制一遍 |
| 配置→序列转换 | 内联在 hotkeys 分支里，散落两处 | 无单一归属，无法独立测试 |
| `lib.rs`（672 行） | 全部 Tauri 命令内联在入口文件 | 入口臃肿，命令与装配逻辑混在一起 |
| `RuntimeStatus` | `RunningKeyboard` / `RunningMouse` 硬分裂 | 与「统一序列 / 未来混合模式」方向冲突（但受前端契约约束，见下） |

**核心洞察**：核心已就绪，缺的是核心与配置/监听之间的**「应用编排层」**——
把「监听到启动 → 选出该页数据 → 转成 `ActionSequence` → 驱动一次运行生命周期」
这条链路抽出来、去重、分层。

---

## 设计目标与约束

### 目标
1. **消除重复**：键盘/鼠标/未来混合三种启动分支收敛为一条参数化链路。
2. **单一归属**：配置→序列的转换逻辑集中到一个可独立单测的模块。
3. **拆分巨石**：`hotkeys_interception.rs` 按职责拆为监听、路由、编排三块。
4. **命令归位**：`lib.rs` 只做装配（setup + handler 注册），命令实现移出。
5. **面向扩展**：新增「操作类型」只改动作层；新增「页面/模式」只加一个序列构建器；
   两者互不影响。

### 约束（不可破坏的契约）
- **仅后端**：前端 Vue 代码不动。因此以下三项是**冻结契约**，内部可重构但对外形状不变：
  - `AppConfig` 序列化形状：`keyboardActions` / `mouseActions` / `hotkeys`（camelCase）。
  - `RuntimeStatus` 枚举字符串：`Idle` / `ReadyKeyboard` / `RunningKeyboard` / … 前端联合类型逐字符匹配。
  - 全部 Tauri 命令名（`load_config` / `save_config` / `start_pick_mouse_position` / …）。
- **干净切换**：不迁移旧 `mimic.ini`；格式变更时首次启动回落默认配置即可。
- **§2/§3 原则**：简单优先、外科手术式修改；不为一次性代码加抽象，不引入未要求的灵活性。
- **平台**：Windows only，Interception 依赖不变。

---

## 关键决策（已与用户确认）

| # | 决策 | 结论 |
|---|------|------|
| D1 | 数据模型走向 | **数据跟随页面**：键盘 / 鼠标 / 未来混合各为独立页面，各自独立存储，**同一时刻只运行一个**。内部模型在有收益处可统一（见 D5）。 |
| D2 | 本轮范围 | **仅后端 Rust**，前端保持现状。 |
| D3 | 旧配置兼容 | **干净切换，不迁移**。 |
| D4 | 交付形式 | **先出设计文档待批**，批准后分阶段实施。 |
| D5 | 内部统一策略（本文档推断，待批） | 对外冻结 `keyboardActions`/`mouseActions` 形状；对内引入 **`SequenceBuilder` 抽象**，让「某页配置 → `ActionSequence`」成为可插拔的转换器。新增混合页时只加一个 builder，不动监听与调度。 |

> D5 是把 D1「数据跟页面走」与 v2.0「核心统一」调和的关键：
> **页面各自独立存数据（对外分离），但都转换成同一种 `ActionSequence` 喂给同一个核心（对内统一）**。

---

## 目标架构总览

### 分层职责

```
┌─────────────────────────────────────────────────────────────┐
│ 命令层 commands/         Tauri 命令实现（薄，仅参数校验+转发）    │
├─────────────────────────────────────────────────────────────┤
│ 运行器层 runner/         运行生命周期：选数据→构建序列→起停一次运行 │
│   ├─ mod(SimulationRunner) 一次「启动→循环→停止」的生命周期封装      │
│   └─ builder             配置 → ActionSequence 的转换器（可插拔）   │
├─────────────────────────────────────────────────────────────┤
│ 输入监听层 listener/      Interception wait/receive 循环 + 事件路由 │
│   ├─ filter              filter 设置                            │
│   ├─ hotkey              热键匹配 + 状态机门控                    │
│   └─ picker 接线         鼠标透传 + 坐标拾取分发（复用现有 picker） │
├─────────────────────────────────────────────────────────────┤
│ 模拟核心 simulation/      【v2.0 已完成，本轮不动其内部】           │
│   event / action / keyboard / mouse / driver / executor       │
├─────────────────────────────────────────────────────────────┤
│ 基础设施  config / state / sound / driver(安装) / admin         │
└─────────────────────────────────────────────────────────────┘
```

### 目标目录结构

```
src-tauri/src/
├── lib.rs                    # 瘦身：仅 setup 装配 + invoke_handler 注册
├── commands/                 # 【新】Tauri 命令实现（从 lib.rs 抽出）
│   ├── mod.rs
│   ├── config_cmd.rs         #   load_config / save_config / get_init_warning
│   ├── driver_cmd.rs         #   check/install/uninstall/reboot
│   ├── runtime_cmd.rs        #   set_current_page / stop_simulation / get_runtime_status
│   ├── pick_cmd.rs           #   start_pick_mouse_position
│   └── sound_cmd.rs          #   录制/预览相关命令
│
├── runner/                   # 【新】运行器层（去重的核心）
│   ├── mod.rs                #   SimulationRunner：一次运行的生命周期（启动→循环→停止）
│   └── builder.rs            #   SequenceBuilder trait + 键盘/鼠标 builder
│
├── listener/                 # 【新】由 hotkeys_interception.rs 拆分而来
│   ├── mod.rs                #   start_listener：wait/receive 主循环 + 路由
│   ├── filter.rs             #   filter 设置
│   └── hotkey.rs             #   热键匹配 + 状态机门控 → 调 runner
│
├── simulation/               # 【不动】v2.0 模拟核心
├── simulation_worker.rs      # 【不动】
├── config.rs                 # 【小改】新增 SequenceBuilder 需要的读取便捷方法（若需要）
├── state.rs                  # 【小改】RuntimeStatus 保持字符串契约；补充注释
├── hotkeys.rs                # 【不动】热键配置校验/持久化
├── mouse_picker.rs           # 【不动】坐标拾取，被 listener 复用
├── sound.rs / sound_recorder.rs / admin.rs / driver.rs  # 【不动】
```

> `hotkeys_interception.rs` 会被 `listener/` + `runner/` 取代后删除。

---

## 模块设计

### 1. 运行器层 — `runner/`（本轮核心价值）

这是消除 `handle_start_keyboard` / `handle_start_mouse` 重复的地方。

#### 1.1 序列构建器 — `runner/builder.rs`

```rust
/// 把「某个页面的配置」转换为统一的 ActionSequence。
///
/// 每种模拟模式（键盘 / 鼠标 / 未来混合）实现一个 builder。
/// 新增模式 = 新增一个 builder，监听层与 runner 完全不用改。
pub trait SequenceBuilder {
    /// 从当前配置构建序列；返回 None 表示「无有效动作，忽略本次启动」
    /// （对应现有鼠标「坐标全空则忽略」、键盘「无勾选则回 Idle」的语义）。
    fn build(&self, config: &AppConfig) -> Option<ActionSequence>;

    /// 该模式对应的运行态（受前端字符串契约约束）。
    fn running_status(&self) -> RuntimeStatus;
}

/// 键盘 builder：勾选项 → KeyAction::Press，无勾选返回 None。
pub struct KeyboardSequenceBuilder;

/// 鼠标 builder：有效坐标 → 左键 Click，全空返回 None。
pub struct MouseSequenceBuilder;

// 未来：MixedSequenceBuilder 读取混合页配置，混排键鼠动作。
```

**收益**：配置→序列的两段内联逻辑（现在散在 hotkeys 的两个 `handle_start_*` 里）
收敛到此，成为**纯函数**，可独立单测（属于 CLAUDE.md §4「可独立自动化的纯逻辑」，值得补测试）。

#### 1.2 运行生命周期 — `runner/mod.rs`

```rust
/// 封装一次模拟运行的完整生命周期，替代两个重复的 handle_start_* 分支。
///
/// start 流程（参数化，键鼠混合共用）：
///   1. builder.build(config) → 若 None 直接忽略（不切状态/不播音/不 emit）
///   2. 置 running_status + 清 stop_flag
///   3. play_start() + emit runtime_status_changed
///   4. spawn 生产者线程：Scheduler::execute_loop(sequence, stop_flag)
pub struct SimulationRunner;

impl SimulationRunner {
    pub fn start(
        app: &AppHandle,
        state: &SharedState,
        builder: &dyn SequenceBuilder,
    ) { /* 统一实现 */ }

    /// stop：置 stop_flag → 短等待 → 回 Idle + play_stop + emit
    pub fn stop(app: &AppHandle, state: &SharedState) { /* 统一实现 */ }
}
```

**收益**：`handle_start_keyboard` + `handle_start_mouse` + `handle_stop_hotkey`
从 ~180 行重复代码收敛为一份参数化实现。热键层只需按当前页选一个 builder 传入。

### 2. 输入监听层 — `listener/`

把 556 行的 `hotkeys_interception.rs` 按职责拆开：

- **`listener/filter.rs`**：现有 filter 设置逻辑原样搬入（键盘 DOWN|UP + 鼠标左键）。
- **`listener/mod.rs`**：`start_listener` 主循环——`wait()` → 分派鼠标分支
  （透传 + 拾取，调用现有 `mouse_picker::finish_pick`）/ 键盘分支（转 `hotkey`）。
- **`listener/hotkey.rs`**：热键匹配 + 状态机门控。命中启动时按 `current_page`
  选 builder 调 `SimulationRunner::start`；命中停止调 `SimulationRunner::stop`。

**边界**：监听层只负责「识别事件 + 决定调哪个编排动作」，**不再自己构建序列、不自己 spawn**。

### 3. 命令层 — `commands/`

把 `lib.rs` 里内联的 ~20 个 `#[tauri::command]` 按主题分文件搬出。命令体保持原逻辑，
仅移动位置 + 必要的 `pub` 调整。`lib.rs` 只留 `run()`：日志/配置/状态装配 + `generate_handler!` 注册。

> 命令层是**纯搬迁**（外科手术式），不改任何命令签名或行为，风险最低，可最先做。

---

## 数据模型与序列构建

### 对外冻结，对内统一

```
前端（不动）          后端配置（形状冻结）        编排层（新，统一）        核心（不动）
─────────           ──────────────────        ─────────────────       ──────────
KeyboardPage  ──►   AppConfig.keyboardActions ─┐
MousePage     ──►   AppConfig.mouseActions   ──┤► SequenceBuilder ──► ActionSequence ──► Scheduler
(未来)MixedPage ─►  AppConfig.mixedActions   ──┘   (每页一个 builder)                    (v2.0)
```

- **对外**：`keyboardActions` / `mouseActions` 各自独立存储（满足 D1「数据跟页面走」），
  前端契约零改动。
- **对内**：三条配置都经各自 builder 归一到同一个 `ActionSequence`，喂给同一个 v2.0 核心。

### 未来「混合序列」如何落地（本轮只留接口，不实现）

当进入混合序列需求时：
1. `AppConfig` 增加 `mixedActions` 字段（新增，不动现有两个字段 → 前端旧契约不破）。
2. 新增 `MixedSequenceBuilder` 读取该字段，混排键鼠/延迟/未来动作。
3. `listener/hotkey.rs` 的 `current_page == "mixed"` 分支选此 builder。
4. `RuntimeStatus` 若需 `RunningMixed`，作为**新增枚举值**（前端同步加联合类型时才生效）。

→ 三处新增、零处修改，验证「新增模式不动既有链路」的扩展性目标。

### 未来「更丰富操作类型」如何落地

操作类型（长按 Hold、滚轮 Scroll、拖拽 Drag、组合键 Combo）**已在 v2.0 的
`KeyAction` / `MouseAction` 中预留**。新增一种类型时：
1. 在 `KeyAction`/`MouseAction` 加变体 + `to_events()` 展开规则（核心层，已具备扩展点）。
2. 对应 builder 增加「配置项 → 该动作」的映射。

→ 操作类型的扩展**完全隔离在动作层 + builder**，监听/编排/调度不受影响。

---

## 生命周期与线程模型

线程模型**沿用 v2.0，不改**。本轮只是把「监听线程内的编排逻辑」抽到 `runner/`，
线程数量与归属不变：

```
应用启动 (lib.rs::run / setup)
  ├─ channel(event_tx, event_rx) 存入 AppState              【不变】
  ├─ simulation_worker::start(event_rx, state, worker_ctx)  【不变，常驻消费线程】
  └─ listener::start_listener(app, state, listener_ctx)     【原 hotkeys_interception，拆分后】

用户按启动热键
  └─【监听线程】listener/hotkey.rs 命中启动
        └─ 按 current_page 选 builder
              └─ SimulationRunner::start(app, state, builder)   【新：统一编排】
                    ├─ builder.build(config) → Option<ActionSequence>
                    ├─ 置状态 + play_start + emit
                    └─ spawn【生产者线程】Scheduler::execute_loop → event_tx
                                                        └─►【worker 线程】串行执行
用户按停止热键
  └─ SimulationRunner::stop → stop_flag=true → 回 Idle + play_stop + emit
```

**并发/所有权不变**：`event_tx` 仍在 `AppState`；`stop_flag` 仍是 `Arc<AtomicBool>`；
两个 Interception context（listener 阻塞 wait / worker 仅 send）职责不变。

---

## 扩展性演进路径

| 演进项 | 改动范围 | 是否动既有链路 |
|--------|---------|--------------|
| 新增操作类型（Hold/Scroll/Drag/Combo） | `KeyAction`/`MouseAction` + 对应 builder | 否 |
| 新增混合序列页面 | `mixedActions` 字段 + `MixedSequenceBuilder` + hotkey 分支 | 否（纯新增） |
| 替换底层驱动（如 SendInput/Mock） | 实现 `InputDriver` trait | 否（v2.0 已抽象） |
| 多序列并发 / 宏录制回放 | 需扩展 runner（未来议题，本轮不设计） | 是（届时再评估） |

---

## 实施计划

分阶段，每阶段独立可验证（`cargo fmt` / `clippy -D warnings` / `check`），风险从低到高：

### 阶段 A：命令层抽离（纯搬迁，零行为变更）
- 建 `commands/`，把 `lib.rs` 的命令按主题搬入，`lib.rs` 仅留装配。
- **验收**：`cargo clippy -D warnings` + `cargo check` 通过；命令名/签名零变化。

### 阶段 B：运行器层建立（去重核心）
- 建 `runner/`：`SequenceBuilder` trait + 键盘/鼠标 builder + `SimulationRunner`。
- 为两个 builder 补**单元测试**（纯逻辑：配置→序列，含「空返回 None」边界）。
- 暂时让现有 `hotkeys_interception.rs` 改调用 runner/builder（先不拆监听）。
- **验收**：静态检查通过；builder 单测通过；**实机**：键盘循环 / 鼠标点击循环 / 停止响应正常。

### 阶段 C：监听层拆分
- 把 `hotkeys_interception.rs` 拆为 `listener/{mod,filter,hotkey}.rs`，删除原文件。
- **验收**：静态检查通过；**实机**：热键匹配、页面过滤、坐标拾取、透传全部照旧。

### 阶段 D：清理与文档
- 删除死代码/冗余注释（仅本轮产生的）；更新 `CLAUDE.md` 模块指引与本文档修订history。

> 每阶段完成后交付验收，再进入下一阶段。若某阶段实机不通过，停下诊断根因（CLAUDE.md §规则）。

---

## 验证策略（遵循 CLAUDE.md §4）

- **静态**：`cargo fmt`（无 diff）、`cargo clippy -- -D warnings`、`cargo check`。
- **单元测试**：仅对 `runner/builder.rs` 的纯转换逻辑补测（配置→序列、None 边界）。
  监听/驱动/GUI 交互**不硬造测试**。
- **实机验收**（涉及运行时行为的阶段 B/C 必做）：
  - 键盘模拟循环 / 鼠标点击循环 / 停止响应（< 500ms）
  - 热键匹配、页面过滤、坐标拾取、鼠标事件透传
  - 提示音启停时机
- 响应中明确区分「已静态验证」与「待实机复核」。

---

## 文档修订历史

| 版本 | 日期 | 说明 |
|------|------|------|
| v3.0 | 2026-07-25 | 应用层重构设计：commands / runner / listener 三层拆分，SequenceBuilder 抽象 |
| v3.0 实施 | 2026-07-25 | 阶段 A–D 全部落地：commands/ 抽离；新增 runner/（SequenceBuilder + Keyboard/MouseSequenceBuilder + SimulationRunner，7 个 builder 单测）；hotkeys_interception.rs 拆为 listener/{mod,filter,hotkey}.rs 并删除原文件。静态检查（fmt/clippy -D warnings/check）+ builder 单测通过；运行时行为待实机复核。 |

**文档结束**
