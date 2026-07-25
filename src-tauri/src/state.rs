// 全局状态管理 — DESIGN 9.2
//
// 本模块定义应用的全局状态结构，用于在 Tauri 命令间共享运行时信息。
// 状态包括当前页面、配置、运行状态、驱动状态和停止标记。

use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};

use crate::config::AppConfig;

// 为 Interception 创建 Send + Sync 包装器
// SAFETY: Interception 内部封装的是 Windows 内核 HANDLE（DeviceIoControl 句柄）。
// Windows 内核对象在内核层面是线程安全的，可在线程间传递；但 interception crate 上层
// 的 Rust 状态（如 buffer、filter 设置）不是内部同步的，因此调用方必须自行串行化访问。
// 本项目所有 SendInterception 实例均通过 Arc<Mutex<Option<SendInterception>>> 持有，
// 由 Mutex 保证同一时刻只有一个线程在调用其方法（wait/receive/send）。
//
// 维护提示：如未来要去掉外层 Mutex，必须先核实 interception crate 当前版本的
// 内部状态是否已变为线程安全；否则会引入数据竞争 UB。
pub struct SendInterception(pub interception::Interception);
unsafe impl Send for SendInterception {}
unsafe impl Sync for SendInterception {}

/// 运行状态机 — DESIGN 9.2
///
/// serde 默认将无字段枚举序列化为其名字字符串（如 "RunningKeyboard"），
/// 正好匹配前端的 PascalCase 联合类型，无需额外 rename。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RuntimeStatus {
    /// 待机
    Idle,
    /// 当前处于按键模拟页面，可启动按键模拟
    ReadyKeyboard,
    /// 当前处于鼠标模拟页面，可启动鼠标模拟
    ReadyMouse,
    /// 按键模拟运行中
    RunningKeyboard,
    /// 鼠标模拟运行中
    RunningMouse,
    /// 当前处于自定义序列详情页，可启动该序列模拟
    ReadyCustom,
    /// 自定义序列模拟运行中
    RunningCustom,
    /// 正在拾取鼠标坐标
    PickingMouse,
    /// 正在录制提示音（阶段 18）
    Recording,
    /// 驱动或配置错误
    Error,
}

/// 驱动状态 — DESIGN 9.2
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DriverStatus {
    /// 驱动未安装
    NotInstalled,
    /// 驱动已安装但需要重启系统
    InstalledNeedReboot,
    /// 驱动已加载，可以使用
    Ready,
    /// 驱动错误
    Error,
}

/// 应用全局状态 — DESIGN 9.2
///
/// 阶段 9 增加 `config_warning`：启动时若 INI 写盘失败，此字段记录原因，
/// 前端可通过 `get_init_warning` 命令读取并在首页展示小字提示。
/// 其余字段在阶段 10-13 接入真实功能后逐步使用，当前先 allow(dead_code)。
#[allow(dead_code)]
pub struct AppState {
    /// 当前配置（从 INI 加载或默认）
    pub config: AppConfig,
    /// 启动时配置写盘失败的原因；None 表示无问题
    pub config_warning: Option<String>,
    /// 当前页面（用于判断热键是否可触发）
    pub current_page: String,
    /// 运行状态机
    pub runtime_status: RuntimeStatus,
    /// 驱动状态
    pub driver_status: DriverStatus,
    /// 停止标记，供 worker 线程检查
    pub stop_flag: Arc<AtomicBool>,
    /// 当前坐标拾取的目标行 ID；Some 表示拾取进行中（PickingMouse 状态下由 listener 读取）
    pub pick_row_id: Option<String>,
    /// 当前激活的自定义序列 ID；Some 表示处于某序列详情页，热键启动作用于该序列。
    /// 列表页为 None → 热键无效（CustomSequenceBuilder 找不到序列返回 None）。
    pub active_custom_sequence_id: Option<String>,
    /// 监听专用 context（设置 filter + 阻塞 wait）
    pub interception_listener: Arc<Mutex<Option<SendInterception>>>,
    /// 模拟专用 context（仅 send，非阻塞）
    pub interception_worker: Arc<Mutex<Option<SendInterception>>>,
    /// 统一模拟事件发送器 — ARCHITECTURE v2.0
    /// 键盘/鼠标统一走此 channel，由 simulation_worker 消费。
    pub event_tx: SyncSender<crate::simulation::event::SimulationEvent>,
    /// 提示音录制句柄（阶段 18）— Some 表示录制进行中，由录制线程管理
    pub recording: crate::sound_recorder::RecordingHandle,
    /// 录制完成待剪裁的 PCM 缓冲（阶段 18 剪裁）— (samples, sampleRate)
    pub recording_buffer: RecordingBuffer,
}

/// PCM 录制缓冲类型：(samples, sampleRate)
pub type RecordingBuffer = Arc<Mutex<Option<(Vec<i16>, u32)>>>;

/// 共享状态类型（Arc + Mutex 包装）
pub type SharedState = Arc<Mutex<AppState>>;
