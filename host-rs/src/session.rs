use crate::{
    control::{self, Command, Event},
    settings::Settings,
    usb::DeviceId,
};
use eframe::egui;
use std::{
    io::BufReader,
    process::{Child, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub enum State {
    Idle,
    Connecting,
    ApprovalPending(String),
    Waiting,
    Streaming,
    Stopping,
    Failed(String),
}
#[derive(Default)]
struct Pending {
    quality: Option<Command>,
    transform: Option<Command>,
}
impl Pending {
    fn take(&mut self) -> Option<Command> {
        self.quality.take().or_else(|| self.transform.take())
    }
}
pub struct Session {
    executable: std::path::PathBuf,
    child: Option<Child>,
    events: Option<mpsc::Receiver<Event>>,
    stop: Arc<AtomicBool>,
    pending: Arc<Mutex<Pending>>,
    deadline: Option<Instant>,
    quality_deadline: Option<Instant>,
    writer: Option<std::thread::Thread>,
    /// A capture fixture drives this session instead of a worker, and the sender
    /// stays alive so the fixture's silence is not read as a closed channel.
    #[cfg(feature = "capture")]
    capture_fixture: Option<mpsc::Sender<Event>>,
    /// Where the worker publishes its preview record; set before `connect`.
    preview_path: Option<std::path::PathBuf>,
    /// Pairing details the phone sent over USB, waiting for the UI to consume them.
    pairing_info: Option<crate::wire::PairingInfo>,
    pub state: State,
    pub stats: Option<Event>,
    stats_at: Option<Instant>,
    pub usb_permission: Option<DeviceId>,
    pub quality: crate::quality::Client,
    #[cfg(target_os = "linux")]
    auxiliary: Option<Auxiliary>,
    #[cfg(windows)]
    job: Option<crate::platform::windows::Job>,
}
impl Default for Session {
    fn default() -> Self {
        Self::new(std::env::current_exe().expect("application executable path"))
    }
}
impl Session {
    /// A session driven by a recorded event list instead of a worker, for the
    /// native capture entry point. The events are replayed through the normal
    /// `poll` path, so what the capture shows is what those events produce —
    /// not a second rendering of them.
    #[cfg(feature = "capture")]
    pub fn for_capture(events: Vec<control::Event>) -> Self {
        let (sender, receiver) = mpsc::channel();
        for event in events {
            sender.send(event).expect("capture fixture channel");
        }
        let mut session = Self::new(std::path::PathBuf::new());
        session.events = Some(receiver);
        session.capture_fixture = Some(sender);
        // A capture must look connected: the quality controls and the Apply
        // button are only reachable while a session runs.
        session.state = State::Connecting;
        session
    }
    /// Tell the worker where to publish its preview; call before `connect`.
    pub fn set_preview_path(&mut self, path: Option<std::path::PathBuf>) {
        self.preview_path = path;
    }

    pub fn new(executable: std::path::PathBuf) -> Self {
        Self {
            executable,
            child: None,
            events: None,
            stop: Arc::new(AtomicBool::new(false)),
            pending: Arc::new(Mutex::new(Pending::default())),
            deadline: None,
            quality_deadline: None,
            writer: None,
            #[cfg(feature = "capture")]
            capture_fixture: None,
            preview_path: None,
            pairing_info: None,
            state: State::Idle,
            stats: None,
            stats_at: None,
            usb_permission: None,
            quality: crate::quality::Client::default(),
            #[cfg(target_os = "linux")]
            auxiliary: None,
            #[cfg(windows)]
            job: None,
        }
    }
}
impl Session {
    pub fn running(&self) -> bool {
        // A fixture session has no worker process, but it is running by intent:
        // the quality controls and the Apply path must behave as they do live,
        // otherwise a capture cannot show the state those events produce.
        #[cfg(feature = "capture")]
        if self.capture_fixture.is_some() {
            return true;
        }
        #[cfg(target_os = "linux")]
        if self.auxiliary.is_some() {
            return true;
        }
        self.child.is_some()
    }
    pub fn start(
        &mut self,
        settings: Settings,
        device: Option<DeviceId>,
        ctx: egui::Context,
    ) -> crate::Result<()> {
        if self.running() {
            return Err("session is already running".into());
        }
        self.stop = Arc::new(AtomicBool::new(false));
        self.pending = Arc::new(Mutex::new(Pending::default()));
        self.stats = None;
        self.stats_at = None;
        self.usb_permission = None;
        self.quality = crate::quality::Client::new(settings.quality_wish());
        self.quality_deadline = None;
        let mut command = std::process::Command::new(&self.executable);
        command
            .args(["--worker", &std::process::id().to_string()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        if let Some(path) = &self.preview_path {
            command.env(crate::preview::PATH_ENV, path);
        }
        let mut child = command.spawn()?;
        #[cfg(windows)]
        {
            match crate::platform::windows::Job::assign(&child) {
                Ok(job) => self.job = Some(job),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error);
                }
            }
        }
        let mut stdin = child.stdin.take().ok_or("worker input unavailable")?;
        let stdout = child.stdout.take().ok_or("worker output unavailable")?;
        let (sender, receiver) = mpsc::sync_channel(32);
        let stop = self.stop.clone();
        let pending = self.pending.clone();
        let writer = std::thread::spawn(move || {
            let start = Command::Start {
                settings: Box::new(settings),
                device,
            };
            if control::write(&mut stdin, &start).is_err() {
                return;
            }
            while !stop.load(Ordering::Acquire) {
                let message = pending.lock().unwrap().take();
                if let Some(message) = message {
                    if control::write(&mut stdin, &message).is_err() {
                        return;
                    }
                    continue;
                }
                std::thread::park();
            }
            let _ = control::write(&mut stdin, &Command::Stop {});
        });
        self.writer = Some(writer.thread().clone());
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut stopped = false;
            loop {
                match control::read::<Event>(&mut reader) {
                    Ok(Some(event)) => {
                        stopped |= matches!(event, Event::Stopped {});
                        if sender.send(event).is_err() {
                            return;
                        }
                        ctx.request_repaint();
                    }
                    Ok(None) => break,
                    Err(e) => {
                        let _ = sender.send(Event::Error {
                            message: e.to_string(),
                        });
                        break;
                    }
                }
            }
            if !stopped {
                let _ = sender.send(Event::Error {
                    message: "Receiver stopped unexpectedly".into(),
                });
            }
            let _ = sender.send(Event::Stopped {});
            ctx.request_repaint();
        });
        self.child = Some(child);
        self.events = Some(receiver);
        self.state = State::Connecting;
        Ok(())
    }
    fn queue_quality(&self, mode: crate::quality::Mode) {
        self.pending.lock().unwrap().quality = Some(Command::PrepareQuality {
            width: mode.width,
            height: mode.height,
            fps: mode.fps,
        });
        if let Some(writer) = &self.writer {
            writer.unpark();
        }
    }
    /// Resolve Auto against this connection; only concrete tuples cross IPC.
    /// Accept the wish, including an already-confirmed target with no new RPC.
    pub fn prepare_quality(&mut self, width: u16, height: u16, fps: u16) -> bool {
        let wish = crate::quality::Wish::from((width, height, fps));
        if !self.running()
            || matches!(self.state, State::Stopping | State::Failed(_))
            || self.quality.pending.is_some()
            || !self.quality.ready()
            || self.quality.resolve(wish).is_none()
        {
            return false;
        }
        if let Some(mode) = self.quality.begin(wish) {
            self.quality_deadline = Some(apply_expiry());
            self.queue_quality(mode);
        }
        true
    }

    pub fn transform(&mut self, s: &Settings) {
        #[cfg(target_os = "linux")]
        if let Some(aux) = &mut self.auxiliary {
            if let Some((settings, _, _)) = &mut aux.restart {
                settings.rotation = s.rotation;
                settings.mirror = s.mirror;
                settings.vertical_flip = s.vertical_flip;
            }
            return;
        }
        if self.running() {
            self.pending.lock().unwrap().transform = Some(Command::Transform {
                rotation: s.rotation,
                mirror: s.mirror,
                vertical_flip: s.vertical_flip,
            });
            if let Some(writer) = &self.writer {
                writer.unpark();
            }
        }
    }
    pub fn stop(&mut self, ctx: &egui::Context) {
        self.quality_deadline = None;
        self.quality.pending = None;
        self.quality.staged = false;
        #[cfg(target_os = "linux")]
        if let Some(aux) = &mut self.auxiliary {
            aux.restart = None;
            let _ = aux.child.kill();
            self.state = State::Stopping;
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        if self.running() {
            self.stop.store(true, Ordering::Release);
            if let Some(writer) = &self.writer {
                writer.unpark();
            }
            self.deadline = Some(Instant::now() + Duration::from_secs(1));
            self.state = State::Stopping;
            ctx.request_repaint_after(Duration::from_secs(1));
        }
    }
    /// Take the pairing details the phone sent, if any arrived since the last call.
    pub fn take_pairing_info(&mut self) -> Option<crate::wire::PairingInfo> {
        self.pairing_info.take()
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        #[cfg(target_os = "linux")]
        if self.auxiliary.is_some() {
            let aux = self.auxiliary.as_mut().unwrap();
            match aux.child.try_wait() {
                Ok(Some(status)) => {
                    let aux = self.auxiliary.take().unwrap();
                    if status.success() || matches!(self.state, State::Stopping) {
                        self.state = State::Idle;
                        if let Some((settings, device, switched)) = aux.restart {
                            let device = if switched {
                                device
                                    .ok_or_else(|| "missing switched USB device".into())
                                    .and_then(|device| crate::usb::switched_identity(&device))
                                    .map(Some)
                            } else {
                                Ok(device)
                            };
                            match device
                                .and_then(|device| self.start(settings, device, ctx.clone()))
                            {
                                Ok(()) => {}
                                Err(error) => self.state = State::Failed(error.to_string()),
                            }
                        }
                    } else {
                        self.state = State::Failed(format!(
                            "USB/setup request failed ({status}): {}",
                            aux.error.lock().unwrap()
                        ));
                    }
                }
                Err(e) => {
                    self.auxiliary.take();
                    self.state = State::Failed(e.to_string());
                }
                Ok(None) => ctx.request_repaint_after(Duration::from_millis(100)),
            }
            return;
        }
        let mut finished = false;
        if let Some(receiver) = &self.events {
            while let Ok(event) = receiver.try_recv() {
                let stopping = matches!(self.state, State::Stopping) || finished;
                match &event {
                    Event::PairingInfo {
                        addresses,
                        fingerprint,
                        key,
                    } if !stopping => {
                        // Rebuild the typed form from the event so the wire struct stays
                        // the only place that knows the payload layout.
                        let parsed: Vec<std::net::Ipv4Addr> = addresses
                            .iter()
                            .filter_map(|address| address.parse().ok())
                            .collect();
                        if !parsed.is_empty()
                            && fingerprint.len() == 64
                            && key.len() == 32
                            && crate::secure::is_hex(fingerprint, 32)
                            && crate::secure::is_hex(key, 16)
                        {
                            self.pairing_info = Some(crate::wire::PairingInfo {
                                addresses: parsed,
                                fingerprint: fingerprint.clone(),
                                key: key.clone(),
                            });
                        }
                    }
                    Event::Connecting {} if !stopping => self.state = State::Connecting,
                    Event::Waiting {} if !stopping => self.state = State::Waiting,
                    Event::AuthorizationPending { client_id } if !stopping => {
                        if client_id.len() == 32
                            && client_id.bytes().all(|byte| byte.is_ascii_hexdigit())
                        {
                            self.state = State::ApprovalPending(client_id.clone());
                        } else {
                            self.state = State::Failed("receiver reported an invalid PC ID".into());
                            finished = true;
                        }
                    }
                    Event::Stats {
                        active,
                        submitted,
                        width,
                        height,
                        target_fps,
                        ..
                    } if !stopping => {
                        // Old-epoch output can share the requested tuple.
                        // Stats must not complete or label a pending apply.
                        if *active && *submitted > 0 && self.quality.pending.is_none() {
                            self.quality.applied =
                                Some((*width as u16, *height as u16, *target_fps as u16));
                        }
                        self.state = if *active && *submitted > 0 {
                            State::Streaming
                        } else {
                            State::Waiting
                        };
                        self.stats = Some(event);
                        self.stats_at = Some(Instant::now());
                        ctx.request_repaint_after(Duration::from_secs(3));
                    }
                    Event::Error { message } if !stopping => {
                        self.state = State::Failed(message.clone())
                    }
                    Event::UsbPermission { device } if !stopping => {
                        self.usb_permission = Some(device.clone())
                    }
                    Event::QualityCapabilities {
                        permission, modes, ..
                    } if !stopping => {
                        if let Some(mode) = self.quality.advertise(*permission, modes.clone()) {
                            self.quality_deadline = Some(apply_expiry());
                            self.queue_quality(mode);
                        }
                    }
                    Event::QualityProgress { staged } if !stopping => {
                        let was_staged = self.quality.staged;
                        self.quality.progress(*staged);
                        if *staged {
                            self.quality_deadline = None;
                        } else if was_staged && self.quality.pending.is_some() {
                            self.quality_deadline = Some(apply_expiry());
                        }
                    }
                    Event::QualityApplied { width, height, fps } if !stopping => {
                        self.quality_deadline = None;
                        if !self.quality.confirmed(*width, *height, *fps) {
                            self.state =
                                State::Failed("receiver confirmed an unexpected quality".into());
                            finished = true;
                        }
                    }
                    Event::QualityFailed { reason, terminal } if !stopping => {
                        self.quality_deadline = None;
                        self.quality.failed(reason.clone());
                        if *terminal {
                            self.state = State::Failed(reason.clone());
                            finished = true;
                        }
                    }
                    Event::Stopped {} => finished = true,
                    _ => {}
                }
            }
        }
        if matches!(self.state, State::Streaming)
            && self
                .stats_at
                .is_some_and(|time| time.elapsed() >= Duration::from_secs(3))
        {
            self.state = State::Waiting;
        }
        // The parent can enforce the UI transaction deadline even when native
        // decoder/output work traps the worker before its next protocol poll.
        if self
            .quality_deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.quality_deadline = None;
            self.quality
                .failed("the quality apply deadline expired".into());
            self.state = State::Failed("the quality apply deadline expired".into());
            finished = true;
        }
        if self.deadline.is_some_and(|d| Instant::now() >= d) {
            if let Some(child) = &mut self.child {
                let _ = child.kill();
            }
            finished = true;
        }
        let mut reaped = self.child.is_none();
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    reaped = true;
                    if !status.success()
                        && !matches!(self.state, State::Failed(_) | State::Stopping)
                    {
                        self.state = State::Failed(format!("receiver exited ({status})"));
                    }
                    finished = true;
                }
                Err(e) => {
                    self.state = State::Failed(e.to_string());
                    finished = true;
                }
                _ => {}
            }
        }
        if finished {
            self.quality_deadline = None;
            self.quality.disconnected();
            self.stop.store(true, Ordering::Release);
            if let Some(writer) = self.writer.take() {
                writer.unpark();
            }
            self.events = None;
            if !reaped {
                if let Some(child) = &mut self.child {
                    let _ = child.kill();
                }
                if !matches!(self.state, State::Failed(_)) {
                    self.state = State::Stopping;
                }
                self.deadline = Some(Instant::now() + Duration::from_millis(100));
                ctx.request_repaint_after(Duration::from_millis(100));
                return;
            }
            self.child = None;
            self.deadline = None;
            #[cfg(windows)]
            {
                self.job = None;
            }
            if !matches!(self.state, State::Failed(_)) {
                self.state = State::Idle;
            }
        } else if let Some(deadline) = self.deadline {
            ctx.request_repaint_after(deadline.saturating_duration_since(Instant::now()));
        }
        if let Some(deadline) = self.quality_deadline {
            ctx.request_repaint_after(deadline.saturating_duration_since(Instant::now()));
        }
    }
}
fn apply_expiry() -> Instant {
    Instant::now() + Duration::from_millis(crate::quality::APPLY_DEADLINE_MS)
}
impl Drop for Session {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(writer) = self.writer.take() {
            writer.unpark();
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        #[cfg(target_os = "linux")]
        if let Some(mut aux) = self.auxiliary.take()
            && aux.child.kill().is_ok()
        {
            std::thread::spawn(move || {
                let _ = aux.child.wait();
            });
        }
    }
}

#[cfg(target_os = "linux")]
struct Auxiliary {
    child: Child,
    restart: Option<(Settings, Option<DeviceId>, bool)>,
    error: Arc<Mutex<String>>,
}
#[cfg(target_os = "linux")]
fn embedded_setup_script() -> crate::Result<String> {
    const SCRIPT: &str = include_str!("../../linux/install-host-integration.sh");
    const RULES: &str = include_str!("../linux/70-mobile-webcam.rules");
    const MODPROBE: &str = include_str!("../../linux/mobile-webcam-v4l2loopback.conf");
    const MODULES: &str = include_str!("../../linux/mobile-webcam-modules-load.conf");
    const TERMINATOR: &str = "MOBILE_WEBCAM_ASSET_END";
    for asset in [RULES, MODPROBE, MODULES] {
        if asset.lines().any(|line| line == TERMINATOR) || asset.contains("__MOBILE_WEBCAM_") {
            return Err("host setup asset is not safe to embed".into());
        }
    }
    let script = SCRIPT
        .replace("__MOBILE_WEBCAM_UDEV_RULES__", RULES.trim_end_matches('\n'))
        .replace(
            "__MOBILE_WEBCAM_MODPROBE_CONF__",
            MODPROBE.trim_end_matches('\n'),
        )
        .replace(
            "__MOBILE_WEBCAM_MODULES_CONF__",
            MODULES.trim_end_matches('\n'),
        );
    for placeholder in [
        "__MOBILE_WEBCAM_UDEV_RULES__",
        "__MOBILE_WEBCAM_MODPROBE_CONF__",
        "__MOBILE_WEBCAM_MODULES_CONF__",
    ] {
        if script.contains(placeholder) {
            return Err("host setup script is missing embedded assets".into());
        }
    }
    Ok(script)
}

#[cfg(target_os = "linux")]
fn validated_device_json(device: &DeviceId) -> crate::Result<String> {
    if device.ports.len() > 32 {
        return Err("Invalid device identity.".into());
    }
    let ports = device
        .ports
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let json = format!(
        "{{\"bus\":{},\"address\":{},\"vid\":{},\"pid\":{},\"ports\":[{ports}]}}",
        device.bus, device.address, device.vid, device.pid
    );
    let parsed: DeviceId = serde_json::from_str(&json)?;
    if parsed != *device {
        return Err("Invalid device identity.".into());
    }
    Ok(json)
}

/// Absolute path of the packaged USB helper, when this build ships one.
/// Symbolic links are refused so a path next to the executable cannot redirect
/// the privileged copy.
#[cfg(target_os = "linux")]
fn packaged_usb_helper(executable: &std::path::Path) -> crate::Result<Option<String>> {
    let Some(parent) = executable.parent() else {
        return Ok(None);
    };
    let path = parent.join("../share/mobile-webcam/linux/mobile-webcam-usb");
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !meta.file_type().is_file() {
        return Err("Invalid USB helper path.".into());
    }
    let canonical = path.canonicalize()?;
    let Some(text) = canonical.to_str() else {
        return Err("Invalid USB helper path.".into());
    };
    let named = canonical.file_name().and_then(|name| name.to_str()) == Some("mobile-webcam-usb");
    let rooted = text.starts_with('/') && !text.contains('\n') && !text.contains('\0');
    let plain = !canonical.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::CurDir
        )
    });
    if !named || !rooted || !plain {
        return Err("Invalid USB helper path.".into());
    }
    Ok(Some(text.to_string()))
}

/// `pkexec /bin/sh -s` with the embedded setup script. The on-disk script is
/// not an argument: AppImage contents live on a user-writable path.
#[cfg(target_os = "linux")]
fn host_setup_invocation(
    executable: &std::path::Path,
    parent: u32,
    device: Option<&DeviceId>,
) -> crate::Result<(std::process::Command, String)> {
    if parent == 0 {
        return Err("Invalid parent PID.".into());
    }
    let helper = packaged_usb_helper(executable)?;
    if device.is_some() && helper.is_none() {
        return Err("USB helper is not included in this build.".into());
    }
    let mut command = std::process::Command::new("pkexec");
    command.args(["--disable-internal-agent", "/bin/sh", "-s", "--"]);
    if let Some(helper) = helper {
        command.arg("--helper").arg(helper);
    }
    if let Some(device) = device {
        command
            .arg("--switch-parent")
            .arg(parent.to_string())
            .arg("--device-json")
            .arg(validated_device_json(device)?);
    }
    Ok((command, embedded_setup_script()?))
}

#[cfg(target_os = "linux")]
impl Session {
    fn auxiliary(
        &mut self,
        mut command: std::process::Command,
        message: Option<&DeviceId>,
        script: Option<&str>,
        restart: Option<(Settings, Option<DeviceId>, bool)>,
        ctx: egui::Context,
    ) -> crate::Result<()> {
        use std::io::{Read, Write};
        if self.running() {
            return Err("session is already running".into());
        }
        if script.is_some() && message.is_some() {
            return Err("host setup cannot share standard input".into());
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;
        if let Some(script) = script {
            let mut input = child.stdin.take().ok_or("helper input unavailable")?;
            if let Err(error) = input.write_all(script.as_bytes()) {
                let _ = child.kill();
                return Err(error.into());
            }
        } else if let Some(message) = message {
            let result = control::write(
                &mut child.stdin.take().ok_or("helper input unavailable")?,
                message,
            );
            if let Err(error) = result {
                let _ = child.kill();
                return Err(error.into());
            }
        }
        let mut stderr = child
            .stderr
            .take()
            .ok_or("helper diagnostics unavailable")?;
        let error = Arc::new(Mutex::new(String::new()));
        let thread_error = error.clone();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let mut buffer = [0; 1024];
            while let Ok(n) = stderr.read(&mut buffer) {
                if n == 0 {
                    break;
                }
                let count = n.min(2048 - bytes.len());
                bytes.extend_from_slice(&buffer[..count]);
            }
            *thread_error.lock().unwrap() = String::from_utf8_lossy(&bytes).into();
            ctx.request_repaint();
        });
        self.usb_permission = None;
        self.auxiliary = Some(Auxiliary {
            child,
            restart,
            error,
        });
        self.state = State::Connecting;
        Ok(())
    }
    pub fn allow_usb(&mut self, settings: Settings, ctx: egui::Context) -> crate::Result<()> {
        let device = self
            .usb_permission
            .clone()
            .ok_or("USB permission was not requested")?;
        if crate::usb::accessory(device.vid, device.pid) {
            return self.setup(settings, Some(device), ctx);
        }
        let helper = std::path::Path::new("/usr/lib/mobile-webcam/mobile-webcam-usb");
        if !helper.is_file() {
            return self.setup(settings, Some(device), ctx);
        }
        let mut command = std::process::Command::new("pkexec");
        command.arg("--disable-internal-agent").arg(helper).args([
            "--switch",
            "--parent",
            &std::process::id().to_string(),
        ]);
        self.auxiliary(
            command,
            Some(&device),
            None,
            Some((settings, Some(device.clone()), true)),
            ctx,
        )
    }
    fn setup_available(&self) -> bool {
        // Packaging still ships the script next to the executable. Its presence
        // means this build includes host integration. pkexec does not run it.
        self.setup_script().is_file()
    }
    fn setup_script(&self) -> std::path::PathBuf {
        self.executable
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("../share/mobile-webcam/linux/install-host-integration.sh")
    }
    pub fn connect(
        &mut self,
        settings: Settings,
        device: Option<DeviceId>,
        ctx: egui::Context,
    ) -> crate::Result<()> {
        settings.validate()?;
        let device = device.filter(|_| settings.transport == "usb");
        if settings.transport == "usb" && device.is_none() {
            return Err("select a USB device".into());
        }
        if settings.output.trim() == "/dev/video10"
            && !std::ffi::CString::new(settings.output.trim())
                .is_ok_and(|path| unsafe { libc::access(path.as_ptr(), libc::W_OK) } == 0)
            && self.setup_available()
        {
            return self.setup(settings, device, ctx);
        }
        self.start(settings, device, ctx)
    }
    fn setup(
        &mut self,
        settings: Settings,
        device: Option<DeviceId>,
        ctx: egui::Context,
    ) -> crate::Result<()> {
        if !self.setup_available() {
            return Err("host setup is not included in this build".into());
        }
        let switched = match &device {
            Some(id) if !crate::usb::accessory(id.vid, id.pid) => crate::usb::access_denied(id)?,
            _ => false,
        };
        let (command, script) = host_setup_invocation(
            &self.executable,
            std::process::id(),
            if switched { device.as_ref() } else { None },
        )?;
        self.auxiliary(
            command,
            None,
            Some(&script),
            Some((settings, device.clone(), switched)),
            ctx,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transform_coalescing_cannot_overwrite_a_quality_command() {
        let mut pending = Pending {
            quality: Some(Command::PrepareQuality {
                width: 1920,
                height: 1080,
                fps: 30,
            }),
            ..Default::default()
        };
        for rotation in [90, 180] {
            pending.transform = Some(Command::Transform {
                rotation,
                mirror: false,
                vertical_flip: false,
            });
        }
        assert!(matches!(
            pending.take(),
            Some(Command::PrepareQuality {
                width: 1920,
                height: 1080,
                fps: 30
            })
        ));
        assert!(matches!(
            pending.take(),
            Some(Command::Transform {
                rotation: 180,
                mirror: false,
                vertical_flip: false
            })
        ));
        assert!(pending.take().is_none());
    }
    #[cfg(unix)]
    #[test]
    fn parent_watchdog_terminates_a_live_receiver_that_stops_reporting() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("receiver");
        let event = Event::QualityCapabilities {
            permission: crate::wire::PERMISSION_READY,
            revision: 7,
            epoch: 0,
            modes: vec![crate::quality::Mode {
                width: 1280,
                height: 720,
                fps: 30,
                bitrate: 1,
            }],
        };
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\nprintf '%s\\n' '{}'\nexec sleep 30\n",
                serde_json::to_string(&event).unwrap()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut session = Session::new(path);
        let ctx = egui::Context::default();
        session
            .start(Settings::default(), None, ctx.clone())
            .unwrap();
        let pid = session.child.as_ref().unwrap().id();
        let startup = Instant::now() + Duration::from_secs(2);
        while session.quality.pending.is_none() {
            session.poll(&ctx);
            assert!(Instant::now() < startup);
            std::thread::sleep(Duration::from_millis(5));
        }
        session.quality_deadline = Some(Instant::now());
        let closed = Instant::now() + Duration::from_secs(2);
        while session.running() {
            session.poll(&ctx);
            assert!(Instant::now() < closed);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(matches!(&session.state, State::Failed(reason) if reason.contains("deadline")));
        // The state transition is not sufficient: the owned process must exit.
        assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
    #[test]
    fn parent_watchdog_clears_pending_when_the_receiver_cannot_report() {
        let mut session = Session::new("unused".into());
        session.quality.advertise(
            crate::wire::PERMISSION_READY,
            vec![crate::quality::Mode {
                width: 1280,
                height: 720,
                fps: 30,
                bitrate: 1,
            }],
        );
        session.quality_deadline = Some(Instant::now());
        session.poll(&egui::Context::default());
        assert!(session.quality.pending.is_none());
        assert!(session.quality.modes.is_none());
        assert!(matches!(&session.state, State::Failed(reason) if reason.contains("deadline")));
    }
    #[test]
    fn active_boundary_progress_does_not_renew_the_apply_budget() {
        let mut session = Session::new("unused".into());
        session.quality.advertise(
            crate::wire::PERMISSION_READY,
            vec![crate::quality::Mode {
                width: 1280,
                height: 720,
                fps: 30,
                bitrate: 1,
            }],
        );
        let deadline = Instant::now() + Duration::from_secs(60);
        session.quality_deadline = Some(deadline);
        let (sender, receiver) = mpsc::channel();
        session.events = Some(receiver);
        sender
            .send(Event::QualityProgress { staged: false })
            .unwrap();
        session.poll(&egui::Context::default());
        assert_eq!(session.quality_deadline, Some(deadline));
    }
    #[test]
    fn staged_human_wait_disarms_watchdog_but_start_progress_rearms_it() {
        let mut session = Session::new("unused".into());
        session.quality.advertise(
            crate::wire::PERMISSION_READY,
            vec![crate::quality::Mode {
                width: 1280,
                height: 720,
                fps: 30,
                bitrate: 1,
            }],
        );
        let (sender, receiver) = mpsc::channel();
        session.events = Some(receiver);
        session.quality_deadline = Some(Instant::now());
        sender
            .send(Event::QualityProgress { staged: true })
            .unwrap();
        let ctx = egui::Context::default();
        session.poll(&ctx);
        assert!(session.quality.staged);
        assert!(session.quality.pending.is_some());
        assert!(session.quality_deadline.is_none());
        sender
            .send(Event::QualityProgress { staged: false })
            .unwrap();
        session.poll(&ctx);
        assert!(!session.quality.staged);
        assert!(session.quality_deadline.is_some());
    }
    #[test]
    fn matching_old_stream_stats_do_not_label_the_pending_request_applied() {
        let mut session = Session::new("unused".into());
        session.quality.advertise(
            crate::wire::PERMISSION_READY,
            vec![crate::quality::Mode {
                width: 1280,
                height: 720,
                fps: 30,
                bitrate: 1,
            }],
        );
        let (sender, receiver) = mpsc::channel();
        session.events = Some(receiver);
        sender
            .send(Event::Stats {
                decoded: 1,
                submitted: 1,
                active: true,
                width: 1280,
                height: 720,
                target_fps: 30,
                decoder: "software".into(),
            })
            .unwrap();
        let ctx = egui::Context::default();
        session.poll(&ctx);
        assert!(session.quality.pending.is_some());
        assert!(session.quality.applied.is_none());
        sender
            .send(Event::QualityApplied {
                width: 1280,
                height: 720,
                fps: 30,
            })
            .unwrap();
        session.poll(&ctx);
        assert!(session.quality.pending.is_none());
        assert_eq!(session.quality.applied, Some((1280, 720, 30)));
    }
    /// The capture fixtures drive the real session through the normal `poll`
    /// path, so what a screenshot shows is what these events produce. These
    /// assert the resulting UI-visible state; the pixel capture is a separate
    /// step (`MW_SCREENSHOT`) because it needs a display the process can draw to.
    #[cfg(all(test, feature = "capture"))]
    mod capture {
        use super::*;

        /// The seam itself: a fixture session must report as running, or the
        /// quality controls and Apply path stay dead and the capture cannot show
        /// what the events produce.
        #[test]
        fn a_fixture_session_reports_itself_running() {
            let session = replay(fixture("streaming.json"));
            assert!(
                session.running(),
                "a fixture session must report as running"
            );
        }

        fn replay(events: Vec<control::Event>) -> Session {
            let mut session = Session::for_capture(events);
            let ctx = egui::Context::default();
            for _ in 0..10 {
                session.poll(&ctx);
            }
            session
        }

        fn fixture(name: &str) -> Vec<control::Event> {
            // `capture/` holds the fixtures the GUI screenshots are rendered from;
            // `session/` holds ones that only drive session behaviour, because they
            // render the same frame as a sibling and would make the capture gate's
            // "two states must differ" rule fire on a state that legitimately looks
            // identical (an apply to the mode already streaming).
            let path = ["tests/fixtures/capture", "tests/fixtures/session"]
                .into_iter()
                .map(|directory| format!("{directory}/{name}"))
                .find(|candidate| std::path::Path::new(candidate).exists())
                .unwrap_or_else(|| format!("tests/fixtures/capture/{name}"));
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("fixture {name}: {error}"));
            serde_json::from_str(&text).unwrap_or_else(|error| panic!("fixture {name}: {error}"))
        }

        /// The live stream's own tuple is what "applied" means before any
        /// switch: Stats reports what the phone is actually sending.
        #[test]
        fn a_stream_already_at_the_wished_mode_needs_no_apply() {
            let session = replay(fixture("streaming.json"));
            let modes = session
                .quality
                .modes
                .clone()
                .expect("modes were not advertised");
            assert_eq!(modes.len(), 3);
            assert!(session.quality.ready());
            assert_eq!(session.quality.applied, Some((1280, 720, 30)));
            assert!(
                session.quality.pending.is_none(),
                "an already-satisfied wish must not apply"
            );
        }

        /// The phone is at 1080p while the saved wish is Auto, so the initial
        /// apply is in flight: pending, not applied.
        #[test]
        fn a_different_stream_mode_leaves_the_initial_wish_pending() {
            let session = replay(fixture("pending.json"));
            assert_eq!(
                session.quality.pending.map(crate::quality::Mode::tuple),
                Some((1280, 720, 30))
            );
            assert_eq!(session.quality.applied, Some((1920, 1080, 30)));
            assert!(!session.quality.staged);
        }

        #[test]
        fn a_confirmed_apply_moves_the_confirmed_target() {
            let session = replay(fixture("applied.json"));
            assert_eq!(session.quality.applied, Some((1280, 720, 30)));
            assert!(session.quality.pending.is_none());
            assert!(session.quality.error.is_none());
        }

        #[test]
        fn a_rejected_apply_keeps_the_reason_and_the_previous_stream() {
            let session = replay(fixture("failed.json"));
            assert!(
                session
                    .quality
                    .error
                    .as_deref()
                    .is_some_and(|error| error.contains("not supported"))
            );
            assert!(session.quality.pending.is_none());
            assert!(
                session.stats.is_some(),
                "the stream must survive a failed apply"
            );
        }

        #[test]
        fn permission_pending_advertises_nothing_usable() {
            let session = replay(fixture("permission.json"));
            assert_eq!(
                session.quality.permission,
                Some(crate::wire::PERMISSION_PENDING)
            );
            assert!(!session.quality.ready());
            assert!(session.quality.pending.is_none());
        }

        #[test]
        fn an_old_app_streams_without_advertising_modes() {
            let session = replay(fixture("oldapp.json"));
            assert!(session.quality.modes.is_none());
            assert!(session.stats.is_some());
        }
    }

    #[test]
    fn video_pause_waits_and_fresh_frames_resume() {
        let ctx = egui::Context::default();
        let mut session = Session::new("unused".into());
        let (sender, receiver) = mpsc::channel();
        session.events = Some(receiver);
        let stats = Event::Stats {
            decoded: 10,
            submitted: 10,
            active: true,
            width: 64,
            height: 32,
            target_fps: 30,
            decoder: "software".into(),
        };
        sender.send(stats.clone()).unwrap();
        session.poll(&ctx);
        assert!(matches!(session.state, State::Streaming));
        session.stats_at = Some(Instant::now() - Duration::from_secs(4));
        session.poll(&ctx);
        assert!(matches!(session.state, State::Waiting));
        sender.send(stats).unwrap();
        session.poll(&ctx);
        assert!(matches!(session.state, State::Streaming));
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn cancelled_helper_returns_to_idle() {
        let ctx = egui::Context::default();
        let mut session = Session::new("unused".into());
        let mut command = std::process::Command::new("sleep");
        command.arg("30");
        session
            .auxiliary(
                command,
                None,
                None,
                Some((Settings::default(), None, false)),
                ctx.clone(),
            )
            .unwrap();
        session.stop(&ctx);
        let started = Instant::now();
        while session.running() {
            session.poll(&ctx);
            assert!(started.elapsed() < Duration::from_millis(1500));
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(matches!(session.state, State::Idle));
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn setup_resumes_saved_connection_and_failure_does_not_start_worker() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("worker");
        std::fs::write(
            &executable,
            b"#!/bin/sh\nIFS= read -r start\nprintf '%s\\n' \"$start\" > \"$0.json\"\nprintf '{\"event\":\"stopped\"}\\n'\n",
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        let ctx = egui::Context::default();
        let settings = Settings {
            transport: "lan".into(),
            lan_host: "192.168.1.42".into(),
            ..Default::default()
        };
        let mut session = Session::new(executable);
        session
            .auxiliary(
                std::process::Command::new("true"),
                None,
                None,
                Some((settings.clone(), None, false)),
                ctx.clone(),
            )
            .unwrap();
        let mut latest = settings;
        latest.rotation = 90;
        session.transform(&latest);
        let started = Instant::now();
        while session.running() {
            session.poll(&ctx);
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "{:?}",
                session.state
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let captured = directory.path().join("worker.json");
        let command =
            control::read::<Command>(&mut BufReader::new(std::fs::File::open(&captured).unwrap()))
                .unwrap()
                .unwrap();
        match command {
            Command::Start { settings, device } => {
                assert_eq!(*settings, latest);
                assert_eq!(device, None);
            }
            other => panic!("unexpected worker command: {other:?}"),
        }
        std::fs::remove_file(&captured).unwrap();
        session
            .auxiliary(
                std::process::Command::new("false"),
                None,
                None,
                Some((latest, None, false)),
                ctx.clone(),
            )
            .unwrap();
        let started = Instant::now();
        while session.running() {
            session.poll(&ctx);
            assert!(started.elapsed() < Duration::from_secs(2));
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(matches!(session.state, State::Failed(_)));
        assert!(!captured.exists());
    }

    #[cfg(target_os = "linux")]
    fn shell_output(script: &str, args: &[&str]) -> std::process::Output {
        use std::io::Write;
        let mut child = std::process::Command::new("/bin/sh");
        child
            .args(["-s", "--"])
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = child.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(script.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    #[cfg(target_os = "linux")]
    fn assert_shell_failure(output: &std::process::Output, expected: &str) {
        assert!(
            !output.status.success(),
            "setup script succeeded: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(expected),
            "stderr {stderr:?} does not contain {expected:?}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn setup_feeds_pkexec_an_embedded_script_not_a_sibling_path() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("My Webcam");
        let executable_dir = root.join("usr/bin");
        let share = root.join("usr/share/mobile-webcam/linux");
        std::fs::create_dir_all(&executable_dir).unwrap();
        std::fs::create_dir_all(&share).unwrap();
        let executable = executable_dir.join("mobile-webcam");
        std::fs::write(&executable, b"not-executed").unwrap();
        let planted = share.join("install-host-integration.sh");
        std::fs::write(&planted, b"#!/bin/sh\necho pwned\n").unwrap();
        std::fs::set_permissions(&planted, std::fs::Permissions::from_mode(0o755)).unwrap();
        let helper = share.join("mobile-webcam-usb");
        std::fs::write(&helper, b"#!/bin/sh\necho helper\n").unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();

        let device = crate::usb::DeviceId {
            bus: 1,
            address: 4,
            vid: 0x18d1,
            pid: 0x4ee2,
            ports: vec![2, 1],
        };
        let json = super::validated_device_json(&device).unwrap();
        assert_eq!(json, serde_json::to_string(&device).unwrap());
        assert!(
            super::validated_device_json(&crate::usb::DeviceId {
                ports: vec![1; 33],
                ..device.clone()
            })
            .is_err()
        );
        assert!(super::host_setup_invocation(&executable, 0, None).is_err());

        let (command, script) =
            super::host_setup_invocation(&executable, 4242, Some(&device)).unwrap();
        assert_eq!(command.get_program(), "pkexec");
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_str().unwrap().to_string())
            .collect();
        let helper_path = helper.canonicalize().unwrap();
        let helper_text = helper_path.to_str().unwrap();
        assert_eq!(
            args,
            vec![
                "--disable-internal-agent",
                "/bin/sh",
                "-s",
                "--",
                "--helper",
                helper_text,
                "--switch-parent",
                "4242",
                "--device-json",
                json.as_str(),
            ]
        );
        assert!(
            !args
                .iter()
                .any(|arg| arg.contains("install-host-integration"))
        );
        assert!(!args.iter().any(|arg| arg.contains("pwned")));
        assert!(helper_text.contains("My Webcam"));
        assert!(script.contains("Refusing to replace modified"));
        assert!(script.contains("Invalid parent PID."));
        assert!(script.contains("Accessory-mode access is independent"));
        assert!(script.contains(
            "options v4l2loopback video_nr=10 card_label=\"Mobile Webcam\" exclusive_caps=1"
        ));
        assert!(script.contains("\nv4l2loopback\n"));
        assert!(!script.contains("__MOBILE_WEBCAM_UDEV_RULES__"));
        assert!(!script.contains("__MOBILE_WEBCAM_MODPROBE_CONF__"));
        assert!(!script.contains("__MOBILE_WEBCAM_MODULES_CONF__"));
        assert!(!script.contains("pwned"));

        let mut syntax = std::process::Command::new("/bin/sh");
        syntax.arg("-n").stdin(Stdio::piped());
        let mut syntax = syntax.spawn().unwrap();
        {
            use std::io::Write;
            syntax
                .stdin
                .take()
                .unwrap()
                .write_all(script.as_bytes())
                .unwrap();
        }
        assert!(syntax.wait().unwrap().success());

        assert_shell_failure(
            &shell_output(
                &script,
                &[
                    "--helper",
                    "/anywhere/mobile-webcam-usb",
                    "--switch-parent",
                    "0",
                    "--device-json",
                    &json,
                ],
            ),
            "Invalid parent PID.",
        );
        assert_shell_failure(
            &shell_output(
                &script,
                &[
                    "--helper",
                    "/anywhere/mobile-webcam-usb",
                    "--switch-parent",
                    "abc",
                    "--device-json",
                    &json,
                ],
            ),
            "Invalid parent PID.",
        );
        assert_shell_failure(
            &shell_output(&script, &["--helper", "relative/mobile-webcam-usb"]),
            "Invalid USB helper path.",
        );
        assert_shell_failure(
            &shell_output(
                &script,
                &[
                    "--helper",
                    "/tmp/../mobile-webcam-usb",
                    "--switch-parent",
                    "9",
                    "--device-json",
                    &json,
                ],
            ),
            "Invalid USB helper path.",
        );
        assert_shell_failure(
            &shell_output(
                &script,
                &[
                    "--helper",
                    helper_text,
                    "--switch-parent",
                    "9",
                    "--device-json",
                    "{\"bus\":1};touch /tmp/pwned",
                ],
            ),
            "Invalid device identity.",
        );

        let marker = directory.path().join("helper-ran");
        let malicious = directory.path().join("mobile-webcam-usb");
        std::fs::write(
            &malicious,
            format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
        )
        .unwrap();
        std::fs::set_permissions(&malicious, std::fs::Permissions::from_mode(0o755)).unwrap();
        let link_dir = directory.path().join("linked");
        std::fs::create_dir_all(&link_dir).unwrap();
        let link = link_dir.join("mobile-webcam-usb");
        std::os::unix::fs::symlink(&malicious, &link).unwrap();
        assert_shell_failure(
            &shell_output(&script, &["--helper", link.to_str().unwrap()]),
            "USB helper is not included",
        );
        let output = shell_output(
            &script,
            &[
                "--helper",
                helper_text,
                "--switch-parent",
                "9",
                "--device-json",
                &json,
            ],
        );
        assert_shell_failure(&output, "Run this installer with sudo.");
        assert!(!marker.exists());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains("pwned"));
        std::fs::remove_file(&helper).unwrap();
        std::os::unix::fs::symlink(&malicious, &helper).unwrap();
        assert!(super::packaged_usb_helper(&executable).is_err());

        let on_disk = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../linux/install-host-integration.sh");
        let file_mode = std::process::Command::new("/bin/sh")
            .arg(&on_disk)
            .output()
            .unwrap();
        assert_shell_failure(&file_mode, "Run this installer with sudo.");
        let bad_pid = std::process::Command::new("/bin/sh")
            .arg(&on_disk)
            .args(["--switch-parent", "0"])
            .output()
            .unwrap();
        assert_shell_failure(&bad_pid, "Invalid parent PID.");
    }
}
