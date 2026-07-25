# 前后端接口重构方案

## 问题分析

### 1. 命名冲突（严重）

**配置层 vs 模拟层命名冲突**：

```
config.rs::KeyboardAction  (配置数据：id/selected/keyLabel/scanCode/intervalMs)
  ↕️ 名称冲突
simulation/keyboard/action.rs::KeyAction  (模拟动作：Press/Down/Up/Hold/Combo)

config.rs::MouseAction  (配置数据：id/x/y/intervalMs)
  ↕️ 名称冲突  
simulation/mouse/action.rs::MouseAction  (模拟动作：Click/Drag/Scroll/MoveTo)
```

**当前状态**：
- 配置层的 `KeyboardAction` / `MouseAction` 是**用户配置数据结构**（持久化到 INI）
- 模拟层的 `KeyAction` / `MouseAction` 是**业务动作枚举**（运行时转换为事件）
- 两者职责完全不同，但前者与后者领域概念相近，容易混淆

### 2. 数据模型问题

#### 2.1 鼠标动作缺少选中状态

**现状**：
- `KeyboardAction` 有 `selected: bool` 字段
- `MouseAction` **缺少** `selected` 字段

**问题**：
- 前端 MousePage.vue 依赖列表顺序执行所有行（无勾选机制）
- 与 KeyboardPage 的交互模式不一致
- 用户无法部分启用鼠标动作

#### 2.2 鼠标动作缺少动作类型

**现状**：
- `MouseAction` 只包含 `x/y` 坐标，隐式表示"点击左键"
- 无法表示右键点击、滚轮、拖拽等操作

**与架构不一致**：
- `simulation/mouse/action.rs::MouseAction` 支持 `Click/Drag/Scroll` 等多种类型
- 但配置层无法持久化这些类型

#### 2.3 缺少动作类型字段

**现状**：
- `KeyboardAction` 只包含单个 `scanCode`，隐式表示"Press"动作
- 无法表示 Hold（长按）、Combo（组合键）等高级操作

**未来扩展受限**：
- `simulation/keyboard/action.rs::KeyAction` 已支持 `Press/Hold/Combo`
- 但配置层无法持久化这些动作

### 3. 接口设计问题

#### 3.1 save_config 调用频繁

**现状**：
- 前端每次修改（增/删/勾选/数字输入）都调用 `save_config`
- 每次都序列化整个 `AppConfig` + 写 INI 文件

**性能问题**：
- 快速连续操作时可能触发多次磁盘 IO
- 虽然有原子写（.tmp + rename），但仍有优化空间

#### 3.2 热键更新接口复杂

**现状**：
- `update_hotkeys` 返回 `HotkeyUpdateResult`（changed/registered/persisted/message）
- 前端需要解析多个布尔值判断结果

**可简化**：
- 合并到统一的配置更新接口
- 错误时直接返回 `Err`，成功时返回 `Ok(())`

## 重构方案

### 方案 A：最小改动（推荐）

**目标**：修复命名冲突 + 完善数据模型，不改变持久化格式

#### A1. 重命名配置层类型

```rust
// src-tauri/src/config.rs

// 旧名称
pub struct KeyboardAction { ... }  
pub struct MouseAction { ... }

// 新名称（清晰表示是配置层）
pub struct KeyboardStep { ... }   // 或 KeyboardActionConfig
pub struct MouseStep { ... }      // 或 MouseActionConfig
```

**优势**：
- 消除与 simulation 层的命名冲突
- `Step` 语义更准确（一步动作 = 动作 + 间隔）
- 前端类型同步改名即可

#### A2. 扩展鼠标配置模型

```rust
pub struct MouseStep {
    pub id: String,
    pub selected: bool,          // ✅ 新增：与键盘一致
    pub action_type: String,     // ✅ 新增："click_left" / "click_right" / "scroll_up" / "scroll_down"
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub interval_ms: u64,
}
```

**INI 兼容性**：
- 旧配置加载时自动补全：`selected: true, action_type: "click_left"`
- 新配置写入时包含完整字段

#### A3. 扩展键盘配置模型（可选）

```rust
pub struct KeyboardStep {
    pub id: String,
    pub selected: bool,
    pub action_type: String,     // ✅ 新增："press" / "hold" / "combo"（未来扩展）
    pub key_label: String,
    pub scan_code: u16,
    pub hold_duration_ms: Option<u64>,  // ✅ 新增：Hold 动作时长
    pub modifiers: Vec<u16>,     // ✅ 新增：Combo 修饰键（预留）
    pub interval_ms: u64,
}
```

**当前阶段**：
- 暂时保持 `action_type: "press"`（默认）
- 为未来 Hold/Combo 功能预留字段

#### A4. 简化接口

**合并命令**：
```rust
// 旧接口（保留）
fn save_config(config: AppConfig, ...) -> Result<(), String>
fn update_hotkeys(...) -> Result<HotkeyUpdateResult, String>

// 新接口（推荐使用）
fn update_config(config: AppConfig, ...) -> Result<(), String>
// 内部自动：
//   1. 验证配置（sanitize_config）
//   2. 持久化（save）
//   3. 更新内存（state.config）
//   4. 如果热键变化 → 重新注册
```

**优势**：
- 前端只需调用一个命令
- 后端统一处理持久化 + 热键注册
- 返回值简化（成功 Ok / 失败 Err）

### 方案 B：激进重构（不推荐）

**改动**：
- 持久化改为 JSON / TOML
- 拆分 `keyboard_actions` / `mouse_actions` 为独立配置文件
- 增加增量更新接口（patch_keyboard_action / delete_mouse_action）

**缺点**：
- 用户要求保持 INI 格式
- 改动过大，风险高
- 增量接口反而增加复杂度（需要事务语义）

---

## 推荐方案：方案 A

### 重构步骤

#### 阶段 1：重命名配置类型（后端）

**文件**：`src-tauri/src/config.rs`

```rust
// 重命名
pub struct KeyboardAction -> pub struct KeyboardStep
pub struct MouseAction -> pub struct MouseStep

// 更新 AppConfig
pub struct AppConfig {
    pub keyboard_steps: Vec<KeyboardStep>,  // 旧: keyboard_actions
    pub mouse_steps: Vec<MouseStep>,        // 旧: mouse_actions
    pub hotkeys: HotkeyConfig,
}
```

**影响范围**：
- `src/lib.rs`：命令参数类型
- `src/hotkeys_interception.rs`：构建序列时的类型
- 前端所有 TS 类型定义

#### 阶段 2：扩展鼠标配置模型

**文件**：`src-tauri/src/config.rs`

```rust
pub struct MouseStep {
    pub id: String,
    pub selected: bool,          // 新增
    pub action_type: String,     // 新增："click_left" / "click_right" / ...
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub interval_ms: u64,
}
```

**迁移逻辑**：
```rust
// load_from_ini 增加向后兼容
if let Some(actions) = mouse_section.get("actions") {
    let mut steps: Vec<MouseStep> = serde_json::from_str(actions)?;
    // 旧版本没有 selected/action_type，自动补全
    for step in &mut steps {
        if step.selected.is_none() {
            step.selected = Some(true);
        }
        if step.action_type.is_empty() {
            step.action_type = "click_left".to_string();
        }
    }
}
```

#### 阶段 3：扩展键盘配置模型（预留）

保持当前结构，增加预留字段：
```rust
pub struct KeyboardStep {
    // 现有字段...
    #[serde(default)]
    pub action_type: String,  // 默认 "press"
    #[serde(default)]
    pub hold_duration_ms: Option<u64>,
    #[serde(default)]
    pub modifiers: Vec<u16>,
}
```

#### 阶段 4：更新前端类型

**文件**：`src/types/config.ts`

```typescript
// 重命名
export interface KeyboardAction -> export interface KeyboardStep
export interface MouseAction -> export interface MouseStep

// 扩展 MouseStep
export interface MouseStep {
  id: string
  selected: boolean        // 新增
  actionType: string       // 新增
  x: number | null
  y: number | null
  intervalMs: number
}

// 更新 AppConfig
export interface AppConfig {
  keyboardSteps: KeyboardStep[]   // 旧: keyboardActions
  mouseSteps: MouseStep[]         // 旧: mouseActions
  hotkeys: HotkeyConfig
}
```

#### 阶段 5：更新前端 Store

**文件**：`src/stores/appStore.ts`

```typescript
export const appStore = reactive({
  keyboardSteps: [...] as KeyboardStep[],  // 旧: keyboardActions
  mouseSteps: [...] as MouseStep[],        // 旧: mouseActions
  hotkeys: { ... } as HotkeyConfig,
})
```

#### 阶段 6：更新前端页面

**文件**：`src/pages/KeyboardPage.vue` / `MousePage.vue`

- 更新类型引用
- MousePage 增加勾选框（与 KeyboardPage 一致）
- 更新 `invoke` 调用参数名

#### 阶段 7：简化接口（可选）

增加统一更新命令：
```rust
#[tauri::command]
fn update_config(config: AppConfig, state: ...) -> Result<(), String> {
    // 1. 运行态守卫
    // 2. 持久化
    // 3. 更新内存
    // 4. 如果热键变化 → 重新注册
    Ok(())
}
```

前端简化为：
```typescript
await invoke('update_config', { config: appStore.config })
```

---

## 验证检查清单

### 后端验证
- [ ] `cargo fmt` 无 diff
- [ ] `cargo clippy -- -D warnings` 通过
- [ ] `cargo check` / `cargo build` 通过
- [ ] 旧 INI 文件能正常加载（向后兼容）
- [ ] 新 INI 文件格式正确（含新字段）

### 前端验证
- [ ] `npm run build` 通过（含类型检查）
- [ ] 所有页面能正常渲染
- [ ] 配置能正常保存和加载
- [ ] 鼠标页面支持勾选框

### 实机验收
- [ ] 热键能正常触发
- [ ] 按键模拟功能正常
- [ ] 鼠标模拟功能正常
- [ ] 配置持久化正常

---

## 风险与缓解

### 风险 1：前端类型大面积改动

**缓解**：
- 先改后端，保证编译通过
- 再改前端，TypeScript 会指出所有需要修改的位置
- 分阶段提交（先重命名，再扩展字段）

### 风险 2：INI 兼容性

**缓解**：
- 加载时自动补全缺失字段
- 保留旧字段读取逻辑（作为 fallback）
- 单元测试验证迁移逻辑

### 风险 3：运行时类型转换

**缓解**：
- `hotkeys_interception.rs` 中配置 → 模拟层的转换需要同步更新
- 增加 `impl From<KeyboardStep> for Action` 辅助转换
- 单元测试覆盖转换逻辑

---

## 预估工作量

| 阶段 | 工作量 | 风险 |
|------|--------|------|
| 1. 重命名配置类型 | 2-3 小时 | 低（编译器会指出所有位置） |
| 2. 扩展鼠标模型 | 1-2 小时 | 中（需要迁移逻辑） |
| 3. 扩展键盘模型 | 30 分钟 | 低（仅预留字段） |
| 4-6. 前端同步 | 2-3 小时 | 中（需要测试所有页面） |
| 7. 简化接口 | 1 小时 | 低（可选项） |
| **总计** | **7-10 小时** | **中等** |

---

## 决策点

### 需要用户确认：

1. **类型重命名**：`KeyboardAction` → `KeyboardStep` 是否可接受？
   - 备选：`KeyboardActionConfig` / `KeyboardItem`

2. **字段命名**：`action_type` 是否使用字符串？
   - 备选：枚举（但 JSON 序列化更复杂）

3. **阶段性实施**：
   - 是否先完成重命名（阶段 1）验证，再继续扩展模型？
   - 还是一次性完成所有改动？

4. **接口简化**（阶段 7）：
   - 是否需要？还是保持当前 `save_config` 接口？

---

**计划版本**：v1.0  
**关联文档**：ARCHITECTURE_REVIEW.md  
**最后更新**：2026-07-25
