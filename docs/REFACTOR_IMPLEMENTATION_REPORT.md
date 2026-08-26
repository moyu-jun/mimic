# Mimic Rust 重构实施报告

> 更新日期：2026-08-26
> 当前状态：代码级核心重构和独立最小 helper 已完成；Windows 真机与 Authenticode 签名门禁未完成
> 对应方案：[RUST_ARCHITECTURE_REFACTOR_PLAN.md](./RUST_ARCHITECTURE_REFACTOR_PLAN.md)

## 1. 实施结论

本轮已经完成 Runtime Actor、类型化状态、活动 RAII、稳定命令错误 DTO、配置与音频双侧事务、会话 token、便携目录隔离、链接/重解析点防护、前端写入回滚、后台音频预热、独立最小提权 helper 和哈希固定发布链。

旧共享停止标志、跨运行公共事件队列、`simulation_worker` 和 executor/scheduler 已删除。输入运行链路由单线程 Actor 独占驱动、计时、游标和按下账本。Stop 成功返回时旧运行已经终止且输入账本已释放；释放失败进入明确错误态。

当前自动化基线为 54 项 Rust 单元/有界属性测试全部通过，前端生产构建和完整 release 发布链通过。最终门禁结果见第 4 节；真实驱动、UAC、麦克风、音频设备和延迟数据仍不得用自动测试替代。

## 2. 已完成改造

### 2.1 Runtime 与输入安全

- Runtime Actor 使用有界控制通道和同步 reply，Start 返回唯一 `run_id`。
- Delay 可被 Stop/Shutdown 打断，快速重启不会恢复旧 run。
- 成功发送的 KeyDown/MouseDown 才进入输入账本；释放成功才移除。
- Stop/Shutdown 尽力逆序释放；失败保留账本并报告故障。
- `InputDriver` 由 Actor 独占，可用 FakeDriver 测试。
- Interception 设备缓存发送失败后会重新发现，发送数量不完整视为错误。

### 2.2 状态、活动与线程所有权

- `PageId`、`Activity`、`SimulationMode`、`RuntimeHealth` 成为内部事实来源。
- `RuntimeStatus` 只作为前端兼容投影。
- `ActivityLease` 为配置、热键、录音启动和驱动维护提供 Drop 自动释放。
- Runtime、录音和拾取使用各自 token/确认协议，陈旧任务不能释放新活动。
- Runtime、监听器、拾取超时、录音和音频预热均有可回收 Handle/Join。
- Shutdown 在发送控制消息前原子封闭 Runtime，确认返回后 Start/Stop 不再进入关闭中的通道。

### 2.3 配置事务与界面回滚

- 内置默认配置；缺失生成，非法配置完整覆盖为默认。
- 校验数量、名称、间隔、scan code、ID、热键冲突、枚举和文件大小上限。
- 配置事务锁覆盖候选读取、校验、写盘和内存提交。
- 临时文件 `create_new` 独占创建，写入后 `sync_all`，再原子替换。
- 写盘失败不更新后端内存；失败注入测试验证 Activity 回到 Idle。
- 前端写入串行化并保存后端确认快照，失败时回滚。
- 新建序列持久化成功后才进入详情；删除失败保留序列和详情页。
- 页面导航后端确认后再提交，录音期间侧栏禁用，避免前后端页面分叉。

### 2.4 音频与录音事务

- WAV 解析使用检查算术、切片边界、格式和 PCM 长度校验。
- 候选文件、waveOut 设备与播放缓冲全部准备成功后才原子替换文件并交换缓存。
- 准备或替换失败时旧文件和旧内存缓存保持不变。
- 录音临时 WAV 使用独占创建，完成写入、同步和格式验证后才提交。
- 启动后后台读盘、预开设备并准备缓冲；失败回写 `RuntimeHealth::Degraded { audio }`。
- 音频仍只支持需求确定的 PCM WAV。

### 2.5 错误协议与恢复分类

- 所有可能失败的 Tauri command 使用 `CommandResult<T>`。
- `CommandError` 固定包含 code、message、retryable、recovery。
- `ErrorRecoveryPolicy` 分为 CriticalRuntime、LocalOperation、OptionalAudio。
- 对外 message 不暴露内部路径或底层错误；详细信息仅写日志。
- 前端兼容 DTO、JSON 字符串和遗留字符串，驱动错误按稳定 code 展示。

### 2.6 监听、拾取与自定义序列

- 监听线程内部创建/销毁 Interception context，Handle 持有 Shutdown/Join。
- 热键以物理 scan code 去抖；重复 KeyDown 不重复触发。
- 自定义序列运行前检查动作与全局热键冲突。
- 拾取具有唯一 token、30 秒超时和原页面/原坐标快照。
- 取消、超时或失败恢复旧值，不写 `(0, 0)`。
- 动作行类型标签已替换为启用开关；删除整个序列需要确认且不可撤销。

### 2.7 最小权限与路径安全

- 普通 Tauri/WebView 不整体提权，高权限动作迁入独立 `mimic-elevated-helper.exe`。
- helper 不链接 Tauri/WebView，发布体积 265,216 bytes，只允许 install、uninstall、reboot。
- v1 协议校验固定参数、调用 PID、同发布目录 `mimic.exe` 调用者和 128-bit CSPRNG nonce。
- 一次性请求使用 `data/temp` 文件的 create-new + 原子 claim/consume。
- 主程序在 UAC 前验证 build-time 固化的 helper SHA-256；helper 执行动作前验证安装器固化 SHA-256。
- helper 拒绝关键资源重解析点；系统重启使用 System32 绝对路径和固定参数，不经过 shell。
- 发布脚本按“helper 构建/可选签名 → 固化最终哈希 → 主程序构建/可选签名 → 打包副本复核”执行。
- `PortablePaths` 规范化当前可执行文件路径；数据目录和固定文件拒绝符号链接/Windows 重解析点。
- CSP、capabilities 和构建资源检查已收紧。

### 2.8 有界属性与恶意输入测试

- 512 组确定性任意 WAV 字节及所有截断前缀不得 panic。
- 512 组确定性任意 INI 文本不得 panic。
- 配置在候选写入/同步完成后的原子替换故障会保留旧文件并清理临时文件。
- 音频文件发布失败不会切换内存候选；成功路径仅交换一次。
- 独立变异 runner 对真实 INI/WAV 入口完成 500,000 次固定 seed 运行，112,517 cases/s，零 panic。
- 每周 Windows CI 执行 2,000,000 次变异，失败时上传可复现 crash artifact。
- 256 组、每组 256 步的活动获取/释放序列保持单所有者不变式。
- 提权协议拒绝未知动作、错误版本、零/非法 PID、非法 nonce 和额外参数。
- 固化哈希测试以真实临时文件证明：内容被篡改后，helper/安装器共用的校验函数失败关闭。
- 命令 DTO 测试证明内部 Windows 路径不会返回前端。

## 3. 与原方案的实现差异

独立、最小化的 `mimic-elevated-helper.exe` 已按方案落地，主程序不再包含 elevated 分流或直接执行安装器的路径。当前实现以编译期固化 SHA-256 完成应用 → helper → 安装器的完整性闭环，并在提供证书指纹时由发布脚本调用 `signtool` 后再固化最终文件哈希。

当前工作区没有正式 Authenticode 证书、发布主体及证书保管/轮换流程，因此本地发布产物仍是“哈希固定但未签名”。这不阻塞代码结构完成，但生产签名验收仍保持未完成状态。

## 4. 自动化门禁

| 门禁 | 当前结果 |
| --- | --- |
| `cargo fmt --check` | 通过 |
| `cargo check --all-targets --all-features` | 通过 |
| `cargo clippy --all-targets --all-features -- -D warnings` | 通过，零警告 |
| `cargo test --workspace --all-targets` | 通过，54 passed / 0 failed |
| `npm run build` | 通过，61 modules |
| `scripts/run-fuzz.ps1 -Iterations 500000` | 通过；固定 seed，500,000 cases，零 panic，112,517 cases/s |
| `scripts/build-release.ps1` | 通过；主程序、helper、资源和打包副本哈希闭环通过 |
| `git diff --check` | 通过；仅 CRLF 转换提示 |
| Release helper 失败关闭 | 空参数和未知操作均退出 64，未进入 UAC 或维护动作；helper/打包副本 SHA-256 一致 |
| CodeGraph | 已同步，69 files / 1,108 nodes / 2,958 edges |

## 5. 剩余发布门禁

1. **Authenticode 发布链**：签名应用、helper 和安装器，定义证书保管、轮换、吊销和 CI 签名流程。
2. **Windows 真机矩阵**：验证真实键鼠、Interception、同键启停、透传、UAC 取消/失败、安装/卸载/重启、麦克风占用/拒绝、音频设备缺失和播放延迟。
3. **性能门禁**：测量 Stop P95 ≤ 100ms、最坏 ≤ 250ms；音频首播延迟由用户实测验收。

## 6. 下一步顺序

1. 在有证书的发布环境签名应用、helper 和安装器并验证发布方。
2. 执行 Windows 真机与性能矩阵，记录设备、系统版本、驱动版本和结果。
3. 将通过的签名、真机和性能证据回填本报告后，才能关闭 Phase 6～7。
