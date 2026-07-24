# Mimic 模拟模块架构设计文档

> 本文档描述 Mimic 应用的按键/鼠标模拟模块的架构设计方案（方案 B：双线程 + 统一延迟模型）
> 
> **设计日期**: 2026-07-24  
> **架构版本**: v2.0  
> **状态**: 待实施

---

## 目录

1. [需求概述](#需求概述)
2. [设计目标](#设计目标)
3. [架构原则](#架构原则)
4. [整体架构](#整体架构)
5. [核心模块设计](#核心模块设计)
6. [执行流程](#执行流程)
7. [方案选择说明](#方案选择说明)
8. [实施计划](#实施计划)

---

## 需求概述

### 功能需求

#### 键盘模拟
- **按下（Down）**: 按下按键不释放
- **释放（Up）**: 释放按键
- **按下并释放（Press）**: 完整的按键动作（按下 → 短暂延迟 → 释放）
- **长按（Hold）**: 按下 → 保持指定时长 → 释放
- **组合键（Combo）**: 修饰键 + 目标键（预留，低优先级）
- **覆盖范围**: 支持大部分按键，包括鼠标侧键模拟（架构预留）

#### 鼠标模拟
- **移动（MoveTo）**: 移动到绝对屏幕坐标
- **点击（Click）**: 移动 + 按下 + 释放，支持左/中/右键
- **按下（Down）**: 鼠标按键按下不释放
- **释放（Up）**: 鼠标按键释放
- **长按（Hold）**: 按下 → 保持指定时长 → 释放
- **滚轮（Scroll）**: 上滚/下滚指定刻度
- **拖拽（Drag）**: 移动到起点 + 按住 + 移动到终点 + 释放（预留）
- **鼠标侧键**: Button4/Button5（架构预留 Side1/Side2）
- **坐标拾取**: 隐藏窗口 + 全局鼠标左键监听 + 回填坐标（已有功能，保持不变）

#### 混合序列执行
- **严格顺序执行**: 键盘动作和鼠标动作可混合编排在同一序列中
- **独立间隔**: 每个动作执行后可设置独立的等待时间（`interval_ms`）
- **循环执行**: 序列从头到尾执行后立即重新开始，直到用户按停止热键
- **停止响应**: 按下停止热键后，当前正在执行的动作完成，序列立即停止

#### 用户输入隔离
- Interception 驱动已实现模拟输入与真实用户输入的完全隔离
- 模拟运行期间，用户可以正常使用键盘/鼠标操作其他窗口
- 架构层面无需特殊处理

### 非功能需求

1. **时序精确**: 
   - 动作内部延迟（如按下到释放的 10ms）与步骤间隔（如两个动作之间的 100ms）必须严格串行执行
   - 长按动作（如 Hold{5000ms}）不能与后续动作的间隔并行，否则时序错乱

2. **高内聚低耦合**:
   - 键盘和鼠标模块相对独立
   - 驱动通信与业务逻辑解耦
   - 便于测试和替换底层驱动

3. **可扩展性**:
   - 未来可能支持更复杂的动作（如条件判断、变量、子序列）
   - 可能支持多序列并发执行
   - 可能支持宏录制/回放

4. **向后兼容**:
   - 尽量保持现有 worker 架构
   - 减少对现有代码的破坏性修改

---

## 设计目标

1. **统一事件模型**: 键盘和鼠标使用统一的事件类型（`SimulationEvent`），替代当前的 `ActionEvent` 和 `MouseEvent`
2. **驱动抽象**: 通过 `InputDriver` trait 抽象底层驱动，便于测试和替换
3. **职责分离**: 
   - **生产者线程**: 负责序列循环、动作展开、停止检测（纯逻辑，不阻塞）
   - **Worker 线程**: 负责驱动通信、时序控制（IO + sleep，可能阻塞）
4. **时序精确**: 所有延迟（动作内部 + 步骤间隔）都在 worker 单线程串行执行

---

## 架构原则

1. **关注点分离**: 模拟逻辑 ⊥ 驱动通信 ⊥ 状态管理 ⊥ 业务调度
2. **依赖倒置**: 模拟模块不依赖具体驱动实现，通过 trait 抽象
3. **单一职责**: 每个模块只负责一件事，便于测试和替换
4. **独立性**: 键盘和鼠标模块可单独使用，共享基础设施层

---

## 整体架构

### 模块结构

```
src-tauri/src/
├── simulation/              # 新增：模拟模块根目录
│   ├── mod.rs              # 模块入口，导出公共 API
│   ├── event.rs            # 统一事件定义（替代 ActionEvent/MouseEvent）
│   ├── action.rs           # 统一动作定义（支持混合序列）
│   │
│   ├── keyboard/           # 键盘模拟模块
│   │   ├── mod.rs          # 键盘模块入口
│   │   └── action.rs       # 键盘动作类型
│   │
│   ├── mouse/              # 鼠标模拟模块
│   │   ├── mod.rs          # 鼠标模块入口
│   │   ├── action.rs       # 鼠标动作类型
│   │   └── coordinate.rs   # 坐标系转换
│   │
│   ├── driver/             # 驱动抽象层
│   │   ├── mod.rs
│   │   ├── trait.rs        # InputDriver trait（抽象驱动接口）
│   │   ├── interception.rs # Interception 实现
│   │   └── device.rs       # 设备缓存管理
│   │
│   └── executor/           # 执行器（调度 + 状态管理）
│       ├── mod.rs
│       ├── scheduler.rs    # 序列调度器
│       └── context.rs      # 执行上下文（状态门控）
│
├── simulation_worker.rs    # 统一 Worker（事件消费者，驱动通信层）
├── keyboard_worker.rs      # 标记为 deprecated，保留向后兼容
├── mouse_worker.rs         # 标记为 deprecated，保留向后兼容
├── hotkeys_interception.rs # 热键监听 + 调用 executor
└── ...
```

### 线程模型

```
应用启动 (lib.rs::setup)
  │
  ├─ 创建 channel: (event_tx, event_rx) = mpsc::sync_channel(1024)
  ├─ event_tx 存入 AppState
  └─ start_simulation_worker(event_rx, state, interception_context)
        │
        └─ 【Worker 线程】常驻后台
              持有 InterceptionDriver(context)
              阻塞在 rx.recv()，等待事件


用户按启动热键
  │
  └─ 【监听线程】hotkeys_interception.rs
        │
        └─ handle_start_hotkey()
              ├─ 读取配置的动作列表
              ├─ 构建 ActionSequence
              ├─ 状态切换为 Running*
              └─ 启动【生产者线程】
                    │
                    └─ Scheduler::execute_loop()
                          ├─ 循环展开序列
                          ├─ 动作 → 事件流
                          ├─ 步骤间隔 → Delay 事件
                          └─ event_tx.send() ──→ Worker 线程
```

### 数据流

```
配置层 (AppConfig)
  │  KeyboardAction { scanCode, intervalMs, selected }
  │  MouseAction { x, y, intervalMs }
  ↓
业务层 (ActionSequence)
  │  ActionStep { action: KeyAction::Press{...}, interval_ms: 100 }
  │  ActionStep { action: MouseAction::Click{...}, interval_ms: 50 }
  ↓
事件层 (SimulationEvent)
  │  KeyDown{scan_code}
  │  Delay{10}
  │  KeyUp{scan_code}
  │  Delay{100}  ← 步骤间隔也是事件
  │  MouseMove{x, y}
  │  Delay{5}
  │  MouseButtonDown{Left}
  │  ...
  ↓
驱动层 (InputDriver trait)
  │  send_keyboard(scan_code, is_press)
  │  send_mouse_move(x, y)
  │  send_mouse_button(button, is_press)
  ↓
Interception 驱动
  │  interception.send(device, &[stroke])
  ↓
Windows 内核 → 目标应用
```

---

## 核心模块设计

### 1. 统一事件定义 (`simulation/event.rs`)

```rust
/// 统一模拟事件 — 通过 channel 发送给 worker
/// 
/// 这是驱动层的原子事件，worker 接收后直接调用驱动 API。
/// 所有延迟（动作内部 + 步骤间隔）统一用 Delay 事件表示。
#[derive(Debug, Clone)]
pub enum SimulationEvent {
    // === 键盘事件 ===
    
    /// 按下键盘按键
    KeyDown { scan_code: u16 },
    
    /// 释放键盘按键
    KeyUp { scan_code: u16 },
    
    // === 鼠标事件 ===
    
    /// 移动鼠标到绝对屏幕坐标
    MouseMove { x: i32, y: i32 },
    
    /// 按下鼠标按键
    MouseButtonDown { button: MouseButton },
    
    /// 释放鼠标按键
    MouseButtonUp { button: MouseButton },
    
    /// 滚轮滚动（正数向上，负数向下，单位为刻度）
    MouseWheel { delta: i32 },
    
    // === 控制事件 ===
    
    /// 延迟指定毫秒数（在 worker 线程执行 sleep）
    Delay { ms: u64 },
    
    /// 停止信号（保留，当前通过 stop_flag 实现停止）
    Stop,
}

/// 鼠标按钮类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    /// 左键
    Left,
    /// 右键
    Right,
    /// 中键
    Middle,
    /// 侧键 1（通常是前进键，预留）
    Side1,
    /// 侧键 2（通常是后退键，预留）
    Side2,
}
```

**设计要点**:
- **统一性**: 键盘和鼠标事件在同一枚举中，worker 统一处理
- **原子性**: 每个事件对应一次驱动调用，不包含业务逻辑
- **时序控制**: `Delay` 事件让 worker 负责所有延迟，确保串行执行

---

### 2. 动作定义 (`simulation/action.rs`)

#### 2.1 统一动作类型

```rust
use crate::simulation::event::SimulationEvent;
use crate::simulation::keyboard::KeyAction;
use crate::simulation::mouse::MouseAction;

/// 统一动作类型（业务层抽象）
/// 
/// 动作是用户可理解的操作单元（如"按下 W 键"、"点击坐标"），
/// 会被展开为一系列原子事件发送给 worker。
#[derive(Debug, Clone)]
pub enum Action {
    /// 键盘动作
    Keyboard(KeyAction),
    /// 鼠标动作
    Mouse(MouseAction),
    /// 显式延迟（一般不直接使用，由 ActionStep 的 interval_ms 生成）
    Delay(u64),
}

impl Action {
    /// 将动作展开为事件序列
    /// 
    /// 例如: KeyAction::Press{0x11} 
    ///   → [KeyDown{0x11}, Delay{10}, KeyUp{0x11}]
    pub fn to_events(&self) -> Vec<SimulationEvent> {
        match self {
            Action::Keyboard(ka) => ka.to_events(),
            Action::Mouse(ma) => ma.to_events(),
            Action::Delay(ms) => vec![SimulationEvent::Delay { ms: *ms }],
        }
    }
}

/// 动作序列（支持键盘/鼠标混合）
/// 
/// 示例:
/// ```
/// ActionSequence {
///     steps: vec![
///         ActionStep {
///             action: Action::Keyboard(KeyAction::Press { scan_code: 0x11 }),
///             interval_ms: 100,  // W 键按下后等 100ms
///         },
///         ActionStep {
///             action: Action::Mouse(MouseAction::Click {
///                 button: MouseButton::Left, x: 500, y: 300
///             }),
///             interval_ms: 50,   // 点击后等 50ms
///         },
///     ]
/// }
/// ```
#[derive(Debug, Clone)]
pub struct ActionSequence {
    pub steps: Vec<ActionStep>,
}

/// 动作步骤 = 动作 + 执行后的等待时间
#[derive(Debug, Clone)]
pub struct ActionStep {
    /// 要执行的动作
    pub action: Action,
    /// 执行此动作后等待的时间（毫秒）
    /// 会被转换为 Delay 事件追加在动作事件之后
    pub interval_ms: u64,
}

impl ActionSequence {
    pub fn new() -> Self {
        Self { steps: Vec::new() }
    }
    
    /// 添加动作步骤
    pub fn add(mut self, action: Action, interval_ms: u64) -> Self {
        self.steps.push(ActionStep { action, interval_ms });
        self
    }
    
    /// 便捷方法：添加键盘按键
    pub fn add_key_press(self, scan_code: u16, interval_ms: u64) -> Self {
        self.add(
            Action::Keyboard(KeyAction::Press { scan_code }),
            interval_ms
        )
    }
    
    /// 便捷方法：添加鼠标点击
    pub fn add_mouse_click(
        self, 
        button: MouseButton, 
        x: i32, 
        y: i32, 
        interval_ms: u64
    ) -> Self {
        self.add(
            Action::Mouse(MouseAction::Click { button, x, y }),
            interval_ms
        )
    }
}
```

#### 2.2 键盘动作 (`simulation/keyboard/action.rs`)

```rust
use crate::simulation::event::SimulationEvent;

/// 键盘动作类型
#[derive(Debug, Clone)]
pub enum KeyAction {
    /// 单次按键（按下 + 延迟 10ms + 释放）
    /// 
    /// 适用场景: 普通按键输入
    Press { scan_code: u16 },
    
    /// 仅按下（不释放）
    /// 
    /// 适用场景: 需要长期按住某键，由外部逻辑控制释放时机
    Down { scan_code: u16 },
    
    /// 仅释放
    /// 
    /// 适用场景: 与 Down 配合使用
    Up { scan_code: u16 },
    
    /// 长按（按下 → 保持指定时长 → 释放）
    /// 
    /// 适用场景: 游戏中的蓄力、长按技能
    /// 例如: Hold { scan_code: 0x11, duration_ms: 2000 }
    ///   表示按住 W 键 2 秒
    Hold {
        scan_code: u16,
        duration_ms: u64,
    },
    
    /// 组合键（预留，低优先级）
    /// 
    /// 例如: Combo { modifiers: [0x1D, 0x38], key: 0x20 }
    ///   表示 Ctrl+Alt+D
    #[allow(dead_code)]
    Combo {
        modifiers: Vec<u16>,  // Ctrl/Shift/Alt 等修饰键
        key: u16,             // 目标键
    },
}

impl KeyAction {
    /// 转换为事件序列
    pub fn to_events(&self) -> Vec<SimulationEvent> {
        match self {
            KeyAction::Press { scan_code } => vec![
                SimulationEvent::KeyDown { scan_code: *scan_code },
                SimulationEvent::Delay { ms: 10 },
                SimulationEvent::KeyUp { scan_code: *scan_code },
            ],
            
            KeyAction::Down { scan_code } => vec![
                SimulationEvent::KeyDown { scan_code: *scan_code },
            ],
            
            KeyAction::Up { scan_code } => vec![
                SimulationEvent::KeyUp { scan_code: *scan_code },
            ],
            
            KeyAction::Hold { scan_code, duration_ms } => vec![
                SimulationEvent::KeyDown { scan_code: *scan_code },
                SimulationEvent::Delay { ms: *duration_ms },
                SimulationEvent::KeyUp { scan_code: *scan_code },
            ],
            
            KeyAction::Combo { modifiers, key } => {
                let mut events = Vec::new();
                
                // 按下所有修饰键
                for &m in modifiers {
                    events.push(SimulationEvent::KeyDown { scan_code: m });
                    events.push(SimulationEvent::Delay { ms: 5 });
                }
                
                // 按下目标键
                events.push(SimulationEvent::KeyDown { scan_code: *key });
                events.push(SimulationEvent::Delay { ms: 10 });
                events.push(SimulationEvent::KeyUp { scan_code: *key });
                
                // 释放所有修饰键（逆序）
                for &m in modifiers.iter().rev() {
                    events.push(SimulationEvent::Delay { ms: 5 });
                    events.push(SimulationEvent::KeyUp { scan_code: m });
                }
                
                events
            }
        }
    }
}
```

#### 2.3 鼠标动作 (`simulation/mouse/action.rs`)

```rust
use crate::simulation::event::{MouseButton, SimulationEvent};

/// 鼠标动作类型
#[derive(Debug, Clone)]
pub enum MouseAction {
    /// 移动到绝对屏幕坐标
    MoveTo { x: i32, y: i32 },
    
    /// 点击（移动 + 按下 + 释放）
    /// 
    /// 适用场景: 点击界面按钮、点击游戏目标
    Click {
        button: MouseButton,
        x: i32,
        y: i32,
    },
    
    /// 仅按下（不释放）
    Down { button: MouseButton },
    
    /// 仅释放
    Up { button: MouseButton },
    
    /// 长按（按下 → 保持 → 释放）
    Hold {
        button: MouseButton,
        duration_ms: u64,
    },
    
    /// 滚轮滚动
    /// 
    /// delta: 正数向上滚，负数向下滚，单位为刻度
    /// 通常 1 个刻度 = 120 个 Windows 滚轮单位
    Scroll { delta: i32 },
    
    /// 拖拽（预留）
    /// 
    /// 移动到起点 → 按住 → 移动到终点 → 释放
    #[allow(dead_code)]
    Drag {
        button: MouseButton,
        from: (i32, i32),
        to: (i32, i32),
    },
}

impl MouseAction {
    /// 转换为事件序列
    pub fn to_events(&self) -> Vec<SimulationEvent> {
        match self {
            MouseAction::MoveTo { x, y } => vec![
                SimulationEvent::MouseMove { x: *x, y: *y },
            ],
            
            MouseAction::Click { button, x, y } => vec![
                SimulationEvent::MouseMove { x: *x, y: *y },
                SimulationEvent::Delay { ms: 5 },
                SimulationEvent::MouseButtonDown { button: *button },
                SimulationEvent::Delay { ms: 10 },
                SimulationEvent::MouseButtonUp { button: *button },
            ],
            
            MouseAction::Down { button } => vec![
                SimulationEvent::MouseButtonDown { button: *button },
            ],
            
            MouseAction::Up { button } => vec![
                SimulationEvent::MouseButtonUp { button: *button },
            ],
            
            MouseAction::Hold { button, duration_ms } => vec![
                SimulationEvent::MouseButtonDown { button: *button },
                SimulationEvent::Delay { ms: *duration_ms },
                SimulationEvent::MouseButtonUp { button: *button },
            ],
            
            MouseAction::Scroll { delta } => vec![
                SimulationEvent::MouseWheel { delta: *delta },
            ],
            
            MouseAction::Drag { button, from, to } => vec![
                SimulationEvent::MouseMove { x: from.0, y: from.1 },
                SimulationEvent::Delay { ms: 10 },
                SimulationEvent::MouseButtonDown { button: *button },
                SimulationEvent::Delay { ms: 20 },
                SimulationEvent::MouseMove { x: to.0, y: to.1 },
                SimulationEvent::Delay { ms: 10 },
                SimulationEvent::MouseButtonUp { button: *button },
            ],
        }
    }
}
```

---

### 3. 驱动抽象层 (`simulation/driver/`)

#### 3.1 Driver Trait (`simulation/driver/trait.rs`)

```rust
use crate::simulation::event::MouseButton;

/// 输入设备驱动抽象
/// 
/// 通过 trait 抽象底层驱动，使模拟逻辑与具体驱动实现解耦。
/// 当前实现: InterceptionDriver
/// 未来可能: SendInputDriver, MockDriver (测试用)
pub trait InputDriver: Send + Sync {
    /// 发送键盘事件
    /// 
    /// # 参数
    /// - `scan_code`: 硬件扫描码（0-127 标准键，>127 为 E0 扩展键）
    /// - `is_press`: true=按下, false=释放
    fn send_keyboard(&self, scan_code: u16, is_press: bool) -> Result<(), DriverError>;
    
    /// 发送鼠标移动（绝对屏幕坐标）
    /// 
    /// # 参数
    /// - `x`, `y`: 屏幕坐标（单显示器，标准 DPI）
    /// 
    /// # 实现细节
    /// Interception 需要将屏幕坐标归一化到 0-65535 范围
    fn send_mouse_move(&self, x: i32, y: i32) -> Result<(), DriverError>;
    
    /// 发送鼠标按键事件
    /// 
    /// # 参数
    /// - `button`: 按键类型（左/中/右/侧键）
    /// - `is_press`: true=按下, false=释放
    fn send_mouse_button(&self, button: MouseButton, is_press: bool) 
        -> Result<(), DriverError>;
    
    /// 发送鼠标滚轮事件
    /// 
    /// # 参数
    /// - `delta`: 滚动刻度，正数向上，负数向下
    ///   Interception 中 1 刻度 = 120 个滚轮单位（WHEEL_DELTA）
    fn send_mouse_wheel(&self, delta: i32) -> Result<(), DriverError>;
    
    /// 检查驱动是否就绪
    fn is_ready(&self) -> bool;
}

/// 驱动错误类型
#[derive(Debug)]
pub enum DriverError {
    /// 驱动未就绪（context 为 None）
    NotReady,
    /// 设备未找到（"keyboard" 或 "mouse"）
    DeviceNotFound(String),
    /// 发送失败
    SendFailed(String),
}

impl std::fmt::Display for DriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            DriverError::NotReady => write!(f, "Driver not ready"),
            DriverError::DeviceNotFound(dev) => write!(f, "{} device not found", dev),
            DriverError::SendFailed(msg) => write!(f, "Send failed: {}", msg),
        }
    }
}

impl std::error::Error for DriverError {}
```

#### 3.2 设备缓存 (`simulation/driver/device.rs`)

```rust
use std::sync::atomic::{AtomicU8, Ordering};

/// 设备缓存管理器
/// 
/// 避免每次发送事件都扫描设备编号（1-20），启动时扫描一次缓存。
/// Interception 设备编号规则:
/// - 键盘: 1-10
/// - 鼠标: 11-20
pub struct DeviceCache {
    keyboard_device: AtomicU8,
    mouse_device: AtomicU8,
}

impl DeviceCache {
    pub fn new() -> Self {
        Self {
            keyboard_device: AtomicU8::new(0),
            mouse_device: AtomicU8::new(0),
        }
    }
    
    /// 扫描并缓存设备编号
    /// 
    /// 调用时机: InterceptionDriver 创建时（应用启动）
    pub fn scan(&mut self) {
        // 扫描键盘设备 1-10
        if let Some(kb) = (1..=10).find(|d| interception::is_keyboard(*d)) {
            self.keyboard_device.store(kb, Ordering::Relaxed);
        }
        
        // 扫描鼠标设备 11-20
        if let Some(ms) = (11..=20).find(|d| interception::is_mouse(*d)) {
            self.mouse_device.store(ms, Ordering::Relaxed);
        }
    }
    
    /// 获取键盘设备编号
    pub fn get_keyboard(&self) -> Option<u8> {
        let dev = self.keyboard_device.load(Ordering::Relaxed);
        if dev == 0 { None } else { Some(dev) }
    }
    
    /// 获取鼠标设备编号
    pub fn get_mouse(&self) -> Option<u8> {
        let dev = self.mouse_device.load(Ordering::Relaxed);
        if dev == 0 { None } else { Some(dev) }
    }
}
```

#### 3.3 坐标转换 (`simulation/mouse/coordinate.rs`)

```rust
/// 坐标系管理器
/// 
/// 负责屏幕坐标 ↔ Interception 归一化坐标（0-65535）的转换。
/// 当前版本: 单显示器 + 标准 DPI
/// 未来扩展: 多显示器 + DPI 感知
pub struct CoordinateMapper {
    screen_width: i32,
    screen_height: i32,
}

impl CoordinateMapper {
    /// 创建坐标映射器（读取主显示器分辨率）
    pub fn new() -> Self {
        let (w, h) = Self::get_screen_size();
        Self {
            screen_width: w,
            screen_height: h,
        }
    }
    
    /// 屏幕坐标 → Interception 归一化坐标 (0-65535)
    /// 
    /// # 参数
    /// - `x`, `y`: 屏幕像素坐标
    /// 
    /// # 返回
    /// - `(nx, ny)`: 归一化坐标，范围 0-65535
    pub fn to_normalized(&self, x: i32, y: i32) -> (i32, i32) {
        if self.screen_width == 0 || self.screen_height == 0 {
            return (x, y); // 回退：无法获取屏幕尺寸时不转换
        }
        
        let nx = (x as i64 * 65535 / self.screen_width as i64) as i32;
        let ny = (y as i64 * 65535 / self.screen_height as i64) as i32;
        (nx, ny)
    }
    
    /// 归一化坐标 → 屏幕坐标
    /// 
    /// 用于坐标拾取时的反向转换（当前未使用，预留）
    #[allow(dead_code)]
    pub fn to_screen(&self, nx: i32, ny: i32) -> (i32, i32) {
        if self.screen_width == 0 || self.screen_height == 0 {
            return (nx, ny);
        }
        
        let x = (nx as i64 * self.screen_width as i64 / 65535) as i32;
        let y = (ny as i64 * self.screen_height as i64 / 65535) as i32;
        (x, y)
    }
    
    /// 获取主显示器分辨率
    fn get_screen_size() -> (i32, i32) {
        #[cfg(windows)]
        {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN,
            };
            unsafe {
                (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))
            }
        }
        
        #[cfg(not(windows))]
        {
            (0, 0) // 非 Windows 平台返回 0，触发回退逻辑
        }
    }
}
```

---

### 4. Interception 驱动实现 (`simulation/driver/interception.rs`)

```rust
use super::device::DeviceCache;
use super::trait::{DriverError, InputDriver};
use crate::simulation::event::MouseButton;
use crate::simulation::mouse::CoordinateMapper;
use crate::state::SendInterception;
use interception::{KeyState, MouseFlags, MouseState, ScanCode, Stroke};
use log::warn;
use std::convert::TryFrom;
use std::sync::{Arc, Mutex};

/// Interception 驱动适配器
/// 
/// 实现 InputDriver trait，封装 Interception 驱动的具体调用细节。
pub struct InterceptionDriver {
    /// Interception context（与 listener 共享同一个 context）
    context: Arc<Mutex<Option<SendInterception>>>,
    /// 设备缓存（键盘/鼠标设备编号）
    device_cache: DeviceCache,
    /// 坐标转换器
    coord_mapper: CoordinateMapper,
}

impl InterceptionDriver {
    /// 创建驱动实例
    /// 
    /// # 参数
    /// - `context`: 与 listener 共享的 Interception context
    /// 
    /// # 调用时机
    /// 应用启动时，worker 线程创建前
    pub fn new(context: Arc<Mutex<Option<SendInterception>>>) -> Self {
        let mut driver = Self {
            context,
            device_cache: DeviceCache::new(),
            coord_mapper: CoordinateMapper::new(),
        };
        driver.device_cache.scan(); // 启动时扫描一次设备
        driver
    }
    
    /// 获取 Interception context（加锁）
    fn get_context(&self) -> Result<std::sync::MutexGuard<Option<SendInterception>>, DriverError> {
        self.context
            .lock()
            .map_err(|e| DriverError::SendFailed(format!("Lock failed: {}", e)))
    }
}

impl InputDriver for InterceptionDriver {
    fn send_keyboard(&self, scan_code: u16, is_press: bool) -> Result<(), DriverError> {
        let device = self.device_cache.get_keyboard()
            .ok_or_else(|| DriverError::DeviceNotFound("keyboard".to_string()))?;
        
        let ctx_guard = self.get_context()?;
        let interception = ctx_guard.as_ref()
            .ok_or(DriverError::NotReady)?;
        
        let stroke = build_keyboard_stroke(scan_code, is_press);
        interception.0.send(device as i32, &[stroke]);
        Ok(())
    }
    
    fn send_mouse_move(&self, x: i32, y: i32) -> Result<(), DriverError> {
        let device = self.device_cache.get_mouse()
            .ok_or_else(|| DriverError::DeviceNotFound("mouse".to_string()))?;
        
        let ctx_guard = self.get_context()?;
        let interception = ctx_guard.as_ref()
            .ok_or(DriverError::NotReady)?;
        
        // 坐标转换: 屏幕坐标 → 归一化坐标 (0-65535)
        let (norm_x, norm_y) = self.coord_mapper.to_normalized(x, y);
        
        let stroke = Stroke::Mouse {
            state: MouseState::empty(),
            flags: MouseFlags::MOVE_ABSOLUTE,
            rolling: 0,
            x: norm_x,
            y: norm_y,
            information: 0,
        };
        
        interception.0.send(device as i32, &[stroke]);
        Ok(())
    }
    
    fn send_mouse_button(&self, button: MouseButton, is_press: bool) -> Result<(), DriverError> {
        let device = self.device_cache.get_mouse()
            .ok_or_else(|| DriverError::DeviceNotFound("mouse".to_string()))?;
        
        let ctx_guard = self.get_context()?;
        let interception = ctx_guard.as_ref()
            .ok_or(DriverError::NotReady)?;
        
        let state = match (button, is_press) {
            (MouseButton::Left, true) => MouseState::LEFT_BUTTON_DOWN,
            (MouseButton::Left, false) => MouseState::LEFT_BUTTON_UP,
            (MouseButton::Right, true) => MouseState::RIGHT_BUTTON_DOWN,
            (MouseButton::Right, false) => MouseState::RIGHT_BUTTON_UP,
            (MouseButton::Middle, true) => MouseState::MIDDLE_BUTTON_DOWN,
            (MouseButton::Middle, false) => MouseState::MIDDLE_BUTTON_UP,
            
            // 鼠标侧键（预留）
            (MouseButton::Side1, _) | (MouseButton::Side2, _) => {
                warn!("[InterceptionDriver] Side buttons not yet implemented");
                return Err(DriverError::SendFailed(
                    "Side buttons not supported yet".to_string()
                ));
            }
        };
        
        let stroke = Stroke::Mouse {
            state,
            flags: MouseFlags::empty(),
            rolling: 0,
            x: 0,
            y: 0,
            information: 0,
        };
        
        interception.0.send(device as i32, &[stroke]);
        Ok(())
    }
    
    fn send_mouse_wheel(&self, delta: i32) -> Result<(), DriverError> {
        let device = self.device_cache.get_mouse()
            .ok_or_else(|| DriverError::DeviceNotFound("mouse".to_string()))?;
        
        let ctx_guard = self.get_context()?;
        let interception = ctx_guard.as_ref()
            .ok_or(DriverError::NotReady)?;
        
        // Interception 滚轮单位转换: 1 刻度 = 120 个滚轮单位（WHEEL_DELTA）
        let rolling = (delta * 120) as i16;
        
        let stroke = Stroke::Mouse {
            state: MouseState::WHEEL,
            flags: MouseFlags::empty(),
            rolling,
            x: 0,
            y: 0,
            information: 0,
        };
        
        interception.0.send(device as i32, &[stroke]);
        Ok(())
    }
    
    fn is_ready(&self) -> bool {
        self.context.lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
    }
}

/// 构建键盘 Stroke
fn build_keyboard_stroke(scan_code: u16, is_press: bool) -> Stroke {
    let key_state = if is_press {
        KeyState::empty()
    } else {
        KeyState::UP
    };
    
    // E0 扩展键标记（scan_code > 127）
    let state_flags = if scan_code > 127 {
        key_state | KeyState::E0
    } else {
        key_state
    };
    
    let code = ScanCode::try_from(scan_code).unwrap_or_else(|_| {
        warn!("[InterceptionDriver] invalid scan_code {}, using Esc", scan_code);
        ScanCode::Esc
    });
    
    Stroke::Keyboard {
        code,
        state: state_flags,
        information: 0,
    }
}
```

---

## （文档未完，继续下一部分）
### 5. Worker 线程 (`simulation_worker.rs`)

```rust
/// 统一模拟 Worker
/// 
/// 常驻后台线程，负责：
/// 1. 从 channel 接收事件
/// 2. 状态门控（仅在 Running* 状态下执行）
/// 3. 调用驱动发送事件
/// 4. 执行延迟（sleep）
/// 
/// 职责：驱动通信层 + 时序控制层

use crate::simulation::driver::interception::InterceptionDriver;
use crate::simulation::driver::InputDriver;
use crate::simulation::event::SimulationEvent;
use crate::state::{RuntimeStatus, SendInterception, SharedState};
use log::{error, info, warn};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 启动统一模拟 worker 线程
pub fn start_simulation_worker(
    rx: Receiver<SimulationEvent>,
    state: SharedState,
    ctx: Arc<Mutex<Option<SendInterception>>>,
) -> Result<(), String> {
    let driver = Arc::new(InterceptionDriver::new(ctx));
    
    std::thread::spawn(move || {
        info!("[simulation_worker] worker thread started");
        
        loop {
            let event = match rx.recv() {
                Ok(e) => e,
                Err(e) => {
                    warn!("[simulation_worker] channel closed: {}", e);
                    break;
                }
            };
            
            if matches!(event, SimulationEvent::Stop) {
                info!("[simulation_worker] received stop signal");
                break;
            }
            
            // 状态机门控
            let is_running = {
                let app_state = match state.lock() {
                    Ok(s) => s,
                    Err(e) => {
                        error!("[simulation_worker] failed to lock state: {}", e);
                        continue;
                    }
                };
                matches!(
                    app_state.runtime_status,
                    RuntimeStatus::RunningKeyboard | RuntimeStatus::RunningMouse
                )
            };
            
            if !is_running {
                warn!("[simulation_worker] event received but not running, skipping");
                continue;
            }
            
            // 执行事件
            if let Err(e) = execute_event(&*driver, &event) {
                error!("[simulation_worker] event execution failed: {}", e);
            }
        }
        
        info!("[simulation_worker] worker thread exited");
    });
    
    Ok(())
}

fn execute_event<D: InputDriver>(driver: &D, event: &SimulationEvent) -> Result<(), String> {
    match event {
        SimulationEvent::KeyDown { scan_code } => {
            driver.send_keyboard(*scan_code, true).map_err(|e| e.to_string())?;
        }
        SimulationEvent::KeyUp { scan_code } => {
            driver.send_keyboard(*scan_code, false).map_err(|e| e.to_string())?;
        }
        SimulationEvent::MouseMove { x, y } => {
            driver.send_mouse_move(*x, *y).map_err(|e| e.to_string())?;
        }
        SimulationEvent::MouseButtonDown { button } => {
            driver.send_mouse_button(*button, true).map_err(|e| e.to_string())?;
        }
        SimulationEvent::MouseButtonUp { button } => {
            driver.send_mouse_button(*button, false).map_err(|e| e.to_string())?;
        }
        SimulationEvent::MouseWheel { delta } => {
            driver.send_mouse_wheel(*delta).map_err(|e| e.to_string())?;
        }
        SimulationEvent::Delay { ms } => {
            // *** 关键：所有延迟在 worker 线程执行 ***
            std::thread::sleep(Duration::from_millis(*ms));
        }
        SimulationEvent::Stop => {}
    }
    Ok(())
}
```

---

### 6. 执行器 (`simulation/executor/`)

#### 6.1 调度器 (`simulation/executor/scheduler.rs`)

```rust
use crate::simulation::action::{Action, ActionSequence};
use crate::simulation::event::SimulationEvent;
use log::info;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::Arc;

pub struct Scheduler {
    event_tx: SyncSender<SimulationEvent>,
}

impl Scheduler {
    pub fn new(event_tx: SyncSender<SimulationEvent>) -> Self {
        Self { event_tx }
    }
    
    /// 循环执行序列（直到 stop_flag）
    /// 
    /// *** 方案 B 核心：步骤间隔也转换为 Delay 事件 ***
    pub fn execute_loop(
        &self,
        sequence: &ActionSequence,
        stop_flag: &Arc<AtomicBool>,
    ) -> Result<(), String> {
        info!("[Scheduler] Starting execution loop with {} steps", sequence.steps.len());
        
        loop {
            if stop_flag.load(Ordering::Relaxed) {
                info!("[Scheduler] Stop flag detected, exiting loop");
                return Ok(());
            }
            
            for step in &sequence.steps {
                if stop_flag.load(Ordering::Relaxed) {
                    return Ok(());
                }
                
                // 将动作转换为事件序列
                let events = step.action.to_events();
                
                // 发送所有事件
                for event in events {
                    if stop_flag.load(Ordering::Relaxed) {
                        return Ok(());
                    }
                    
                    self.event_tx.send(event)
                        .map_err(|e| format!("Failed to send event: {}", e))?;
                }
                
                // *** 方案 B 关键：步骤间隔也作为 Delay 事件发送 ***
                if step.interval_ms > 0 {
                    self.event_tx.send(SimulationEvent::Delay { ms: step.interval_ms })
                        .map_err(|e| format!("Failed to send delay: {}", e))?;
                }
            }
        }
    }
}
```

**方案 B 核心改动**:
```rust
// 旧版（方案 C）：生产者自己 sleep
for step in &sequence.steps {
    send_events(step.action);
    std::thread::sleep(Duration::from_millis(step.interval_ms)); // ❌ 与 worker 并行
}

// 新版（方案 B）：间隔也发给 worker
for step in &sequence.steps {
    send_events(step.action);
    event_tx.send(Delay { ms: step.interval_ms }); // ✅ worker 串行执行
}
```

---

## 执行流程

### 完整调用链路示例

假设序列：
```
步骤1: 按下 W 键 → 等待 100ms
步骤2: 点击坐标(500, 300) → 等待 50ms
步骤3: 滚轮下滚 3 格 → 等待 200ms
```

#### 时序保证

```
T0:      KeyDown{0x11}        (瞬间)
T0+10:   KeyUp{0x11}          (sleep 10ms 后)
T0+110:  MouseMove{500,300}   (sleep 100ms 后)
T0+115:  MouseButtonDown      (sleep 5ms 后)
T0+125:  MouseButtonUp        (sleep 10ms 后)
T0+175:  MouseWheel{-3}       (sleep 50ms 后)
T0+375:  回到步骤1             (sleep 200ms 后)
```

所有延迟在 worker 单线程串行执行，时序完全精确。

---

## 方案选择说明

### 为什么选择方案 B？

| 对比维度 | 方案 A（单线程） | 方案 B（双线程统一延迟） | 方案 C（双线程分离延迟） |
|---------|----------------|----------------------|---------------------|
| **时序精确度** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐ 长按会错乱 |
| **代码复杂度** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ |
| **扩展性** | ⭐⭐ 受限 | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ |
| **符合现有架构** | ⭐ 需重构 | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ |
| **长按支持** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐ 错乱 |
| **改动成本** | 高 | 低（< 10 行） | 无 |

**决策理由**:
1. 时序精确：支持长按（Hold），方案 C 会错乱
2. 改动成本低：只需 `sleep(interval)` → `send(Delay{interval})`
3. 保持架构：worker 模式优势保留
4. 未来扩展：为并发序列、宏录制留余地

---

## 实施计划

### 阶段 1: 基础架构

**任务**:
1. 创建 `simulation/` 目录
2. 实现 `SimulationEvent`、`KeyAction`、`MouseAction`
3. 实现 `InputDriver` trait
4. 实现 `InterceptionDriver`、`DeviceCache`、`CoordinateMapper`
5. 单元测试

**验收**: `cargo build` 通过，单元测试覆盖核心逻辑

---

### 阶段 2: Worker 统一化

**任务**:
1. 创建 `simulation_worker.rs`
2. 修改 `AppState`: 统一为 `event_tx: SyncSender<SimulationEvent>`
3. 修改 `lib.rs::setup`: 调用 `start_simulation_worker()`
4. 标记旧 worker 为 deprecated

**验收**: 应用启动正常，worker 线程运行

---

### 阶段 3: Scheduler 接入

**任务**:
1. 实现 `Scheduler`
2. 修改 `hotkeys_interception.rs`:
   - 构建 `ActionSequence`
   - 使用 `Scheduler::execute_loop()`
   - **关键**: 间隔改为 `send(Delay{interval_ms})`

**验收**:
- 静态检查通过
- **实机测试**:
  - 按键模拟循环
  - 鼠标点击循环
  - 混合序列时序正确
  - 长按功能正常
  - 停止响应及时（< 500ms）

---

### 阶段 4: 清理与文档

**任务**:
1. 删除旧 worker 文件
2. 更新文档
3. 补充注释

---

## 附录

### A. 事件展开示例

```rust
// 按键
KeyAction::Press { scan_code: 0x11 }
  → [KeyDown{0x11}, Delay{10}, KeyUp{0x11}]

// 长按
KeyAction::Hold { scan_code: 0x11, duration_ms: 2000 }
  → [KeyDown{0x11}, Delay{2000}, KeyUp{0x11}]

// 点击
MouseAction::Click { button: Left, x: 500, y: 300 }
  → [MouseMove{500,300}, Delay{5}, ButtonDown{Left}, Delay{10}, ButtonUp{Left}]
```

### B. 混合序列示例

```rust
ActionSequence {
    steps: vec![
        ActionStep {
            action: Action::Keyboard(KeyAction::Press { scan_code: 0x11 }),
            interval_ms: 100,
        },
        ActionStep {
            action: Action::Mouse(MouseAction::Click {
                button: MouseButton::Left, x: 500, y: 300
            }),
            interval_ms: 50,
        },
    ],
}

// 展开：
// KeyDown{0x11} → Delay{10} → KeyUp{0x11} → Delay{100}
// → MouseMove{500,300} → Delay{5} → ButtonDown{Left} → Delay{10} → ButtonUp{Left} → Delay{50}
// (循环)
```

---

## 文档修订历史

| 版本 | 日期 | 作者 | 说明 |
|-----|------|------|------|
| v2.0 | 2026-07-24 | Claude | 初始版本：方案 B 架构设计 |

---

**文档结束**
