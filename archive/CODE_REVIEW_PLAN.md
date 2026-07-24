# Mimic 代码全面审查计划

> 本文档为代码全面 review 的执行计划与进度追踪。
> 审查目标：发现**隐性逻辑错误**、**安全漏洞**、**未知隐患**。
> 审查源：[REQUIREMENTS.md](./REQUIREMENTS.md) + [DESIGN.md](./DESIGN.md) + 当前代码实现。

## 进度概览

| 阶段 | 名称 | 状态 | 完成时间 | 严重问题数 | 备注 |
|------|------|------|----------|------------|------|
| 1 | 状态与数据层基础 | ✅ 已完成（已修复） | 2026-06-14 | C0 / M2→已修 / m7 | 已修：M1, M2, m2, m6；详见 Phase 1 结果 |
| 2 | 权限与驱动管理 | ✅ 已完成（已修复） | 2026-06-14 | C0 / M2→已修 / m8 | 已修：M1, M2, m1, m3, m7；详见 Phase 2 结果 |
| 3 | 热键监听与 Interception listener | ⬜ 未开始 | — | — | — |
| 4 | 模拟 Worker 与坐标拾取 | ⬜ 未开始 | — | — | — |
| 5 | 音频（播放 + 录制） | ⬜ 未开始 | — | — | — |
| 6 | Tauri 命令边界 | ⬜ 未开始 | — | — | — |
| 7 | 前端基础设施 | ⬜ 未开始 | — | — | — |
| 8 | 前端壳层 | ⬜ 未开始 | — | — | — |
| 9 | 页面（Home / Keyboard / Mouse / Settings） | ⬜ 未开始 | — | — | — |
| 10 | 横切关注点 | ⬜ 未开始 | — | — | — |

**状态图例**：⬜ 未开始 ｜ 🟡 进行中 ｜ ✅ 已完成 ｜ ⏸ 暂停 ｜ ❌ 阻塞

## 审查策略

- **顺序**：Rust 后端先（底层 → 上层 → 命令边界）→ 前端（基础 → 壳层 → 页面）→ 横切关注点收尾。
- **每阶段固定 4 个审查维度**：
  1. **正确性**：逻辑错误、状态机不闭环、边界条件遗漏、与 REQUIREMENTS 偏离。
  2. **并发与生命周期**：锁顺序、死锁、`Send/Sync`、`Drop`、线程退出、闭包捕获、`Arc/Mutex` 误用。
  3. **安全 & 健壮性**：权限边界、命令注入、TOCTOU、资源泄漏、panic 路径、`unwrap`/`expect`、外部输入信任。
  4. **与文档一致性**：命名约定（camelCase）、命令集、事件 payload、状态枚举是否与 DESIGN/REQUIREMENTS 完全对齐。
- **每阶段产出**：一份"问题清单（位置 + 严重度 + 推荐修复方向）"，先不动代码。所有阶段审完后再统一决定修复优先级。
- **每阶段产出结构**：
  ```
  ### 严重（Critical）— 明确 bug / 安全漏洞
  ### 重要（Major）— 与文档不符 / 隐性逻辑错误
  ### 次要（Minor）— 代码异味、健壮性改进点
  ### 与 REQUIREMENTS / DESIGN 偏差
  ### 待确认问题（需要用户拍板）
  ```
- **执行约束**：每次只执行一个阶段，验证无误后再开始下一阶段，禁止并行。

---

## 跨阶段记录（Carry-Over）

> 此区块累积"在某阶段发现、需在后续阶段交叉验证"的问题，避免遗忘。每条标注来源阶段与待验证阶段。

### 待 Phase 3 / 4 交叉验证

- **R1（Phase 1 → 3/4）— 嵌套锁顺序潜在死锁**
  - `AppState` 使用单一 `Arc<Mutex<AppState>>`，内部又持有 `Arc<Mutex<Option<SendInterception>>>` × 2（listener + worker）。
  - 若代码路径出现"先 AppState 锁 → 再 listener_ctx 锁"和反向"先 listener_ctx 锁 → 再 AppState 锁"，存在死锁可能。
  - **Phase 3 任务**：审查 `hotkeys_interception.rs` 时画一遍锁序图，确认所有路径都是同一拿锁顺序。
  - **Phase 4 任务**：worker 路径同样核查。

- **R2（Phase 1 → 3）— listener 线程 wait() 阻塞期间是否仍持锁**
  - listener 线程在 `wait()` 阻塞期间若仍持有 listener_ctx 锁，主线程任何尝试拿该锁的命令（如关闭/重建 context）会卡死。
  - **Phase 3 任务**：审查 `hotkeys_interception.rs` 监听循环，确认 `wait()` 前 drop 锁，stroke 处理时再重新拿锁。

### 待 Phase 6 交叉验证

- **R3（Phase 1 → 6）— save_config 命令路径需调用 sanitize_config**
  - Phase 1 已在 `config::save` 内部加了 sanitize 防御，磁盘配置能保证 ≥ 5ms。
  - 但 `lib.rs::save_config` 命令处理时若未对内存 `AppState.config` 做 sanitize，会出现"内存值为 0、磁盘值为 5"不一致 → worker 跑的是内存版，仍可能 sleep(0)。
  - **Phase 6 任务**：审查 `lib.rs::save_config` 时确认入口调用 `config::sanitize_config(&mut config)` 后再赋值给 AppState.config 和写盘。

### 待 Phase 9 交叉验证

- **R4（Phase 1 → 9）— 前端 interval 输入失焦应 clamp 到 MIN_INTERVAL_MS（5ms）**
  - 后端已加兜底，但前端 UI 如果只过滤"非数字字符"未过滤"小于 5"，用户输入 `1` `2` `3` `4` 会被前端发出，再被后端 clamp。这是 OK 的兜底链，但前端 UX 反馈不够明确。
  - **Phase 9 任务**：审查 `KeyboardPage.vue` / `MousePage.vue` / `SettingsPage.vue` 的间隔输入框，失焦时应 clamp 到 5、并给出视觉反馈（避免用户输入 1 后看到值变成 5 一头雾水）。

---

## 全审查后功能优化项（Optimization Backlog）

> 不属于 bug / 隐患修复，但属于"代码质量提升"。所有审查阶段结束后单独处理，不在审查阶段内并入。

- **OPT-1 — `current_page: String` → `AppPage` enum 化**
  - 来源：Phase 1 m4。
  - 现状：`state.rs:70` 用 `String` 持有当前页面，DESIGN 4.1 已定义 `AppPage = 'home' | 'keyboard' | 'mouse' | 'settings'`，但后端无对应 Rust enum。
  - 影响：失去编译期保证，命令边界传错字符串会静默不生效。
  - 修改范围：`state.rs` + `lib.rs::set_current_page` + 所有读取点（`hotkeys_interception` 检查可触发页面）。

- **OPT-2 — `shutdown` 命令用绝对路径**
  - 来源：Phase 2 m4。
  - 现状：`driver.rs:188` 用 `Command::new("shutdown")` 走 PATH 解析。
  - 影响：PATH 污染场景下可能执行到伪造 shutdown.exe。实际威胁低（攻击者已具备任意 RCE 能力），但属于纵深防御缺位。
  - 修改范围：`driver.rs::reboot_system_windows`，用 `GetSystemDirectoryW` 拼绝对路径。

- **OPT-3 — `check_driver_windows` 要求 keyboard + mouse 两个服务都存在**
  - 来源：Phase 2 m5。
  - 现状：任一服务键存在即报告 `InstalledNeedReboot`。
  - 影响：极少数中断安装场景（注册表非原子）会误报"已安装"。
  - 修改范围：`driver.rs::check_driver_windows` 把 for 循环改为"全部存在"判断。

- **OPT-4 — `SHELLEXECUTEINFOW.lpDirectory` 设为 exe_dir**
  - 来源：Phase 2 m6。
  - 现状：lpDirectory 留 null，installer 继承调用进程 CWD。
  - 影响：当前 installer 不读相对路径资源，无功能影响；未来换 installer 可能踩坑。
  - 修改范围：`driver.rs::run_installer_windows` 加一行 `sei.lpDirectory = dir_wide.as_ptr()`。

---

## Phase 1 — 状态与数据层基础

**状态**：✅ 已完成（2026-06-14）

**文件**：
- `src-tauri/src/state.rs` (97 行)
- `src-tauri/src/config.rs` (268 行)

**审查重点**：
- [x] `AppState` 字段是否覆盖所有运行态需要的共享数据；`SharedState` 锁粒度是否过大（运行循环里持锁会拖慢热键响应）。
- [x] INI 加载分支：文件不存在 / 损坏 / 写盘失败 → 是否真的按 REQUIREMENTS 3.5 的"默认覆盖、不创建备份"处理；解析失败是否吞掉具体错误。
- [x] 默认配置常量(`DEFAULT_INTERVAL_MS = 20`、`F = scanCode 33`、`F12 = 88`)是否硬编码且与文档一致。
- [x] `serde(rename_all = "camelCase")` 是否覆盖所有跨边界结构体（DESIGN 4.2 强制约定）。
- [x] 数字字段边界：`interval_ms: u64` 与前端 `number` 的范围是否吻合，零值 / 极大值是否被允许（运行循环里 `sleep(0)` 是 CPU 跑满风险）。
- [x] `RuntimeStatus` / `DriverStatus` 枚举是否含 `Recording`（REQUIREMENTS 3.10 + DESIGN 20.6 要求）。

### 审查结果

#### 严重（Critical）
- 无。

#### 重要（Major）— 与文档不符 / 隐性逻辑错误

- **M1 — `interval_ms` 缺少下限校验（潜在 CPU 烧满）** ✅ **已修复**
  - **位置**：`config.rs:30-41` `KeyboardAction.interval_ms`、`config.rs:46-55` `MouseAction.interval_ms`、`config.rs:171-233` `load_from_ini`。
  - **问题**：`interval_ms: u64` 反序列化时不校验下限。REQUIREMENTS 3.3.2 / 3.3.3 明确"仅接受正整数"——前端 UI 是用过滤实现的，但若用户**手工编辑 mimic.ini** 把 `intervalMs` 改成 `0`，加载后落到 worker 循环里，`std::thread::sleep(Duration::from_millis(0))` 等价于 yield，模拟循环将以系统调度极限频率发送按键/鼠标事件 → 单核 CPU 占满 + 目标程序被 flood + Interception 驱动写满。这条同样适用于 `save_config` 命令路径——若前端漏校验也会同样落地（守卫缺位）。
  - **修复**：新增 `MIN_INTERVAL_MS = 5` 常量与 `sanitize_config(&mut AppConfig)` 函数；`load_or_init` 解析成功后调用一次、`save` 写盘前调用一次。`< 5` 替换为 `5` 并 `log::warn!`。详见 `config.rs`。
  - **遗留**：`lib.rs::save_config` 命令入口尚未对内存 `AppState.config` 做 sanitize，记入 R3（Phase 6 交叉验证）。前端输入框失焦 clamp 记入 R4（Phase 9）。

- **M2 — `save` 非原子写盘可能损坏 INI** ✅ **已修复**
  - **位置**：`config.rs:238-268` `save()` → `ini.write_to_file(&path)`。
  - **问题**：直接覆盖式写入 `mimic.ini`。若进程在写入中途崩溃 / 断电，文件可能呈半写状态。下次启动会被解析失败分支（REQUIREMENTS 3.5）默认覆盖——**用户配置整体丢失**且无备份。同项目 `sound_recorder::save_trimmed_audio` 已采用 `.wav.tmp + fs::rename` 原子写（DESIGN 20.4），此处行为不一致。
  - **修复**：改为 `mimic.ini.tmp` + `std::fs::rename` 原子替换；写 tmp 失败 / rename 失败时清理残留 tmp。Windows `MoveFileExW + MOVEFILE_REPLACE_EXISTING` 同卷原子可覆盖目标。

#### 次要（Minor）— 代码异味、健壮性改进点

- **m1 — `scan_code` 无白名单校验** ❌ 不修（用户决策：手改 INI 是用户责任）
  - 位置：`config.rs:24, 38, 99-107` 与 `load_from_ini`。
  - 手改 INI 写入非法 scan_code 不会报错，但热键比对永不命中、按键模拟会把无效 scan_code 投给驱动（行为未定义）。
- **m2 — 无 ID 唯一性校验** ✅ **已修复**
  - 位置：`config.rs:30-55` 加载分支。
  - 手改 INI 可能出现重复 `id`，前端以 `id` 作 key 时渲染异常。
  - **修复**：`sanitize_config` 内 `dedupe_ids` 检测重复 id，给后续重复行追加 `-dup-N` 后缀直到唯一，并 `log::warn!`。
- **m3 — `interval_ms` 无上限** ❌ 不修（用户决策：无明确危害，仅行为奇怪；与 Phase 4 stop_flag 检查频率耦合）
  - 极大值（如 `u64::MAX`）会让循环卡几个世纪，停止热键虽然能切 `stop_flag`，但 worker 仍卡在 `sleep` 中无法即时响应。
- **m4 — `current_page: String`** ⏭ 列入功能优化项 OPT-1（用户决策：所有审查后单独优化）
  - 位置：`state.rs:70`。
- **m5 — 解析失败但默认覆盖成功不发 warning** ❌ 不修（用户决策）
  - 位置：`config.rs:155-167`。用户的旧配置被默默丢弃，前端无任何提示。日志中可见，前端不通知。
- **m6 — `SendInterception` 的 SAFETY 注释表述不准确** ✅ **已修复**
  - 位置：`state.rs:14-19`。
  - 注释写"该指针仅在创建它的线程内使用是安全的"，与 `Send`（允许跨线程**移动**）语义反向。
  - **修复**：重写注释，说明依据是 Windows 内核 HANDLE 在内核层面线程安全 + 外层 Mutex 串行化访问；并加"维护提示"防止未来误判取消 Mutex。
- **m7 — `AppConfig` 反序列化无 `#[serde(deny_unknown_fields)]`** ❌ 跳过（实际无需修复）
  - **更正前期分析**：`interval_ms: u64` 已是 serde 默认必填字段（无 `#[serde(default)]`）。用户拼错为 `intervalMS` 时，serde 会**报错** `missing field intervalMs` → 整张 actions 数组反序列化失败 → load_from_ini 返回 Err → load_or_init 触发"INI 损坏 → 默认覆盖"分支。
  - 即"必填校验"已经由 serde 默认行为保证，再加显式校验是冗余的。
  - 副作用是用户配置整体重置（无 warning，因 m5 不修）。这是已知现状。

#### 与 REQUIREMENTS / DESIGN 偏差

- **B1 — REQUIREMENTS 3.3.2 / 3.3.3 "仅接受正整数"未在后端兜底**：见 M1（已修）。
- **B2 — REQUIREMENTS 3.5 "写盘失败降级使用内存默认配置"已实现**（`load_or_init_graceful`，state.rs `config_warning` 字段），✓ 合规。
- **B3 — DESIGN 4.2 camelCase 强制约定**：5/5 跨边界结构体均已标注 `#[serde(rename_all = "camelCase")]`，✓ 合规。
- **B4 — REQUIREMENTS 3.10 / DESIGN 20.6 `Recording` 状态**：`RuntimeStatus::Recording` 已存在，✓ 合规。
- **B5 — DESIGN 5 INI 格式**：[hotkeys] 平铺、[keyboard]/[mouse] JSON 数组——一致，✓ 合规。
- **B6 — DESIGN 16 默认配置常量**：`F` scanCode 33、`F12` scanCode 88、`DEFAULT_INTERVAL_MS = 20`——全部一致，✓ 合规。

#### 待确认问题（已全部决议）

- ~~Q1（M1 兜底策略）~~ → 决议：MIN_INTERVAL_MS = 5 强制 clamp，加载/save 双路径都做。已修。
- ~~Q2（M2 是否本轮一并修）~~ → 决议：本轮修。已改为原子写。
- ~~Q3（m1/m2/m3/m7 是否做）~~ → 决议：m2 修，m1/m3/m7 不修（m7 实际无需修）。
- ~~Q4（m4 改 enum）~~ → 决议：列入功能优化项 OPT-1。

#### 锁与并发的潜在隐患（已转存到"跨阶段记录"区块）

- R1（嵌套锁顺序）→ 待 Phase 3/4 验证。
- R2（listener wait 持锁）→ 待 Phase 3 验证。

#### 修复验证

- `cargo check`：通过（无 error）。
- `cargo clippy -- -D warnings`：通过（无 warning）。
- `rustfmt --check src/config.rs src/state.rs`：通过。
- 修改文件：`src-tauri/src/config.rs`、`src-tauri/src/state.rs`。

---

## Phase 2 — 权限与驱动管理

**状态**：✅ 已完成（2026-06-14）

**文件**：
- `src-tauri/src/admin.rs` (106 行)
- `src-tauri/src/driver.rs` (210 行)

**审查重点**：
- [x] `is_admin()` 在 `OpenProcessToken` / `GetTokenInformation` 失败时是否安全降级为"非管理员"（绝不能误判为管理员，否则越过守卫）。
- [x] `restart_as_admin()` / `reboot_system` 的 `ShellExecuteW` 参数转义、宽字符串 NUL 终止、用户取消 UAC 的返回值处理。
- [x] 安装/卸载 `run_installer_windows`：拼接 exe 路径时对路径含空格、Unicode 是否安全；`/install` `/uninstall` 参数传递有无注入空间。
- [x] `check_driver_status` 顺序：先 `Interception::new()` 再查注册表（DESIGN 8.3 修复点）；`Interception::new()` 在驱动卸载未重启时是否可能仍成功 → 这关系到首页"卸载已生效"判断不可信任后端检测。
- [x] 安装器路径校验：`exists()` 检查后到 `ShellExecuteExW` 之间的 TOCTOU；exe_dir 解析失败时的兜底。

### 审查结果

#### 严重（Critical）
- 无。

#### 重要（Major）— 与文档不符 / 隐性逻辑错误

- **M1 — 安装器退出码不校验，install/uninstall 失败被误报为成功** ✅ **已修复**
  - **位置**：`driver.rs:166-176` `run_installer_windows`。
  - **问题**：`WaitForSingleObject` 等到安装器进程退出后，代码仅判断 `WAIT_OBJECT_0`（即"进程已退出"），**未调用 `GetExitCodeProcess` 取退出码**。安装器若因写注册表失败、文件占用等原因退出码 != 0，前端仍收到 `Ok(())` → 继续走"驱动已安装，需重启电脑"分支 → 用户重启后才发现没装上。
  - **修复**：等待返回 `WAIT_OBJECT_0` 后立即调 `GetExitCodeProcess`（必须在 `CloseHandle` 之前），仅 `exit_code == 0` 时返回 Ok，否则返回 Err 含退出码；同时 `WAIT_OBJECT_0` 之外的 wait 状态也直接返回 Err。

- **M2 — REQUIREMENTS 3.12 偏差：管理员权限检测结果未记 info 日志** ✅ **已修复**
  - **位置**：`admin.rs:32-58` `is_admin()`。
  - **问题**：REQUIREMENTS 3.12 明确要求"管理员权限检测结果（elevated / limited）"作为日志覆盖项。当前实现只在 API 调用失败时 `log::warn!`，**正常路径**无任何日志，运维/支持人员排查问题时拿不到 elevated/limited 信息。
  - **修复**：成功路径末尾加 `log::info!("[admin] elevation check: elevated/limited", ...)`。

#### 次要（Minor）— 代码异味、健壮性改进点

- **m1 — `installer_path.to_string_lossy()` 对非 BMP Unicode 路径不健壮** ✅ **已修复**
  - 位置：`driver.rs:135` `let file = encode_wide(&installer_path.to_string_lossy());`。
  - 修复：改用 `OsStrExt::encode_wide()` 直接在 PathBuf 上展开，绕开 String 中转，保留 WTF-16 原始序列。

- **m2 — `WaitForSingleObject(INFINITE)` + `SW_HIDE` 风险** ⚠️ **不修，已落档**
  - 决策：保持现状。当前 installer 是 oblitum/Interception 上游稳定多年的非交互控制台程序，行为可预期。
  - 已在 DESIGN 12.3 加注释明确"SW_HIDE + INFINITE 假设 installer 是非交互控制台程序"，并指出更换 installer 时必须同步改为 SW_SHOWNORMAL 或加有限超时。

- **m3 — `ShellExecuteExW` 失败不取 `GetLastError`** ✅ **已修复**
  - 修复：失败分支调 `GetLastError()`，把错误码（1223 = 用户拒绝 UAC、2 = 文件不存在、5 = 权限拒绝等）加进 err_msg。

- **m4 — `Command::new("shutdown")` 通过 PATH 解析** ⏭ 列入后续优化
  - 决策：威胁低（能改 PATH 的攻击者已具备任意代码执行能力），列入"全审查后功能优化项 OPT-2"。

- **m5 — `check_driver_windows` 不区分 keyboard / mouse 服务都存在** ⏭ 列入后续优化
  - 决策：installer 注册表写入是顺序执行，崩溃中断窗口期极短，列入"全审查后功能优化项 OPT-3"。

- **m6 — `lpDirectory` 未设置** ⏭ 列入后续优化
  - 决策：当前 installer 不依赖相对路径资源加载，列入"全审查后功能优化项 OPT-4"。

- **m7 — `reboot_system` 内部不做 `is_admin()` 防御检查** ✅ **已修复**
  - 修复：函数顶部加 `if !crate::admin::is_admin() { return Err("permission_denied"); }`，防止外层守卫被回归改坏后悄悄退化。注释更新为"防御 + 命令入口已校验"双层。

- **m8 — `CloseHandle` 返回值未检查** ❌ 不修
  - 决策：调用频率极低、累计影响可忽略；CloseHandle 失败本身极罕见，且失败后无恢复手段，仅能 log。

#### 与 REQUIREMENTS / DESIGN 偏差

- **B1 — REQUIREMENTS 3.12 日志覆盖**：M2 已修 ✓。
- **B2 — DESIGN 8.3 修复点 #1（Ready 检测先 `create_context()` 再查注册表）**：✓ 合规。
- **B3 — DESIGN 12.3.1 install/uninstall 对称**：✓ 合规。本轮在 DESIGN 12.3 补充了"调度细节（2026-06-14 加固）"段落记录退出码校验、路径编码、GetLastError、SW_HIDE+INFINITE 约束。
- **B4 — DESIGN 14.1 启动期不主动 UAC**：admin.rs 仅暴露 `is_admin` / `restart_as_admin`，启动期是否触发 UAC 由 `lib.rs` 决定（Phase 6 验证），admin.rs 本身合规。
- **B5 — REQUIREMENTS 3.11 卸载后状态不依赖 `check_driver_status`**：✓ 合规（已知限制，文档化）。

#### 待确认问题（已全部决议）

- ~~Q1（M1 退出码校验）~~ → 决议：本轮修，加 GetExitCodeProcess。已修。
- ~~Q2（M2 admin 日志）~~ → 决议：在 is_admin() 内部加 info 日志（成功路径）。已修。
- ~~Q3（m1 路径编码）~~ → 决议：改用 OsStrExt::encode_wide。已修。
- ~~Q4（m2 INFINITE 超时）~~ → 决议：不修，DESIGN 12.3 加注释说明假设。
- ~~Q5（m3-m8 逐项）~~ → 决议：m3、m7 修；m4、m5、m6 列入后续优化；m8 不修。

#### 修复验证

- `cargo check`：通过（无 error）。
- `cargo clippy -- -D warnings`：通过（无 warning）。
- `rustfmt --check src/admin.rs src/driver.rs`：通过。
- 修改文件：`src-tauri/src/admin.rs`、`src-tauri/src/driver.rs`、`docs/DESIGN.md`（12.3 调度细节注释）。

---

## Phase 3 — 热键监听与 Interception listener

**状态**：⬜ 未开始

**文件**：
- `src-tauri/src/hotkeys.rs` (128 行)
- `src-tauri/src/hotkeys_interception.rs` (538 行)
- `src/lib/keyMap.ts` (85 行)

**说明**：这是最核心、最易出隐患的一块，单独成阶段。

**审查重点**：
- [ ] listener 线程：`set_filter` 是否在 `wait()` 之前；`wait()` 阻塞期间持锁吗？锁释放与 stroke 分发的顺序。
- [ ] 状态机门控：`Idle/Ready*` 只响应启动键、`Running*` 只响应停止键、状态不匹配按键透传 — 是否有任何分支会 consume 不该 consume 的按键（运行期阻塞用户其他按键 = REQUIREMENTS 3.6 重大违规）。
- [ ] `handle_stop_hotkey` 后的 50ms 等待是否在所有路径都生效；toggle 场景（启停同键）路由是否正确。
- [ ] E0 前缀键（Right Ctrl/Alt）：`stroke.state` 与 `scan_code` 的拆分匹配；`keyMap.ts` 的 scanCode 285/312 是否与后端比较逻辑一致。
- [ ] 热键冲突校验：`keyboard_actions` 列表对比是否遍历完整、是否考虑 toggle 场景下启停同键不算冲突。
- [ ] 监听线程崩溃 / `wait` 返回错误时：是否进入 `Error` 状态、是否重试导致死循环。
- [ ] `pick_row_id` 与 `runtime_status == PickingMouse` 的耦合：左键透传判定不能漏。

**审查结果**：（待执行）

---

## Phase 4 — 模拟 Worker 与坐标拾取

**状态**：⬜ 未开始

**文件**：
- `src-tauri/src/keyboard_worker.rs` (140 行)
- `src-tauri/src/mouse_worker.rs` (172 行)
- `src-tauri/src/mouse_picker.rs` (131 行)

**审查重点**：
- [ ] `stop_flag` 检查频率：每个 action 之间检查就够吗？长 interval 内能否及时退出（如间隔 5000ms 会卡 5 秒才停）。
- [ ] worker_ctx 与 listener_ctx 严格分离（DESIGN 11.2 核心经验）— 检查所有 `send/receive` 调用是否在正确 context 上。
- [ ] 键盘 worker：`key_down` + `key_up` 之间是否需要最小延迟；`info` 字段是否清零（DESIGN 8.3 修复点）。
- [ ] 鼠标 worker：屏幕坐标 → Interception 移动量的转换；`absolute` 移动 vs 相对移动；左键 down/up 是否成对。
- [ ] 鼠标坐标拾取：`GetCursorPos` 失败兜底是否真的恢复窗口；`finish_pick` 在异常路径仍会被调用吗。
- [ ] 窗口 `show()/set_focus()` 必须 `run_on_main_thread`（DESIGN 11.2）— 是否在所有恢复路径都做了 marshal。
- [ ] 动作集合空时的快速路径（无勾选 / 无有效坐标）是否真的不进入 `Running*` 状态（REQUIREMENTS 3.9）。

**审查结果**：（待执行）

---

## Phase 5 — 音频（播放 + 录制）

**状态**：⬜ 未开始

**文件**：
- `src-tauri/src/sound.rs` (484 行)
- `src-tauri/src/sound_recorder.rs` (463 行)

**审查重点**：
- [ ] `sound.rs::init` 失败时整个应用是否仍能启动（提示音不应该阻塞主功能）。
- [ ] `WaveDevice` Drop 顺序：`waveOutReset` → `waveOutUnprepareHeader` × N → `waveOutClose`，是否任一失败导致后续泄漏。
- [ ] `WHDR_DONE` / `WHDR_PREPARED` 标志位手动清/置：与 Windows MME 状态机对齐吗（错位会让 `waveOutWrite` 返回 `WAVERR_STILLPLAYING`）。
- [ ] `reload_cache`：写盘成功 → 关旧 → 重读 → 重 prepare，期间触发 `play_*` 是否会 panic 或读到半新半旧状态。
- [ ] 设备格式动态推断：两个 wav 格式不一致时是否真的"静默跳过"而不是覆盖式打开错误格式。
- [ ] 录制：cpal `Stream` 的 `Send` 处理（独立 `Mutex`，不进 `AppState`）；样本格式 f32/i16/u16 → i16 的转换饱和处理（避免削顶失真甚至溢出 panic）。
- [ ] `save_trimmed_audio`：`start_ms` `end_ms` 是否校验（< 100ms 拒绝、超过 buffer 长度拒绝）；`.wav.tmp` + `fs::rename` 在 Windows 下目标已存在的覆盖语义。
- [ ] 写盘前 `purge_playing` 是否真的释放了 waveOut 设备占用，不会导致 `fs::rename` ACCESS_DENIED。
- [ ] base64 PCM 通过事件传给前端：体积（5s × 44100 × 2B = 440KB → base64 ≈ 590KB）IPC 是否能撑住，会否阻塞事件循环。

**审查结果**：（待执行）

---

## Phase 6 — Tauri 命令边界 (lib.rs)

**状态**：⬜ 未开始

**文件**：
- `src-tauri/src/lib.rs` (687 行)
- `src-tauri/src/main.rs` (6 行)

**审查重点**：
- [ ] 命令清单 vs DESIGN 6 — 是否每个命令都有；命令签名（参数、返回值）与前端调用一致。
- [ ] **运行态守卫拒绝集**（REQUIREMENTS 3.10 + DESIGN 6.1）— 必须包含 `save_config / update_hotkeys / set_current_page / start_pick_mouse_position / install_interception_driver / uninstall_interception_driver / start_recording / save_trimmed_audio`。每个命令进入时是否真的读了 `runtime_status`，不能有漏网之鱼。
- [ ] 始终放行集合（`stop_simulation / get_runtime_status / check_driver_status / load_config`）是否真的不被守卫挡住。
- [ ] `setup` 钩子启动顺序（DESIGN 13.1）：日志 → 配置加载 → 驱动检测 → 热键注册 → SharedState 写入。任何一步失败的传播路径。
- [ ] 事件清单（DESIGN 7）：`runtime_status_changed` / `mouse_position_picked` / `config_reloaded` / `hotkey_registration_failed` / `simulation_error` / `driver_status_changed` / `recording_amplitude` / `recording_finished` / `recording_error` — 实际 emit 与文档对齐？
- [ ] 命令错误字符串：是否有结构化（如 `permission_denied` / `busy`）方便前端识别，还是裸字符串。
- [ ] 模拟运行期间 `save_config` 拒绝后前端"静默忽略"（REQUIREMENTS 3.5）— 错误码是否能被前端识别为 busy。

**审查结果**：（待执行）

---

## Phase 7 — 前端基础设施

**状态**：⬜ 未开始

**文件**：
- `src/types/config.ts` (72 行)
- `src/stores/appStore.ts` (59 行)
- `src/lib/configUtil.ts` (33 行)
- `src/lib/keyMap.ts` (85 行)
- `src/lib/pages.ts` (17 行)
- `src/main.ts` (6 行)
- `src/vite-env.d.ts` (7 行)

**审查重点**：
- [ ] `RuntimeStatus` / `DriverStatus` 类型联合是否包含 `Recording`、与后端 enum 同步。
- [ ] `keyMap.ts`：白名单是否覆盖 REQUIREMENTS 3.7 列出的所有键；scanCode 与后端期望一致；E0 编码方式与后端一致。
- [ ] `appStore`：`reactive` 共享状态在多组件读写时是否有竞态；`isLocked` 与 `runtimeStatus` 派生关系。
- [ ] `configUtil`：本地与后端 config 对比、间隔时间 0 / 空字符串处理、ID 生成的唯一性。
- [ ] `pages.ts`：页面 → 中文标签 → 是否触发模拟页 的判断是否一致。

**审查结果**：（待执行）

---

## Phase 8 — 前端壳层

**状态**：⬜ 未开始

**文件**：
- `src/App.vue` (124 行)
- `src/components/AppTitleBar.vue` (129 行)
- `src/components/AppSidebar.vue` (118 行)
- `src/components/AppStatusBar.vue` (81 行)
- `src/components/KeyCaptureInput.vue` (117 行)
- `src/components/MenuIcon.vue` (63 行)

**审查重点**：
- [ ] 标题栏拖拽：`data-tauri-drag-region` 范围；按钮区是否阻止拖拽（REQUIREMENTS 3.1）。
- [ ] 锁定蒙版：是否仅覆盖中部，绝不覆盖标题栏与状态栏；`pointer-events` 是否真的拦截。
- [ ] 菜单切换：运行态禁止切换是否仅靠前端，还是与后端守卫双保险。
- [ ] KeyCaptureInput：`preventDefault + stopPropagation` 是否在所有 keydown 路径生效；白名单外按键"静默忽略，继续等待"；失焦未捕获时回显原值（快照机制）；设置页内捕获已注册热键时也能拦截（REQUIREMENTS 3.3.4）。
- [ ] 状态栏热键摘要：与 `appStore.hotkeys` 同源、热键变更后即时刷新。

**审查结果**：（待执行）

---

## Phase 9 — 页面（Home / Keyboard / Mouse / Settings）

**状态**：⬜ 未开始

**文件**：
- `src/pages/HomePage.vue` (642 行)
- `src/pages/KeyboardPage.vue` (358 行)
- `src/pages/MousePage.vue` (379 行)
- `src/pages/SettingsPage.vue` (1031 行) — 最大文件，含录制 UI 与剪裁逻辑

**审查重点**：

### HomePage
- [ ] 驱动状态卡片四态切换 + `pendingReboot: 'installed' | 'uninstalled' | null` 的状态机。
- [ ] 卸载文字二次确认（"卸载驱动"四字精确匹配）。
- [ ] 管理员权限前置。
- [ ] `overflow-y: auto + scrollbar-gutter: stable`。

### KeyboardPage
- [ ] 捕获 → 添加 → scanCode 重复检测（2s 橙色提示）。
- [ ] 勾选/未勾选视觉差异禁删除线。
- [ ] 间隔输入框只接受正整数、空值兜底、失焦/Enter 提交。

### MousePage
- [ ] X/Y 仅能通过坐标拾取修改；空数据占位符。
- [ ] 间隔输入规则同 KeyboardPage。

### SettingsPage
- [ ] 热键保存的"无变化静默 / 有变化对比已持久化 / 冲突拒绝"三分支。
- [ ] 录制面板：开始 → 录制 → 5s 自动停 → 剪裁双标记（最小 100ms）→ 试听（Web Audio，不走后端）→ 保存（裁剪写盘）→ 取消（丢缓冲）。
- [ ] 录制中切换离开设置页要自动 `cancel_recording`。
- [ ] 试听进度线 vs 标记颜色冲突（绿色进度 vs 橙色标记）。
- [ ] base64 → AudioBuffer 解码错误处理。
- [ ] 与按键模拟列表 scanCode 冲突的提示。

**审查结果**：（待执行）

---

## Phase 10 — 横切关注点

**状态**：⬜ 未开始

**说明**：跨文件的一致性问题，单文件审查时容易漏。最后统一审。

**审查清单**：
- [ ] **守卫覆盖矩阵**：把所有命令 × `RuntimeStatus` 列成表，逐格确认守卫策略，对照 REQUIREMENTS 3.10。
- [ ] **命名约定**：grep `#[serde(rename_all = ` 看是否全部跨边界结构都有；前后端字段命名一致。
- [ ] **错误分类**：`permission_denied` / `busy` / 持久化失败 / 注册失败 / 冲突拒绝 — 错误字符串是否前后端约定一致。
- [ ] **日志覆盖**：REQUIREMENTS 3.12 列出的 6 类事件是否都有 `info!` / `error!`。
- [ ] **路径处理**：所有 `current_exe().parent()` 使用点是否有 None 兜底；扩展名拼接是否考虑 Unicode 中文文件名（`按键开启.wav`）。
- [ ] **panic 审计**：搜索 `unwrap()` / `expect()` / `panic!`，归类是"逻辑保证不可能 panic" 还是"运行时可能触发"。
- [ ] **资源生命周期总览**：listener_ctx / worker_ctx / waveOut device / cpal stream / 监听线程 / worker 线程 → 在 Drop / app_exit 时是否都有归还路径。
- [ ] **REQUIREMENTS 反向追溯**：从需求条目反向找代码实现，避免"代码里有但需求里没要求"和"需求要求但代码漏了"。

**审查结果**：（待执行）

---

## 收尾：修复优先级汇总

**状态**：⬜ 未开始（需所有阶段审完后执行）

- [ ] 汇总所有阶段的 Critical 问题，制定修复顺序。
- [ ] 评估 Major 问题的影响范围与修复成本。
- [ ] Minor 问题归档，列入后续迭代清单。
- [ ] 更新 REQUIREMENTS / DESIGN / CHANGELOG（如审查中发现文档与代码不一致）。
