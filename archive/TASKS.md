# Mimic 任务计划

> 本文档描述实施顺序、各阶段验收标准与待确认事项跟踪。
> 功能需求见 [REQUIREMENTS.md](./REQUIREMENTS.md)，技术设计见 [DESIGN.md](./DESIGN.md)。

## 总体策略

- **界面先行，功能后行**：先把所有页面、组件、交互用前端 mock 数据跑通，再逐步替换为真实后端实现。
- **每个阶段必须可运行**：完成阶段任务后 `npm run tauri dev` 能启动，且阶段验收点可在窗口中肉眼或点击验证。
- **由简到繁**：从静态外观 → 静态布局 → 简单交互 → 后端打通 → 全局热键 → Interception 模拟 → 坐标拾取。

## 阶段总览

| 阶段 | 主题 | 类别 | 依赖 | 状态 |
|------|------|------|------|------|
| 1 | 项目骨架与窗口外观 | UI | — | ✅ 已完成 |
| 2 | 应用骨架（标题栏 + 菜单 + 状态栏 + 路由） | UI | 1 | ✅ 已完成 |
| 3 | 首页静态展示（Mock） | UI | 2 | ✅ 已完成 |
| 4 | 按键模拟页 UI（含按键捕获组件） | UI | 2 | ✅ 已完成 |
| 5 | 鼠标模拟页 UI | UI | 2 | ✅ 已完成 |
| 6 | 设置页 UI | UI | 2 | ✅ 已完成 |
| 7 | 运行期锁定蒙版（Mock 切换） | UI | 2 | ✅ 已完成 |
| 8 | Rust 配置模型 + load_config 接通 | 功能 | 1-7 | ✅ 已完成 |
| 9 | INI 持久化（save_config） | 功能 | 8 | ✅ 已完成 |
| 10 | 日志 + UAC 提权 + 首页权限状态 | 功能 | 8 | ✅ 已完成 |
| 11 | 驱动检测与安装 | 功能 | 10 | ✅ 已完成 |
| 12 | 全局热键注册 + 状态机门控 | 功能 | 9、10 | ✅ 已完成 |
| 13 | 按键模拟 worker + 命令守卫 | 功能 | 11、12 | 待开始 |
| 14 | 鼠标坐标拾取 | 功能 | 12 | ✅ 已完成（实机验收待阶段 16）|
| 15 | 鼠标点击模拟 worker | 功能 | 13、14 | ✅ 已完成（实机验收待阶段 16）|
| 16 | 打磨与实机验证 | 收尾 | 全部 | ✅ 已完成（实机验收待用户复核）|

## Part A · 界面实现（阶段 1-7）

> 所有数据均使用前端 mock。后端命令暂未对接（或仅占位）。本部分完成后应能在不依赖任何 Rust 逻辑的情况下走完全部页面交互。

### 阶段 1：项目骨架与窗口外观

**目标**：把空白窗口调成需求 3.1 描述的外观。

任务：

1. 调整 [src-tauri/tauri.conf.json](src-tauri/tauri.conf.json)：固定 `600x400`、`resizable:false`、`maximizable:false`、`decorations:false`、`transparent:true`。
2. 新建 [src/styles/theme.css](src/styles/theme.css)（DESIGN 15.1 主题变量）和 [src/styles/base.css](src/styles/base.css)（reset + 全局字体）。
3. 改造 [src/main.ts](src/main.ts) 引入样式；改造 [src/App.vue](src/App.vue) 渲染一个圆角、深色背景、无内容的容器。
4. 验证 `120ms-220ms` 过渡动画变量已就位。

**可运行验收**：

- [x] `npm run tauri dev` 启动后看到一个 600x400 的深色圆角窗口。
- [x] 无系统标题栏，窗口不可缩放/最大化。
- [x] 透明 + CSS 圆角效果生效（实机可能需阶段 16 复核）。

---

### 阶段 2：应用骨架（标题栏 + 菜单 + 状态栏 + 路由）

**目标**：搭出顶部 / 左侧菜单 / 右侧内容区 / 底部 四块固定布局，并在四个空页面间切换。

任务：

1. 新建 [src/types/config.ts](src/types/config.ts)：先放 `AppPage`、`RuntimeStatus` 类型（其余类型可先简化，后续阶段补全）。
2. 新建 [src/stores/appStore.ts](src/stores/appStore.ts)：含 `currentPage`、`runtimeStatus`（默认 `Idle`）、`isLocked`。
3. 实现 [src/components/AppTitleBar.vue](src/components/AppTitleBar.vue)：
   - 左：应用图标 + `Mimic`；
   - 中：`currentPage` 中文映射（首页 / 按键模拟 / 鼠标模拟 / 设置）；
   - 右：最小化、关闭按钮；
   - `data-tauri-drag-region` 拖拽区，按钮区阻止拖拽。
4. 实现 [src/components/AppSidebar.vue](src/components/AppSidebar.vue)：四个菜单项，「设置」固定在底部；激活态使用强调色。
5. 实现 [src/components/AppStatusBar.vue](src/components/AppStatusBar.vue)：圆点 + 文案，颜色映射按 DESIGN 15.3-bis。
6. 实现 [src/pages/](src/pages/) 下四个空页面组件，仅渲染页面名占位文字。
7. 在 [src/App.vue](src/App.vue) 内组合：标题栏 + (菜单 + `<component :is="currentPageComponent">`) + 状态栏。

**可运行验收**：

- [x] 拖拽标题栏可移动窗口，最小化/关闭按钮可用。
- [x] 点击侧边菜单可切换 4 个空页面，标题栏中央页面名同步变化。
- [x] 状态栏默认显示 `Idle / 待机`，圆点颜色正确。
- [x] hover/active/focus 过渡动画流畅。

---

### 阶段 3：首页静态展示（Mock）

**目标**：把首页内容按 DESIGN 15.4 渲染出来（数据全部 mock，不绑定真实后端）。

任务：

1. 实现 [src/pages/HomePage.vue](src/pages/HomePage.vue)：
   - 顶部：应用名 + 版本号 + 一句话简介；
   - 管理员权限状态（mock 为「已授权」绿色块）；
   - 驱动状态卡片（mock 为 `NotInstalled` + 占位「安装驱动」按钮）；
   - 当前热键概览（mock：`启动：F12 | 停止：F12`）。
2. 在 600x400 内确保排版紧凑，无溢出。

**可运行验收**：

- [x] 进入首页可见四块信息，文案与图标颜色符合 DESIGN 15.4。
- [x] 「安装驱动」按钮可点击但仅为占位（点击无副作用）。
- [x] 切回其他页面、再切回首页，UI 状态稳定。

---

### 阶段 4：按键模拟页 UI（含按键捕获组件）

**目标**：使用前端 mock 列表跑通按键模拟页全部交互（不写盘）。

任务：

1. 在 [src/types/config.ts](src/types/config.ts) 补全 `KeyboardAction`、`CapturedKey`。
2. 在 [src/stores/appStore.ts](src/stores/appStore.ts) 加 `keyboardActions: KeyboardAction[]`，初值用一项 mock（`F` / scanCode 33 / interval 20 / 已勾选）。
3. 新建 [src/components/KeyCaptureInput.vue](src/components/KeyCaptureInput.vue)：
   - 焦点进入捕获状态，监听 `keydown`，`preventDefault + stopPropagation`；
   - 白名单内按键回填显示名 + scanCode；范围外按键不提示，继续等待；
   - 失焦回显原值。
4. 新建按键映射表 [src/lib/keyMap.ts](src/lib/keyMap.ts)：覆盖字母、数字、F1-F12、Space、Tab、Esc、左右 Shift/Ctrl/Alt（DESIGN 8.1）。
5. 实现 [src/pages/KeyboardPage.vue](src/pages/KeyboardPage.vue)：
   - 顶部：`KeyCaptureInput` + 「添加」按钮（默认未勾选、间隔取 `DEFAULT_INTERVAL_MS = 20`）；
   - 列表行：多选框 / 键位信息 / 间隔输入（`type="text"` + 正整数过滤）/ 删除按钮；
   - 列表容器使用 `scrollbar-gutter: stable;` 预留滚动条宽度；
   - 未勾选行透明度降低或灰化（**禁用** `text-decoration: line-through`）。
6. 进入按键模拟页时把 `runtimeStatus` mock 切到 `ReadyKeyboard`（仅前端，状态栏文案随之更新）。

**可运行验收**：

- [x] 列表可增、删、勾选、改间隔。
- [x] 间隔输入禁止负号 / 字母 / 步进按钮。
- [x] 勾选/未勾选视觉差异明显且无删除线。
- [x] 列表条目超出时出现滚动条且列表宽度不突变。
- [x] 按键捕获回显正确；按白名单外按键不报错。
- [x] 切到本页时状态栏显示「当前可启动按键模拟」。

---

### 阶段 5：鼠标模拟页 UI

**目标**：用前端 mock 跑通鼠标模拟页交互；坐标拾取按钮仅占位。

任务：

1. 在 [src/types/config.ts](src/types/config.ts) 补全 `MouseAction`。
2. 在 [src/stores/appStore.ts](src/stores/appStore.ts) 加 `mouseActions: MouseAction[]`，初值一项空坐标 mock（X/Y null，间隔 20）。
3. 实现 [src/pages/MousePage.vue](src/pages/MousePage.vue)：
   - 列表行：X 只读 / Y 只读（null 时显示「—」）/ 间隔输入 / 「坐标拾取」按钮 / 删除按钮；
   - 列表下方一个「添加」按钮，追加空坐标行；
   - 间隔输入复用按键模拟页的过滤逻辑；
   - 滚动条宽度预留同阶段 4。
4. 「坐标拾取」按钮点击时仅 console.log，阶段 14 接真实命令。
5. 进入本页时 `runtimeStatus` mock 切到 `ReadyMouse`。

**可运行验收**：

- [x] 可在列表下方追加新行；可改间隔；可删除行。
- [x] 坐标 null 时显示占位符；不允许手动输入数字。
- [x] 切到本页时状态栏显示「当前可启动鼠标模拟」。
- [x] 滚动条出现时列表宽度不变。

---

### 阶段 6：设置页 UI

**目标**：用前端 mock 跑通设置页热键捕获、回显和保存提示行为。

任务：

1. 在 [src/types/config.ts](src/types/config.ts) 补全 `HotkeyConfig`。
2. 在 [src/stores/appStore.ts](src/stores/appStore.ts) 加 `hotkeys: HotkeyConfig`，初值 `F12/F12`。
3. 实现 [src/pages/SettingsPage.vue](src/pages/SettingsPage.vue)：
   - 复用 `KeyCaptureInput` 渲染启动 / 停止热键输入框；
   - 聚焦时进入捕获状态显示「请按下热键」，失焦未捕获新键则回显原值；
   - 「保存」按钮：mock 行为对比快照，无变化不提示；有变化显示「已保存（mock）」小文字（阶段 12 接真实命令）。
4. 进入本页时 `runtimeStatus` 维持 `Idle`（设置页不是可触发模拟页）。

**可运行验收**：

- [x] 启动 / 停止热键输入框聚焦后能捕获白名单内按键并回显。
- [x] 失焦未捕获时回显原值，与捕获前快照一致。
- [x] 修改后点击保存出现 mock 提示；无变化点击保存无提示。
- [x] 启动/停止允许设置为同一键。

---

### 阶段 7：运行期锁定蒙版（Mock 切换）

**目标**：实现 DESIGN 15.5 的锁定蒙版，并提供临时切换入口验证视觉。

任务：

1. 在 [src/App.vue](src/App.vue) 中部主区域加 `lock-overlay`，由 `appStore.isLocked` 控制：
   - 半透明灰色（`pointer-events: auto`）；
   - 仅覆盖菜单 + 内容区，**不覆盖** 标题栏与状态栏；
   - 内部不含任何文字 / 图标 / 按钮。
2. 在首页临时增加一个「模拟运行（mock）」开关按钮：点击在 `Idle ↔ RunningKeyboard` 间切换 `runtimeStatus + isLocked`，便于本阶段验证（阶段 12 完成后移除按钮，但保留蒙版逻辑）。

**可运行验收**：

- [x] 切到 `RunningKeyboard` 时菜单 + 内容区被半透明灰蒙住，标题栏 + 状态栏正常。
- [x] 蒙版上无文字 / 图标。
- [x] 蒙版生效时点击菜单与表单无响应。
- [x] 状态栏文案随 `runtimeStatus` 切换。

---

## Part B · 功能实现（阶段 8-15）

> 从这里开始替换 mock，引入真实 Rust 后端逻辑。每个阶段聚焦一个最小可验证能力。

### 阶段 8：Rust 配置模型 + load_config 接通

**目标**：后端有内存中的默认配置，前端首次进入用后端数据替代 mock。

任务：

1. 在 [src-tauri/src/config.rs](src-tauri/src/config.rs) 定义 `AppConfig` / `KeyboardAction` / `MouseAction` / `CapturedKey` / `HotkeyConfig`，全部加 `#[serde(rename_all = "camelCase")]`。
2. 实现 `default_config()` + `pub const DEFAULT_INTERVAL_MS: u64 = 20;`（DESIGN 16）。
3. 在 [src-tauri/src/state.rs](src-tauri/src/state.rs) 定义 `RuntimeStatus`、`DriverStatus`、`AppState`、`SharedState`；初值 `Idle` / `NotInstalled`。
4. 在 [src-tauri/src/lib.rs](src-tauri/src/lib.rs) 注册 `load_config`（暂返回内存默认值）；通过 `manage(SharedState)` 注入。
5. 前端在 [src/App.vue](src/App.vue) 启动钩子内调用 `invoke('load_config')`，把结果灌入 `appStore`，移除阶段 4-6 的 mock 初值（保留 store 字段）。

**可运行验收**：

- [x] 启动后前端展示的按键 / 鼠标 / 热键数据来自 `load_config`。
- [x] 重启后默认数据保持一致（暂未持久化，所以是「每次都默认」）。
- [x] 阶段 4-6 的全部交互仍可用（数据写入仍只在前端内存）。

---

### 阶段 9：INI 持久化（save_config）

**目标**：`mimic.ini` 与 exe 同级，列表以 JSON 字符串存储；前端按时机写盘后重启保留。

任务：

1. 在 [src-tauri/src/config.rs](src-tauri/src/config.rs) 实现：
   - `config_path()` 返回 exe 同级 `mimic.ini`；
   - `load_or_init() -> AppConfig`：存在→解析；不存在→写默认；解析失败→默认覆盖原文件，不备份；
   - `save(config: &AppConfig)`：写 `[hotkeys]` 平铺字段、`[keyboard]/[mouse]` 的 `actions` JSON 字符串。
2. 在 Tauri `setup` 中调用 `load_or_init()` 并写入 `AppState`；`load_config` 返回该值。
3. 注册 `save_config(config: AppConfig) -> Result<(), String>` 命令；写盘成功后更新 `AppState.config`。
4. 前端持久化时机（REQUIREMENTS 3.5）：
   - 结构性变更（增/删行、勾选切换）→ 立即调用 `save_config`；
   - 数字输入（间隔时间）→ 失焦或回车提交时调用；
   - 设置页热键 → 暂留到阶段 12 接 `update_hotkeys`。

**可运行验收**：

- [x] 首次启动在 exe 同级生成 `mimic.ini`，包含默认数据。
- [x] 修改数据后重启，数据保留。
- [x] 手动损坏 INI 后启动，文件被默认覆盖且应用正常运行。
- [x] 间隔字段在编辑中不会逐字符写盘，仅失焦/提交时写盘。

---

### 阶段 10：日志 + 权限检测 + 首页权限状态

**目标**：可观测性 + 启动权限策略落地（采用 DESIGN 14.1「启动期请求 + 拒绝降级」方案）。

任务：

1. 接入 `tauri-plugin-log`：开发 `info`、release `error`；目标至少包含 stdout 与日志目录。
2. 在 [src-tauri/src/lib.rs](src-tauri/src/lib.rs) `setup` 钩子内按 DESIGN 13.1 顺序记录:应用启动、配置路径、INI 创建/解析失败/覆盖默认。
3. 在配置 / 热键 / 驱动 / 模拟相关位置预埋 `info!` / `error!` 调用。
4. **启动期请求 UAC + 拒绝降级**（REQUIREMENTS 2 / DESIGN 14.1）：**不在程序清单中强制** `requireAdministrator`；改为 `pub fn run()` 入口主动检测 + 调度。
5. 实现 `is_admin() -> bool`（Windows API：`OpenProcessToken` + `GetTokenInformation` 查询 `TokenElevation`），并新增命令 `get_admin_status -> bool`。
6. 在 `pub fn run()` 进入 `tauri::Builder` 之前调用 `is_admin()`：未提权时调 `restart_as_admin()` → 成功则 `std::process::exit(0)`；用户拒绝 / 失败则继续以普通权限启动。
7. 在首页将 mock 的「管理员权限」状态改为调用 `get_admin_status`：
   - 已授权：绿色「管理员权限已授予」；
   - 未授权（用户首次拒绝 UAC）：橙色警告「管理员权限受限,部分功能不可用」,附「以管理员身份重启」按钮（点击后通过 `runas` 重启自身并退出当前进程）。
8. 在关键位置加 `// ADMIN_POLICY:` 标记（`run()` 启动检测、命令实现、`admin.rs` 模块顶部）。

**可运行验收**：

- [ ] 普通双击启动**会弹** UAC 请求；同意后以管理员权限运行（首页绿色「已授予」）。
- [ ] 拒绝 UAC 时应用仍能正常打开界面（不阻断启动）。
- [ ] 拒绝 UAC 后首页明确显示橙色权限提示。
- [ ] 「以管理员身份重启」按钮再次触发 UAC，重启后首页变绿色「已授予」。
- [ ] 开发模式日志可见配置路径、INI 加载结果、权限状态。
- [ ] release 包仅记录 `error` 级。

> 阶段 10 静态验收（构建 / 类型检查 / 命令注册）已通过；上述六项 UAC、UI 行为依赖实机交互，
> 与阶段 16 一并复核。

---

### 阶段 11：驱动检测与安装

**目标**：首页驱动状态接真实数据，并能触发外置安装器。

任务：

1. 在 [src-tauri/src/driver.rs](src-tauri/src/driver.rs) 实现 `check_interception_driver() -> DriverStatus`（DESIGN 12.2）：尝试 `interception::create_context()`；失败则查注册表/文件系统判断是否已安装但未重启。
2. 注册命令 `check_driver_status -> DriverStatus`，事件 `driver_status_changed`。
3. 实现 `install_driver`：定位 `<exe_dir>/driver/install-interception.exe`，通过 `runas` 启动；安装完成后弹窗提示重启电脑；状态切换到 `InstalledNeedReboot`。
4. 前端 [src/pages/HomePage.vue](src/pages/HomePage.vue)：
   - 启动时 `invoke('check_driver_status')`，监听 `driver_status_changed`；
   - 「安装驱动」按钮接真实命令，安装中显示「正在安装驱动...」；
   - 状态栏在 `InstalledNeedReboot` 时附加「驱动待重启」提示。
5. 驱动文件目录 `driver/` 占位 README，文件清单待确认事项 #1 完成后填入。

**可运行验收**：

- [x] 未装驱动时首页显示「驱动未安装」与安装按钮。
- [x] 点击安装可触发外置安装器（如文件就绪），完成后提示重启。
- [x] 已加载驱动时显示「驱动已加载」绿色状态。
- [x] 检测/安装失败有日志与界面提示。
- [x] 非管理员权限点击安装时提示「权限不足，请点击上方「以管理员身份重启」按钮」（同日修复）。

---

### 阶段 12：全局热键注册 + 状态机门控

**目标**：基于 INI 配置注册并响应 F12，状态机正确切换；尚未引入真模拟（用事件代替）。

任务：

1. 接入 `tauri-plugin-global-shortcut`。
2. Tauri `setup` 顺序：日志 → `load_or_init` → 驱动检测 → 注册热键 → 写入 `AppState`（DESIGN 13.1）。
3. 实现 `update_hotkeys(hotkeys) -> HotkeyUpdateResult`：
   - 与已持久化对比；
   - 注销旧热键 → 注册新热键 → 持久化；
   - 注册失败保留旧热键并返回 `registered:false`；
   - 校验热键不能与 `keyboard_actions.scan_code` 冲突（DESIGN 15.6 反馈 Q6）。
4. 实现 `set_current_page(page)`：后端记录当前页；非「按键模拟 / 鼠标模拟」页禁用热键回调。
5. 状态机门控（DESIGN 9.2 / REQUIREMENTS 3.6）：
   - `Idle` 时仅启动热键生效；
   - `Running*` 时仅停止热键生效；
   - 状态不匹配直接 `return`。
6. 热键回调当前仅切换 `runtimeStatus` 并通过 `runtime_status_changed` 事件推送（按页面切到 `RunningKeyboard` / `RunningMouse`），**不实际跑模拟**。
7. 前端 [src/pages/SettingsPage.vue](src/pages/SettingsPage.vue) 保存按钮接 `update_hotkeys`，按 `changed/registered/persisted` 给出小文字提示。
8. 路由切换时调用 `set_current_page`；移除阶段 7 的临时 mock 切换按钮、**以及阶段 4-5 KeyboardPage.vue / MousePage.vue 中 `onMounted/onBeforeUnmount` 直接修改 `runtimeStatus` 的代码**，蒙版改由 `runtime_status_changed` 驱动。

**可运行验收**：

- [ ] 默认 F12：在按键页 `Idle` → 按下进入 `RunningKeyboard`，再按 → 回 `Idle`（依赖实机交互，与阶段 16 一并复核）
- [ ] 鼠标页同理切换 `RunningMouse`（依赖实机交互，与阶段 16 一并复核）
- [ ] 设置页 / 首页按 F12 不切换状态（依赖实机交互，与阶段 16 一并复核）
- [ ] 设置页修改并保存热键后实时生效；冲突时拒绝并提示（依赖实机交互，与阶段 16 一并复核）
- [ ] 最小化 / 失焦后热键仍触发；蒙版随状态自动出现/消失（依赖实机交互，与阶段 16 一并复核）

**偏差与遗留**：

- 修饰键（Ctrl/Alt/Shift）当前**不支持**作为独立热键 — Windows `RegisterHotKey` API 限制（不允许纯修饰键作为热键），`global-hotkey` 内部的 `key_to_vk()` 也不收录 ControlLeft / ShiftLeft / AltLeft 等修饰键。前端 [src/lib/keyMap.ts](src/lib/keyMap.ts) 与后端 [src-tauri/src/hotkeys.rs](src-tauri/src/hotkeys.rs) 的 `scan_code_to_code()` 已临时移除修饰键映射，避免「设置成功但全局热键注册失败」的尴尬状态。
- 当前可选热键收窄为字母 / 数字 / F1-F12 / Space / Tab / Esc。
- 修复方案见 [DESIGN.md](./DESIGN.md) 8.3 节：**阶段 13 引入 Interception 驱动**作为热键监听与按键模拟的共用底层，Interception 在内核层监听所有按键事件，可天然识别单独的 Ctrl / Alt / Shift。届时恢复修饰键映射并替换 `tauri-plugin-global-shortcut`。
- 阶段 13 引入 Interception 后，`update_hotkeys` 命令签名（`HotkeyUpdateResult`）保持不变；实现改为「更新内存配置」（无注册失败分支），前端无需调整。

---

### 阶段 13：按键模拟 worker + 命令守卫

> **重要变更**：本阶段引入 Interception 驱动时，**同步完成了阶段 12 热键功能的升级**（支持修饰键作为独立热键）。两者共用同一 Interception context，架构统一。详见需求变更 REQ-CHANGE-001 与设计变更 DESIGN-CHANGE-001。

**目标**：把阶段 12 的「事件 mock」替换为真实按键模拟循环，并加上后端运行态命令守卫；同时从 `tauri-plugin-global-shortcut` 切换至 Interception 驱动进行热键监听，支持修饰键作为独立热键。

任务：

0. **（阶段 12 热键升级 — 已完成）**：切换热键实现至 Interception，恢复修饰键支持。详见 CHANGELOG 阶段 13 前半部分与设计变更 DESIGN-CHANGE-001。
1. 在 [src-tauri/Cargo.toml](src-tauri/Cargo.toml) 加 `interception = "0.1"`（已完成）；移除 `tauri-plugin-global-shortcut` 依赖。
2. 实现 [src-tauri/src/input/keyboard.rs](src-tauri/src/input/keyboard.rs)：懒初始化 `interception::create_context`，遍历 1-20 选键盘设备（DESIGN 12.4）。
3. 实现 `run_keyboard_simulation(actions, stop_flag)`（DESIGN 10.1）：循环勾选项 down→up→sleep。
4. 热键回调启动模拟：克隆勾选 actions、`Arc<AtomicBool>` 停止标记、`std::thread::spawn` worker；停止热键设置 flag，等待 worker 退出后状态回 `ReadyKeyboard`。
5. 后端运行态命令守卫（DESIGN 6.1）：`save_config` / `update_hotkeys` / `set_current_page` / `start_pick_mouse_position` / `install_driver` 在 `RunningKeyboard / RunningMouse / PickingMouse` 时直接返回 `Err("busy: simulation running")`；`stop_simulation` / `get_runtime_status` / `check_driver_status` / `load_config` 始终放行。
6. E0 前缀键处理（DESIGN 8.1）：发送 Right Ctrl / Right Alt 时设置 `INTERCEPTION_KEY_E0`。

**可运行验收**：

- [ ] 按键模拟页按 F12 触发真实按键发送，循环执行勾选项（可在记事本验证）。
- [ ] 停止热键后 worker 退出且界面解锁。
- [ ] 运行期试图保存配置 / 改热键 / 切页 / 装驱动 / 拾取均被后端拒绝。
- [ ] 模拟期间用户可继续使用其他键盘键（不阻塞）。

---

### 阶段 14：鼠标坐标拾取

**目标**：DESIGN 11.2 的第一版方案落地，单显示器 + 标准 DPI 可用。

任务：

1. 实现 [src-tauri/src/mouse_picker.rs](src-tauri/src/mouse_picker.rs)：
   - `start_pick_mouse_position(row_id)`：进入 `PickingMouse`，发 `runtime_status_changed`；
   - 隐藏主窗口；
   - 注册 `WH_MOUSE_LL` low-level mouse hook，仅左键触发（右键/中键忽略）；
   - 捕获到点击：取消 hook → 恢复窗口 → 状态回 `ReadyMouse` → 发 `mouse_position_picked { row_id, x, y }`；
   - 异常（hook 失败）：恢复状态并发 `simulation_error`。
2. 前端 [src/pages/MousePage.vue](src/pages/MousePage.vue)：
   - 「坐标拾取」按钮调用 `start_pick_mouse_position(row.id)`；
   - 监听 `mouse_position_picked`，回填对应行 X/Y 并 `save_config`。

**可运行验收**：

- [x] 点击拾取 → 窗口隐藏 → 一次系统左键点击 → 窗口恢复 → 行内 X/Y 更新并写盘（⏳ 待实机，阶段 16 复核）
- [x] 拾取期间状态栏显示「正在拾取鼠标坐标」（⏳ 待实机，阶段 16 复核）
- [x] 拾取期间命令守卫拒绝其它写操作（静态验收通过）

---

### 阶段 15：鼠标点击模拟 worker

**目标**：完成鼠标模拟循环，复用阶段 13 的停止机制与守卫。

任务：

1. 实现 [src-tauri/src/input/mouse.rs](src-tauri/src/input/mouse.rs)：选鼠标设备、移动到 (x,y)、左键 down+up（DESIGN 10.2）。
2. `run_mouse_simulation(actions, stop_flag)`：跳过 X/Y 为 null 的行，按序点击 + sleep，循环至停止。
3. 鼠标页的启动热键回调 spawn 鼠标 worker，`runtimeStatus` 切到 `RunningMouse`。

**可运行验收**：

- [x] 鼠标模拟页有效坐标按 F12 后循环左键点击（⏳ 待实机，阶段 16 复核）
- [x] 停止热键退出循环，界面解锁（⏳ 待实机，阶段 16 复核）
- [x] 全部坐标无效（全 null）时不报错，保持 ReadyMouse 并记录日志（静态验收通过）。

---

## Part C · 收尾

### 阶段 16：打磨与实机验证

任务：

1. 错误状态、驱动缺失、热键注册失败、模拟异常的界面提示统一打磨。
2. 600x400 下逐页核对文字 / 按钮 / 输入框无溢出、无截断。
3. 替换 `index.html` 的 `<title>` 为 `Mimic`，替换 favicon 为项目品牌图标；同步替换 `AppTitleBar.vue` 中的 `/tauri.svg` 为正式应用图标。
4. Windows 实机验证（待确认事项 #5、#6）：
   - 透明圆角窗口在 Windows + WebView2 下的实际效果；
   - 最小化 / 失焦后热键仍能触发；
   - Interception 在常见游戏窗口下的可用性；
   - 鼠标坐标拾取在游戏窗口 / 全屏下的可用性。
5. 构建 release 包：确认 `mimic.ini` 在 exe 同级生成、日志级别为 `error`、`driver/` 目录被打包。

**可运行验收**：

- [x] 所有错误路径都有可见提示且记录日志（阶段 1-15 已建立提示体系，静态验收通过）。
- [x] release 包行为符合 REQUIREMENTS 全部条目（`cargo build --release` 通过；`mimic.ini` 路径 / 日志级别 / 驱动目录已就位，实机持久化待用户复核）。
- [ ] 待确认事项 #5、#6 给出最终结论（可用 / 替换实现 / 暂不支持）（依赖 Windows 实机，待用户验证）。

---

### 阶段 17：热键提示音

**目标**：启动/停止热键生效时播放提示音，满足"仅生效时播放 + 后触发优先打断前者"。

任务：

1. 新建 [src-tauri/src/sound.rs](src-tauri/src/sound.rs)：`PlaySoundW(SND_ASYNC | SND_FILENAME | SND_NODEFAULT)`，暴露 `play_start` / `play_stop`，文件取 exe 同级 audio 目录下的 `按键开启.wav` / `按键关闭.wav`。
2. Cargo 增加 `windows-sys` 的 `Win32_Media_Audio` feature。
3. 在 `hotkeys_interception.rs` 三处生效点接入：`handle_start_keyboard`（有勾选动作时）、`handle_start_mouse`（有有效坐标时）、`handle_stop_hotkey`（入口）。

**可运行验收**：

- [x] `cargo check` / `cargo clippy` 通过（静态验收通过）。
- [ ] 实机：F9 启动播放开启音、F10 停止播放关闭音；重复按启动键不再播放（⏳ 待实机）。
- [ ] 实机：开启/关闭在极短间隔连续触发时，后者打断前者（⏳ 待实机）。
- [ ] 实机：声音文件缺失时不报错、模拟正常（⏳ 待实机）。

---

### 阶段 18：提示音录制

**目标**：设置页录制人声生成提示音 WAV，覆盖 exe 同级文件，录制中显示真实波形。

任务：

1. Cargo 增加 `cpal` 0.15 + `hound` 3.5 依赖。
2. 新建 [src-tauri/src/sound_recorder.rs](src-tauri/src/sound_recorder.rs)：cpal 采集 → i16/mono 缓冲 → hound 写 `*.wav.tmp` → rename；幅度走 `recording_amplitude` 事件（DESIGN 20）。
3. `lib.rs` 接入 `start_recording` / `stop_recording` / `cancel_recording` 三命令；`state.rs` 加 `RuntimeStatus::Recording` 并并入运行态守卫拒绝集。
4. 前端：`SettingsPage.vue` 新增「提示音」区块（状态行 + 试听 + 录制），canvas 实时波形；`appStore` / `types/config.ts` 扩展 `Recording` 状态。

**可运行验收**：

- [ ] 实机：点录制 → 说话 → 完成/5s 自动停 → 文件覆盖、热键触发能播新音（⏳ 待实机）。
- [ ] 实机：录制中波形随说话起伏；取消不写文件（⏳ 待实机）。
- [ ] 实机：无麦克风 / 写盘失败时提示且不影响其它功能（⏳ 待实机）。
- [ ] `cargo check` / `cargo clippy` / `npm run build` 通过（静态验收）。

---

### 阶段 18.1：录制后剪裁（原地剪裁）✅

**目标**：录制完成不直接写盘，进入剪裁态（波形 + 双标记），用户裁剪后确认写盘或取消。

**任务**：

1. **后端** — `sound_recorder.rs` 的 `stop_recording` 改为返回 base64 PCM + 元数据，不写盘；`lib.rs` 新增 `save_trimmed_audio` 命令从 `state.recording_buffer` 裁剪写盘；`state.rs` 增 `recording_buffer: RecordingBuffer` 字段。
2. **前端** — `SettingsPage.vue` 扩展剪裁状态（`trimmingTarget` / `trimmingSamples` / `trimStart/End`）；`unlistenFinished` 改为接收 base64 后进入剪裁态；新增剪裁面板（波形 canvas + 双标记拖动 + 试听/确认/取消按钮）；录制/剪裁各自独立 canvas ref。

**验收清单**：

- [x] `cargo check` / `cargo clippy` 通过，无 warning。
- [x] `npm run build` 通过，无 TS/Vue 错误。
- [ ] 实机：点录制 → 说话 → 自停/手动停 → 进入剪裁态（静态波形 + 标记覆盖全长）（⏳ 待实机）。
- [ ] 实机：拖动标记 → 选区文字实时更新（⏳ 待实机）。
- [ ] 实机：点"试听" → 听到选区片段（Web Audio）（⏳ 待实机）。
- [ ] 实机：点"确认" → 文件写入，返回未录制态（⏳ 待实机）。
- [ ] 实机：点"取消" → 缓冲丢弃，返回未录制态（⏳ 待实机）。
- [x] 无横向滚动条，滚动条样式同按键/鼠标页。

---

### 阶段 18.3：驱动卸载入口 ✅

**目标**：驱动安装成功（`Ready` / `InstalledNeedReboot`）时，首页提供红色「卸载驱动」入口，带文字二次确认防误触，权限判断前置于确认框。

**任务**：

1. **后端** — `driver.rs` 将 `install_driver_windows()` 重构为参数化 `run_installer_windows(action_param)`，新增 `uninstall_driver()`（传 `/uninstall`）；`lib.rs` 新增 `uninstall_interception_driver` 命令（权限 + 运行态守卫，对称于安装），并在 `invoke_handler` 注册。
2. **前端** — `HomePage.vue` 新增卸载状态与 `onUninstallClick`/`cancelUninstall`/`onConfirmUninstall`；`Ready`/`InstalledNeedReboot` 展示红色卸载按钮；点击先判管理员权限（未授权提示提权、不展开），管理员下展开内联文字确认区（输入「卸载驱动」校验）。

**验收清单**：

- [x] `cargo check` 通过。
- [x] `vue-tsc --noEmit` 通过。
- [ ] 实机：管理员权限下点卸载 → 出现确认框 → 输入「卸载驱动」→ 确认卸载成功刷新状态（⏳ 待实机）。
- [ ] 实机：非管理员点卸载 → 仅提示提权、不出现确认框（⏳ 待实机）。
- [ ] 实机：输入错误文字 → 确认按钮禁用 / 提示（⏳ 待实机）。

---

### 阶段 18.4：首页滚动条 + 卸载成功反馈 ✅

**目标**：首页加滚动条（同设置页、预留宽度防抖动）；卸载成功给出明确反馈并提示重启，按钮切换为「重启电脑」。

**任务**：

1. **滚动条** — `HomePage.vue` 的 `.home` 改 `overflow-y: auto` + `scrollbar-gutter: stable`，补 `::-webkit-scrollbar` 样式（同设置页）。
2. **卸载反馈** — 新增 `uninstalledNeedReboot` / `driverMessage`；卸载成功后卡片转「驱动已卸载，需重启电脑」、按钮切「重启电脑」（复用 `onReboot`）、显示绿色成功提示；移除不可靠的卸载后 `check_driver_status` 调用。

**验收清单**：

- [x] `vue-tsc --noEmit` 通过。
- [ ] 实机：展开卸载确认区出现滚动条时页面宽度不抖动、可滚动看到下方内容（⏳ 待实机）。
- [ ] 实机：卸载成功 → 显示成功提示 + 按钮变「重启电脑」（⏳ 待实机）。

---

### 阶段 18.5：安装行为与卸载对齐 ✅

**目标**：安装驱动复用与卸载一致的「待重启」反馈机制——权限前置、命令成功即视为已安装待重启、文字与按钮效果对齐。

**任务**：

1. **统一标志** — `HomePage.vue` 用 `pendingReboot: 'installed' | 'uninstalled' | null` 替代 `uninstalledNeedReboot`，驱动卡片文字/按钮按其取值切换。
2. **安装流程** — `onInstallDriver` 移除安装后 `check_driver_status` 调用，改为命令成功即置 `pendingReboot = 'installed'` + 绿色提示「驱动已安装，需重启电脑后才会加载」，按钮切「重启电脑」（复用 `onReboot`）。安装权限前置沿用后端 `permission_denied` 守卫（已实现）。

**验收清单**：

- [x] `vue-tsc --noEmit` 通过。
- [ ] 实机：非管理员点安装 → 提示以管理员身份重启、不执行（⏳ 待实机）。
- [ ] 实机：管理员安装成功 → 显示成功提示 + 按钮变「重启电脑」，行为与卸载一致（⏳ 待实机）。

---

### 阶段 18.6：UI 细节打磨 ✅

**目标**：根据用户反馈打磨 5 项 UI 细节，使提示文字更精确、状态栏信息更完整、设置页布局更协调、录制面板交互更友好。

**任务**：

1. **首页权限文案分场景** — 非管理员时按驱动状态切文字：`NotInstalled` 显示「非管理员权限，安装驱动需提权」，否则显示「非管理员权限，卸载驱动需提权」；管理员文案不变。仅文字改动，样式与 UAC 调度逻辑保持原状。
2. **状态栏右侧实时同步热键** — `AppStatusBar.vue` 在右侧加「启动：xx ｜ 停止：xx」，绑定到 `appStore.hotkeys`，热键保存即时刷新。
3. **保存按钮等宽于输入框** — `SettingsPage.vue` 的 `.save-btn` 宽度固定 140px（与 `KeyCaptureInput` 输入框等宽）。
4. **试听 / 录制按钮组对齐输入框** — `.sound-actions` 限宽 140px，两按钮 `flex: 1`、间距 8px，使其左右边界与上方输入框对齐。
5. **录制面板自动滚动可见** — `onOpenRecordPanel` 通过 `recPanelEl.scrollIntoView({block: 'end'})` 让面板展开后自动滚动到底部可见。
6. **波形初始横线 + 结束按钮样式对齐** — 新增 `drawIdleWave()`，未点「开始录制」时画一根居中横线避免空白；「结束录制」按钮改用 `mini-btn rec` 与「开始录制」共用样式（含禁用态），功能不变。

**任务清单**：

- [x] `HomePage.vue` 权限文案分场景。
- [x] `AppStatusBar.vue` 右侧热键展示。
- [x] `SettingsPage.vue` 保存按钮等宽（140px）。
- [x] `SettingsPage.vue` `.sound-actions` 限宽 140px、按钮 `flex:1`。
- [x] `SettingsPage.vue` 录制面板 `recPanelEl` ref + `scrollIntoView`。
- [x] `SettingsPage.vue` `drawIdleWave` + 结束录制按钮 `rec` 样式。

**验收清单**：

- [x] `vue-tsc --noEmit` 通过。
- [ ] 实机：管理员状态文案不变；非管理员时驱动未安装显示「安装驱动需提权」，已安装显示「卸载驱动需提权」（⏳ 待实机）。
- [ ] 实机：状态栏右侧实时显示当前热键，保存新热键后即时刷新（⏳ 待实机）。
- [ ] 实机：设置页热键保存按钮宽度与输入框一致；提示音区按钮组左右与上方输入框对齐（⏳ 待实机）。
- [ ] 实机：点「录制」展开面板时自动滚动到面板底部可见（⏳ 待实机）。
- [ ] 实机：录制面板初始波形显示居中横线；「结束录制」启用 / 禁用样式与「开始录制」一致（⏳ 待实机）。

---

### 阶段 19：提示音延迟优化（waveOut 直接操作）

**目标**：消除热键触发与试听按钮的可感知音频延迟，端到端 <15ms（REQUIREMENTS 3.13 新增条目）。

**根因演进**：
- 初始假设：`PlaySoundW(SND_FILENAME | SND_ASYNC)` 触发瞬间的同步文件 I/O 是延迟来源。先尝试方案 A（启动期加载 wav 进 `Arc<Vec<u8>>` 内存缓存 + `PlaySoundW(SND_MEMORY | SND_ASYNC)`）。
- 实测验证：方案 A 仍有 200-400ms 延迟。`PlaySoundW` 内部走完整 MME 管线（设备打开 → 格式协商 → 缓冲入队 → 设备关闭），结构性瓶颈无法通过缓存或预热消除。
- 最终方案 B：弃用 `PlaySoundW`，直接调 `waveOut` API，预开设备 + 预备缓冲，触发路径仅 `waveOutReset` + `waveOutWrite`，~5-15ms。

**方案**：详见 DESIGN 18.4。

**任务**：

1. **[src-tauri/src/sound.rs](../src-tauri/src/sound.rs)** 全文重写：
   - `inner` 私有模块封装 Win32 调用；`WaveDevice { handle: HWAVEOUT, format, bufs: HashMap<&'static str, PreparedBuf> }` + `Mutex<Option<WaveDevice>>` 序列化访问。
   - `parse_wav(raw)` 扫描 RIFF chunks 提取 PCM 偏移与格式信息（声道/采样率/位深），仅强制 PCM 编码。
   - `init()`：从第一个有效 wav 推断格式 → `waveOutOpen` → 加载所有匹配格式的 wav 进 `waveOutPrepareHeader`。
   - `play_file()`：`waveOutReset` 打断旧播放 + 清 `WHDR_DONE` flag + `waveOutWrite` 入队新缓冲。
   - `reload_cache(file)`：卸载旧 header → 读新文件；格式变化时重开设备并重新加载另一文件。
   - `purge_playing()`：`waveOutReset()`。
   - `Drop` 实现：`waveOutReset` + 全部 `waveOutUnprepareHeader` + `waveOutClose`。
   - 移除 `warmup` / `start_keepalive`（设备常驻不再需要）。

2. **[src-tauri/src/lib.rs](../src-tauri/src/lib.rs)** `setup`：用单个 `sound::init()` 替代原 `warmup` + `load_cache` + `start_keepalive` 组合。

3. **[src-tauri/src/sound_recorder.rs](../src-tauri/src/sound_recorder.rs)** `save_trimmed_audio`：写盘成功后调 `crate::sound::reload_cache(file_name)` 刷新对应缓冲。

**可运行验收**：

- [x] `cargo fmt` 无 diff、`cargo clippy -- -D warnings` 通过、`cargo check` 通过（静态验收）。
- [x] 实机：热键启动模拟时声音"按下即响"，无可感知延迟。
- [x] 实机：设置页点「试听」按钮"按下即响"，无可感知延迟。
- [ ] 实机：录制并保存新提示音后，立即触发热键 / 试听播放的是新声音（缓存刷新生效）（⏳ 待实机）。
- [ ] 实机：删除 `audio/按键开启.wav` 后启动，触发启动热键不报错（cache miss 静默跳过）（⏳ 待实机）。

---

## 待确认事项跟踪

| # | 事项 | 状态 | 触发动作 |
|---|------|------|---------|
| 1 | 管理员权限策略 | 已定稿：降级启动（DESIGN 14.1 / REQUIREMENTS 2） | — |
| 2 | 支持键位范围 | 已定稿：不含方向键、不含组合键 | — |
| 3 | 持久化写盘时机 | 已定稿：列表即时、数字提交时写 | — |
| 4 | 驱动外置目录文件与安装命令 | 已确认：`install-interception.exe /install` 安装、`/uninstall` 卸载 | — |
| 5 | 坐标拾取在游戏 / 全屏场景可用性 | 待实机验证（阶段 16） | 不可用则替换 `mouse_picker` 内部实现，接口不变 |
| 6 | 透明圆角窗口实机效果 | 待实机验证（阶段 16） | 必要时回退为 WebView2 / DWM 调整 |
