// 序列调度器 — ARCHITECTURE v2.0
//
// 方案 B 核心：生产者只负责展开动作、发送事件，不自己 sleep；
// 步骤间隔也转换为 Delay 事件发给 worker，由 worker 单线程串行执行，保证时序精确。

use crate::simulation::action::ActionSequence;
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

    /// 循环执行序列，直到 stop_flag 置位或 channel 关闭。
    ///
    /// 每步：展开动作事件 → 发送；再把 interval_ms 作为 Delay 事件发送（方案 B）。
    /// stop_flag 在每个事件发送前检查，保证停止响应及时。
    pub fn execute_loop(&self, sequence: &ActionSequence, stop_flag: &Arc<AtomicBool>) {
        info!(
            "[Scheduler] execution loop started with {} steps",
            sequence.steps.len()
        );

        loop {
            for step in &sequence.steps {
                if stop_flag.load(Ordering::Relaxed) {
                    info!("[Scheduler] stop flag detected, exiting loop");
                    return;
                }

                // 展开动作为事件序列并发送
                for event in step.action.to_events() {
                    if stop_flag.load(Ordering::Relaxed) {
                        return;
                    }
                    if self.event_tx.send(event).is_err() {
                        info!("[Scheduler] event channel closed, exiting loop");
                        return;
                    }
                }

                // 步骤间隔也作为 Delay 事件发送（方案 B 关键）
                if step.interval_ms > 0
                    && self
                        .event_tx
                        .send(SimulationEvent::Delay {
                            ms: step.interval_ms,
                        })
                        .is_err()
                {
                    info!("[Scheduler] delay channel closed, exiting loop");
                    return;
                }
            }
        }
    }
}
