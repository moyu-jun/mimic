# 自定义序列 — 需求 & 方案 & 进度

> 本阶段单文档：需求、技术方案、实施进度合并记录。**代码是最终事实来源**，文档与代码不符以代码为准。
> 起始日期：2026-07-25。

## 1. 需求

左侧菜单新增「自定义」页面。用户可创建、保存**多个具名的自定义序列**，每个序列内可自由混排**按键**与**鼠标**动作，按启动热键后按序列内顺序循环执行。

### 1.1 两级页面结构

- **卡片列表页（自定义主页）**：进入「自定义」菜单后的初始页面。展示已保存序列的卡片列表，每张卡片含序列名称。
  - **第一张卡片固定为 `+`**，点击创建新序列并进入详情页。
  - 在此页面**热键启动无效**。
- **序列详情页（子页面）**：编辑单个序列的动作列表；**可在此页通过热键直接启动**该序列的模拟。

### 1.2 序列与动作

- 一个序列 = `{ id, name, actions[] }`，`actions` 为**有序数组**，执行顺序 = 数组顺序（核心价值：跨类型混排 + 可定序）。
- 每个动作行独立带：启用开关 `enabled`、时间间隔 `intervalMs`、各自类型的动作参数。
- 详情页支持**完整行内编辑**（捕获按键 / 拾取坐标 / 选动作类型 / 改间隔），体验对齐现有键盘、鼠标页。
- 支持**上移 / 下移**调整动作顺序（先不做拖拽）。

### 1.3 命名 / 删除（本轮确认）

- 新建序列时**自动生成默认名**（如「未命名序列 N」）。
- 在**详情页顶部就地重命名**。
- **删除入口在详情页内**（列表页只负责展示与新建）。

### 1.4 数据独立性 & 边界

- 数据**完全独立**于现有 keyboard/mouse 页；现有两页保持不变。
- 未勾选行跳过；鼠标行坐标全空跳过；序列无有效动作则忽略本次启动（builder 返回 `None`）。
- `intervalMs` 下限 `MIN_INTERVAL_MS`；动作 id、序列 id 各自去重。

## 2. 技术方案

### 2.1 关键前提

执行层（`ActionSequence` = `Vec<ActionStep>`，`Action = Keyboard | Mouse | Delay`，`Scheduler::execute_loop`）**本就支持异构混合序列**，无需改动执行层。改动集中在：数据建模、builder、状态机/门控、持久化、UI。

### 2.2 数据模型（判别联合 + 具名序列）

Rust（`config.rs`）：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CustomAction {
    Keyboard(KeyboardConfig),   // {"kind":"keyboard", ...KeyboardConfig 字段}
    Mouse(MouseConfig),         // {"kind":"mouse",    ...MouseConfig 字段}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomSequence {
    pub id: String,
    pub name: String,
    pub actions: Vec<CustomAction>,   // 有序，执行顺序 = 数组顺序
}

// AppConfig 新增字段
pub custom_sequences: Vec<CustomSequence>,
```

TypeScript（`types/config.ts`）：

```typescript
export type CustomAction =
  | ({ kind: 'keyboard' } & KeyboardConfig)
  | ({ kind: 'mouse' } & MouseConfig)

export interface CustomSequence {
  id: string
  name: string
  actions: CustomAction[]
}
```

复用 `KeyboardConfig` / `MouseConfig` 的理由：已含 custom 所需全部字段（id/enabled/intervalMs/actionType + 配套），可直接复用 builder 的 `actionType → Action` 转换逻辑，零重复。

### 2.3 热键门控 —「激活序列」机制（本轮核心）

「列表页热键无效、详情页有效」通过一个「当前激活序列」概念实现：

- `AppState` 新增 `active_custom_sequence_id: Option<String>`。
- **列表页**：`current_page = "custom"`，**不进入 `ReadyCustom`**（`set_current_page` 中 `"custom"` 归入 `Idle` 分支）→ 热键天然无效。
- **详情页**：新增命令 `enter_custom_sequence(id)`，置 `active_custom_sequence_id = Some(id)` + `runtime_status = ReadyCustom`；返回列表时前端调 `set_current_page('custom')` 复位为 `Idle` 并清空 active id。
- `CustomSequenceBuilder::build`：按 `active_custom_sequence_id` 找到对应序列 → 遍历其 `actions`；找不到 / 序列空 → `None`。

### 2.4 Builder（`runner/builder.rs`）

抽出两个自由函数供三 builder 共用：

```rust
fn keyboard_config_to_action(cfg: &KeyboardConfig) -> Action;
fn mouse_config_to_action(cfg: &MouseConfig) -> Option<Action>;  // 坐标全空 → None
```

`KeyboardSequenceBuilder` / `MouseSequenceBuilder` 改为调用它们；新增 `CustomSequenceBuilder`（按 active id 取序列，遍历 actions 分派 + 过滤），`running_status()` 返回 `RunningCustom`。

### 2.5 状态机 & 热键门控清单

| 位置 | 改动 |
|---|---|
| `AppPage` (TS) | 加 `'custom'` |
| `RuntimeStatus` (TS + Rust state) | 加 `ReadyCustom` / `RunningCustom` |
| `AppState` (Rust) | 加 `active_custom_sequence_id: Option<String>` |
| 新命令 | `enter_custom_sequence(id)`；删除/重命名/新建序列的命令（或统一走 persist） |
| `set_current_page` | `"custom"` → `Idle`（列表页不 Ready），并清空 active id |
| `hotkey.rs` 页面白名单 | 加 `"custom"` |
| `hotkey.rs` `handle_start_hotkey` | `custom` → `CustomSequenceBuilder` |
| `hotkey.rs` START/STOP 分支 | 覆盖 `ReadyCustom` / `RunningCustom` |
| `App.vue` isLocked 判定 | 加 `'RunningCustom'` |

### 2.6 持久化

`mimic.ini` 新增 `[custom]` section，`sequences = <JSON>`（`Vec<CustomSequence>` 序列化），方式同 keyboard/mouse。`sanitize_config`：每个序列 actions 做 interval clamp + 动作 id 去重；序列 id 去重。

### 2.7 UI

- `lib/pages.ts`：`MAIN_PAGES` 加 `'custom'`，`PAGE_LABELS.custom = '自定义'`；`MenuIcon` 加 custom 图标。
- `appStore`：加 `customSequences: [] as CustomSequence[]`，及本地 UI 态 `customView: 'list' | 'detail'`、`activeSequenceId: string | null`（详情页路由用，不动全局 `AppPage`）。`App.vue` load_config 注入 `customSequences`。
- `CustomPage.vue`（容器）：按 `customView` 渲染两个子视图：
  - **CustomListView**：首张固定 `+` 卡片 → 新建序列（自动默认名）+ 切 detail + `enter_custom_sequence`；其余卡片显示 `name`，点击进详情。
  - **CustomDetailView**：顶部序列名可就地重命名 + 删除按钮；动作列表行内编辑（keyboard 行复用 `KeyCaptureInput`，mouse 行复用坐标拾取）；「+ 添加按键 / + 添加鼠标」；上移/下移；返回列表。`onMounted` 调 `enter_custom_sequence(id)`，离开调 `set_current_page('custom')`。

## 3. 验证方式

- 后端：`cargo fmt`（无 diff）、`cargo clippy -- -D warnings`、`cargo check`。builder 纯逻辑（按 active id 取序列 + 分派/过滤）补单元测试。
- 前端：`npm run build`（含 vue-tsc）。
- 运行时（热键门控：列表页无效/详情页有效、混排执行顺序、坐标拾取、持久化往返）需**实机验收**。

## 4. 进度

| # | 任务 | 状态 | 备注 |
|---|---|---|---|
| 1 | 需求&方案文档 | ✅ 完成 | 本文档（已按多序列卡片方案重写） |
| 2 | Rust 数据模型（CustomAction + CustomSequence + AppConfig 字段 + 默认配置） | ✅ 完成 | config.rs；内部标签 `kind`，`custom_sequences` 带 `#[serde(default)]` 向后兼容 |
| 3 | AppState.active_custom_sequence_id + enter_custom_sequence 命令 | ✅ 完成 | state.rs + runtime_cmd.rs；lib.rs 注册命令 |
| 4 | builder 抽函数 + CustomSequenceBuilder（按 active id） + 单测 | ✅ 完成 | 抽 keyboard/mouse_config_to_action 三 builder 共用；改为按 sequence_id 字段取序列（便于单测）；3 个新单测 |
| 5 | 状态机 & 热键门控（RuntimeStatus / set_current_page / hotkey.rs） | ✅ 完成 | 新增 ReadyCustom/RunningCustom；hotkey 分派改按 RuntimeStatus；stop/pick/finish_pick 全部覆盖 |
| 6 | 持久化（[custom] section + sanitize） | ✅ 完成 | save/load_from_ini + sanitize（clamp + 序列内动作 id 去重 + 序列 id 去重） |
| 7 | 前端类型 + appStore + pages/MenuIcon | ✅ 完成 | types/config.ts、appStore（customView/activeSequenceId）、App.vue 注入、pages.ts、MenuIcon、AppStatusBar |
| 8 | CustomPage 容器 + CustomListView（卡片列表 + 新建） | ✅ 完成 | 固定 + 卡片，自动默认名，进详情调 enter_custom_sequence |
| 9 | CustomDetailView（行内编辑 + 排序 + 重命名 + 删除） | ✅ 完成 | 键盘/鼠标行按 kind 条件渲染，上移/下移，就地重命名，详情页内删除 |
| 10 | 静态检查 | ✅ 完成 | cargo fmt/clippy(-D warnings)/test(10 passed) + npm run build 均通过 |
| 11 | 实机验收 | ⏳ 待办 | 需装 Interception 驱动 + GUI 会话，见 §5 清单 |

## 5. 实机验收清单（待人工执行）

静态检查已全绿，但涉及驱动/热键/窗口的运行时行为无法自动化，需实机逐项确认：

- [ ] **列表页热键无效**：进入「自定义」菜单（卡片列表），按启动热键 → 无任何模拟发生。
- [ ] **新建 + 默认名**：点 `+` 卡片 → 生成「未命名序列 N」并进入详情页。
- [ ] **详情页重命名**：改名并失焦/回车 → 持久化；返回列表卡片显示新名。
- [ ] **混排执行 + 顺序**：加「按键 A → 鼠标点击 → 按键 B」，详情页按启动热键 → 按数组顺序循环执行；上移/下移后顺序随之改变。
- [ ] **坐标拾取**：鼠标行「坐标拾取」→ 窗口隐藏 → 点击目标位 → 窗口恢复、坐标回填、状态回 ReadyCustom（而非 ReadyMouse）。
- [ ] **停止后可再启动**：运行中按停止热键 → 回 ReadyCustom（仍在详情页），可再次启动。
- [ ] **未勾选 / 空坐标跳过**：取消勾选某行或鼠标行坐标为空 → 该行被跳过；整个序列无有效动作则启动被静默忽略。
- [ ] **删除序列**：详情页「删除序列」→ 回列表且卡片消失，mimic.ini 同步。
- [ ] **持久化往返**：重启应用 → 序列、动作、名称、顺序均保留（含旧版无 [custom] 段的 ini 能正常加载）。
- [ ] **回归**：现有按键页 / 鼠标页功能不受影响。
