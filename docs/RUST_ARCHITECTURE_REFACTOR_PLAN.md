# Mimic Rust 后端架构重构计划与实施方案

> 状态：核心架构重构已实施，发布安全验收待完成
> 版本：v1.2
> 日期：2026-08-26
> 范围：`src-tauri` Rust 后端、Tauri 命令边界及相关构建/安全配置
> 依据：当前仓库代码审查、Rust 架构审查与安全/可靠性问题清单

## 1. 执行摘要

Mimic 当前 Rust 后端已经具备较清晰的业务概念：配置会被转换为动作序列，动作序列再拆分为可执行的原子输入事件；键鼠驱动也存在抽象接口。这些设计应当保留。

当前最需要重构的不是动作模型，而是运行时生命周期与全局状态管理。模拟任务通过共享停止标志、公共事件队列和游离线程协作，任务没有单一所有者，因此可能出现旧任务污染新任务、停止不及时、输入未释放、线程无法回收等问题。同时，`AppState` 承担配置仓库、页面状态、模拟状态、驱动上下文、录音器和事件队列等多种职责，状态组合和锁边界难以验证。

目标方案如下：

1. 使用专用 **Runtime Actor** 单线程拥有模拟任务、输入驱动、计时器和按下状态。
2. 使用类型化状态替代单一 `RuntimeStatus` 大枚举和字符串页面状态。
3. 将配置、录音、鼠标拾取、导航和活动互斥拆分为独立服务。
4. 将 Tauri 限定为适配层，核心运行时不依赖窗口、事件发射器或全局 `AppState`。
5. 缩小提权边界，避免整个 WebView 进程以管理员身份运行。
6. 采用“兼容外部命令、先建旁路、再切换生产流量”的渐进迁移方式，避免大爆炸重写。

结论：当前分层概念基本合理，但运行生命周期没有单一所有者。应优先替换共享停止标志、游离生产线程和跨任务公共事件队列，再拆分状态与服务，最后完成权限和构建资源加固。

## 2. 当前架构评估

### 2.1 建议保留的设计

- `SequenceBuilder` 及“配置 → 业务动作 → 原子事件”的转换思路。
- 动作、序列和配置模型的领域边界。
- `InputDriver` 抽象方向及可替换驱动的意图。
- 监听器与执行器运行在不同线程/上下文的基本方向。
- 模块按 simulation、listener、recorder、config 等功能拆分的基础。
- 有界通道以及后台任务不阻塞 UI 的设计目标。

### 2.2 当前运行链路

```text
Tauri command / hotkey
        |
        v
shared AppState + RuntimeStatus + stop_flag
        |
        +--> detached producer thread
        |        |
        |        v
        |   shared bounded event queue
        |        |
        |        v
        +--> simulation worker --> concrete driver --> Windows input
```

问题不在于线程数量本身，而在于任务生命周期分散在多个共享对象中：生产线程、消费线程、停止标志、状态枚举和队列都只掌握任务的一部分信息。

### 2.3 核心问题与根因

| 优先级 | 问题 | 根因 | 风险 |
|---|---|---|---|
| P0 | 新任务启动时复用并清除全局 `stop_flag` | 任务缺少独立身份和所有者 | 旧生产线程恢复投递，污染新任务 |
| P0 | 公共事件队列不携带 `run_id` | 队列与任务生命周期解耦 | 上一轮积压事件可能在下一轮执行 |
| P0 | 生产线程没有 `JoinHandle` | 线程被分离，停止仅靠轮询和固定等待 | 无法证明任务已结束，也无法可靠回收 |
| P0 | 取消仅发生在原子事件之间 | 未跟踪已按下的键鼠状态 | 中止或错误时可能留下“卡键/卡鼠标” |
| P1 | `Delay` 使用不可中断睡眠 | 控制指令与时间等待不在同一事件循环 | 停止响应延迟，不易测试 |
| P1 | `RuntimeStatus` 混合页面、模拟、录音、拾取和错误 | 多个正交状态被压入一个枚举 | 合法并发被禁止，非法组合仍可能出现 |
| P1 | `AppState` 是 God Object | 数据、服务、线程上下文和 UI 状态没有所有权边界 | 锁竞争、耦合和测试成本持续上升 |
| P1 | 核心模块直接依赖 Tauri 和具体驱动 | 端口/适配器边界未落实 | 难以做纯单元测试和故障注入 |
| P1 | 驱动上下文使用宽泛 `unsafe Send/Sync` 包装 | 上下文没有由实际使用线程创建和持有 | 线程安全依赖人工约定 |
| P2 | 命令返回 `Result<_, String>` | 错误协议没有类型和稳定代码 | 前端难以区分、恢复和观测错误 |

### 2.4 安全与可靠性问题

- 整个 Tauri/WebView 进程可能以管理员权限运行，攻击面过大。
- 可执行目录旁的 DLL、安装器或其他可变资源在高权限进程中加载/执行，存在替换风险。
- 使用裸命令名调用系统程序时依赖路径解析，应改用受控 API 或经过验证的绝对路径。
- 配置清洗后写盘的是副本，但内存可能仍保留未清洗值，形成“磁盘与运行时不一致”。
- 配置缺少严格上限，超大次数、时长或序列可能造成资源耗尽。
- 录音启动存在 check-then-act 竞态；损坏 WAV 的解析可能发生越界 panic。
- 驱动设备选择逻辑没有真正扫描/验证目标设备，发送结果未被完整检查。
- 构建期资源复制失败可能只警告，产物完整性不能得到保证。
- Tauri CSP、opener 和 capability 应按最小权限原则复核。

## 3. 重构目标、非目标与约束

### 3.1 目标

- 任意时刻至多存在一个活动模拟任务，生命周期由一个组件完整拥有。
- `stop` 成功返回时，旧任务已停止产生输入并完成按键/鼠标释放。
- 新任务永远不会消费旧任务遗留事件。
- 延时、循环和长动作可快速中断，停止延迟 P95 不超过 100ms、最坏不超过 250ms。
- 后端核心可在无 Tauri、无真实 Interception 驱动环境中测试。
- 状态迁移合法性可由类型和集中式协调器验证。
- Tauri command 保持薄适配层，首轮迁移保留外部命令名。
- 权限、资源路径、配置边界和错误协议具备明确安全约束。

### 3.2 非目标

- 不在本轮重写动作 DSL 或改变现有配置文件格式。
- 不引入 Tokio；当前工作负载适合一个受控线程和标准通道。
- 不同时重做前端 UI。
- 不在同一提交中完成全部文件移动、逻辑改写和协议升级。
- 不为了形式上的“无锁”消除所有合理的锁。

### 3.3 兼容性约束

- Windows 输入驱动行为与最终需求文档中已确认的配置语义保持一致。
- 第一阶段保留现有 Tauri command 名称和主要事件名称。
- 启动时仅对需求明确允许缺省的旧字段补默认值；其余格式、结构或边界非法配置直接由代码内置默认配置覆盖，不做备份或局部抢救。
- 每个阶段必须可独立构建和测试；切换 Runtime 前保留可回退入口。

## 4. 关键架构决策（ADR）

### ADR-01：模拟运行时采用专用 Actor

Actor 在一个长期存活线程中独占：

- 当前活动任务及游标；
- `InputDriver` 实例；
- 已按下输入账本；
- 计时和取消处理；
- 运行时快照更新。

所有 Start、Stop、Shutdown 和查询操作通过控制通道发送。用消息顺序定义并发语义，不再依赖多个共享原子量和固定睡眠。

### ADR-02：Actor 直接推进动作，不预填充跨任务公共事件队列

事件序列由 Actor 按需生成和执行。当前动作执行完毕后才推进游标；延时使用 `recv_timeout` 同时等待控制指令。公共 `SimulationEvent` 队列从生产路径移除，消除跨任务积压。若保留 `SimulationEvent`，它仅是 Actor 内部执行模型，不作为跨线程生命周期协议。

### ADR-03：状态拆分为正交类型

状态事实来源拆为 Navigation、Activity、SimulationMode 和 RuntimeHealth：

    enum Navigation {
        Loading,
        Page(PageId),
    }

    enum RuntimeHealth {
        Healthy,
        Degraded { capability: Capability, code: ErrorCode },
        Error { code: ErrorCode },
    }

    enum SimulationMode {
        Idle,
        RunningBuiltIn,
        RunningCustom,
    }

    enum Activity {
        Idle,
        Simulating,
        Recording,
        PickingMouse,
        DriverMaintenance,
    }

CriticalRuntime 错误映射为 RuntimeHealth::Error；OptionalAudio 仅映射为对应音频能力 Degraded。前端旧 RuntimeStatus 只能由完整快照派生，不再作为事实来源。

### ADR-04：核心层不依赖 Tauri

Actor 只产生领域事件和快照。Tauri 适配器负责转换为 `emit`、command 返回值和前端 DTO。核心层不得导入 `tauri::AppHandle`、窗口类型或全局 `AppState`。

### ADR-05：线程亲和资源由实际线程创建并持有

监听上下文由监听线程创建；执行驱动由 Runtime Actor 线程创建。去除覆盖范围过大的 `unsafe Send/Sync` 包装，除非第三方 API 安全契约已被明确记录和验证。

### ADR-06：错误使用稳定代码而不是任意字符串

核心错误采用类型化错误；Tauri 边界映射为稳定 DTO：

```rust
struct CommandErrorDto {
    code: ErrorCode,
    message: String,
    retryable: bool,
    details: Option<serde_json::Value>,
}
```

日志可以保留底层错误链，前端协议不得暴露敏感路径或内部实现细节。

## 5. 目标架构

### 5.1 组件关系

```text
Frontend
   |
   v
Tauri Commands / Event Adapter
   |
   +-----------> ConfigService ------> ConfigRepository
   |
   +-----------> ActivityCoordinator
   |                    |
   |                    +--> RecorderService
   |                    +--> MousePickerService
   |                    +--> RuntimeHandle
   |                               |
   |                       Runtime Actor Thread
   |                               |
   |                   Sequence Cursor + Timer
   |                               |
   |                   PressedInputLedger
   |                               |
   |                    InputDriver (port)
   |                               |
   |                 InterceptionDriver (adapter)
   |
Listener Thread --> HotkeyRouter --> UserIntent --> application facade
```

### 5.2 依赖规则

1. `commands` 可以依赖 application service 和 DTO，不得直接操作驱动上下文。
2. application service 可以依赖 domain/runtime port，不依赖 Tauri。
3. runtime 可以依赖 simulation 领域模型和 driver trait，不依赖 command、window、sound 或 config repository。
4. Interception、文件系统、WAV、Tauri、Windows API 属于 adapter。
5. domain 模块不得依赖 adapter。
6. 所有跨线程边界传递拥有所有权的数据或只读快照，不传递线程亲和上下文。

### 5.3 建议目录

```text
src-tauri/src/
├── application/
│   ├── activity.rs
│   ├── facade.rs
│   ├── navigation.rs
│   └── error.rs
├── runtime/
│   ├── actor.rs
│   ├── command.rs
│   ├── handle.rs
│   ├── run.rs
│   ├── snapshot.rs
│   └── pressed_input.rs
├── simulation/
│   ├── action/
│   ├── keyboard/
│   ├── mouse/
│   ├── executor/
│   ├── driver/
│   ├── event.rs
│   └── timing.rs
├── services/
│   ├── config_service.rs
│   ├── recorder_service.rs
│   └── mouse_picker_service.rs
├── adapters/
│   ├── interception/
│   ├── filesystem/
│   ├── audio/
│   └── tauri_events.rs
├── listener/
├── commands/
├── state.rs
└── lib.rs
```

目录移动应晚于行为迁移：先在现有位置引入 facade 和 Runtime Actor，切换成功后再机械移动文件，避免一次提交同时包含重命名与逻辑变化。

## 6. Runtime Actor 详细设计

### 6.1 控制协议

```rust
type RunId = u64;

enum RuntimeCommand {
    Start {
        request: StartRequest,
        reply: SyncSender<Result<RunId, RuntimeError>>,
    },
    Stop {
        reply: SyncSender<Result<StopOutcome, RuntimeError>>,
    },
    GetSnapshot {
        reply: SyncSender<RuntimeSnapshot>,
    },
    Shutdown {
        reply: SyncSender<Result<(), RuntimeError>>,
    },
}
```

Start/Stop 必须有确认通道，command 的成功应表示请求已完成到定义明确的状态，而不是“后台可能开始处理”。

### 6.2 RuntimeHandle

```rust
struct RuntimeHandle {
    command_tx: SyncSender<RuntimeCommand>,
    snapshot: Arc<RwLock<RuntimeSnapshot>>,
    join: Mutex<Option<JoinHandle<()>>>,
}
```

- `command_tx` 使用小容量有界通道，防止调用方无限堆积控制请求。
- `snapshot` 仅存只读投影，不能被外部写入驱动运行时。
- `JoinHandle` 由 Handle 持有，应用退出时必须 Shutdown + Join。
- Actor 意外退出后，Handle 应返回 `RuntimeUnavailable`，不得悄悄创建第二个线程。

### 6.3 Actor 主循环

```text
create driver on actor thread
publish Idle snapshot
loop:
    if no active run:
        block on control command
    else:
        drain at most N control commands
        if stopping:
            release_all, finalize run, publish Idle, acknowledge Stop
        else:
            get next atomic event from current cursor
            execute event and update pressed-input ledger
            publish progress when needed
on shutdown/error/panic boundary:
    best-effort release_all
    publish terminal state
    acknowledge and exit
```

处理规则：

- Start 在 Busy 时返回类型化错误，不隐式覆盖活动任务。
- Stop 是幂等操作；Idle 时返回 `AlreadyIdle`。
- Start 的 `run_id` 单调递增，用于日志、快照和 UI 事件关联。
- 任何 run 完成、失败、停止后，`ActiveRun` 被整体丢弃，不留下可被后续 run 消费的队列。
- Actor 每处理一定数量的零延时事件后主动检查控制通道，避免停止请求饥饿。

### 6.4 可中断计时

禁止在运行路径直接使用长时间 `thread::sleep`。Delay 应使用：

```rust
match command_rx.recv_timeout(remaining) {
    Ok(command) => handle_control(command),
    Err(RecvTimeoutError::Timeout) => finish_delay(),
    Err(RecvTimeoutError::Disconnected) => shutdown(),
}
```

超长 delay 可以一次等待；如果需要周期性进度更新，再按受控最大片段等待。测试时通过 `Clock`/调度接口或短时长验证，不使用真实长等待。

### 6.5 输入释放账本

`PressedInputLedger` 跟踪由当前 run 成功发送的：

- 键盘 scan code；
- 鼠标按键；
- 必要时的组合键顺序和设备标识。

仅在驱动报告发送成功后更新账本。正常 KeyUp/MouseUp 后移除；停止、驱动错误、Actor shutdown 和 panic 保护边界都执行 `release_all`。释放使用逆序，并记录失败但继续尝试剩余项。

### 6.6 Driver port

```rust
trait InputDriver: Send {
    fn key_down(&mut self, key: KeyCode) -> Result<(), DriverError>;
    fn key_up(&mut self, key: KeyCode) -> Result<(), DriverError>;
    fn mouse_down(&mut self, button: MouseButton) -> Result<(), DriverError>;
    fn mouse_up(&mut self, button: MouseButton) -> Result<(), DriverError>;
    fn move_mouse(&mut self, movement: MouseMovement) -> Result<(), DriverError>;
}
```

接口应采用 `&mut self`，只要求 `Send`，不要求 `Sync`。通过 `DriverFactory` 在 Actor 线程内创建真实驱动；测试注入 `FakeDriver`，记录调用和模拟错误。

## 7. 状态与服务拆分

### 7.1 ActivityCoordinator

`ActivityCoordinator` 只管理互斥活动，不保存业务对象。推荐使用 token/lease：

```rust
let lease = coordinator.acquire(ActivityKind::Simulating)?;
runtime.start(request)?;
lease.commit();
```

活动结束由持有者显式释放；异常路径由 lease 的 Drop 或守卫恢复，防止状态永久卡在 Running。互斥矩阵必须集中定义，避免遗漏 `RunningCustom` 之类的分支。

| 当前活动 | 模拟 | 录音 | 鼠标拾取 | 驱动维护 |
|---|---:|---:|---:|---:|
| Idle | 允许 | 允许 | 允许 | 允许 |
| Simulating | 拒绝 | 拒绝 | 拒绝 | 拒绝 |
| Recording | 拒绝 | 拒绝 | 拒绝 | 拒绝 |
| PickingMouse | 拒绝 | 拒绝 | 拒绝 | 拒绝 |
| DriverMaintenance | 拒绝 | 拒绝 | 拒绝 | 拒绝 |

### 7.2 ConfigService

启动加载与用户更新使用两条明确事务：

    startup:
    resolve data/mimic.ini
      -> missing: embedded_defaults -> persist atomically -> publish snapshot
      -> existing: size check -> decode -> supported default-fill -> validate
           -> valid: publish snapshot
           -> invalid: embedded_defaults -> persist atomically -> publish snapshot + warning

    user update:
    candidate -> validate -> persist atomically -> swap in-memory snapshot -> emit changed

要求：

- 默认配置编译进代码，不依赖外部模板。
- 写入同目录临时文件、flush 后原子替换，防止崩溃留下半文件。
- 无效启动配置直接整体覆盖，不备份、不局部保留；覆盖失败则使用内存默认值并报告初始化警告。
- 用户更新保存失败时不得更新内存。
- 固定限制集中定义并在前后端复用，但后端必须独立验证：独立键盘 500、独立鼠标 500、自定义序列 100、单序列动作 1000、名称 64 个 Unicode 字符、间隔 5..3,600,000ms、配置文件 5MiB。
- 对所有文件名做规范化绝对路径校验，拒绝越出对应 data 子目录。
- 日志等级更新采用同一持久化事务；落盘成功后调用运行时日志过滤器即时生效，失败则保持原等级。
### 7.3 RecorderService

- 使用单一锁内的状态迁移消除 check-then-act。
- 录音 session 使用唯一 token；停止只能作用于匹配 token。
- Service 持有 worker `JoinHandle`，退出时可靠停止并回收。
- WAV 解析先验证 RIFF/chunk 长度和算术溢出，再切片读取。
- 最大录制时长固定为 5 秒，并限制采样参数和输出大小。
- 临时录音只写入 portable data/temp，成功后原子发布到 data/audio。

### 7.4 MousePickerService

- 每次拾取分配 token，旧回调不能完成新请求。
- 监听线程只上报输入，Service 决定是否消费/完成。
- Esc、window close 和 30 秒 timeout 必须取消并回收会话。
- 会话保存发起页和原坐标；取消、超时或读坐标失败时恢复原 Ready 状态和原坐标，不持久化 (0, 0)。
- 坐标转换集中在领域值对象，明确物理像素、逻辑像素与绝对/相对坐标。

### 7.5 AudioService

- 应用启动后创建后台预热任务，将两个 PCM WAV 读入内存，完成解析、输出设备初始化和播放缓冲准备。
- play 实时路径只能操作已准备的内存资源，不得读盘、解析文件或打开设备。
- 预热未完成或失败时返回可降级结果，声音可跳过，但不得阻塞模拟 Runtime。
- 新录音发布采用“临时文件写入与校验 -> 构造新内存播放资源 -> 原子替换文件和内存快照”；只有两侧均成功才向界面报告保存成功。
- 只实现 PCM WAV，不引入转换、重采样或通用解码框架。

### 7.6 NavigationState

current_page: String 改为 PageId/类型化枚举。页面是否可用与后台 Activity 解耦。显示状态由 Navigation、Activity、SimulationMode 和 RuntimeHealth 派生，不把任意显示文本作为控制条件。

### 7.7 ErrorRecoveryPolicy

错误恢复由 application 层的集中策略决定，adapter 和前端不得各自猜测最终状态：

- CriticalRuntime：驱动发送、监听器或 Runtime 关键错误。先取消活动 run，执行 release_all，发布 Error 快照；只有重新检测或重新初始化成功后才按当前页面恢复 Ready/Idle。
- LocalOperation：配置保存、拾取、录音或提示音保存失败。事务回滚本次候选值和会话，恢复操作前快照并返回可重试错误，不改变全局 health。
- OptionalAudio：预加载、试听或播放失败。标记音频能力降级并记录日志，不停止 Runtime，不改变全局页面/活动状态。
- 同时出现多类错误时按 CriticalRuntime > LocalOperation > OptionalAudio 取最终状态。

恢复结果必须由单个 RecoveryOutcome 同时携带 Runtime 快照、数据回滚结果、错误 DTO 和可重试条件，避免分别更新造成短暂或永久不一致。

## 8. 监听器与热键

### 8.1 上下文所有权

监听线程自己创建、使用并销毁 Interception 监听上下文，不再把上下文存入全局 `AppState`。线程启动通过一次性 reply 报告成功或错误；主程序保存 `ListenerHandle` 以便 shutdown/join。

### 8.2 纯热键路由

路由函数接收 InputEvent、Bindings 和可变 PressedKeys，以 scan code 记录物理按下状态：首次 KeyDown 可产生意图，重复 KeyDown 被忽略，对应 KeyUp 后才允许再次触发。该函数只判断绑定并返回 StartBuiltIn、StartCustom、Stop、PickMouse 等意图，不直接锁状态、启动线程或 emit UI；application facade 统一验证活动和调用服务。

冲突校验分两层：

- 热键保存时始终检查独立键盘动作列表，冲突则拒绝保存。
- 自定义序列只在进入详情、准备运行或相关配置变化时校验当前序列；冲突动作可继续编辑，但 Runtime Start 必须被拒绝并返回动作定位信息。

### 8.3 输入处置

过滤结果使用明确类型：

```rust
enum InputDisposition {
    PassThrough,
    Consume,
    Emit(UserIntent),
}
```

默认故障策略应倾向 PassThrough，避免异常时吞掉用户真实输入。需要消费的热键应在文档和测试中逐项列出。

### 8.4 设备发现

不要用设备编号范围判断结果冒充“扫描”。应：

1. 枚举候选设备；
2. 发送/接收能力探测或基于实际事件识别；
3. 缓存已验证的键盘/鼠标设备；
4. 设备失效后重新发现；
5. 所有 send 返回值映射为 `DriverError`。

## 9. Tauri 命令边界

Command 只负责反序列化、授权/状态校验、调用 application service 和 DTO 映射。

| 当前模块 | 重构后职责 |
|---|---|
| `commands/runtime_cmd.rs` | 调用 `ApplicationFacade`/RuntimeHandle，不操作 stop flag |
| `commands/config_cmd.rs` | 调用 ConfigService，不直接维护双份状态 |
| `commands/sound_cmd.rs` | 调用 RecorderService/AudioAdapter |
| `commands/pick_cmd.rs` | 调用 MousePickerService |
| `commands/driver_cmd.rs` | 调用受控 DriverMaintenanceService |
| `lib.rs` | 组装依赖、注册 command、管理 Handle 生命周期 |

事件统一通过 `TauriEventAdapter` 转换：

```text
RuntimeDomainEvent -> FrontendEventDto -> app.emit(...)
```

事件必须携带 `run_id`、事件种类和必要的进度信息。前端丢弃不匹配当前 run 的迟到事件。


## 10. 安全边界与资源治理

### 10.1 最小提权模型

已确认采用以下模型：

    normal-privilege Tauri app
              |
              | user initiated UAC launch + fixed versioned request
              v
    small signed elevated helper
              |
              +--> install driver
              +--> uninstall driver
              +--> reboot system

实施状态（2026-08-26）：普通主进程不整体提权；独立 `mimic-elevated-helper.exe` 已从主程序拆出，不链接 Tauri/WebView。v1 固定协议、操作白名单、调用进程映像校验、128-bit CSPRNG nonce、一次性请求消费、关键路径重解析点拒绝，以及应用 → helper → 安装器的编译期 SHA-256 闭环均已实现。发布脚本支持在固化哈希前签名 helper，并在最后签名主程序；由于当前未提供正式证书，Authenticode 仍属于外部发布门禁。

要求：

- 主 Tauri/WebView 进程始终默认以普通用户运行，不提供整体提权重启入口。
- 只有用户点击并确认具体高权限操作后才启动 helper；启动应用、检测驱动和普通模拟不得触发 UAC。
- helper 不加载网页、不解析通用脚本，不接受任意命令、任意可执行路径或任意附加参数。
- 请求协议使用固定 schema、版本、操作白名单、长度限制、nonce 和调用方身份校验；每个请求只表达一个预定义操作。
- 主程序在启动 helper 前验证构建时固化的 SHA-256；helper 在执行驱动安装器前验证编译期固化的 SHA-256。正式生产发布还必须对主程序、helper 和安装器做 Authenticode 签名及发布方验证。
- helper 使用绝对路径访问固定 driver 目录，返回结构化结果后立即退出，不作为常驻高权限服务。
- 用户拒绝 UAC、签名/哈希失败或协议非法时返回可恢复错误，不改变原驱动状态，也不影响普通功能。

### 10.2 便携数据与可执行资源分离

已确认使用 portable mode，但采用固定子目录做逻辑隔离：

    Mimic/
      Mimic.exe
      interception.dll
      driver/
        mimic-elevated-helper.exe
        install-interception.exe
      data/
        mimic.ini
        audio/
          按键开启.wav
          按键关闭.wav
        logs/
        temp/

要求：

- 路径由单一 PortablePaths 组件从规范化后的可执行文件目录派生，不使用当前工作目录，不写入 AppData。
- 所有运行时可写内容只能进入 data：配置写入 data/mimic.ini，日志进入 data/logs，用户 WAV 进入 data/audio，临时文件进入 data/temp。
- Mimic.exe、DLL、helper 和安装器不得写入或从 data 加载；helper 只访问固定 driver 目录中的安装器。
- portable 目录通常可被当前用户整体修改，逻辑分目录不等于 ACL 安全边界；因此每次提权执行都必须重新做签名/哈希校验。
- 配置、日志、音频和临时路径都要规范化并校验父目录，拒绝绝对路径注入、父目录跳转、链接绕过和超长路径异常。
- 发布包可以整目录移动，移动后下一次启动应以新可执行文件目录重新解析 data 路径。

### 10.3 构建资源

优先使用 Tauri resource/bundle 配置管理运行资源。若 `build.rs` 仍复制关键文件：

- 缺少源文件、目标写入失败或校验失败应使构建失败；
- 输出 `rerun-if-changed`；
- 校验文件版本/哈希；
- CI 对打包产物做资源存在性检查；
- 删除仅打印 warning 后继续生成不完整包的路径。

### 10.4 系统调用与 Web 安全

- 系统关机等操作优先使用明确 Windows API；若必须启动进程，使用受信绝对路径和固定参数。
- 复核 `capabilities/default.json`，仅开放已使用 capability。
- CSP 不应保持 `null`；限制脚本、连接、图片和媒体来源。
- 对外部链接使用安全 opener 策略，不向新页面暴露控制能力。
- command 对路径、字符串、数值和枚举进行服务端验证，不依赖前端校验。

## 11. 文件级改造映射

| 当前文件/目录 | 改造动作 | 目标 |
|---|---|---|
| `src/state.rs` | 拆分，只保留依赖容器/只读快照；移除驱动上下文、公共事件队列和共享 stop flag | 消除 God Object |
| `src/runner/mod.rs` | 先封装旧 Backend，后由 RuntimeHandle 替代，最终删除生产线程逻辑 | 统一生命周期入口 |
| `src/runner/builder.rs` | 保留构建能力，改为生成可迭代 run plan/cursor | 支持 Actor 按需执行 |
| `src/simulation_worker.rs` | 迁入 `runtime/actor.rs` 后删除 | 驱动与任务由同一线程拥有 |
| `src/simulation/event.rs` | 删除跨线程 Stop 语义，事件仅供 Actor 内部使用 | 避免双重控制协议 |
| `src/simulation/executor/*` | 改为无 Tauri 依赖的执行/游标组件 | 纯核心、可单测 |
| `src/simulation/driver/input_driver.rs` | 改为 `Send + &mut self`、类型化错误 | 明确线程和错误契约 |
| `src/simulation/driver/interception.rs` | 作为 adapter，由 factory 在 Actor 线程创建 | 隔离 unsafe/FFI |
| `src/simulation/driver/device.rs` | 实现真实发现、失效重试和 send 结果检查 | 提高设备可靠性 |
| `src/listener/mod.rs` | 监听线程自建上下文，提供 ListenerHandle | 删除宽泛 unsafe 共享 |
| `src/listener/hotkey.rs`、`filter.rs` | 提取纯 HotkeyRouter/InputDisposition | 降低监听线程职责 |
| `src/hotkeys.rs` | 合并或作为绑定配置模型，避免重复规则 | 单一热键事实来源 |
| `src/config.rs` | 拆为 ConfigService、Repository、Validator | 原子配置事务 |
| `src/sound_recorder.rs` | 迁入 RecorderService，加入 session token/join | 消除启动竞态 |
| `src/sound.rs` | 作为 AudioAdapter，强化 WAV 边界校验 | 隔离格式/设备细节 |
| `src/mouse_picker.rs` | 迁入 MousePickerService | 明确会话生命周期 |
| `src/commands/*.rs` | 收敛为薄适配层，统一错误 DTO | 稳定前端协议 |
| `src/admin.rs` | 拆出最小权限 helper 客户端 | 缩小提权边界 |
| `src/lib.rs` | 仅做 composition root 和 shutdown 协调 | 统一依赖组装 |
| `build.rs`、`tauri.conf.json`、capabilities | 资源、CSP、权限和打包加固 | 提升产物完整性 |

## 12. 分阶段实施计划

总体原则：每个阶段单独提交，先增加测试和新路径，再切换调用，最后删除旧路径。逻辑改动与大规模文件重命名分开。

### Phase 0：基线冻结与回归保护（2～3 人日）

目标：建立可比较基线，不改变生产行为。

任务：

- 记录现有 command、前端事件、配置 schema 和运行状态协议。
- 为 SequenceBuilder、原子动作展开和关键配置校验补充 characterization tests。
- 增加 FakeDriver，能够记录按下/释放顺序并注入第 N 次发送失败。
- 固化以下门禁：`cargo fmt --check`、`cargo clippy -- -D warnings`、`cargo test`、`cargo check`。
- 增加手工用例：启动、停止、快速重启、长 delay 停止、驱动缺失、录音损坏文件。

验收：

- 当前行为由测试/协议清单覆盖。
- CI 或本地统一脚本可重复执行全部门禁。
- 不包含业务行为变化。

回滚：直接撤销测试提交，不影响产物。

### Phase 1：类型化状态、错误和配置事务（3～5 人日）

目标：先消除状态表达与数据一致性问题。

任务：

- 引入 Navigation、Activity、SimulationMode、RuntimeHealth、ErrorCode 和 RecoveryOutcome。
- 建立旧 `RuntimeStatus` 派生映射，补齐 `RunningCustom` 所有守卫。
- 实现 ActivityCoordinator 和互斥矩阵测试。
- 实现集中 ErrorRecoveryPolicy，以及 CriticalRuntime、LocalOperation、OptionalAudio 的优先级和恢复矩阵测试。
- 实现 ConfigService 的启动校验/默认覆盖事务和用户更新 validate → atomic persist → swap 事务。
- 固化并测试已确认上限：键盘 500、鼠标 500、自定义序列 100、单序列动作 1000、名称 64 字符、间隔 5..3,600,000ms、配置 5MiB。
- command 内部切换到类型化错误，边界暂时可兼容旧字符串返回。

验收：

- 不再以字符串页面或旧状态分支作为内部事实来源。
- 保存成功后内存与磁盘值完全一致；保存失败时二者均不变化。
- 互斥矩阵每个单元格均有测试。
- 三类错误的最终 health、Activity、数据快照和重试条件均有确定断言，adapter 不得绕过集中恢复策略。
- 前端无需改动或只增加兼容解析。

回滚：保留旧 DTO 映射，切回旧 command 输出。

### Phase 2：Runtime Actor 旁路实现（5～8 人日）

目标：完成新运行时，但暂不成为默认生产路径。

任务：

- 新增 `runtime/{actor,command,handle,run,snapshot,pressed_input}.rs`。
- 将 SequenceBuilder 输出适配为 `ActiveRun` cursor。
- 引入 DriverFactory、FakeDriver 和新的 `InputDriver` 契约。
- 实现 Start/Stop/Shutdown、幂等停止、可中断 delay、release_all。
- 建立临时 `RuntimeBackend::{Legacy, Actor}` facade，仅用于迁移期选择。
- 通过单元/集成测试对比 Legacy 与 Actor 的确定性动作序列。

验收：

- Actor 路径在无 Tauri 环境完成测试。
- 1000 次 start/stop/quick restart 压测无旧事件串入新 run。
- stop 成功返回后 FakeDriver 账本为空；长延时停止 P95 不超过 100ms、最坏不超过 250ms。
- Actor shutdown 后线程已 join。
- 驱动第 N 次失败时仍尝试释放其余已按下输入。

回滚：保持 `Legacy` 为默认；删除或关闭 Actor feature/内部开关。

### Phase 3：生产切换与旧运行时删除（3～5 人日）

目标：Tauri command 和热键统一使用 Actor。

任务：

- `runtime_cmd.rs` 仅调用 ApplicationFacade/RuntimeHandle。
- 热键 UserIntent 进入同一个 facade，禁止复制启动/停止逻辑。
- RuntimeDomainEvent 经 adapter emit，并携带 run_id；蒙版中央“取消”按钮复用同一 facade Stop 路径，不建立第二套停止逻辑。
- 应用退出执行 Stop → Shutdown → Join。
- 稳定观察后删除 stop flag、公共模拟事件队列、游离 producer 和 `simulation_worker.rs`。
- 删除临时 Backend 双实现，避免永久维护两套路径。

验收：

- command 与热键路径具有相同状态迁移和错误。
- 旧 run 无法影响新 run；延时中停止 P95 不超过 100ms、最坏不超过 250ms。
- Windows 手工矩阵全部通过。
- 代码中不存在固定 50 ms 等待任务退出的逻辑。
- 代码中不存在无所有者模拟线程。

回滚：删除旧实现前保留一个发布版本的可恢复 tag；出现问题时回滚版本，不在运行中动态切换已启动任务的 Backend。

### Phase 4：监听器所有权与服务拆分（5～7 人日）

目标：移除全局线程上下文和 AppState 多余职责。

任务：

- ListenerHandle 管理启动、shutdown、join。
- 监听上下文在线程内创建/销毁。
- 提取纯 HotkeyRouter、PressedKeys 去抖状态和 InputDisposition；补齐 KeyDown 重复、KeyUp 复位及同键启停测试。
- 引入 RecorderService、MousePickerService 及 session token；拾取支持 Esc、30 秒超时和失败恢复原坐标/原 Ready 状态；整个自定义序列删除由前端确认后才调用单一删除事务。
- AppState 缩减为 composition root 持有的 handles/services。
- 审计并移除可避免的 `unsafe Send/Sync`。

验收：

- listener、recorder、picker 每个后台线程都能被 shutdown/join。
- 重复启动/停止不存在 check-then-act。
- `unsafe` 块逐项有 Safety 注释、最小作用域和测试依据。
- `AppState` 不再直接保存线程亲和驱动上下文。

回滚：服务保持 facade 接口兼容，可按服务独立回退。

### Phase 5：驱动、音频与输入边界强化（3～5 人日）

目标：修复外围适配器的可靠性和 panic 风险。

任务：

- 实现真实设备发现、缓存失效与重新发现。
- 检查每次驱动 send 的结果，定义重试/不可重试分类。
- WAV parser 对所有 chunk 长度、偏移和整数运算做 checked 校验，仅接受需求支持的 PCM WAV。
- 模糊测试 WAV parser 和配置反序列化。
- 增加录音上限、临时文件原子发布和清理策略；保存成功前完成新 WAV 的内存加载和播放资源准备。
- 实现 AudioService 启动后台预热，预先完成读盘、解析、设备初始化和播放缓冲准备；实时 play 路径只使用内存快照。

验收：

- 截断/畸形 WAV 不 panic。
- 设备断开返回可观测错误并触发安全释放。
- 超限输入在执行前被拒绝。
- fuzz/property tests 能在 CI 的有界时间运行。

回滚：adapter 级独立回退，不影响 Actor 协议。

### Phase 6：权限、数据目录与构建加固（5～10 人日）

目标：完成安全边界调整。

任务：

- 主应用默认非管理员运行。
- 将驱动安装、卸载和系统重启迁移到最小签名 helper。
- 定义版本化 IPC、调用方校验和操作白名单。
- 实现 PortablePaths，将配置、日志、用户音频和临时文件固定到可执行文件同级 data 子目录，不使用 AppData。
- 将 helper/安装器与 data 逻辑隔离；每次提权执行前校验 helper 签名以及安装器签名/固化哈希。
- 使用 Tauri resource/bundle 替代脆弱复制，关键失败使构建失败。
- 设置 CSP 并收紧 capabilities/opener。
- 替换裸系统命令路径。

验收：

- 普通功能无需管理员权限。
- helper 只支持列入清单的操作。
- 修改后的 helper/安装器无法通过完整性验证，helper 不能执行任意路径或任意命令。
- 缺少关键资源时构建或启动明确失败。
- 安全配置通过发布构建验证。

回滚：helper 采用版本化部署；可回滚安装包。不得为了回滚重新让 WebView 长期以管理员权限运行，紧急版本应禁用需要高权限的功能。

### Phase 7：清理、文档与发布（2～3 人日）

目标：移除迁移债务并完成交付。

任务：

- 删除 Legacy Backend、旧 RuntimeStatus 写路径和废弃 feature flag。
- 完成物理目录整理和 import 机械调整。
- 更新架构文档、状态图、错误码、线程所有权和发布手册。
- 添加性能/停止延迟基准结果。
- 发布前执行全量自动与手工矩阵。

验收：

- 无双实现、无死 feature flag、无过期 TODO。
- 新成员可从文档定位每个资源的所有者。
- 全部门禁通过，发布包完成签名与资源校验。

回滚：使用发布 tag 回滚；数据库/配置格式若升级必须保留向后兼容读取。

## 13. 测试与验证方案

### 13.1 单元测试矩阵

| 对象 | 必测场景 |
|---|---|
| Sequence/Run cursor | 空序列、单动作/最大动作数、循环回到首项、动作顺序、超限拒绝 |
| Runtime Actor | Idle Start、Busy Start、Idle Stop、运行 Stop、Shutdown、通道断开 |
| Delay | 延时完成、延时中 Stop、延时中 Shutdown、控制请求不饥饿 |
| PressedInputLedger | 重复 down、正常 up、逆序释放、部分释放失败 |
| ActivityCoordinator | 完整互斥矩阵、lease drop、重复 release、异常恢复 |
| ConfigService | 缺失生成默认、非法整体覆盖、覆盖失败内存降级、原子替换失败、路径穿越、已确认各类上限、日志等级即时生效 |
| HotkeyRouter | 独立列表冲突、当前自定义序列冲突定位、重复 KeyDown、KeyUp 复位、同键启停、PassThrough、Consume、RunningCustom |
| WAV/AudioService | 截断 header、超长 chunk、奇数 padding、溢出、非 PCM 拒绝、启动预热、未就绪降级、内存快照原子替换 |
| Device adapter | 无设备、多个设备、设备失效、send 失败、重新发现 |
| ErrorRecoveryPolicy | CriticalRuntime 安全释放并进入 Error、释放失败保持 Error、LocalOperation 回滚、OptionalAudio 降级、并发错误优先级、稳定 code 和敏感细节不外泄 |

### 13.2 生命周期/并发测试

- 快速执行 Start → Stop → Start，验证仅第二个 `run_id` 继续产生输入。
- 多线程同时发 Start，只有一个成功，其余返回 Busy。
- Stop 与自然完成竞争，最终恰好完成一次终止和一次 release。
- Stop 与驱动错误竞争，最终进入 Error 且账本为空；只有驱动重新检测成功后才恢复对应 Ready/Idle。
- 配置保存、坐标拾取、录音和提示音保存分别注入失败，验证候选修改被回滚、原持久化数据保留且状态恢复为操作前 Ready/Idle。
- 音频预加载和播放注入失败，验证声音降级但 Runtime 继续运行，整体状态不进入 Error。
- 应用 shutdown 时存在长 delay，线程在超时预算内 join。
- 监听、录音、拾取并发申请活动，结果符合互斥矩阵。
- command receiver 断开时 Actor 安全释放并退出。

不使用“sleep 一段时间后猜测完成”的脆弱断言；测试通过 reply、barrier、fake clock 或 snapshot 条件等待同步。

### 13.3 属性测试与模糊测试

- 任意合法动作序列执行/中止后，按下账本最终为空。
- 任意状态事件序列不能产生互斥矩阵之外的组合。
- 任意字节 WAV 输入不得 panic 或越界。
- 任意配置文件字节要么触发受控的默认覆盖流程，要么得到满足全部 invariant 的配置。
- 任意动作数量、索引和延时运算使用 checked arithmetic，不产生溢出。

### 13.4 Windows 手工验证矩阵

| 场景 | 验证结果 |
|---|---|
| 无 Interception 驱动启动 | UI 可用，返回 DriverUnavailable，不崩溃 |
| 标准键盘/鼠标 | 动作顺序、坐标和按钮正确 |
| 多键盘/多鼠标 | 选择策略明确、设备失效可恢复 |
| 长按时点击停止 | 所有按键/按钮释放 |
| 长 delay 时停止 | P95 不超过 100ms，最坏不超过 250ms，输入账本为空 |
| 100 次快速启动/停止 | 无串台、无僵尸线程、无卡键 |
| 录音中退出 | 文件收尾/清理正确，线程已 join |
| 损坏音频导入 | 结构化错误，无 panic |
| 普通用户运行 | 主程序保持普通权限，仅用户确认的 helper 操作按需 UAC |
| 拒绝 UAC | 普通功能继续可用，错误可恢复 |
| 缺少/篡改资源 | helper/安装器签名或哈希失败即拒绝提权执行；普通功能可继续 |

### 13.5 质量门禁

每个阶段至少执行：

```powershell
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
cargo check --manifest-path src-tauri/Cargo.toml --all-targets --all-features
npm run build
```

涉及 Windows 驱动、打包或提权的阶段还必须执行真实 Windows 发布构建和手工矩阵。测试通过不等同于安全边界完成，Phase 6 必须单独验收。

## 14. 可观测性

### 14.1 结构化字段

每条关键日志包含：

- `run_id` 或 `session_id`；
- component；
- command/event；
- previous_state/new_state；
- duration_ms；
- error_code；
- driver/device 标识的非敏感摘要。

### 14.2 关键指标

- Start/Stop 成功率和错误码分布；
- Stop 请求到输入全部释放的 P50/P95/P99；
- 活动任务数（应始终为 0 或 1）；
- 后台线程存活数；
- 驱动发送失败和重新发现次数；
- 配置拒绝、录音失败、资源完整性失败次数。

日志不得记录完整用户路径、原始按键内容、录音内容或其他不必要敏感数据。

### 14.3 运行时日志控制

- LogController 是日志等级唯一写入口；启动时读取配置，发布环境缺省 Error，开发环境缺省 Info。
- 设置页只暴露 Error、Warn、Info、Debug；配置原子落盘成功后，通过 log::set_max_level 或 tracing reload handle 在当前进程即时切换，失败则维持原过滤等级。
- 文件 sink 固定写入 data/logs，单文件 5MiB，保留当前文件和 3 个历史文件；轮转失败只能降级报告，不能中止应用。
- 日志等级调整、停止耗时、默认配置覆盖、提权完整性失败和音频预热结果必须结构化记录。

## 15. 迁移与回滚策略

### 15.1 迁移原则

- 先增加新接口和适配层，后迁移调用方，最后删除旧实现。
- 一次提交只处理一种风险：行为变更、目录移动、协议变更尽量分开。
- `RuntimeBackend::{Legacy, Actor}` 只允许存在于 Phase 2～3，必须设置删除任务。
- command 名称先兼容；结构化错误可先“双解析”，再移除字符串错误。
- 仅对明确允许缺省的旧字段补默认值；其余非法配置按需求直接用代码内置默认值覆盖，不生成配置备份。
- 每个阶段结束打可恢复 tag，并记录回滚条件。

### 15.2 触发回滚的条件

- 出现卡键/卡鼠标；
- Stop P95 明显超过目标或出现无法停止；
- 发现旧 run 事件进入新 run；
- 后台线程无法回收；
- 配置丢失/写坏；
- 普通用户核心功能不可用；
- 发布资源完整性校验误伤正常安装。

### 15.3 禁止的回滚方式

- 不在同一进程中把正在运行的 Actor run 热切换回 Legacy。
- 不恢复长期管理员 WebView 作为权限问题的永久解决方案。
- 不通过忽略驱动发送错误维持“看似可用”。
- 不使用固定 sleep 代替 join/ack。

## 16. 完成定义（Definition of Done）

### 架构

- [x] 模拟任务、驱动、计时和按下账本由单个 Actor 所有。
- [x] 不存在共享 stop flag、跨 run 公共事件积压和游离模拟线程。
- [x] 核心 Runtime 不依赖 Tauri。
- [ ] 每个后台线程都有 Handle、Shutdown 和 Join。
- [x] AppState 不再保存线程亲和上下文。
- [ ] Navigation、Activity、SimulationMode、RuntimeHealth 和 ErrorRecoveryPolicy 是状态及错误终态的唯一事实来源。

### 正确性

- [x] Stop 成功返回时输入全部释放。
- [x] 快速重启不会执行旧任务事件。
- [x] Delay 可中断，停止延迟 P95 不超过 100ms、最坏不超过 250ms（200 样本自动分布门禁）。
- [x] 配置内存值与落盘值一致。
- [x] 录音、拾取和运行时 session 不会相互串台。
- [ ] 所有 FFI/send/文件解析错误均被处理。

### 安全

- [x] 主应用默认普通权限。
- [x] 高权限操作位于最小 helper 且协议白名单化。
- [x] portable data 与可执行资源逻辑隔离，每次提权前验证 helper/安装器签名或固化哈希。
- [x] 路径、长度、次数、时长和文件大小均有上限。
- [x] CSP/capabilities/opener 按最小权限配置。
- [ ] unsafe 块最小化并有 Safety 说明。

### 验证

- [x] fmt、clippy、test、check、前端 build 全通过。
- [x] Runtime 生命周期并发测试通过。
- [x] WAV/config fuzz 或 property tests 通过；固定 seed runner、语料、artifact 和每周 Windows CI 已建立。
- [ ] Windows 手工矩阵通过。
- [ ] 发布构建、签名和资源校验通过。
- [x] 架构、错误码、线程所有权和运维文档已更新。

## 17. 风险登记表

| 风险 | 概率 | 影响 | 缓解措施 |
|---|---:|---:|---|
| Actor 切换改变细微动作时序 | 中 | 高 | characterization tests、FakeDriver 对比、Windows 灰度验证 |
| Stop 与自然完成竞争产生双通知 | 中 | 中 | 单线程终止状态机、一次性 completion |
| release_all 本身部分失败 | 中 | 高 | 逆序尽力释放、继续后续释放、明确错误和人工恢复提示 |
| 第三方驱动线程契约不清楚 | 中 | 高 | 线程内创建/销毁、最小 unsafe、针对版本验证 |
| helper IPC 设计过宽 | 中 | 高 | 固定 schema、白名单、调用方校验、安全评审 |
| portable 目录被当前用户整体篡改 | 中 | 高 | 明示安全降级；helper/安装器每次提权前重新验证签名/固化哈希；协议拒绝任意路径 |
| 双 Backend 长期共存 | 中 | 中 | Phase 3 强制删除项和完成门禁 |
| 目录移动造成评审困难 | 高 | 低 | 与逻辑提交分离 |
| 测试依赖真实 sleep 导致不稳定 | 中 | 中 | reply/barrier/fake clock，禁止猜测式等待 |
| 打包资源差异只在发布时出现 | 中 | 高 | CI 发布构建与产物内容断言 |

## 18. 里程碑与工作量

以下为单名熟悉 Rust/Windows/Tauri 工程师的粗略估算，不含产品决策等待和第三方签名证书流程：

| 里程碑 | 阶段 | 预计工作量 | 交付物 |
|---|---|---:|---|
| M1 基线可控 | Phase 0～1 | 5～8 人日 | 回归测试、类型化状态、配置事务 |
| M2 Runtime 可切换 | Phase 2 | 5～8 人日 | Actor、FakeDriver、生命周期测试 |
| M3 生产链路统一 | Phase 3～4 | 8～12 人日 | 生产切换、线程所有权、服务拆分 |
| M4 可靠性加固 | Phase 5 | 3～5 人日 | 驱动/WAV/上限修复 |
| M5 安全发布 | Phase 6～7 | 7～13 人日 | helper、资源治理、安全配置、发布文档 |

总计约 28～46 人日。建议按 M1～M3 先解决运行正确性，再推进 M4～M5；P0 生命周期问题不应等待安全 helper 设计完成。

## 19. 已确认决策

### 19.1 已确认

| 主题 | 最终结论 |
|---|---|
| 坐标读取失败 | 保留原坐标，恢复窗口和原 Ready 状态，提示错误，不写 (0, 0) |
| 拾取取消 | Esc 取消，30 秒超时自动取消，均不持久化 |
| 热键去抖 | 首次 KeyDown 触发，重复 KeyDown 忽略，KeyUp 后复位 |
| 停止指标 | P95 不超过 100ms，最坏不超过 250ms，成功时已释放输入且旧 run 不再产出 |
| 删除序列 | 整个序列删除需确认且不可撤销；单行动作删除不加确认 |
| D1 数据模式 | portable mode，全部可写数据放入可执行文件同级 data 子目录 |
| D2 提权 | 普通主程序 + 最小签名 helper，用户确认具体操作后才触发 UAC |
| D3 热键冲突 | 独立按键列表始终校验；自定义序列只校验当前准备运行的序列，冲突则定位动作并阻止启动 |
| D4 运行取消 | 运行蒙版中央提供“取消”，复用安全 Stop 并恢复运行前 Ready 状态 |
| D5 配置恢复 | 默认值写入代码；每次启动校验，缺失生成默认，非法直接默认覆盖，不备份 |
| D6 数据边界 | 键盘 500、鼠标 500、序列 100、单序列动作 1000、名称 64 字符、间隔 5..3,600,000ms、配置 5MiB |
| D7 日志 | 发布默认 Error，设置可选 Error/Warn/Info/Debug，持久化且当前进程即时生效 |
| D8 音频延迟 | 不设固定毫秒门槛，由用户验收；启动后后台读入内存并完成设备及缓冲预热，实时路径不做可提前工作 |
| D9 音频格式 | 仅 PCM WAV，不实现格式转换、重采样或通用解码 |

### 19.2 错误后的最终状态

已确认采用分层错误恢复规则，并同步写入最终需求：

- 输入驱动、监听器或 Runtime 关键错误：立即停止、尽力释放全部输入并进入 Error；只有重新检测或重新初始化成功后才恢复相应 Ready/Idle。
- 配置保存、坐标拾取、录音和提示音保存等局部操作错误：回滚本次操作，恢复操作前数据及 Ready/Idle，并返回可重试错误。
- 可选提示音预加载、试听或播放错误：仅降级声音并记录日志，模拟继续运行，整体状态不进入 Error。
- 多类错误同时发生时，CriticalRuntime 优先于 LocalOperation，LocalOperation 优先于 OptionalAudio。
- 恢复状态、数据回滚和用户提示由集中 ErrorRecoveryPolicy 一次性提交，禁止前端和各 adapter 分别修改最终状态。

## 20. 推荐的首个实施批次

第一个开发批次只做 Phase 0 和 Phase 1，不立即切换运行时：

1. 补齐快速重启、停止释放、RunningCustom 守卫和配置一致性测试。
2. 建立 ErrorCode、RuntimeHealth、ActivityCoordinator、ErrorRecoveryPolicy 和旧 DTO 映射。
3. 实现 ConfigService 启动校验/默认覆盖与用户更新原子落盘/内存交换顺序。
4. 引入 FakeDriver，为 Runtime Actor 建立可测试基础。
5. 输出基线测试结果和 Phase 2 接口评审记录。

该批次风险较低，并能为后续 Actor 重构提供状态约束、错误协议和回归基线。完成后再进入 Runtime Actor 实现，避免在缺少测试护栏时直接替换核心执行链路。

---

本计划的核心判断是：保留现有领域模型和动作构建能力，把“任务由多个共享对象协作”改成“任务由 Runtime Actor 完整拥有”。一旦生命周期所有权清晰，状态拆分、服务测试、错误处理和权限治理都会显著简化。
