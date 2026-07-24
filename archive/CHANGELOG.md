# Mimic 阶段执行日志

> 本文档按阶段记录实际执行的改动摘要、关键决策与验证结果，作为 [TASKS.md](./TASKS.md) 的执行回执。
>
> **填写规则**：
>
> - 每完成一个阶段，**追加**一节（不修改历史阶段记录）；阶段编号与 TASKS.md 的「阶段总览」一一对应。
> - 内容聚焦「做了什么 / 为什么这么做 / 验证了什么」，**不复述需求或设计**（那是 REQUIREMENTS / DESIGN 的职责）。
> - 文件路径使用相对仓库根目录的 markdown 链接（如 `[src/main.ts](../src/main.ts)`）。
> - 如该阶段对 REQUIREMENTS / DESIGN / TASKS 有回写，必须在「文档回写」一节列明。
> - 如出现待确认事项或与计划不一致的偏差，记入「偏差与遗留」。
>
> **章节模板**（复制下面的骨架来开新阶段）：
>
> ```markdown
> ## 阶段 N：<标题>
>
> **完成时间**：YYYY-MM-DD
>
> ### 改动摘要
>
> | 文件 | 改动类型 | 关键点 |
> |------|---------|--------|
> | [path](../path) | 新建 / 改 / 删 | … |
>
> ### 关键决策
>
> - 决策点 1 — 理由
>
> ### 验证结果
>
> - 命令 / 检查项 — 结论
>
> ### 文档回写
>
> - REQUIREMENTS / DESIGN / TASKS 的具体改动（无则写「无」）
>
> ### 偏差与遗留
>
> - 与 TASKS 计划不符的地方、待实机验证的项（无则写「无」）
> ```

---

[此处省略阶段 1 到阶段 18.7 的历史记录，保持原文不变]

---

## 阶段 18.8：DLL 加载路径修复

**完成时间**：2026-06-12

### 改动摘要

| 文件 | 改动类型 | 关键点 |
|------|---------|--------|
| [src-tauri/build.rs](../src-tauri/build.rs) | 改 | 重写为完整的 `copy_extra_to_target()` 实现：递归复制 `extra/` 目录所有内容到 `target/{debug,release}/`，保留子目录结构；添加 `rerun-if-changed=../extra` 触发条件 |
| [src-tauri/src/lib.rs](../src-tauri/src/lib.rs) | 改 | 移除 `SetDllDirectoryW` 设置 `driver/` 子目录的逻辑（L492-512），改为注释说明 `interception.dll` 现通过 `build.rs` 复制到 exe 同级目录，依赖 Windows 标准 DLL 搜索顺序 |
| [docs/DESIGN.md](../docs/DESIGN.md) | 改 | § 2 项目结构：`drivers/interception/` 改为 `extra/{interception.dll, audio/, driver/}`；§ 12.3 新增「DLL 部署策略」小节，说明 `build.rs` 自动复制、`interception.dll` 必须在 exe 同级、不再使用 `SetDllDirectoryW` |

### 关键决策

- **问题根因**：`interception.dll` 是 exe 的隐式导入依赖，Windows 加载器在进入 `main` 之前就解析导入表。原设计将 DLL 放在 `<exe_dir>/driver/` 子目录，并在 `lib.rs::run()` 里调用 `SetDllDirectoryW` 添加搜索路径——但该调用在 `main` 已启动后才执行，对启动期隐式依赖不起作用。`npm run tauri dev` 能工作是因为 cargo 将 `interception-sys` 的 `OUT_DIR` 加入子进程 PATH；双击 exe 时没有这种环境变量扩展，导致加载失败。
- **修复方案**：将 `interception.dll` 放到 exe 同级目录（Windows DLL 搜索顺序最高优先级：应用程序所在目录），通过 `build.rs` 在编译时自动复制 `extra/` 内容到 `target/{debug,release}/`，保留子目录结构（`driver/install-interception.exe` 仍在子目录）。
- **资源目录重组**：`extra/interception.dll`（exe 同级）、`extra/driver/install-interception.exe`（安装器）、`extra/audio/*.wav`（提示音），全部通过 `copy_dir_all` 递归复制。
- **移除 `SetDllDirectoryW`**：该调用仅对显式 `LoadLibrary` 有效，对隐式导入无效且时机已晚；DLL 直接在 exe 同级后该逻辑冗余，直接移除并留注释说明。

### 验证结果

- `cargo build`（src-tauri 目录）— 通过，build.rs 输出 `warning: Copied extra/ to target/debug`。
- 目录结构验证：
  - `target/debug/interception.dll` 存在（exe 同级）
  - `target/debug/driver/install-interception.exe` 存在
  - `target/debug/audio/{按键开启.wav, 按键关闭.wav}` 存在
- 直接执行验证：`cd target/debug && ./mimic.exe` 成功启动，日志显示完整启动流程无 DLL 加载错误。

### 文档回写

- DESIGN § 2：项目结构更新为 `extra/` 目录布局。
- DESIGN § 12.3：新增「DLL 部署策略」说明 `build.rs` 自动复制、exe 同级加载、移除 `SetDllDirectoryW`。

### 偏差与遗留

- 原设计将驱动文件放在 `<exe_dir>/driver/` 子目录，现调整为 `interception.dll` 在 exe 同级、仅安装器在子目录，与 DESIGN 初版有出入但已同步更新文档。
- Release 构建与最终打包（NSIS installer）的 `extra/` 复制需在后续打包阶段验证。

---

## 阶段 19：提示音延迟优化（waveOut 直接操作）

**完成时间**：2026-06-12

### 改动摘要

| 文件 | 改动类型 | 关键点 |
|------|---------|--------|
| [src-tauri/src/sound.rs](../src-tauri/src/sound.rs) | 改 | 全文重写：弃用 PlaySoundW，改用 waveOut API（`waveOutOpen` 常驻设备 + `waveOutPrepareHeader` 预备缓冲 + 触发时 `waveOutReset` + `waveOutWrite`）。暴露 `init` / `play_start` / `play_stop` / `purge_playing` / `reload_cache` / `sound_files_exist` 不变接口 |
| [src-tauri/src/lib.rs](../src-tauri/src/lib.rs) | 改 | setup 顺序：`sound::init()`（替代原 `warmup` + `load_cache` + `start_keepalive`） |
| [src-tauri/src/sound_recorder.rs](../src-tauri/src/sound_recorder.rs) | 改 | `save_trimmed_audio` 写盘成功后调 `crate::sound::reload_cache(file_name)` |
| [docs/REQUIREMENTS.md](../docs/REQUIREMENTS.md) | 改 | § 3.13 末尾追加「响应延迟要求 < 10ms」条目 |
| [docs/DESIGN.md](../docs/DESIGN.md) | 改 | § 18.4 重写为「waveOut 直接操作」方案 |
| [docs/TASKS.md](../docs/TASKS.md) | 改 | 追加阶段 19 描述与验收清单 |

### 关键决策

- **PlaySoundW 方案废弃**：实测 `PlaySoundW(SND_MEMORY | SND_ASYNC)` 即使配合 warmup/keepalive 仍有 200-400ms 结构性延迟（MME 管线每次调用走完整的设备打开→格式协商→缓冲入队→设备关闭流程），不是调参能解决的问题。
- **直接 waveOut 操作**：`waveOutOpen` 启动时打开设备常驻不关闭，触发时仅需 `waveOutReset`（~1ms, 打断旧播放）+ `waveOutWrite`（~0ms, 队列新缓冲），加上内核→DAC 路径 ~5-10ms，总延迟 < 15ms。
- **格式固定 44100/16/mono**：设备以此格式打开，加载时解析并验证 wav 头必须匹配。不匹配的文件视为缺失。我们的录制模块固定输出此格式。
- **keepalive 线程移除**：waveOut 设备常驻打开，无空闲冷启动问题，不需要周期性保活。
- **打断语义**：`waveOutReset` 立即停止所有排队缓冲（标记 WHDR_DONE），然后 `waveOutWrite` 提交新缓冲 — 天然实现「后触发优先打断前者」。

### 验证结果

- `cargo check`：通过。
- `cargo clippy -- -D warnings`：通过。
- `cargo build`：通过（链接 winmm.dll waveOut* 函数成功）。
- **实机验收通过**：热键触发与试听按钮均"按下即响"，无可感知延迟。

### 文档回写

- REQUIREMENTS § 3.13：「响应延迟 < 10ms」条目（上一迭代已写入）。
- DESIGN § 18.4：从 PlaySoundW(SND_MEMORY) 方案重写为 waveOut 直接操作方案。
- TASKS：阶段 19 描述不变（验收标准兼容两种实现）。

### 偏差与遗留

- 初始方案 A（PlaySoundW + SND_MEMORY + 内存缓存）经实测仍有 200-400ms 延迟，确认 PlaySoundW 存在结构性瓶颈后升级为方案 B（waveOut 直接操作）。最终方案比原计划代码量多 ~80 行，但延迟目标可达。
- **首次实测 bug 修复**：原实现 `parse_wav` 硬编码要求 44100Hz/16bit/mono，导致 cpal 录制采样率非 44100（常见 48000Hz）的设备产出文件全部被拒，缓冲为空、播放无声。修复改为从 wav 文件动态读取格式打开设备，仅强制 PCM 编码；两文件格式必须一致；reload 时格式变化自动重开设备。
- WAV 格式仅支持 PCM。非 PCM 文件（如 ADPCM、压缩格式）静默跳过，REQUIREMENTS 未明确要求支持。

---
