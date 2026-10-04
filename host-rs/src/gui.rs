use crate::{
    control::Event,
    session::{Session, State},
    settings::{Settings, Store},
    usb::{self, Candidate, DeviceId},
};
use eframe::egui;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

fn japanese() -> bool {
    sys_locale::get_locale().is_some_and(|l| {
        l.split(['-', '_', '.'])
            .next()
            .is_some_and(|s| s.eq_ignore_ascii_case("ja"))
    })
}
pub fn run() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([420.0, 440.0])
            .with_min_inner_size([360.0, 360.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Mobile Webcam",
        options,
        Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx)))),
    )
}
struct App {
    settings: Settings,
    store: Option<Store>,
    save_error: Option<String>,
    dirty: Option<Instant>,
    session: Session,
    ja: bool,
    devices: Vec<Candidate>,
    selected: Option<DeviceId>,
    selection_lost: bool,
    usb_enabled: Arc<AtomicBool>,
    usb_stop: Arc<AtomicBool>,
    usb_thread: std::thread::Thread,
    usb_events: mpsc::Receiver<crate::Result<Vec<Candidate>>>,
    usb_error: Option<String>,
    show_unknown: bool,
    #[cfg(feature = "capture")]
    capture_requested: bool,
}
impl App {
    fn new(ctx: &egui::Context) -> Self {
        let store = Store::native();
        let mut error = None;
        let (store, settings) = match store {
            Ok(store) => {
                let s = store.load().unwrap_or_else(|e| {
                    error = Some(e.to_string());
                    Settings::default()
                });
                (Some(store), s)
            }
            Err(e) => {
                error = Some(e.to_string());
                (None, Settings::default())
            }
        };
        let mut fonts = egui::FontDefinitions::default();
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let families = [
            fontdb::Family::Name("Noto Sans CJK JP"),
            fontdb::Family::Name("Yu Gothic"),
            fontdb::Family::Name("Meiryo"),
        ];
        if let Some(id) = db.query(&fontdb::Query {
            families: &families,
            ..Default::default()
        }) && let Some((bytes, index)) =
            db.with_face_data(id, |data, index| (data.to_vec(), index))
        {
            let mut font = egui::FontData::from_owned(bytes);
            font.index = index;
            fonts.font_data.insert("system-ja".into(), Arc::new(font));
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .push("system-ja".into());
        }
        ctx.set_fonts(fonts);
        ctx.all_styles_mut(|style| {
            style
                .text_styles
                .insert(egui::TextStyle::Heading, egui::FontId::proportional(22.0));
            style
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
            style
                .text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
        });
        let (sender, receiver) = mpsc::sync_channel(1);
        let enabled = Arc::new(AtomicBool::new(settings.transport == "usb"));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_enabled = enabled.clone();
        let thread_stop = stop.clone();
        let ctx = ctx.clone();
        let inventory = std::thread::spawn(move || {
            let mut previous = String::new();
            while !thread_stop.load(Ordering::Acquire) {
                if thread_enabled.load(Ordering::Acquire) {
                    let list = usb::list();
                    let signature = format!("{list:?}");
                    if signature != previous && sender.try_send(list).is_ok() {
                        previous = signature;
                        ctx.request_repaint();
                    }
                }
                if thread_enabled.load(Ordering::Acquire) {
                    std::thread::park_timeout(Duration::from_secs(2));
                } else {
                    std::thread::park();
                }
            }
        });
        Self {
            settings,
            store,
            save_error: error,
            dirty: None,
            session: Session::default(),
            ja: japanese(),
            devices: Vec::new(),
            selected: None,
            selection_lost: false,
            usb_enabled: enabled,
            usb_stop: stop,
            usb_thread: inventory.thread().clone(),
            usb_events: receiver,
            usb_error: None,
            show_unknown: false,
            #[cfg(feature = "capture")]
            capture_requested: false,
        }
    }
    fn text<'a>(&self, en: &'a str, ja: &'a str) -> &'a str {
        if self.ja { ja } else { en }
    }
    fn decode_name<'a>(&self, mode: &'a str) -> &'a str {
        match mode {
            "auto" => self.text("Auto", "自動"),
            "off" => self.text("Software", "ソフトウェア"),
            _ => mode,
        }
    }
    fn save(&mut self) {
        if let Some(store) = &self.store {
            self.save_error = store.save(&self.settings).err().map(|e| e.to_string());
        }
        self.dirty = None;
    }
}
impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(feature = "capture")]
        if let Some(path) = std::env::var_os("MW_SCREENSHOT") {
            for event in ctx.input(|i| i.events.clone()) {
                if let egui::Event::Screenshot { image, .. } = event {
                    let bytes: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                    image::save_buffer(
                        path.clone(),
                        &bytes,
                        image.size[0] as u32,
                        image.size[1] as u32,
                        image::ColorType::Rgba8,
                    )
                    .expect("save test capture");
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        self.session.poll(ctx);
        while let Ok(result) = self.usb_events.try_recv() {
            match result {
                Ok(devices) => {
                    if self
                        .selected
                        .as_ref()
                        .is_some_and(|id| !devices.iter().any(|c| c.id == *id))
                    {
                        let old = self.selected.as_ref().unwrap();
                        let switched = if self.session.running() {
                            devices.iter().find(|candidate| {
                                candidate.id.bus == old.bus
                                    && candidate.id.ports == old.ports
                                    && usb::accessory(candidate.id.vid, candidate.id.pid)
                            })
                        } else {
                            None
                        };
                        if let Some(candidate) = switched {
                            self.selected = Some(candidate.id.clone());
                            self.selection_lost = false;
                        } else {
                            self.selection_lost = true;
                        }
                    }
                    if self.selected.is_none() && !self.selection_lost {
                        let suggested: Vec<_> = devices.iter().filter(|c| c.suggested).collect();
                        if suggested.len() == 1 {
                            self.selected = Some(suggested[0].id.clone());
                        }
                    }
                    self.devices = devices;
                    self.usb_error = None;
                }
                Err(e) => self.usb_error = Some(e.to_string()),
            }
        }
        if let Some(dirty) = self.dirty {
            if dirty.elapsed() >= Duration::from_millis(300) {
                self.save();
            } else {
                ctx.request_repaint_after(Duration::from_millis(300) - dirty.elapsed());
            }
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let before = self.settings.clone();
        egui::Frame::central_panel(ui.style())
            .inner_margin(24.0)
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                ui.heading("Mobile Webcam");
                ui.add_space(18.0);
                ui.add_enabled_ui(!self.session.running(), |ui| {
                    ui.horizontal(|ui| {
                        for (mode, label) in [("usb", "USB"), ("lan", "Wi-Fi")] {
                            if ui
                                .add(
                                    egui::Button::selectable(self.settings.transport == mode, label)
                                        .frame_when_inactive(true),
                                )
                                .clicked()
                            {
                                self.settings.transport = mode.into();
                            }
                        }
                    });
                    ui.add_space(12.0);
                    if self.settings.transport == "lan" {
                        ui.label(self.text("IP address", "IPアドレス"));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.settings.lan_host)
                                .hint_text("192.168.1.42")
                                .desired_width(f32::INFINITY),
                        );
                    } else {
                        let available: Vec<_> = self
                            .devices
                            .iter()
                            .filter(|c| {
                                c.suggested
                                    || self.show_unknown
                                    || self.selected.as_ref() == Some(&c.id)
                            })
                            .collect();
                        let name = self
                            .selected
                            .as_ref()
                            .and_then(|id| available.iter().find(|c| c.id == *id))
                            .map(|c| c.name.as_str())
                            .unwrap_or(self.text("Select device", "端末を選択"));
                        if available.len() != 1 || self.selection_lost || self.selected.is_none() {
                            egui::ComboBox::from_id_salt("usb-device")
                                .selected_text(name)
                                .width(320.0)
                                .show_ui(ui, |ui| {
                                    for candidate in available {
                                        if ui
                                            .selectable_value(
                                                &mut self.selected,
                                                Some(candidate.id.clone()),
                                                &candidate.name,
                                            )
                                            .changed()
                                        {
                                            self.selection_lost = false;
                                            self.session.usb_permission = None;
                                        }
                                    }
                                });
                        } else {
                            ui.label(name);
                        }
                        if self.devices.iter().all(|device| !device.suggested) && !self.show_unknown
                        {
                            ui.label(self.text("Connect your phone by USB", "スマホをUSBで接続"));
                        }
                        if self.selection_lost {
                            ui.colored_label(
                                egui::Color32::LIGHT_RED,
                                self.text(
                                    "Selected device disconnected",
                                    "選択した端末が切断されました",
                                ),
                            );
                        }
                        if let Some(error) = &self.usb_error {
                            ui.colored_label(egui::Color32::LIGHT_RED, error);
                        }
                    }
                });
                ui.add_space(18.0);
                let running = self.session.running();
                let label = if running {
                    self.text("Disconnect", "切断")
                } else {
                    self.text("Connect", "接続")
                };
                #[cfg(target_os = "linux")]
                let label = if !running
                    && self.settings.transport == "usb"
                    && self.session.usb_permission.is_some()
                {
                    self.text("Allow USB access", "USBアクセスを許可")
                } else {
                    label
                };
                let allowed = running
                    || self.settings.transport == "lan"
                        && !self.settings.lan_host.trim().is_empty()
                    || self.settings.transport == "usb"
                        && self.selected.is_some()
                        && !self.selection_lost;
                if ui
                    .add_enabled(
                        allowed,
                        egui::Button::new(label).min_size(egui::vec2(ui.available_width(), 38.0)),
                    )
                    .clicked()
                {
                    self.save();
                    if running {
                        self.session.stop(ui.ctx());
                    } else {
                        #[cfg(target_os = "linux")]
                        let result = if self.settings.transport == "usb"
                            && self.session.usb_permission.is_some()
                        {
                            self.session
                                .allow_usb(self.settings.clone(), ui.ctx().clone())
                        } else {
                            self.session.start(
                                self.settings.clone(),
                                self.selected.clone(),
                                ui.ctx().clone(),
                            )
                        };
                        #[cfg(windows)]
                        let result = self.session.start(
                            self.settings.clone(),
                            self.selected.clone(),
                            ui.ctx().clone(),
                        );
                        if let Err(e) = result {
                            self.session.state = State::Failed(e.to_string());
                        }
                    }
                }
                #[cfg(target_os = "linux")]
                if !running
                    && !std::ffi::CString::new(self.settings.output.trim())
                        .is_ok_and(|path| unsafe { libc::access(path.as_ptr(), libc::W_OK) } == 0)
                    && self.session.setup_available()
                    && ui
                        .button(self.text("Set up camera", "カメラを設定"))
                        .clicked()
                        && let Err(e) = self.session.setup(ui.ctx().clone()) {
                            self.session.state = State::Failed(e.to_string());
                        }
                ui.add_space(12.0);
                let status = match &self.session.state {
                    State::Idle => "",
                    State::Connecting => self.text("Connecting…", "接続中…"),
                    State::Waiting => {
                        if cfg!(windows) && matches!(&self.session.stats, Some(Event::Stats { decoded, active: false, .. }) if *decoded > 0) {
                            self.text("Open a camera app", "カメラアプリを開いてください")
                        } else { self.text("Waiting for phone video", "スマホの映像を待機中") }
                    },
                    State::Streaming => self.text("Streaming", "配信中"),
                    State::Stopping => self.text("Disconnecting…", "切断中…"),
                    State::Failed(error) => {
                        if error.contains("not installed") || error.contains("one-time host setup")
                        {
                            self.text("Run camera setup", "カメラの初回設定が必要です")
                        } else if self.session.usb_permission.is_some() {
                            self.text("Allow USB access", "USBアクセスを許可してください")
                        } else if error.contains("WinUSB") {
                            self.text("USB driver setup required", "USBドライバーの設定が必要です")
                        } else {
                            self.text("Connection failed", "接続できませんでした")
                        }
                    }
                };
                if !status.is_empty() {
                    ui.label(status);
                }
                if let Some(error) = &self.save_error {
                    ui.colored_label(
                        egui::Color32::LIGHT_RED,
                        format!(
                            "{}: {error}",
                            self.text("Settings could not be saved", "設定を保存できませんでした")
                        ),
                    );
                }
                ui.add_space(12.0);
                egui::CollapsingHeader::new(self.text("Details", "詳細"))
                    .default_open(
                        cfg!(feature = "capture")
                            && std::env::var_os("MW_CAPTURE_DETAILS").is_some(),
                    )
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(ui.available_height())
                            .show(ui, |ui| {
                                if let State::Failed(error) = &self.session.state {
                                    ui.label(error);
                                }

                                if self.settings.transport == "usb" {
                                    ui.checkbox(
                                        &mut self.show_unknown,
                                        if self.ja {
                                            "すべてのUSB端末"
                                        } else {
                                            "All USB devices"
                                        },
                                    );
                                }
                                #[cfg(unix)]
                                {
                                    ui.label(self.text("Camera output", "カメラ出力"));
                                    ui.add_enabled(
                                        !running,
                                        egui::TextEdit::singleline(&mut self.settings.output),
                                    );
                                }
                                ui.label(self.text("Video decode", "映像デコード"));
                                egui::ComboBox::from_id_salt("decoder")
                                    .selected_text(self.decode_name(&self.settings.decode))
                                    .show_ui(ui, |ui| {
                                        for mode in [
                                            "auto",
                                            "off",
                                            if cfg!(windows) { "d3d11va" } else { "vaapi" },
                                            "cuda",
                                        ] {
                                            ui.add_enabled_ui(!running, |ui| {
                                                ui.selectable_value(
                                                    &mut self.settings.decode,
                                                    mode.into(),
                                                    if mode == "auto" {
                                                        if self.ja { "自動" } else { "Auto" }
                                                    } else if mode == "off" {
                                                        if self.ja {
                                                            "ソフトウェア"
                                                        } else {
                                                            "Software"
                                                        }
                                                    } else {
                                                        mode
                                                    },
                                                );
                                            });
                                        }
                                    });
                                ui.horizontal(|ui| {
                                    ui.label(if self.ja { "回転" } else { "Rotation" });
                                    for rotation in [0, 90, 180, 270] {
                                        ui.selectable_value(
                                            &mut self.settings.rotation,
                                            rotation,
                                            format!("{rotation}°"),
                                        );
                                    }
                                });
                                ui.checkbox(
                                    &mut self.settings.mirror,
                                    if self.ja { "左右反転" } else { "Mirror" },
                                );
                                ui.checkbox(
                                    &mut self.settings.vertical_flip,
                                    if self.ja {
                                        "上下反転"
                                    } else {
                                        "Vertical flip"
                                    },
                                );
                                if let Some(Event::Stats {
                                    decoded,
                                    submitted,
                                    width,
                                    height,
                                    decoder,
                                    ..
                                }) = &self.session.stats
                                {
                                    ui.label(format!(
                            "{width}×{height} · {decoder}\n{} {decoded} · {} {submitted}",
                            self.text("Decoded", "デコード"),
                            self.text("Submitted", "出力")
                        ));
                                }
                            });
                    });
            });
        let enabled = self.settings.transport == "usb";
        if self.usb_enabled.swap(enabled, Ordering::AcqRel) != enabled {
            self.usb_thread.unpark();
        }
        if before != self.settings {
            self.dirty = Some(Instant::now());
            self.session.transform(&self.settings);
            ui.ctx().request_repaint_after(Duration::from_millis(300));
        }
        #[cfg(feature = "capture")]
        if std::env::var_os("MW_SCREENSHOT").is_some() && !self.capture_requested {
            self.capture_requested = true;
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
    }
}
impl Drop for App {
    fn drop(&mut self) {
        self.usb_stop.store(true, Ordering::Release);
        self.usb_thread.unpark();
        if self.dirty.is_some() {
            self.save();
        }
    }
}
