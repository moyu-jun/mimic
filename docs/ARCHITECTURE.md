# Mimic Rust 后端架构

> 更新日期：2026-08-26
> 适用范围：`src-tauri/` 当前实现
> 平台：Windows，输入能力依赖 Interception 驱动

## 1. 架构结论

Mimic 后端采用“薄 Tauri 命令适配层 + 类型化应用状态 + 单所有者 Runtime Actor + 独立会话服务 + 受控基础设施”的结构。

模拟运行期间，动作序列、输入驱动、可中断计时和已按下输入账本只由 Runtime Actor 线程持有。旧共享 `stop_flag`、跨运行公共事件队列、`simulation_worker` 和 executor/scheduler 已删除。Stop 只有在旧运行停止且尽力释放全部已按下输入后才确认；释放失败会进入错误态并保留未释放账本，不会伪装成安全停止。

## 2. 模块与依赖方向

```text
Vue / TypeScript
        |
        | Tauri invoke + typed DTO/events
        v
commands/* ------------------------------+
        |                                |
        v                                v
runner / hotkeys                 config / recorder / picker
        |                                |
        v                                v
runtime::RuntimeHandle            scoped services + ActivityLease
        |
        v
Runtime Actor -> InputDriver -> Interception / Windows API

shared infrastructure:
state.rs | error.rs | paths.rs | sound.rs | driver.rs
```

依赖规则：

- `commands/*` 只负责参数边界、DTO 转换和服务调用，不拥有线程或驱动。
- `runtime` 不依赖 Tauri，可用假驱动进行生命周期测试。
- `runner` 将配置转换为不可变 `ActionSequence`，再提交给 Runtime。
- `listener` 只负责物理输入监听、透传、热键去抖和启动/停止路由。
- 配置、录音、拾取、驱动维护分别通过服务边界协调，不直接改写 Runtime 内部状态。
- Windows FFI 集中在驱动、监听、音频、路径和提权维护边界。

## 3. 当前目录职责

```text
src-tauri/src/
├── lib.rs                 # 依赖装配、启动健康状态、事件适配、退出治理
├── main.rs                # 普通权限 Tauri 入口
├── commands/
│   ├── config_cmd.rs      # 配置事务和日志等级
│   ├── runtime_cmd.rs     # 导航、状态查询和安全停止
│   ├── driver_cmd.rs      # 驱动维护活动边界
│   ├── pick_cmd.rs        # 坐标拾取命令
│   ├── sound_cmd.rs       # 录音、剪裁、试听命令
│   └── system_cmd.rs      # 只读系统权限状态
├── runtime/mod.rs         # Runtime Actor、协议、输入账本、生命周期测试
├── runner/
│   ├── mod.rs             # Runtime facade
│   └── builder.rs         # 配置到 ActionSequence 的纯转换
├── listener/
│   ├── mod.rs             # 监听线程所有权、关闭与 Join
│   ├── filter.rs          # Interception 过滤器
│   └── hotkey.rs          # scan code 去抖与热键路由
├── simulation/
│   ├── action/            # Action / ActionSequence
│   ├── driver/            # InputDriver 与 Interception 实现
│   ├── event.rs           # 原子输入事件
│   └── mouse/             # 坐标转换
├── state.rs               # PageId / Activity / RuntimeHealth / ActivityLease
├── error.rs               # CommandError DTO 与 ErrorRecoveryPolicy
├── config.rs              # 严格校验、INI 编解码、原子持久化
├── paths.rs               # portable 路径、目录隔离、链接/重解析点拒绝
├── hotkeys.rs             # 热键候选事务
├── mouse_picker.rs        # token 化拾取会话与超时服务
├── sound_recorder.rs      # token 化录音任务与 WAV 候选发布
├── sound.rs               # WAV 解析、内存缓存、waveOut 预热与播放
└── driver.rs              # 驱动检测、helper 完整性校验和提权客户端
```

## 4. 状态模型

内部唯一事实来源由四组类型组成：

- `PageId`：Home、Keyboard、Mouse、Custom、Settings。
- `Activity`：Idle、Simulating、Recording、PickingMouse、DriverMaintenance、PersistingConfig。
- `SimulationMode`：Keyboard、Mouse、Custom。
- `RuntimeHealth`：Healthy、Degraded、Error。

`RuntimeStatus` 只用于前端兼容展示，由上述状态派生，不作为后端可写控制变量。

活动协调采用单活动模型。普通局部服务通过 `ActivityLease` 获取活动，离开作用域自动释放；Runtime、录音和拾取因存在跨线程 token/确认协议，分别在其所有者完成后释放。配置持久化也占用 `PersistingConfig`，避免运行、录音或驱动维护与写盘交叠。

## 5. Runtime Actor

### 5.1 协议

`RuntimeHandle` 使用有界通道发送：

- `Start { run_id, sequence, mode, reply }`
- `Stop { reply }`
- `Shutdown { reply }`

Actor 返回同步确认。`run_id` 唯一标识一次运行，旧运行事件不能影响新运行。

### 5.2 单所有者资源

Actor 线程独占：

- `InputDriver`
- 当前动作和步骤游标
- 可中断等待状态
- 当前 `run_id`
- 已按下键盘键和鼠标按钮账本

执行动作产生的 Delay 由 Actor 等待，并可被 Stop/Shutdown 控制消息打断。每个按下事件成功发送后才进入账本；释放成功后才从账本移除。

### 5.3 停止与故障语义

```text
Running
  | Stop / natural completion / Shutdown
  v
best-effort release_all
  | all released              | release failed
  v                           v
Idle + Stopped ack        Error + pressed ledger retained
```

Stop 成功确认意味着旧 run 不再产生输入且账本已清空。快速重启、长延迟中断、Shutdown 和释放失败均由假驱动单元测试覆盖。

## 6. 监听器与热键

监听线程内部创建并销毁自己的 Interception context，不把线程亲和对象放入 `AppState`。`ListenerHandle` 持有关闭通道和 `JoinHandle`，应用退出时回收。

热键使用物理 scan code 去抖：第一次 KeyDown 触发，重复 KeyDown 忽略，KeyUp 后复位。键盘/鼠标透传检查实际发送数量，不完整发送会记录错误。

自定义序列只在详情页存在激活 ID 时可启动。启动前检查当前序列内启用的键盘动作与全局热键冲突，冲突时阻止启动并向前端报告动作 ID。

## 7. 配置与前端一致性

配置写入流程：

```text
immutable candidate
  -> validate
  -> acquire config transaction + ActivityLease
  -> create_new temporary file
  -> write + sync_all + size check
  -> atomic replace
  -> swap AppState.config
  -> frontend marks confirmed snapshot
```

任何写盘失败都不会提交后端内存候选。前端配置写入串行化，并保存最后一次后端确认快照；最新写入失败时回滚界面。创建自定义序列仅在持久化成功后进入详情，删除仅在持久化成功后返回列表，失败时恢复原序列且不提供撤销功能。

导航采用后端确认后提交：录音、拾取或模拟期间，后端拒绝切页，前端不会提前切换。录音时侧栏禁用，但录音面板的结束、保存和取消仍可使用；模拟与拾取使用主区域蒙版。

## 8. 录音、提示音与拾取

录音任务具有 token、控制通道、启动握手和 `JoinHandle`。旧 token 不能释放新会话活动。停止后缓冲仅保存在内存，保存时先生成单声道 16-bit PCM WAV 候选并校验。

提示音提交先完成：

1. 读取并严格解析候选 WAV；
2. 创建候选 waveOut 设备和播放缓冲；
3. 获取音频缓存锁；
4. 原子替换目标文件；
5. 无失败步骤地交换内存缓存。

因此候选准备或文件替换失败时，旧文件和旧缓存保持不变。启动后后台完成 WAV 读盘、设备打开和缓冲准备；失败将 `RuntimeHealth` 标记为音频降级，不阻塞输入 Runtime。

坐标拾取使用唯一 token、30 秒超时服务和原页面/原坐标快照。Esc、显式取消、超时或读取失败都恢复快照，不写入伪造坐标。

## 9. 错误协议

所有可能失败的 Tauri command 返回稳定 `CommandError`：

```text
code       稳定机器码
message    不含内部路径和底层敏感细节的用户安全描述
retryable  是否可重试
recovery   criticalRuntime | localOperation | optionalAudio
```

底层详细错误只写日志。分类集中在 `error.rs`：

- `CriticalRuntime`：输入安全或 Runtime/监听器故障。
- `LocalOperation`：配置、导航、录音、驱动维护等局部失败。
- `OptionalAudio`：启动预热或播放能力降级。

录音事件仍使用事件 DTO，不属于 command 返回协议。

## 10. 权限与便携数据

普通 Tauri/WebView 进程不整体提权。安装、卸载或重启由用户操作触发，并只启动独立的 `mimic-elevated-helper.exe`：

- helper 是独立 workspace crate，不链接 Tauri/WebView，也不加载网页或通用脚本；
- 协议固定为 v1，仅允许 install、uninstall、reboot；
- 参数数量、PID 和 128-bit CSPRNG nonce 严格校验；
- helper 校验调用进程映像必须是同一发布目录中的 `mimic.exe`；
- 一次性请求在 `data/temp` 中使用 create-new 创建并通过原子 rename 消费；
- 不接受任意命令、任意路径或附加参数；
- 主程序在 UAC 前校验 build-time 固化的 helper SHA-256，helper 执行动作前再校验安装器固化 SHA-256；
- helper 拒绝关键目录/文件的 Windows 重解析点，重启只执行 System32 下的 `shutdown.exe` 和固定参数；
- 发布脚本先构建 helper，再把其最终 SHA-256 嵌入主程序，最后验证构建副本和打包副本哈希一致。

当前产品仅按个人、朋友使用及开源项目维护，不面向商业销售或正式渠道分发，因此发布架构只保留 SHA-256 完整性闭环，不包含商业代码签名和证书生命周期流程。若未来改变分发定位，应重新进行威胁建模和发布方案评审。

可写数据固定在：

```text
data/
├── mimic.ini
├── audio/
├── logs/
└── temp/
```

可执行资源位于应用根目录和 `driver/`。`PortablePaths` 从规范化后的可执行路径派生目录，数据目录及固定文件目标拒绝符号链接/Windows 重解析点。配置、WAV 和 helper 请求临时文件使用独占创建。

## 11. 线程所有权

| 线程/任务 | 所有者 | 停止与回收 |
| --- | --- | --- |
| Runtime Actor | `RuntimeHandle` | Shutdown 确认 + Join |
| Interception listener | `ListenerHandle` | shutdown channel + Join |
| Mouse picker timeout | `PickerTimeoutHandle` | shutdown channel + Join |
| Recording worker | `RecordingHandle` | control channel + Join |
| Audio warmup | `AudioWarmupHandle` | 有界一次性任务 + Join |

应用状态只保存服务 Handle，不保存跨线程共享的 Interception context。

## 12. 验证边界

自动测试覆盖 Runtime 并发/停止、活动不变式、配置事务与原子替换故障、音频双侧提交故障、INI/WAV 任意输入、热键去抖、拾取 token、错误分类和提权协议解析；发布脚本额外校验独立 helper 的参数失败关闭、嵌入哈希和打包副本一致性。独立 fuzz runner 使用固定 seed 变异真实解析入口，崩溃语料落入隔离 artifacts 目录，并由每周 Windows CI 持续运行。

Runtime Actor 的 200 样本 Stop 分布已自动验证 P95 ≤ 100ms、最大值 ≤ 250ms。下列内容仍必须在 Windows 真机环境完成：真实键鼠与 Interception 行为、物理驱动释放耗时、UAC 取消和驱动安装/卸载/重启、麦克风设备异常及 waveOut 延迟。
