# 架构改进建议实施方案

## 基于 ARCHITECTURE_REVIEW.md 的可选改进路线图

---

## P1：近期改进（3 个月内）

### 1. 增加基础单元测试

**目标**：为纯逻辑模块增加测试覆盖，防止回归。

#### 1.1 坐标转换测试

**文件**：`src/simulation/mouse/coordinate.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_screen_center_normalization() {
        let mapper = CoordinateMapper::new();
        // 假设屏幕 1920x1080
        let (nx, ny) = mapper.to_normalized(960, 540);
        // 中心点应映射到归一化空间中心（32767 ≈ 65535/2）
        assert!((nx - 32767).abs() < 100);
        assert!((ny - 32767).abs() < 100);
    }

    #[test]
    fn test_origin_normalization() {
        let mapper = CoordinateMapper::new();
        let (nx, ny) = mapper.to_normalized(0, 0);
        assert_eq!(nx, 0);
        assert_eq!(ny, 0);
    }

    #[test]
    fn test_max_coord_normalization() {
        let mapper = CoordinateMapper::new();
        let screen_width = mapper.screen_width;
        let screen_height = mapper.screen_height;
        let (nx, ny) = mapper.to_normalized(screen_width - 1, screen_height - 1);
        assert_eq!(nx, 65535);
        assert_eq!(ny, 65535);
    }
}
```

**工作量**：1-2 小时

---

#### 1.2 动作展开测试

**文件**：`src/simulation/keyboard/action.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::simulation::event::SimulationEvent;
    use crate::simulation::timing::KEY_PRESS_HOLD_MS;

    #[test]
    fn test_key_press_expansion() {
        let action = KeyAction::Press { scan_code: 0x1E }; // A 键
        let events = action.to_events();
        
        assert_eq!(events.len(), 3);
        
        match &events[0] {
            SimulationEvent::KeyDown { scan_code } => assert_eq!(*scan_code, 0x1E),
            _ => panic!("Expected KeyDown"),
        }
        
        match &events[1] {
            SimulationEvent::Delay { ms } => assert_eq!(*ms, KEY_PRESS_HOLD_MS),
            _ => panic!("Expected Delay"),
        }
        
        match &events[2] {
            SimulationEvent::KeyUp { scan_code } => assert_eq!(*scan_code, 0x1E),
            _ => panic!("Expected KeyUp"),
        }
    }

    #[test]
    fn test_combo_expansion() {
        let action = KeyAction::Combo {
            modifiers: vec![0x1D, 0x38], // Ctrl + Alt
            key: 0x20, // D 键
        };
        let events = action.to_events();
        
        // 应该展开为：Ctrl按下 → 延迟 → Alt按下 → 延迟 → D按下 → 延迟 → D释放 → 延迟 → Alt释放 → 延迟 → Ctrl释放
        assert!(events.len() >= 11);
        
        // 验证第一个事件是 Ctrl 按下
        match &events[0] {
            SimulationEvent::KeyDown { scan_code } => assert_eq!(*scan_code, 0x1D),
            _ => panic!("Expected Ctrl down"),
        }
        
        // 验证最后一个事件是 Ctrl 释放
        match events.last().unwrap() {
            SimulationEvent::KeyUp { scan_code } => assert_eq!(*scan_code, 0x1D),
            _ => panic!("Expected Ctrl up"),
        }
    }
}
```

**工作量**：2-3 小时

---

#### 1.3 鼠标动作测试

**文件**：`src/simulation/mouse/action.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::simulation::event::{MouseButton, SimulationEvent};
    use crate::simulation::timing::{MOUSE_CLICK_HOLD_MS, MOUSE_CLICK_SETTLE_MS};

    #[test]
    fn test_click_expansion() {
        let action = MouseAction::Click {
            button: MouseButton::Left,
            x: 100,
            y: 200,
        };
        let events = action.to_events();
        
        assert_eq!(events.len(), 5);
        
        // 验证事件序列
        match &events[0] {
            SimulationEvent::MouseMove { x, y } => {
                assert_eq!(*x, 100);
                assert_eq!(*y, 200);
            }
            _ => panic!("Expected MouseMove"),
        }
        
        match &events[1] {
            SimulationEvent::Delay { ms } => assert_eq!(*ms, MOUSE_CLICK_SETTLE_MS),
            _ => panic!("Expected settle delay"),
        }
        
        match &events[2] {
            SimulationEvent::MouseButtonDown { button } => assert_eq!(*button, MouseButton::Left),
            _ => panic!("Expected MouseButtonDown"),
        }
        
        match &events[3] {
            SimulationEvent::Delay { ms } => assert_eq!(*ms, MOUSE_CLICK_HOLD_MS),
            _ => panic!("Expected hold delay"),
        }
        
        match &events[4] {
            SimulationEvent::MouseButtonUp { button } => assert_eq!(*button, MouseButton::Left),
            _ => panic!("Expected MouseButtonUp"),
        }
    }
}
```

**工作量**：1-2 小时

---

#### 1.4 序列构建测试

**文件**：`src/simulation/action/sequence.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::simulation::keyboard::KeyAction;

    #[test]
    fn test_sequence_construction() {
        let mut seq = ActionSequence::new();
        assert!(seq.is_empty());
        
        seq.add(Action::Keyboard(KeyAction::Press { scan_code: 0x1E }), 50);
        seq.add(Action::Keyboard(KeyAction::Press { scan_code: 0x1F }), 100);
        
        assert_eq!(seq.steps.len(), 2);
        assert_eq!(seq.steps[0].interval_ms, 50);
        assert_eq!(seq.steps[1].interval_ms, 100);
        
        assert!(!seq.is_empty());
    }
}
```

**工作量**：30 分钟

---

**运行测试**：
```bash
cd src-tauri
cargo test
```

**总工作量**：5-8 小时

---

### 2. 序列调度线程管理增强

**问题**：当前启动序列时未保存线程 handle，无法查询状态或强制停止。

#### 2.1 修改 AppState

**文件**：`src/state.rs`

在 `AppState` 中增加字段：

```rust
use std::thread::JoinHandle;

pub struct AppState {
    // ... 现有字段 ...
    
    /// 当前运行的序列调度线程句柄
    pub simulation_thread: Arc<Mutex<Option<JoinHandle<()>>>>,
}
```

初始化时：

```rust
simulation_thread: Arc::new(Mutex::new(None)),
```

---

#### 2.2 修改 hotkeys_interception.rs

启动序列时保存 handle：

```rust
// 旧代码（163 行）
std::thread::spawn(move || {
    scheduler.execute_loop(&sequence, &stop_flag);
});

// 新代码
let handle = std::thread::spawn(move || {
    scheduler.execute_loop(&sequence, &stop_flag);
    log::info!("[hotkeys] simulation thread exited normally");
});

// 保存 handle
if let Ok(mut thread_guard) = state.simulation_thread.lock() {
    *thread_guard = Some(handle);
}
```

---

#### 2.3 停止时清理

在停止热键处理中（256 行附近）：

```rust
// 置位 stop_flag
app_state.stop_flag.store(true, Ordering::Relaxed);

// 可选：等待线程退出（带超时）
if let Ok(mut thread_guard) = app_state.simulation_thread.lock() {
    if let Some(handle) = thread_guard.take() {
        // join 或 detach
        drop(handle); // 当前保持 detach 行为，未来可改为 join
    }
}
```

---

#### 2.4 增加查询命令（可选）

**文件**：`src/lib.rs`

```rust
#[tauri::command]
fn is_simulation_running(state: tauri::State<SharedState>) -> Result<bool, String> {
    let app_state = state.inner().lock()
        .map_err(|e| format!("Failed to lock state: {}", e))?;
    
    let thread_alive = app_state.simulation_thread.lock()
        .map(|guard| guard.is_some())
        .unwrap_or(false);
    
    Ok(thread_alive && matches!(
        app_state.runtime_status,
        RuntimeStatus::RunningKeyboard | RuntimeStatus::RunningMouse
    ))
}
```

前端可轮询此命令，检测线程是否异常退出。

---

**工作量**：3-4 小时

---

## P2：中期改进（6-12 个月）

### 1. 热键模块重构

**目标**：分离监听与业务编排，提升可复用性。

#### 新结构

```
src/hotkeys/
├── mod.rs          # 公开 API（update_hotkeys / validate_hotkey）
├── config.rs       # 热键配置管理（INI 读写、验证）
├── listener.rs     # Interception 监听（纯事件接收 + 匹配）
└── executor.rs     # 热键触发 → 模拟启动编排
```

#### 重构步骤

1. **创建 hotkeys/ 目录**
2. **拆分 hotkeys.rs → config.rs**
   - 保留 `HotkeyConfig` / `HotkeyUpdateResult` / `validate_hotkey` / `update_hotkeys`
3. **拆分 hotkeys_interception.rs → listener.rs + executor.rs**
   - `listener.rs`：filter + wait + receive + 匹配 → 回调
   - `executor.rs`：构建 ActionSequence + 启动 Scheduler
4. **更新导入**

**优势**：
- listener 可复用于录制宏功能
- executor 逻辑独立，易于单测（mock listener）

**风险**：
- 涉及较多文件移动，需仔细验证导入

**工作量**：1-2 天

---

### 2. 实现 SendInputDriver

**目标**：提供无需驱动的备选方案。

#### 新增文件

**文件**：`src/simulation/driver/sendinput.rs`

```rust
// Windows SendInput API 驱动实现 — ARCHITECTURE v3.5
//
// 优势：无需安装驱动，开箱即用
// 劣势：部分游戏会屏蔽 SendInput 模拟（检测到非真实输入）

use super::input_driver::{DriverError, InputDriver};
use crate::simulation::event::MouseButton;
use crate::simulation::mouse::CoordinateMapper;
use winapi::um::winuser::{SendInput, INPUT, ...};

pub struct SendInputDriver {
    coord_mapper: CoordinateMapper,
}

impl SendInputDriver {
    pub fn new() -> Self {
        Self {
            coord_mapper: CoordinateMapper::new(),
        }
    }
}

impl InputDriver for SendInputDriver {
    fn send_keyboard(&self, scan_code: u16, is_press: bool) -> Result<(), DriverError> {
        // 调用 SendInput API
        // ...
    }

    // ... 其他方法实现
}
```

#### 配置支持

**文件**：`src/config.rs`

在 `AppConfig` 中增加：

```rust
pub driver: String, // "interception" | "sendinput"
```

INI 默认：

```ini
[general]
driver=interception
```

#### 动态选择

**文件**：`src/simulation_worker.rs`

```rust
let driver: Box<dyn InputDriver> = match app_state.config.driver.as_str() {
    "sendinput" => Box::new(SendInputDriver::new()),
    _ => Box::new(InterceptionDriver::new(ctx)),
};
```

**工作量**：3-5 天

---

## P3：长期改进（1 年+）

### 1. 多配置方案支持

**需求**：用户可能玩多款游戏，每个需要不同的按键序列。

#### 方案

- INI 扩展：支持 `[profile:game_a]` / `[profile:game_b]`
- 前端增加配置切换下拉框
- 热键快速切换（如 F9 切换到下一个配置）

**涉及模块**：
- `config.rs`：解析多 profile
- 前端：UI 重构
- `hotkeys_interception.rs`：增加配置切换热键

**工作量**：1-2 周

---

### 2. 序列编辑器增强（条件分支）

**需求**：根据游戏状态执行不同动作（如血量低于 50% 喝药）。

#### 方案

- 增加 `Action::Condition { check: Fn() -> bool, then: Vec<Action>, else: Vec<Action> }`
- Scheduler 执行时动态评估条件

**技术挑战**：
- 如何让用户定义条件（脚本语言？Lua？）
- 如何读取游戏状态（OCR？内存读取？）

**工作量**：1-2 个月（需配合前端重构）

---

## 优先级总结

| 改进项 | 优先级 | 工作量 | 收益 |
|--------|--------|--------|------|
| 基础单元测试 | P1 | 5-8 小时 | 防回归、提升信心 |
| 线程管理增强 | P1 | 3-4 小时 | 提升可观测性 |
| 热键模块重构 | P2 | 1-2 天 | 提升可维护性 |
| SendInputDriver | P2 | 3-5 天 | 降低用户门槛 |
| 多配置方案 | P3 | 1-2 周 | 增强易用性 |
| 条件分支 | P3 | 1-2 月 | 大幅提升能力 |

---

## 实施建议

1. **先实施 P1**（总计 1-2 天工作量）：
   - 测试覆盖立即见效，防止未来回归
   - 线程管理增强为调试提供便利

2. **按需实施 P2**：
   - 如果用户反馈"驱动安装麻烦" → 实施 SendInputDriver
   - 如果团队扩大需代码审查 → 实施热键模块重构

3. **谨慎实施 P3**：
   - 需配合前端大改，评估用户需求强度
   - 条件分支属于"高级功能"，可能增加学习成本

---

**文档版本**：v1.0  
**关联文档**：ARCHITECTURE_REVIEW.md  
**最后更新**：2026-07-25
