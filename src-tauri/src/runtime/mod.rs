//! 单一所有者模拟运行时。
//!
//! Runtime Actor 在线程内独占输入驱动、当前任务、计时和按下账本。
//! 外部只能通过有界控制通道发送 Start/Stop/Shutdown，并等待明确确认。

use crate::simulation::action::ActionSequence;
use crate::simulation::driver::InputDriver;
use crate::simulation::event::{MouseButton, SimulationEvent};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const COMMAND_CAPACITY: usize = 8;
const START_TIMEOUT: Duration = Duration::from_secs(2);
const STOP_TIMEOUT: Duration = Duration::from_secs(1);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);

pub type RunId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    Keyboard,
    Mouse,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimePhase {
    Idle,
    Running { run_id: RunId, mode: RuntimeMode },
    Error { message: String },
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSnapshot {
    pub phase: RuntimePhase,
    pub pressed_count: usize,
}

impl Default for RuntimeSnapshot {
    fn default() -> Self {
        Self {
            phase: RuntimePhase::Idle,
            pressed_count: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEvent {
    Started {
        run_id: RunId,
        mode: RuntimeMode,
    },
    Stopped {
        run_id: RunId,
        mode: RuntimeMode,
    },
    Failed {
        run_id: Option<RunId>,
        message: String,
        pressed_count: usize,
    },
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    Unavailable,
    Busy,
    EmptySequence,
    Faulted(String),
    Driver(String),
    Timeout(&'static str),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => write!(f, "runtime unavailable"),
            Self::Busy => write!(f, "runtime busy"),
            Self::EmptySequence => write!(f, "sequence has no executable events"),
            Self::Faulted(message) => write!(f, "runtime faulted: {message}"),
            Self::Driver(message) => write!(f, "driver error: {message}"),
            Self::Timeout(operation) => write!(f, "{operation} timed out"),
        }
    }
}

impl std::error::Error for RuntimeError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopOutcome {
    Stopped,
    AlreadyIdle,
}

enum RuntimeCommand {
    Start {
        sequence: ActionSequence,
        mode: RuntimeMode,
        reply: SyncSender<Result<RunId, RuntimeError>>,
    },
    Stop {
        reply: SyncSender<Result<StopOutcome, RuntimeError>>,
    },
    Shutdown {
        reply: SyncSender<Result<(), RuntimeError>>,
    },
}

type EventSink = Arc<dyn Fn(RuntimeEvent) + Send + Sync + 'static>;

struct RuntimeInner {
    command_tx: SyncSender<RuntimeCommand>,
    snapshot: Arc<RwLock<RuntimeSnapshot>>,
    join: Mutex<Option<JoinHandle<()>>>,
    closed: AtomicBool,
}

impl Drop for RuntimeInner {
    fn drop(&mut self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            let (reply_tx, reply_rx) = mpsc::sync_channel(1);
            match self
                .command_tx
                .send(RuntimeCommand::Shutdown { reply: reply_tx })
            {
                Ok(()) => match reply_rx.recv_timeout(SHUTDOWN_TIMEOUT) {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        log::error!("runtime shutdown during handle drop failed: {error}");
                    }
                    Err(error) => {
                        log::error!("runtime shutdown acknowledgement failed: {error}");
                    }
                },
                Err(_) => log::debug!("runtime actor already stopped during handle drop"),
            }
        }
        if let Ok(join) = self.join.get_mut() {
            if let Some(join) = join.take() {
                if join.join().is_err() {
                    log::error!("runtime actor panicked during handle drop");
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct RuntimeHandle {
    inner: Arc<RuntimeInner>,
}

impl RuntimeHandle {
    pub fn spawn<F, D>(factory: F, event_sink: EventSink) -> Result<Self, RuntimeError>
    where
        F: FnOnce() -> Result<D, RuntimeError> + Send + 'static,
        D: InputDriver + 'static,
    {
        let (command_tx, command_rx) = mpsc::sync_channel(COMMAND_CAPACITY);
        let snapshot = Arc::new(RwLock::new(RuntimeSnapshot::default()));
        let actor_snapshot = snapshot.clone();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);

        let join = thread::Builder::new()
            .name("mimic-runtime".to_string())
            .spawn(move || match factory() {
                Ok(driver) => {
                    if ready_tx.send(Ok(())).is_err() {
                        log::debug!("runtime startup receiver dropped before success report");
                    }
                    actor_loop(driver, command_rx, actor_snapshot, event_sink);
                }
                Err(error) => {
                    if ready_tx.send(Err(error)).is_err() {
                        log::debug!("runtime startup receiver dropped before failure report");
                    }
                }
            })
            .map_err(|_| RuntimeError::Unavailable)?;

        match ready_rx.recv_timeout(START_TIMEOUT) {
            Ok(Ok(())) => Ok(Self {
                inner: Arc::new(RuntimeInner {
                    command_tx,
                    snapshot,
                    join: Mutex::new(Some(join)),
                    closed: AtomicBool::new(false),
                }),
            }),
            Ok(Err(error)) => {
                if join.join().is_err() {
                    log::error!("runtime factory thread panicked after reporting failure");
                }
                Err(error)
            }
            Err(_) => Err(RuntimeError::Timeout("runtime startup")),
        }
    }

    pub fn start(
        &self,
        sequence: ActionSequence,
        mode: RuntimeMode,
    ) -> Result<RunId, RuntimeError> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(RuntimeError::Unavailable);
        }
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.inner
            .command_tx
            .send(RuntimeCommand::Start {
                sequence,
                mode,
                reply: reply_tx,
            })
            .map_err(|_| RuntimeError::Unavailable)?;
        reply_rx
            .recv_timeout(START_TIMEOUT)
            .map_err(|_| RuntimeError::Timeout("start"))?
    }

    pub fn stop(&self) -> Result<StopOutcome, RuntimeError> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(RuntimeError::Unavailable);
        }
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.inner
            .command_tx
            .send(RuntimeCommand::Stop { reply: reply_tx })
            .map_err(|_| RuntimeError::Unavailable)?;
        reply_rx
            .recv_timeout(STOP_TIMEOUT)
            .map_err(|_| RuntimeError::Timeout("stop"))?
    }

    pub fn shutdown(&self) -> Result<(), RuntimeError> {
        if self.inner.closed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.inner
            .command_tx
            .send(RuntimeCommand::Shutdown { reply: reply_tx })
            .map_err(|_| RuntimeError::Unavailable)?;
        reply_rx
            .recv_timeout(SHUTDOWN_TIMEOUT)
            .map_err(|_| RuntimeError::Timeout("shutdown"))?
    }
    pub fn snapshot(&self) -> RuntimeSnapshot {
        self.inner
            .snapshot
            .read()
            .map(|snapshot| snapshot.clone())
            .unwrap_or_else(|_| RuntimeSnapshot {
                phase: RuntimePhase::Error {
                    message: "runtime snapshot poisoned".to_string(),
                },
                pressed_count: 0,
            })
    }
}

struct ActiveRun {
    run_id: RunId,
    mode: RuntimeMode,
    events: Vec<SimulationEvent>,
    cursor: usize,
}

impl ActiveRun {
    fn new(
        run_id: RunId,
        mode: RuntimeMode,
        sequence: ActionSequence,
    ) -> Result<Self, RuntimeError> {
        let mut events = Vec::new();
        for step in sequence.steps {
            events.extend(step.action.to_events());
            if step.interval_ms > 0 {
                events.push(SimulationEvent::Delay {
                    ms: step.interval_ms,
                });
            }
        }
        if events.is_empty() {
            return Err(RuntimeError::EmptySequence);
        }
        Ok(Self {
            run_id,
            mode,
            events,
            cursor: 0,
        })
    }

    fn next_event(&mut self) -> SimulationEvent {
        let event = self.events[self.cursor].clone();
        self.cursor = (self.cursor + 1) % self.events.len();
        event
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PressedInput {
    Keyboard(u16),
    Mouse(MouseButton),
}

#[derive(Default)]
struct PressedInputLedger {
    inputs: Vec<PressedInput>,
}

impl PressedInputLedger {
    fn record_down(&mut self, input: PressedInput) {
        if !self.inputs.contains(&input) {
            self.inputs.push(input);
        }
    }

    fn record_up(&mut self, input: PressedInput) {
        if let Some(index) = self.inputs.iter().position(|current| *current == input) {
            self.inputs.remove(index);
        }
    }

    fn len(&self) -> usize {
        self.inputs.len()
    }

    fn release_all<D: InputDriver>(&mut self, driver: &mut D) -> Vec<String> {
        let mut errors = Vec::new();
        for input in self.inputs.clone().into_iter().rev() {
            let result = match input {
                PressedInput::Keyboard(scan_code) => driver.send_keyboard(scan_code, false),
                PressedInput::Mouse(button) => driver.send_mouse_button(button, false),
            };
            match result {
                Ok(()) => self.record_up(input),
                Err(error) => errors.push(error.to_string()),
            }
        }
        errors
    }
}

fn actor_loop<D: InputDriver>(
    mut driver: D,
    command_rx: Receiver<RuntimeCommand>,
    snapshot: Arc<RwLock<RuntimeSnapshot>>,
    event_sink: EventSink,
) {
    let mut active: Option<ActiveRun> = None;
    let mut ledger = PressedInputLedger::default();
    let mut next_run_id: RunId = 1;

    loop {
        if active.is_none() {
            match command_rx.recv() {
                Ok(command) => {
                    if !handle_command(
                        command,
                        &mut active,
                        &mut ledger,
                        &mut driver,
                        &snapshot,
                        &event_sink,
                        &mut next_run_id,
                    ) {
                        return;
                    }
                }
                Err(_) => {
                    if let Err(error) = shutdown_actor(
                        &mut active,
                        &mut ledger,
                        &mut driver,
                        &snapshot,
                        &event_sink,
                    ) {
                        log::error!(
                            "runtime shutdown after idle command disconnect failed: {error}"
                        );
                    }
                    return;
                }
            }
            continue;
        }

        match command_rx.try_recv() {
            Ok(command) => {
                if !handle_command(
                    command,
                    &mut active,
                    &mut ledger,
                    &mut driver,
                    &snapshot,
                    &event_sink,
                    &mut next_run_id,
                ) {
                    return;
                }
                continue;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                if let Err(error) = shutdown_actor(
                    &mut active,
                    &mut ledger,
                    &mut driver,
                    &snapshot,
                    &event_sink,
                ) {
                    log::error!("runtime shutdown after active command disconnect failed: {error}");
                }
                return;
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }

        let event = active
            .as_mut()
            .expect("active run checked above")
            .next_event();

        if let SimulationEvent::Delay { ms } = event {
            if !wait_delay(
                Duration::from_millis(ms),
                &command_rx,
                &mut active,
                &mut ledger,
                &mut driver,
                &snapshot,
                &event_sink,
                &mut next_run_id,
            ) {
                return;
            }
            continue;
        }

        if let Err(error) = execute_event(&mut driver, &mut ledger, &event) {
            fail_active(
                &mut active,
                &mut ledger,
                &mut driver,
                &snapshot,
                &event_sink,
                error,
            );
        } else {
            update_pressed_count(&snapshot, ledger.len());
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn wait_delay<D: InputDriver>(
    duration: Duration,
    command_rx: &Receiver<RuntimeCommand>,
    active: &mut Option<ActiveRun>,
    ledger: &mut PressedInputLedger,
    driver: &mut D,
    snapshot: &Arc<RwLock<RuntimeSnapshot>>,
    event_sink: &EventSink,
    next_run_id: &mut RunId,
) -> bool {
    let deadline = Instant::now() + duration;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return true;
        }
        match command_rx.recv_timeout(remaining) {
            Ok(command) => {
                if !handle_command(
                    command,
                    active,
                    ledger,
                    driver,
                    snapshot,
                    event_sink,
                    next_run_id,
                ) {
                    return false;
                }
                if active.is_none() {
                    return true;
                }
            }
            Err(RecvTimeoutError::Timeout) => return true,
            Err(RecvTimeoutError::Disconnected) => {
                if let Err(error) = shutdown_actor(active, ledger, driver, snapshot, event_sink) {
                    log::error!("runtime shutdown during delay disconnect failed: {error}");
                }
                return false;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_command<D: InputDriver>(
    command: RuntimeCommand,
    active: &mut Option<ActiveRun>,
    ledger: &mut PressedInputLedger,
    driver: &mut D,
    snapshot: &Arc<RwLock<RuntimeSnapshot>>,
    event_sink: &EventSink,
    next_run_id: &mut RunId,
) -> bool {
    match command {
        RuntimeCommand::Start {
            sequence,
            mode,
            reply,
        } => {
            if active.is_some() {
                let _ = reply.send(Err(RuntimeError::Busy));
                return true;
            }
            if let RuntimePhase::Error { message } = snapshot_value(snapshot).phase {
                let _ = reply.send(Err(RuntimeError::Faulted(message)));
                return true;
            }

            let run_id = *next_run_id;
            match ActiveRun::new(run_id, mode, sequence) {
                Ok(run) => {
                    *next_run_id = next_run_id.saturating_add(1);
                    *active = Some(run);
                    set_snapshot(
                        snapshot,
                        RuntimePhase::Running { run_id, mode },
                        ledger.len(),
                    );
                    event_sink(RuntimeEvent::Started { run_id, mode });
                    let _ = reply.send(Ok(run_id));
                }
                Err(error) => {
                    let _ = reply.send(Err(error));
                }
            }
            true
        }
        RuntimeCommand::Stop { reply } => {
            let Some(run) = active.take() else {
                let _ = reply.send(Ok(StopOutcome::AlreadyIdle));
                return true;
            };

            let release_errors = ledger.release_all(driver);
            if release_errors.is_empty() {
                set_snapshot(snapshot, RuntimePhase::Idle, ledger.len());
                event_sink(RuntimeEvent::Stopped {
                    run_id: run.run_id,
                    mode: run.mode,
                });
                let _ = reply.send(Ok(StopOutcome::Stopped));
            } else {
                let message = format!("failed to release inputs: {}", release_errors.join("; "));
                set_snapshot(
                    snapshot,
                    RuntimePhase::Error {
                        message: message.clone(),
                    },
                    ledger.len(),
                );
                event_sink(RuntimeEvent::Failed {
                    run_id: Some(run.run_id),
                    message: message.clone(),
                    pressed_count: ledger.len(),
                });
                let _ = reply.send(Err(RuntimeError::Driver(message)));
            }
            true
        }
        RuntimeCommand::Shutdown { reply } => {
            let result = shutdown_actor(active, ledger, driver, snapshot, event_sink);
            let _ = reply.send(result);
            false
        }
    }
}

fn execute_event<D: InputDriver>(
    driver: &mut D,
    ledger: &mut PressedInputLedger,
    event: &SimulationEvent,
) -> Result<(), RuntimeError> {
    match event {
        SimulationEvent::KeyDown { scan_code } => {
            driver
                .send_keyboard(*scan_code, true)
                .map_err(|error| RuntimeError::Driver(error.to_string()))?;
            ledger.record_down(PressedInput::Keyboard(*scan_code));
        }
        SimulationEvent::KeyUp { scan_code } => {
            driver
                .send_keyboard(*scan_code, false)
                .map_err(|error| RuntimeError::Driver(error.to_string()))?;
            ledger.record_up(PressedInput::Keyboard(*scan_code));
        }
        SimulationEvent::MouseMove { x, y } => driver
            .send_mouse_move(*x, *y)
            .map_err(|error| RuntimeError::Driver(error.to_string()))?,
        SimulationEvent::MouseButtonDown { button } => {
            driver
                .send_mouse_button(*button, true)
                .map_err(|error| RuntimeError::Driver(error.to_string()))?;
            ledger.record_down(PressedInput::Mouse(*button));
        }
        SimulationEvent::MouseButtonUp { button } => {
            driver
                .send_mouse_button(*button, false)
                .map_err(|error| RuntimeError::Driver(error.to_string()))?;
            ledger.record_up(PressedInput::Mouse(*button));
        }
        SimulationEvent::MouseWheel { delta } => driver
            .send_mouse_wheel(*delta)
            .map_err(|error| RuntimeError::Driver(error.to_string()))?,
        SimulationEvent::Delay { .. } => {}
    }
    Ok(())
}

fn fail_active<D: InputDriver>(
    active: &mut Option<ActiveRun>,
    ledger: &mut PressedInputLedger,
    driver: &mut D,
    snapshot: &Arc<RwLock<RuntimeSnapshot>>,
    event_sink: &EventSink,
    error: RuntimeError,
) {
    let run_id = active.as_ref().map(|run| run.run_id);
    active.take();

    let mut message = error.to_string();
    let release_errors = ledger.release_all(driver);
    if !release_errors.is_empty() {
        message.push_str("; release failures: ");
        message.push_str(&release_errors.join("; "));
    }

    set_snapshot(
        snapshot,
        RuntimePhase::Error {
            message: message.clone(),
        },
        ledger.len(),
    );
    event_sink(RuntimeEvent::Failed {
        run_id,
        message,
        pressed_count: ledger.len(),
    });
}

fn shutdown_actor<D: InputDriver>(
    active: &mut Option<ActiveRun>,
    ledger: &mut PressedInputLedger,
    driver: &mut D,
    snapshot: &Arc<RwLock<RuntimeSnapshot>>,
    event_sink: &EventSink,
) -> Result<(), RuntimeError> {
    let run = active.take();
    let release_errors = ledger.release_all(driver);
    if release_errors.is_empty() {
        set_snapshot(snapshot, RuntimePhase::Shutdown, ledger.len());
        event_sink(RuntimeEvent::Shutdown);
        return Ok(());
    }

    let message = format!(
        "failed to release inputs during shutdown: {}",
        release_errors.join("; ")
    );
    set_snapshot(
        snapshot,
        RuntimePhase::Error {
            message: message.clone(),
        },
        ledger.len(),
    );
    event_sink(RuntimeEvent::Failed {
        run_id: run.map(|value| value.run_id),
        message: message.clone(),
        pressed_count: ledger.len(),
    });
    Err(RuntimeError::Driver(message))
}

fn set_snapshot(
    snapshot: &Arc<RwLock<RuntimeSnapshot>>,
    phase: RuntimePhase,
    pressed_count: usize,
) {
    if let Ok(mut current) = snapshot.write() {
        current.phase = phase;
        current.pressed_count = pressed_count;
    }
}

fn update_pressed_count(snapshot: &Arc<RwLock<RuntimeSnapshot>>, pressed_count: usize) {
    if let Ok(mut current) = snapshot.write() {
        current.pressed_count = pressed_count;
    }
}

fn snapshot_value(snapshot: &Arc<RwLock<RuntimeSnapshot>>) -> RuntimeSnapshot {
    snapshot
        .read()
        .map(|current| current.clone())
        .unwrap_or_else(|_| RuntimeSnapshot {
            phase: RuntimePhase::Error {
                message: "runtime snapshot poisoned".to_string(),
            },
            pressed_count: 0,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simulation::action::{Action, ActionSequence};
    use crate::simulation::driver::DriverError;
    use crate::simulation::keyboard::KeyAction;
    use std::sync::mpsc::Sender;

    #[derive(Clone)]
    struct FakeDriverState {
        calls: Arc<Mutex<Vec<String>>>,
        notifications: Sender<String>,
        fail_on_call: Option<usize>,
        call_count: Arc<Mutex<usize>>,
    }

    struct FakeDriver {
        state: FakeDriverState,
    }

    impl FakeDriver {
        fn record(&mut self, call: String) -> Result<(), DriverError> {
            let mut count = self.state.call_count.lock().expect("call count");
            *count += 1;
            if self.state.fail_on_call == Some(*count) {
                return Err(DriverError::SendFailed(format!(
                    "injected failure at call {}",
                    *count
                )));
            }
            self.state.calls.lock().expect("calls").push(call.clone());
            let _ = self.state.notifications.send(call);
            Ok(())
        }
    }

    impl InputDriver for FakeDriver {
        fn send_keyboard(&mut self, scan_code: u16, is_press: bool) -> Result<(), DriverError> {
            self.record(format!(
                "key:{scan_code}:{}",
                if is_press { "down" } else { "up" }
            ))
        }

        fn send_mouse_move(&mut self, x: i32, y: i32) -> Result<(), DriverError> {
            self.record(format!("move:{x}:{y}"))
        }

        fn send_mouse_button(
            &mut self,
            button: MouseButton,
            is_press: bool,
        ) -> Result<(), DriverError> {
            self.record(format!(
                "mouse:{button:?}:{}",
                if is_press { "down" } else { "up" }
            ))
        }

        fn send_mouse_wheel(&mut self, delta: i32) -> Result<(), DriverError> {
            self.record(format!("wheel:{delta}"))
        }
    }

    fn runtime(
        fail_on_call: Option<usize>,
    ) -> (
        RuntimeHandle,
        Arc<Mutex<Vec<String>>>,
        mpsc::Receiver<String>,
    ) {
        let (notifications, receiver) = mpsc::channel();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let state = FakeDriverState {
            calls: calls.clone(),
            notifications,
            fail_on_call,
            call_count: Arc::new(Mutex::new(0)),
        };
        let handle =
            RuntimeHandle::spawn(move || Ok(FakeDriver { state }), Arc::new(|_| {})).unwrap();
        (handle, calls, receiver)
    }

    fn key_down_sequence(scan_code: u16, interval_ms: u64) -> ActionSequence {
        let mut sequence = ActionSequence::new();
        sequence.add(Action::Keyboard(KeyAction::Down { scan_code }), interval_ms);
        sequence
    }

    #[test]
    fn stop_interrupts_long_delay_and_releases_pressed_key() {
        let (runtime, calls, notifications) = runtime(None);
        runtime
            .start(key_down_sequence(30, 10_000), RuntimeMode::Keyboard)
            .unwrap();
        assert_eq!(
            notifications.recv_timeout(Duration::from_secs(1)).unwrap(),
            "key:30:down"
        );

        let started = Instant::now();
        assert_eq!(runtime.stop().unwrap(), StopOutcome::Stopped);
        assert!(started.elapsed() < Duration::from_millis(100));
        assert!(calls.lock().unwrap().contains(&"key:30:up".to_string()));
        assert_eq!(runtime.snapshot().pressed_count, 0);
    }

    #[test]
    fn stop_latency_distribution_meets_budget() {
        const SAMPLE_COUNT: usize = 200;
        let (runtime, _calls, notifications) = runtime(None);
        let mut samples = Vec::with_capacity(SAMPLE_COUNT);

        for index in 0..SAMPLE_COUNT {
            let scan_code = 50 + (index % 50) as u16;
            runtime
                .start(key_down_sequence(scan_code, 60_000), RuntimeMode::Keyboard)
                .unwrap();
            assert_eq!(
                notifications.recv_timeout(Duration::from_secs(1)).unwrap(),
                format!("key:{scan_code}:down")
            );

            let started = Instant::now();
            assert_eq!(runtime.stop().unwrap(), StopOutcome::Stopped);
            samples.push(started.elapsed());
            assert_eq!(
                notifications.recv_timeout(Duration::from_secs(1)).unwrap(),
                format!("key:{scan_code}:up")
            );
        }

        samples.sort_unstable();
        let p95_index = (samples.len() * 95).div_ceil(100) - 1;
        let p95 = samples[p95_index];
        let worst = *samples.last().unwrap();
        println!(
            "stop_latency samples={SAMPLE_COUNT} p95_us={} max_us={}",
            p95.as_micros(),
            worst.as_micros()
        );

        assert!(
            p95 <= Duration::from_millis(100),
            "stop latency P95 {p95:?} exceeds 100ms"
        );
        assert!(
            worst <= Duration::from_millis(250),
            "stop latency maximum {worst:?} exceeds 250ms"
        );
    }

    #[test]
    fn quick_restart_does_not_resume_old_run() {
        let (runtime, calls, notifications) = runtime(None);
        runtime
            .start(key_down_sequence(31, 10_000), RuntimeMode::Keyboard)
            .unwrap();
        assert_eq!(
            notifications.recv_timeout(Duration::from_secs(1)).unwrap(),
            "key:31:down"
        );
        runtime.stop().unwrap();

        runtime
            .start(key_down_sequence(32, 10_000), RuntimeMode::Keyboard)
            .unwrap();
        assert_eq!(
            notifications.recv_timeout(Duration::from_secs(1)).unwrap(),
            "key:31:up"
        );
        assert_eq!(
            notifications.recv_timeout(Duration::from_secs(1)).unwrap(),
            "key:32:down"
        );
        runtime.stop().unwrap();

        let calls = calls.lock().unwrap();
        assert_eq!(
            calls
                .iter()
                .filter(|call| call.as_str() == "key:31:down")
                .count(),
            1
        );
    }

    #[test]
    fn release_failure_leaves_runtime_faulted_and_tracks_pressed_input() {
        let (runtime, _calls, notifications) = runtime(Some(2));
        runtime
            .start(key_down_sequence(33, 10_000), RuntimeMode::Keyboard)
            .unwrap();
        assert_eq!(
            notifications.recv_timeout(Duration::from_secs(1)).unwrap(),
            "key:33:down"
        );

        assert!(matches!(runtime.stop(), Err(RuntimeError::Driver(_))));
        let snapshot = runtime.snapshot();
        assert!(matches!(snapshot.phase, RuntimePhase::Error { .. }));
        assert_eq!(snapshot.pressed_count, 1);
    }

    #[test]
    fn busy_start_is_rejected_and_idle_stop_is_idempotent() {
        let (runtime, _calls, notifications) = runtime(None);
        assert_eq!(runtime.stop().unwrap(), StopOutcome::AlreadyIdle);
        runtime
            .start(key_down_sequence(40, 10_000), RuntimeMode::Keyboard)
            .unwrap();
        notifications.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(matches!(
            runtime.start(key_down_sequence(41, 10_000), RuntimeMode::Keyboard),
            Err(RuntimeError::Busy)
        ));
        runtime.stop().unwrap();
    }

    #[test]
    fn shutdown_interrupts_delay_releases_inputs_and_closes_channel() {
        let (runtime, calls, notifications) = runtime(None);
        runtime
            .start(key_down_sequence(42, 10_000), RuntimeMode::Keyboard)
            .unwrap();
        notifications.recv_timeout(Duration::from_secs(1)).unwrap();
        runtime.shutdown().unwrap();
        assert!(calls.lock().unwrap().contains(&"key:42:up".to_string()));
        assert!(matches!(runtime.snapshot().phase, RuntimePhase::Shutdown));
        assert!(matches!(
            runtime.start(key_down_sequence(43, 10), RuntimeMode::Keyboard),
            Err(RuntimeError::Unavailable)
        ));
    }
    #[test]
    fn shutdown_release_failure_is_reported_and_preserves_ledger() {
        let (runtime, _calls, notifications) = runtime(Some(2));
        runtime
            .start(key_down_sequence(44, 10_000), RuntimeMode::Keyboard)
            .unwrap();
        assert_eq!(
            notifications.recv_timeout(Duration::from_secs(1)).unwrap(),
            "key:44:down"
        );

        assert!(matches!(runtime.shutdown(), Err(RuntimeError::Driver(_))));
        let snapshot = runtime.snapshot();
        assert!(matches!(snapshot.phase, RuntimePhase::Error { .. }));
        assert_eq!(snapshot.pressed_count, 1);
        assert!(matches!(
            runtime.start(key_down_sequence(45, 10), RuntimeMode::Keyboard),
            Err(RuntimeError::Unavailable)
        ));
    }
}
