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
            if control::write(&mut stdin, &Command::Start { settings, device }).is_err() {
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
                    Event::Connecting {} if !stopping => self.state = State::Connecting,
                    Event::Waiting {} if !stopping => self.state = State::Waiting,
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
impl Session {
    fn auxiliary(
        &mut self,
        mut command: std::process::Command,
        message: Option<&DeviceId>,
        restart: Option<(Settings, Option<DeviceId>, bool)>,
        ctx: egui::Context,
    ) -> crate::Result<()> {
        use std::io::Read;
        if self.running() {
            return Err("session is already running".into());
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;
        if let Some(message) = message {
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
            Some((settings, Some(device.clone()), true)),
            ctx,
        )
    }
    fn setup_available(&self) -> bool {
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
        let mut command = std::process::Command::new("pkexec");
        command
            .args(["--disable-internal-agent", "/bin/sh"])
            .arg(self.setup_script());
        if switched {
            command.args(["--switch-parent", &std::process::id().to_string()]);
        }
        self.auxiliary(
            command,
            if switched { device.as_ref() } else { None },
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
                assert_eq!(settings, latest);
                assert_eq!(device, None);
            }
            other => panic!("unexpected worker command: {other:?}"),
        }
        std::fs::remove_file(&captured).unwrap();
        session
            .auxiliary(
                std::process::Command::new("false"),
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
}
