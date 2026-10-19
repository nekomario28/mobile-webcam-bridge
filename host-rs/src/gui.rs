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
    // The only mutation is the capture-only position pin below.
    #[cfg_attr(not(feature = "capture"), allow(unused_mut))]
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([420.0, 440.0])
        .with_min_inner_size([360.0, 360.0]);
    // Without a window manager the server places the window wherever it likes,
    // which on a headless display can be off-screen entirely. A capture pins
    // the position so the screenshot is reproducible.
    #[cfg(feature = "capture")]
    if std::env::var_os("MW_CAPTURE_DETAILS").is_some() {
        // The quality controls live behind Details; a 440-point window clips them
        // out of the capture, so a details capture gets a window tall enough to
        // show the whole section it opens.
        viewport = viewport.with_inner_size([420.0, 900.0]);
        let (x, y) = std::env::var("MW_CAPTURE_POSITION")
            .ok()
            .and_then(|value| {
                value
                    .split_once(',')
                    .map(|(x, y)| (x.to_owned(), y.to_owned()))
            })
            .unwrap_or_else(|| ("0".into(), "0".into()));
        viewport = viewport.with_position([x.trim().parse().unwrap(), y.trim().parse().unwrap()]);
    }
    let options = eframe::NativeOptions {
        viewport,
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
    load_error: Option<String>,
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
    /// The quality draft; 0 fields mean auto (slice 03).
    quality_draft: (u16, u16, u16),
    confirm_forget_phone: bool,
    /// Two-step confirmation for the only action that replaces a stored key.
    confirm_replace_identity: bool,
    /// Where the unusable key was moved, shown after a replacement.
    recovery_note: Option<String>,
    #[cfg(feature = "capture")]
    capture_requested: bool,
}
/// The native capture entry point: replay a recorded event list through the
/// real session so a screenshot shows what those events produce.
#[cfg(feature = "capture")]
fn capture_session() -> Session {
    let Some(path) = std::env::var_os("MW_CAPTURE_FIXTURE") else {
        return Session::default();
    };
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("capture fixture {}: {error}", path.to_string_lossy()));
    let events: Vec<crate::control::Event> = serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("capture fixture {}: {error}", path.to_string_lossy()));
    Session::for_capture(events)
}

#[cfg(not(feature = "capture"))]
fn capture_session() -> Session {
    Session::default()
}

impl App {
    fn new(ctx: &egui::Context) -> Self {
        let store = Store::native();
        let mut save_error = None;
        let mut load_error = None;
        let (store, settings) = match store {
            Ok(store) => {
                let s = store.load().unwrap_or_else(|e| {
                    load_error = Some(e.to_string());
                    Settings::default()
                });
                (Some(store), s)
            }
            Err(e) => {
                save_error = Some(e.to_string());
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
            style.spacing.interact_size.y = 24.0;
            style.spacing.icon_width = 18.0;
            style.spacing.item_spacing.y = 5.0;
            style.spacing.scroll = egui::style::ScrollStyle::solid();
            style.visuals.override_text_color = Some(egui::Color32::from_gray(220));
            style.visuals.widgets.inactive.bg_stroke =
                egui::Stroke::new(1.0, egui::Color32::from_gray(100));
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
        let quality_wish = (
            settings.quality_width,
            settings.quality_height,
            settings.quality_fps,
        );
        Self {
            settings,
            store,
            save_error,
            load_error,
            dirty: None,
            session: capture_session(),
            ja: japanese(),
            devices: Vec::new(),
            selected: None,
            selection_lost: false,
            usb_enabled: enabled,
            usb_stop: stop,
            usb_thread: inventory.thread().clone(),
            usb_events: receiver,
            usb_error: None,
            quality_draft: quality_wish,
            confirm_forget_phone: false,
            confirm_replace_identity: false,
            recovery_note: None,
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

    fn quality_controls(&mut self, ui: &mut egui::Ui, running: bool) {
        let busy = self.session.quality.pending.is_some();
        let stopping = matches!(self.session.state, State::Stopping);
        let modes = self.session.quality.modes.clone().unwrap_or_default();
        let mut sizes: Vec<_> = modes.iter().map(|mode| (mode.width, mode.height)).collect();
        sizes.sort_unstable();
        sizes.dedup();
        let old_size = (self.quality_draft.0, self.quality_draft.1);
        ui.add_enabled_ui(!busy && !stopping, |ui| {
            ui.label(self.text("Resolution", "解像度"));
            egui::ComboBox::from_id_salt("quality-size")
                .selected_text(if self.quality_draft.0 == 0 {
                    self.text("Auto", "自動").to_owned()
                } else {
                    format!("{}×{}", self.quality_draft.0, self.quality_draft.1)
                })
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.quality_draft.0 == 0, self.text("Auto", "自動"))
                        .clicked()
                    {
                        self.quality_draft.0 = 0;
                        self.quality_draft.1 = 0;
                    }
                    for (width, height) in &sizes {
                        if ui
                            .selectable_label(
                                (self.quality_draft.0, self.quality_draft.1) == (*width, *height),
                                format!("{width}×{height}"),
                            )
                            .clicked()
                        {
                            self.quality_draft.0 = *width;
                            self.quality_draft.1 = *height;
                        }
                    }
                });
            let mut rates: Vec<_> = modes
                .iter()
                .filter(|mode| {
                    self.quality_draft.0 == 0
                        || (mode.width, mode.height) == (self.quality_draft.0, self.quality_draft.1)
                })
                .map(|mode| mode.fps)
                .collect();
            rates.sort_unstable();
            rates.dedup();
            // Only a user's size edit resets an incompatible draft fps.
            // A different phone must not silently rewrite the saved wish.
            if old_size != (self.quality_draft.0, self.quality_draft.1)
                && !modes.is_empty()
                && self.quality_draft.2 != 0
                && !rates.contains(&self.quality_draft.2)
            {
                self.quality_draft.2 = 0;
            }
            ui.label(self.text("Frame rate", "フレームレート"));
            egui::ComboBox::from_id_salt("quality-fps")
                .selected_text(if self.quality_draft.2 == 0 {
                    self.text("Auto", "自動").to_owned()
                } else {
                    format!("{} fps", self.quality_draft.2)
                })
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.quality_draft.2 == 0, self.text("Auto", "自動"))
                        .clicked()
                    {
                        self.quality_draft.2 = 0;
                    }
                    for fps in rates {
                        if ui
                            .selectable_label(self.quality_draft.2 == fps, format!("{fps} fps"))
                            .clicked()
                        {
                            self.quality_draft.2 = fps;
                        }
                    }
                });
        });
        let draft = crate::quality::Wish::from(self.quality_draft);
        if !running && draft != self.settings.quality_wish() {
            self.settings.set_quality_wish(draft);
        }
        let resolved = self.session.quality.resolve(draft);
        let changed = draft != self.settings.quality_wish()
            || self.session.quality.error.is_some()
            || resolved.is_some_and(|mode| self.session.quality.applied != Some(mode.tuple()));
        if running && changed && !busy {
            let enabled = !stopping && self.session.quality.ready() && resolved.is_some();
            if ui
                .add_enabled(enabled, egui::Button::new(self.text("Apply", "適用")))
                .clicked()
                && self
                    .session
                    .prepare_quality(draft.width, draft.height, draft.fps)
            {
                self.settings.set_quality_wish(draft);
            }
        }
        if self.session.quality.pending.is_some() {
            ui.weak(if self.session.quality.staged {
                self.text("Start the camera on your phone", "スマホでカメラを開始")
            } else {
                self.text("Applying", "切替中")
            });
        } else if !running {
            ui.weak(self.text("Connect a phone", "スマホを接続"));
        } else if self.session.quality.permission == Some(crate::wire::PERMISSION_PENDING) {
            ui.weak(self.text("Allow camera on the phone", "スマホでカメラを許可"));
        } else if self.session.quality.permission == Some(crate::wire::PERMISSION_UNAVAILABLE)
            || self
                .session
                .quality
                .modes
                .as_ref()
                .is_some_and(|modes| modes.is_empty())
        {
            ui.weak(self.text("Quality controls unavailable", "品質設定は利用できません"));
        } else if self.session.quality.modes.is_none() {
            ui.weak(if self.session.stats.is_some() {
                self.text("Update the phone app", "スマホアプリを更新")
            } else {
                self.text(
                    "Waiting for phone modes",
                    "スマホの対応モードを待っています",
                )
            });
        }
        if let Some((width, height, fps)) = self.session.quality.applied {
            ui.weak(format!(
                "{}: {width}×{height}, {fps} fps",
                self.text("Applied capture target", "適用済み撮影目標")
            ));
        }
        if let Some(error) = &self.session.quality.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        if let Some(warning) = &self.settings.quality_warning {
            ui.colored_label(egui::Color32::LIGHT_RED, warning);
        }
        let unsupported = self.session.quality.ready() && resolved.is_none();
        if unsupported || self.settings.quality_warning.is_some() {
            let enabled = !busy && !stopping && (!running || self.session.quality.ready());
            if ui
                .add_enabled(
                    enabled,
                    egui::Button::new(self.text("Use auto", "自動に戻す")),
                )
                .clicked()
                && (!running || self.session.prepare_quality(0, 0, 0))
            {
                self.quality_draft = (0, 0, 0);
                self.settings
                    .set_quality_wish(crate::quality::Wish::default());
            }
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
        let was_running = self.session.running();
        self.session.poll(ctx);
        if was_running && !self.session.running() {
            self.quality_draft = self.settings.quality_wish().tuple();
        }
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
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        if let Some(error) = &self.load_error {
                            ui.colored_label(
                                egui::Color32::LIGHT_RED,
                                format!(
                                    "{}: {error}",
                                    self.text(
                                        "Settings could not be loaded",
                                        "設定を読み込めませんでした"
                                    )
                                ),
                            );
                            ui.add_space(8.0);
                        }
                        ui.add_enabled_ui(!self.session.running(), |ui| {
                            ui.horizontal(|ui| {
                                let width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                                for (mode, label) in [("usb", "USB"), ("lan", "Wi-Fi")] {
                                    let selected = self.settings.transport == mode;
                                    let label = if selected {
                                        egui::RichText::new(label).strong()
                                    } else {
                                        egui::RichText::new(label)
                                    };
                                    if ui
                                        .add(
                                            egui::Button::selectable(
                                                selected,
                                                (egui::Atom::grow(), label, egui::Atom::grow()),
                                            )
                                                .frame_when_inactive(true)
                                                .min_size(egui::vec2(width, 34.0)),
                                        )
                                        .clicked()
                                    {
                                        self.settings.transport = mode.into();
                                        self.session.state = State::Idle;
                                        self.session.stats = None;
                                        self.session.usb_permission = None;
                                    }
                                }
                            });
                            ui.add_space(12.0);
                            if self.settings.transport == "lan" {
                                if matches!(self.session.state, State::Idle) {
                                    ui.label(self.text(
                                        "On your phone, select Wi-Fi in Mobile Webcam.\nUse the same Wi-Fi as this PC.",
                                        "スマホでアプリを開き、Wi-Fiを選択。\nPCとスマホを同じWi-Fiにつなぐ。",
                                    ));
                                    ui.add_space(8.0);
                                }
                                ui.label(self.text("Phone address", "スマホに表示されたアドレス"));
                                ui.add(
                            egui::TextEdit::singleline(&mut self.settings.lan_host)
                                .hint_text("192.168.1.42")
                                .min_size(egui::vec2(0.0, 28.0))
                                        .desired_width(f32::INFINITY),
                                );
                                ui.label(self.text(
                                    "Phone certificate SHA-256 (verify on phone)",
                                    "スマホの証明書SHA-256（端末画面で確認）",
                                ));
                                ui.add_enabled(
                                    !self.session.running(),
                                    egui::TextEdit::singleline(&mut self.settings.lan_fingerprint)
                                        .hint_text("64 hexadecimal characters")
                                        .desired_width(f32::INFINITY),
                                );
                                ui.label(self.text("128-bit Wi-Fi pairing key", "128ビットWi-Fi接続キー"));
                                ui.add_enabled(
                                    !self.session.running(),
                                    egui::TextEdit::singleline(&mut self.settings.lan_key)
                                        .password(true)
                                        .hint_text("32 hexadecimal characters")
                                        .desired_width(f32::INFINITY),
                                );
                                let valid_fingerprint =
                                    crate::secure::is_hex(&self.settings.lan_fingerprint, 32);
                                let valid_key =
                                    crate::secure::is_hex(&self.settings.lan_key, 16);
                                if !valid_fingerprint || !valid_key {
                                    ui.colored_label(
                                        egui::Color32::YELLOW,
                                        self.text(
                                            "Pairing required: enter the 64-digit phone fingerprint and 32-digit access key.",
                                            "ペアリングが必要です：端末の64桁の指紋と32桁の接続キーを入力してください。",
                                        ),
                                    );
                                }
                                ui.weak(self.text(
                                    "Wi-Fi requires TLS 1.3. No plaintext fallback.",
                                    "Wi-FiはTLS 1.3必須。平文接続への切替なし。",
                                ));
                                if !self.session.running()
                                    && (!self.settings.lan_key.is_empty()
                                        || !self.settings.lan_fingerprint.is_empty())
                                {
                                    if self.confirm_forget_phone {
                                        ui.horizontal(|ui| {
                                            ui.label(self.text(
                                                "Remove stored pairing details?",
                                                "保存したペアリング情報を削除しますか？",
                                            ));
                                            if ui.button(self.text("Remove", "削除")).clicked() {
                                                self.settings.lan_host.clear();
                                                self.settings.lan_fingerprint.clear();
                                                self.settings.lan_key.clear();
                                                self.confirm_forget_phone = false;
                                                self.save();
                                            }
                                            if ui.button(self.text("Cancel", "キャンセル")).clicked() {
                                                self.confirm_forget_phone = false;
                                            }
                                        });
                                    } else if ui
                                        .small_button(self.text(
                                            "Forget paired phone",
                                            "登録済みスマホの情報を削除",
                                        ))
                                        .clicked()
                                    {
                                        self.confirm_forget_phone = true;
                                    }
                                }
                            } else {
                                if matches!(self.session.state, State::Idle) {
                                    ui.label(self.text(
                                        "On your phone, select USB in Mobile Webcam.\nConnect it to this PC with a USB cable.",
                                        "スマホでアプリを開き、USBを選択。\nUSBケーブルでPCにつなぐ。",
                                    ));
                                    ui.add_space(8.0);
                                }
                                let available = &self.devices;
                                let name = self
                                    .selected
                                    .as_ref()
                                    .and_then(|id| available.iter().find(|c| c.id == *id))
                                    .map(|c| c.name.as_str())
                                    .unwrap_or(self.text("Select your phone", "スマホを選択"));
                                if available.is_empty() {
                                    ui.weak(self.text("No USB devices detected", "USB機器が見つかりません"));
                                } else if available.len() != 1 || self.selection_lost || self.selected.is_none() {
                                    egui::ComboBox::from_id_salt("usb-device")
                                        .selected_text(name)
                                        .width(ui.available_width())
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
                        let label = if running && matches!(self.session.state, State::Connecting | State::ApprovalPending(_)) {
                            self.text("Cancel", "キャンセル")
                        } else if running {
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
                                && crate::secure::is_hex(&self.settings.lan_fingerprint, 32)
                                && crate::secure::is_hex(&self.settings.lan_key, 16)
                            || self.settings.transport == "usb"
                                && self.selected.is_some()
                                && !self.selection_lost;
                        let mut button = egui::Button::new((
                            egui::Atom::grow(),
                            egui::RichText::new(label).color(if allowed {
                                ui.visuals().selection.stroke.color
                            } else {
                                ui.visuals().text_color()
                            }),
                            egui::Atom::grow(),
                        )).min_size(egui::vec2(ui.available_width(), 42.0));
                        if allowed {
                            button = button.fill(ui.visuals().selection.bg_fill);
                        }
                        if ui.add_enabled(allowed, button).clicked() {
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
                                    self.session.connect(
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
                        ui.add_space(12.0);
                        let status = match &self.session.state {
                            State::Idle => "",
                            State::Connecting => self.text("Connecting…", "接続中…"),
                            State::ApprovalPending(_) => self.text("Approve this PC on your phone", "スマホでこのPCの接続を許可してください"),
                            State::Waiting => {
                                if self.settings.transport == "lan" {
                                    self.text(
                                        "TLS 1.3 verified · Press Start camera on your phone",
                                        "TLS 1.3で認証済み · スマホで「カメラ開始」を押してください",
                                    )
                                } else if cfg!(windows) && matches!(&self.session.stats, Some(Event::Stats { decoded, active: false, .. }) if *decoded > 0) {
                                    self.text("Select Mobile Webcam in your PC's camera app", "PCのカメラアプリでMobile Webcamを選択")
                            } else { self.text("Connected · Press Start camera on your phone", "接続済み · スマホで「カメラ開始」を押してください") }
                            },
                            State::Streaming => self.text("Camera connected", "カメラ接続中"),
                            State::Stopping => self.text("Disconnecting…", "切断中…"),
                            State::Failed(error) => {
                                if error.contains("v4l2loopback") || error.contains("host setup is not included")
                                {
                                    self.text("Camera driver unavailable", "カメラドライバーを利用できません")
                                } else if error.contains("fingerprint mismatch") {
                                    self.text("Fingerprint mismatch", "フィンガープリントが一致しません")
                                } else if error.contains("was not authorized") {
                                    self.text("Access key rejected", "接続キーが拒否されました")
                                } else if self.session.usb_permission.is_some() {
                                    self.text("Allow USB access", "USBアクセスを許可してください")
                                } else if error.contains("WinUSB") {
                                    self.text("USB driver unavailable", "USBドライバーを利用できません")
                                } else {
                                    self.text("Connection failed", "接続できませんでした")
                                }
                            }
                        };
                        if !status.is_empty() {
                            if matches!(self.session.state, State::Failed(_)) {
                                ui.colored_label(egui::Color32::LIGHT_RED, egui::RichText::new(status).strong());
                            } else {
                                ui.label(egui::RichText::new(status).strong());
                            }
                        }
                        if let State::ApprovalPending(client_id) = &self.session.state {
                            let grouped = client_id.as_bytes().chunks(4)
                                .map(|part| std::str::from_utf8(part).expect("validated ASCII PC ID"))
                                .collect::<Vec<_>>().join(" ");
                            ui.label(self.text("Match this PC ID before allowing. Approval does not start the camera.", "許可する前にPC IDを照合してください。許可だけでは撮影を開始しません。"));
                            ui.add(egui::Label::new(egui::RichText::new(grouped).monospace()).wrap());
                        }
                        if let State::Failed(error) = &self.session.state {
                            let next = if error.contains("v4l2loopback") || error.contains("host setup is not included") {
                                self.text("Install v4l2loopback for your OS, then reconnect.", "OSにv4l2loopbackを導入して、接続し直してください。")
                            } else if error.contains("fingerprint mismatch") {
                                self.text(
                                    "The certificate fingerprint does not match your phone. Re-check all 64 characters.",
                                    "証明書のフィンガープリントがスマホと一致しません。64桁すべてを再確認してください。",
                                )
                            } else if error.contains("was not authorized") {
                                self.text(
                                    "The phone rejected the access key. Re-check all 32 characters; a rotated key revokes the old one.",
                                    "スマホが接続キーを拒否しました。32桁を再確認してください。ローテーション後のスマホでは旧キーは無効です。",
                                )
                            } else if error.contains("explicit recovery required") {
                                self.text(
                                    "The stored PC key cannot be read. It has been left untouched. Replacing it moves that file aside permanently and every phone that approved this PC must approve it again.",
                                    "保存されたPC鍵を読み取れません。ファイルはそのまま残しています。置き換えると元のファイルは別名で保存され、このPCを許可したすべてのスマホで再許可が必要です。",
                                )
                            } else if error.contains("peer disconnected") {
                                self.text(
                                    "The phone closed the connection. Check the phone's Wi-Fi screen or app.",
                                    "スマホが接続を閉じました。スマホのWi-Fi画面またはアプリを確認してください。",
                                )
                            } else if error.contains("WinUSB") {
                                self.text("The selected phone needs a compatible USB driver.", "選択したスマホ用のUSBドライバーが必要です。")
                            } else if self.session.usb_permission.is_some() {
                                self.text("Press Allow USB access and approve the OS prompt.", "「USBアクセスを許可」を押し、OSの認証を進めてください。")
                            } else if self.settings.transport == "lan" {
                            self.text("Check the address shown on your phone and select Wi-Fi there.", "スマホのWi-Fi選択と、表示されたアドレスを確認してください。")
                            } else {
                                self.text("Check the selected phone and its USB permission. See Details for the error.", "選択したスマホとUSBの利用許可を確認してください。エラーは「詳細」に表示します。")
                            };
                            ui.label(next);
                            if error.contains("explicit recovery required") {
                                ui.add_space(6.0);
                                if !self.confirm_replace_identity {
                                    if ui
                                        .button(self.text(
                                            "Replace the stored PC key…",
                                            "保存されたPC鍵を置き換える…",
                                        ))
                                        .clicked()
                                    {
                                        self.confirm_replace_identity = true;
                                    }
                                } else {
                                    ui.label(self.text(
                                        "Replace it now? The old file is kept under a .corrupt- name; every approved phone must approve this PC again.",
                                        "今すぐ置き換えますか？ 古いファイルは .corrupt- 名で保存され、許可済みのスマホは再許可が必要です。",
                                    ));
                                    ui.horizontal(|ui| {
                                        if ui.button(self.text("Replace", "置き換える")).clicked() {
                                            self.confirm_replace_identity = false;
                                            match crate::identity::Identity::replace_stored() {
                                                Ok(moved) => {
                                                    self.recovery_note =
                                                        Some(moved.display().to_string());
                                                    self.session.state = State::Idle;
                                                }
                                                Err(error) => {
                                                    self.session.state =
                                                        State::Failed(error.to_string())
                                                }
                                            }
                                        }
                                        if ui
                                            .button(self.text("Cancel", "キャンセル"))
                                            .clicked()
                                        {
                                            self.confirm_replace_identity = false;
                                        }
                                    });
                                }
                            }
                        }
                        if let Some(note) = &self.recovery_note {
                            ui.colored_label(
                                egui::Color32::LIGHT_GREEN,
                                format!(
                                    "{}: {note}",
                                    self.text(
                                        "The old PC key was kept here; press Connect to use the new one",
                                        "古いPC鍵はここに保存しました。「接続」で新しい鍵を使えます"
                                    )
                                ),
                            );
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
                                self.quality_controls(ui, running);
                                if let State::Failed(error) = &self.session.state {
                                    ui.label(error);
                                }

                                #[cfg(unix)]
                                {
                                    ui.label(self.text("Camera output", "カメラ出力"));
                                    ui.add_enabled(
                                        !running,
                                        egui::TextEdit::singleline(&mut self.settings.output)
                                            .desired_width(f32::INFINITY),
                                    );
                                }
                                ui.label(self.text("Video decode", "デコード方式"));
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
                                        let label = egui::RichText::new(format!("{rotation}°"));
                                        let label = if self.settings.rotation == rotation {
                                            label.strong()
                                        } else {
                                            label
                                        };
                                        ui.selectable_value(
                                            &mut self.settings.rotation,
                                            rotation,
                                            label,
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
