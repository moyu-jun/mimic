# 架构设计总结

## 已完成工作

### 1. 文档归档
- ✅ 旧文档已移至 `docs/archive/`
  - `REQUIREMENTS.md` — 第一阶段功能需求
  - `DESIGN.md` — 第一阶段技术设计
  - `TASKS.md` — 第一阶段实施记录
  - `CHANGELOG.md` — 第一阶段变更日志

### 2. 新架构文档
- ✅ 创建 `docs/ARCHITECTURE.md` (1308 行)
  - **需求分析**: 键盘/鼠标模拟的完整功能需求
  - **架构设计**: 模块结构、线程模型、数据流
  - **核心模块**: 7 个主要组件的详细设计和代码示例
  - **执行流程**: 完整的调用链路和时序保证
  - **方案对比**: 三种方案的详细分析和选型理由
  - **实施计划**: 4 个阶段的渐进式重构路径

- ✅ 创建 `docs/README.md`
  - 文档索引和使用指南
  - 归档文档说明
  - 贡献指南

### 3. 代码质量检查
- ✅ Rust 代码格式化: `cargo fmt` 已执行
- ✅ 前端构建通过: `npm run build` 成功

---

## 架构方案概述

### 选定方案：方案 B（双线程 + 统一延迟）

#### 核心理念
```
【生产者线程】                    【Worker 线程】
循环展开序列                       串行执行所有事件
动作 → 事件流                      (包括所有延迟)
步骤间隔 → Delay 事件   ────→     保证时序精确
```

#### 关键改进点
1. **统一事件模型**: `SimulationEvent` 替代 `ActionEvent` + `MouseEvent`
2. **时序精确**: 所有延迟（动作内 + 步骤间隔）在 worker 单线程串行执行
3. **驱动抽象**: `InputDriver` trait 解耦底层实现
4. **混合序列**: 键盘和鼠标动作可任意混排，严格顺序执行

---

## 模块结构

```
src-tauri/src/
├── simulation/              # 新增模块（待实施）
│   ├── event.rs            # SimulationEvent 定义
│   ├── action.rs           # ActionSequence、ActionStep
│   ├── keyboard/
│   │   └── action.rs       # KeyAction (Press/Down/Up/Hold/Combo)
│   ├── mouse/
│   │   ├── action.rs       # MouseAction (Click/Scroll/Drag...)
│   │   └── coordinate.rs   # 坐标转换器
│   ├── driver/
│   │   ├── trait.rs        # InputDriver trait
│   │   ├── interception.rs # Interception 实现
│   │   └── device.rs       # 设备缓存
│   └── executor/
│       └── scheduler.rs    # 序列调度器
│
├── simulation_worker.rs    # 统一 Worker（待实施）
├── keyboard_worker.rs      # 现有（待标记 deprecated）
├── mouse_worker.rs         # 现有（待标记 deprecated）
└── hotkeys_interception.rs # 现有（待改造）
```

---

## 功能覆盖

### 键盘模拟
- ✅ 按下（Down）
- ✅ 释放（Up）
- ✅ 按下并释放（Press）
- ✅ 长按（Hold{duration_ms}）
- 🔲 组合键（Combo，预留）
- 🔲 鼠标侧键模拟（架构已预留）

### 鼠标模拟
- ✅ 移动到坐标（MoveTo）
- ✅ 点击（Click: 左/中/右键）
- ✅ 按下/释放（Down/Up）
- ✅ 长按（Hold）
- ✅ 滚轮（Scroll: 上滚/下滚）
- 🔲 拖拽（Drag，预留）
- 🔲 侧键（Side1/Side2，预留）

### 混合序列
- ✅ 键盘 + 鼠标混合编排
- ✅ 严格顺序执行
- ✅ 每个动作独立间隔
- ✅ 循环执行
- ✅ 及时停止响应

---

## 实施路径

### 阶段 1: 基础架构（不破坏现有功能）
- 创建 `simulation/` 模块
- 实现事件、动作、驱动抽象
- 单元测试覆盖核心逻辑
- **验收**: `cargo build` 通过

### 阶段 2: Worker 统一化
- 创建 `simulation_worker.rs`
- 修改 `AppState`: 统一 `event_tx`
- 标记旧 worker 为 deprecated
- **验收**: 应用启动正常

### 阶段 3: Scheduler 接入
- 实现 `Scheduler`
- 改造 `hotkeys_interception.rs`
- **关键**: 间隔改为 `send(Delay{interval_ms})`
- **验收**: 实机测试所有功能

### 阶段 4: 清理与文档
- 删除旧代码
- 更新 CLAUDE.md
- 补充注释

---

## 设计亮点

### 1. 时序精确性
```rust
// 问题：方案 C（当前）
生产者 sleep(100ms) 与 worker sleep(10ms) 并行
→ 长按 Hold{2000ms} 会导致后续动作提前发送

// 解决：方案 B
所有 sleep 在 worker 串行
→ 时序完全可控，适合长按和复杂序列
```

### 2. 职责清晰
```
生产者线程: 纯逻辑（循环、展开、停止检测）
Worker 线程: IO + 时序（驱动通信、延迟执行）
```

### 3. 扩展友好
- 未来支持并发序列：多个生产者 → 同一 worker
- 事件录制/回放：在 worker 收到事件时记录
- Mock 测试：实现 `MockDriver` 无需真实驱动

### 4. 向后兼容
- 保留 worker 架构
- 核心改动 < 10 行
- 分阶段渐进式重构

---

## 技术决策记录

### 为什么不用方案 A（单线程）？
- ✅ 优势：代码最简单，时序完美
- ❌ 劣势：
  - 需要重构全局架构（移除 channel）
  - 扩展性受限（未来并发序列困难）
  - 已有 worker 基础设施浪费

### 为什么不用方案 C（当前状态）？
- ✅ 优势：无需改动
- ❌ 劣势：
  - 长按功能会时序错乱
  - 停止响应延迟（可能数秒）
  - 不符合"严格顺序执行"需求

### 方案 B 的权衡
- 略增加复杂度（相比方案 A）
- 但保持架构一致性
- 时序精确 + 扩展性 + 低改动成本

---

## 下一步行动

### 立即可做
1. 阅读 `docs/ARCHITECTURE.md` 熟悉设计
2. 创建 `simulation/` 目录和基础模块
3. 编写单元测试（Mock Driver）

### 实施前准备
1. 确认所有归档文档已备份
2. 创建功能分支 `refactor/simulation-module`
3. 设置里程碑和验收标准

### 风险缓解
- 分阶段实施，每阶段独立验收
- 保留旧代码标记 deprecated，出问题可快速回退
- 实机测试覆盖所有关键场景

---

## 文档维护

- **当前文档**: `docs/ARCHITECTURE.md` — 反映最新设计
- **归档文档**: `docs/archive/*` — 历史参考，只读
- **代码注释**: 复杂逻辑在代码中注释
- **CLAUDE.md**: 实施完成后同步更新架构说明

---

**创建日期**: 2026-07-24  
**架构版本**: v2.0  
**状态**: 设计完成，待实施
