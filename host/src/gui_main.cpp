#include <QApplication>
#include <QButtonGroup>
#include <QCheckBox>
#include <QCloseEvent>
#include <QComboBox>
#include <QCoreApplication>
#include <QDir>
#include <QFileInfo>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QProcess>
#include <QPushButton>
#include <QRegularExpression>
#include <QStandardPaths>
#include <QStringList>
#include <QTextCursor>
#include <QTimer>
#include <QToolButton>
#include <QVBoxLayout>
#include <QWidget>

namespace {

class BridgeWindow final : public QWidget {
  public:
    BridgeWindow() {
        setWindowTitle("Mobile Webcam");
        resize(680, 360);

        auto* title = new QLabel("Mobile Webcam", this);
        QFont title_font = title->font();
        title_font.setPointSize(title_font.pointSize() + 5);
        title_font.setBold(true);
        title->setFont(title_font);

        status_ = new QLabel("USB機器を確認しています…", this);
        status_->setWordWrap(true);

        transport_ = new QComboBox(this);
        transport_->addItem("USB", "usb");
        transport_->addItem("Wi-Fi / LAN", "lan");

        devices_ = new QComboBox(this);
        devices_->setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Fixed);
        refresh_ = new QPushButton("再読込", this);
        switch_ = new QPushButton("AOAへ切替", this);
        auto* usb_buttons = new QHBoxLayout;
        usb_buttons->addWidget(refresh_);
        usb_buttons->addWidget(switch_);
        usb_buttons_container_ = new QWidget(this);
        usb_buttons_container_->setLayout(usb_buttons);

        lan_host_ = new QLineEdit(this);
        lan_host_->setPlaceholderText("Phone IP (例: 192.168.1.42)");
        lan_pin_ = new QLineEdit(this);
        lan_pin_->setPlaceholderText("6桁PIN");
        lan_pin_->setMaxLength(6);

        hw_decode_ = new QComboBox(this);
        hw_decode_->addItem("自動", "auto");
        hw_decode_->addItem("VAAPI", "vaapi");
        hw_decode_->addItem("Software", "off");

        output_ = new QLineEdit("/dev/video10", this);
        rotation_group_ = new QButtonGroup(this);
        auto* rotation_buttons = new QHBoxLayout;
        for (int angle : {0, 90, 180, 270}) {
            auto* button = new QPushButton(
                (angle == 0 ? "● " : "") + QString::number(angle) + "°", this);
            button->setCheckable(true);
            button->setMinimumWidth(72);
            rotation_group_->addButton(button, angle);
            rotation_buttons->addWidget(button);
            if (angle == 0) button->setChecked(true);
        }
        mirror_ = new QCheckBox("左右", this);
        mirror_->setChecked(true);
        vertical_flip_ = new QCheckBox("上下", this);
        auto* flip_controls = new QHBoxLayout;
        flip_controls->addWidget(mirror_);
        flip_controls->addWidget(vertical_flip_);
        flip_controls->addStretch();

        start_ = new QPushButton("カメラブリッジ開始", this);
        start_->setEnabled(false);
        stop_ = new QPushButton("停止", this);
        stop_->setEnabled(false);
        auto* bridge_buttons = new QHBoxLayout;
        bridge_buttons->addWidget(start_);
        bridge_buttons->addWidget(stop_);

        log_ = new QPlainTextEdit(this);
        log_->setReadOnly(true);
        log_->setMaximumBlockCount(1000);
        log_->setVisible(false);
        log_toggle_ = new QToolButton(this);
        log_toggle_->setText("詳細ログ ▾");
        log_toggle_->setCheckable(true);

        form_ = new QFormLayout;
        form_->addRow("接続", transport_);
        form_->addRow("USB機器", devices_);
        form_->addRow("", usb_buttons_container_);
        form_->addRow("Phone IP", lan_host_);
        form_->addRow("PIN", lan_pin_);
        form_->addRow("デコード", hw_decode_);
        form_->addRow("仮想カメラ", output_);
        form_->addRow("回転", rotation_buttons);
        form_->addRow("反転", flip_controls);

        auto* layout = new QVBoxLayout(this);
        layout->addWidget(title);
        layout->addWidget(status_);
        layout->addLayout(form_);
        layout->addLayout(bridge_buttons);
        layout->addWidget(log_toggle_);
        layout->addWidget(log_, 1);

        control_.setProcessChannelMode(QProcess::MergedChannels);
        bridge_.setProcessChannelMode(QProcess::MergedChannels);

        connect(refresh_, &QPushButton::clicked, this, [this] { refreshDevices(); });
        connect(switch_, &QPushButton::clicked, this, [this] { switchToAccessory(); });
        connect(transport_, qOverload<int>(&QComboBox::currentIndexChanged), this, [this] {
            updateConnectionUi();
        });
        connect(lan_host_, &QLineEdit::textChanged, this, [this] {
            setBridgeRunning(bridge_.state() != QProcess::NotRunning);
        });
        connect(lan_pin_, &QLineEdit::textChanged, this, [this] {
            setBridgeRunning(bridge_.state() != QProcess::NotRunning);
        });
        connect(devices_, qOverload<int>(&QComboBox::currentIndexChanged),
                this, [this] {
                    updateUsbActions();
                    setBridgeRunning(bridge_.state() != QProcess::NotRunning);
                });
        connect(start_, &QPushButton::clicked, this, [this] { startBridge(); });
        connect(stop_, &QPushButton::clicked, this, [this] { stopBridge(); });
        connect(rotation_group_, &QButtonGroup::idClicked, this, [this](int angle) {
            rotation_degrees_ = angle;
            for (int candidate : {0, 90, 180, 270}) {
                rotation_group_->button(candidate)->setText(
                    (candidate == angle ? "● " : "") + QString::number(candidate) + "°");
            }
            sendTransform();
        });
        connect(mirror_, &QCheckBox::toggled, this, [this] { sendTransform(); });
        connect(vertical_flip_, &QCheckBox::toggled, this, [this] { sendTransform(); });
        connect(log_toggle_, &QToolButton::toggled, this, [this](bool visible) {
            log_->setVisible(visible);
            log_toggle_->setText(visible ? "詳細ログ ▴" : "詳細ログ ▾");
        });
        connect(&control_, &QProcess::readyReadStandardOutput, this, [this] {
            const QByteArray bytes = control_.readAllStandardOutput();
            control_output_.append(bytes);
            if (control_action_ != ControlAction::Refresh) appendLog(bytes);
        });
        connect(&bridge_, &QProcess::readyReadStandardOutput, this, [this] {
            const QByteArray bytes = bridge_.readAllStandardOutput();
            appendLog(bytes);
            bridge_output_.append(bytes);
            int end;
            while ((end = bridge_output_.indexOf('\n')) >= 0) {
                const QByteArray line = bridge_output_.left(end);
                bridge_output_.remove(0, end + 1);
                if (line.startsWith("frames=1 ")) {
                    status_->setText("配信中 · " + QString::number(rotation_degrees_) +
                                     "° · ↔ " + (mirror_->isChecked() ? "ON" : "OFF") +
                                     " · ↕ " + (vertical_flip_->isChecked() ? "ON" : "OFF"));
                } else if (line.startsWith("transform ")) {
                    const QRegularExpression pattern(
                        "^transform rotation=(0|90|180|270) horizontal_flip=(on|off) "
                        "vertical_flip=(on|off)$");
                    const auto match = pattern.match(QString::fromLocal8Bit(line));
                    if (match.hasMatch()) {
                        status_->setText("配信中 · " + match.captured(1) +
                                         "° · ↔ " +
                                         (match.captured(2) == "on" ? "ON" : "OFF") +
                                         " · ↕ " +
                                         (match.captured(3) == "on" ? "ON" : "OFF"));
                    }
                }
            }
            if (bridge_output_.size() > 4096) bridge_output_.clear();
        });
        connect(&bridge_, &QProcess::started, this, [this] { sendTransform(); });
        connect(&control_, qOverload<int, QProcess::ExitStatus>(&QProcess::finished),
                this, [this](int code, QProcess::ExitStatus) { controlFinished(code); });
        connect(&bridge_, qOverload<int, QProcess::ExitStatus>(&QProcess::finished),
                this, [this](int code, QProcess::ExitStatus) {
                    setBridgeRunning(false);
                    if (stop_requested_) {
                        status_->setText("カメラブリッジを停止しました。");
                    } else {
                        status_->setText(code == 0
                                             ? "カメラブリッジを終了しました。"
                                             : QString("カメラブリッジが終了しました (code %1)。").arg(code));
                        if (code != 0) log_toggle_->setChecked(true);
                    }
                    stop_requested_ = false;
                });
        connect(&control_, &QProcess::errorOccurred, this, [this](QProcess::ProcessError error) {
            status_->setText("USB操作を開始できません: " + control_.errorString());
            if (error == QProcess::FailedToStart) {
                control_action_ = ControlAction::None;
                updateUsbActions();
            }
        });
        connect(&bridge_, &QProcess::errorOccurred, this, [this](QProcess::ProcessError error) {
            status_->setText("カメラブリッジを開始できません: " + bridge_.errorString());
            if (error == QProcess::FailedToStart) {
                setBridgeRunning(false);
                log_toggle_->setChecked(true);
            }
        });

        updateConnectionUi();
        QTimer::singleShot(0, this, [this] { refreshDevices(); });
    }

  protected:
    void closeEvent(QCloseEvent* event) override {
        stopProcess(bridge_);
        stopProcess(control_);
        event->accept();
    }

  private:
    enum class ControlAction { None, Refresh, Switch };

    QString toolPath(const QString& name) const {
        return QDir(QCoreApplication::applicationDirPath()).filePath(name);
    }

    void appendLog(const QByteArray& bytes) {
        log_->moveCursor(QTextCursor::End);
        log_->insertPlainText(QString::fromLocal8Bit(bytes));
        log_->moveCursor(QTextCursor::End);
    }

    bool selectedIsAccessory() const {
        const QStringList selected = devices_->currentData().toStringList();
        return selected.size() == 2 && selected[0].startsWith("18d1:2d0");
    }

    bool lanSelected() const {
        return transport_->currentData().toString() == "lan";
    }

    bool lanReady() const {
        static const QRegularExpression pin_pattern("^[0-9]{6}$");
        return !lan_host_->text().trimmed().isEmpty() &&
               pin_pattern.match(lan_pin_->text().trimmed()).hasMatch();
    }

    void updateConnectionUi() {
        const bool lan = lanSelected();
        devices_->setEnabled(!lan);
        form_->setRowVisible(devices_, !lan);
        form_->setRowVisible(usb_buttons_container_, !lan);
        form_->setRowVisible(lan_host_, lan);
        form_->setRowVisible(lan_pin_, lan);
        lan_host_->setEnabled(lan);
        lan_pin_->setEnabled(lan);
        if (bridge_.state() == QProcess::NotRunning) {
            status_->setText(lan ? "PhoneのWi-Fi画面に表示されたIPとPINを入力してください。"
                                 : "USB機器を確認しています…");
        }
        setBridgeRunning(bridge_.state() != QProcess::NotRunning);
    }

    void updateUsbActions() {
        if (lanSelected()) {
            refresh_->setEnabled(false);
            switch_->setEnabled(false);
            return;
        }
        const bool idle = control_.state() == QProcess::NotRunning;
        refresh_->setEnabled(idle);
        switch_->setText(selectedIsAccessory() ? "AOA接続済み" : "AOAへ切替");
        switch_->setEnabled(idle && bridge_.state() == QProcess::NotRunning &&
                            devices_->count() > 0 && !selectedIsAccessory());
    }

    void setBridgeRunning(bool running) {
        const bool connection_ready = lanSelected() ? lanReady() : selectedIsAccessory();
        start_->setEnabled(!running && connection_ready);
        stop_->setEnabled(running);
        output_->setEnabled(!running);
        transport_->setEnabled(!running);
        lan_host_->setEnabled(!running && lanSelected());
        lan_pin_->setEnabled(!running && lanSelected());
        hw_decode_->setEnabled(!running);
        updateUsbActions();
    }

    void sendTransform() {
        if (bridge_.state() != QProcess::Running) return;
        const QByteArray command = QByteArray::number(rotation_degrees_) + " " +
                                   (mirror_->isChecked() ? "1 " : "0 ") +
                                   (vertical_flip_->isChecked() ? "1\n" : "0\n");
        if (bridge_.write(command) != command.size()) {
            status_->setText("向きの変更を送信できませんでした。");
        } else {
            status_->setText("向き変更中 · " + QString::number(rotation_degrees_) + "°");
        }
    }

    void startControl(const QString& program, const QStringList& arguments,
                      ControlAction action) {
        if (control_.state() != QProcess::NotRunning) return;
        control_action_ = action;
        control_output_.clear();
        refresh_->setEnabled(false);
        switch_->setEnabled(false);
        control_.start(program, arguments);
    }

    void refreshDevices() {
        if (lanSelected()) return;
        const QString probe = toolPath("amb-aoa-probe");
        if (!QFileInfo::exists(probe)) {
            status_->setText("amb-aoa-probe が見つかりません: " + probe);
            return;
        }
        status_->setText("USB機器を確認しています…");
        startControl(probe, {"--list"}, ControlAction::Refresh);
    }

    void parseDeviceList() {
        devices_->clear();
        const QRegularExpression pattern(
            "^bus=(\\d+) addr=(\\d+) VID=([0-9a-fA-F]{4}) PID=([0-9a-fA-F]{4})(.*)$");
        int preferred = -1;
        for (const QString& line : QString::fromLocal8Bit(control_output_).split('\n')) {
            const auto match = pattern.match(line.trimmed());
            if (!match.hasMatch()) continue;
            const QString vid = match.captured(3).toLower();
            const QString pid = match.captured(4).toLower();
            const bool accessory = vid == "18d1" && pid.startsWith("2d0");
            const bool xperia = vid == "0fce";
            if (!accessory && !xperia) continue;

            const QString label = QString("%1 %2:%3  (bus %4, addr %5)")
                                      .arg(accessory ? "Android accessory" : "Xperia",
                                           vid, pid, match.captured(1), match.captured(2));
            devices_->addItem(label,
                              QStringList{vid + ":" + pid,
                                          match.captured(1) + ":" + match.captured(2)});
            if (accessory) preferred = devices_->count() - 1;
        }
        if (preferred >= 0) devices_->setCurrentIndex(preferred);
        const bool found = devices_->count() > 0;
        setBridgeRunning(bridge_.state() != QProcess::NotRunning);
        if (bridge_.state() != QProcess::NotRunning) return;
        if (!found) {
            status_->setText("USB: Xperiaが見つかりません");
        } else if (selectedIsAccessory()) {
            status_->setText("USB: AOA接続済み");
        } else {
            status_->setText("USB: Xperia接続中 · AOAへ切替");
        }
    }

    void switchToAccessory() {
        const QStringList selected = devices_->currentData().toStringList();
        if (selected.size() != 2) return;
        if (selected[0].startsWith("18d1:2d0")) {
            status_->setText("すでに AOA accessory モードです。");
            return;
        }
        const QString pkexec = QStandardPaths::findExecutable("pkexec");
        if (pkexec.isEmpty()) {
            status_->setText("pkexec が見つかりません。端末から AOA 切替を実行してください。");
            return;
        }
        status_->setText("管理者認証後、Xperia を AOA モードへ切り替えます…");
        startControl(pkexec,
                     {toolPath("amb-aoa-probe"), "--device", selected[0],
                      "--bus-address", selected[1], "--switch"},
                     ControlAction::Switch);
    }

    void controlFinished(int code) {
        const ControlAction completed = control_action_;
        control_action_ = ControlAction::None;
        if (completed == ControlAction::Refresh) {
            if (code == 0) parseDeviceList();
            else status_->setText(QString("USB一覧の取得に失敗しました (code %1)。").arg(code));
        } else if (completed == ControlAction::Switch) {
            if (code == 0) {
                status_->setText("AOA切替が完了しました。USB機器を再確認します…");
                QTimer::singleShot(1000, this, [this] { refreshDevices(); });
            } else {
                status_->setText(QString("AOA切替に失敗しました (code %1)。").arg(code));
            }
        }
        updateUsbActions();
    }

    void startBridge() {
        if (bridge_.state() != QProcess::NotRunning) return;
        if (!lanSelected() && !selectedIsAccessory()) {
            status_->setText("先にAOAへ切替");
            return;
        }
        if (lanSelected() && !lanReady()) {
            status_->setText("Phone IPと6桁PINを入力してください。");
            return;
        }
        const QString sink = toolPath("amb-v4l2-sink");
        if (!QFileInfo::exists(sink)) {
            status_->setText("amb-v4l2-sink が見つかりません: " + sink);
            return;
        }
        const QString device = output_->text().trimmed();
        if (device.isEmpty()) {
            status_->setText("仮想カメラのパスを入力してください。");
            return;
        }
        QStringList arguments{"--device", device, "--timeout-ms", "120000",
                              "--control-stdin", "--rotate", QString::number(rotation_degrees_)};
        arguments << "--hw-decode" << hw_decode_->currentData().toString();
        if (lanSelected()) {
            arguments << "--lan" << lan_host_->text().trimmed()
                      << "--port" << "48527"
                      << "--pin" << lan_pin_->text().trimmed();
        }
        if (!mirror_->isChecked()) arguments << "--no-horizontal-flip";
        if (vertical_flip_->isChecked()) arguments << "--vertical-flip";

        log_->clear();
        bridge_output_.clear();
        stop_requested_ = false;
        setBridgeRunning(true);
        status_->setText(lanSelected() ? "Wi-Fi接続中…" : "USB受信待機中 · Phoneでカメラ開始");
        bridge_.start(sink, arguments);
    }

    void stopBridge() {
        stop_requested_ = true;
        stopProcess(bridge_);
    }

    static void stopProcess(QProcess& process) {
        if (process.state() == QProcess::NotRunning) return;
        process.terminate();
        if (!process.waitForFinished(1000)) {
            process.kill();
            process.waitForFinished(1000);
        }
    }

    QLabel* status_ = nullptr;
    QFormLayout* form_ = nullptr;
    QComboBox* transport_ = nullptr;
    QComboBox* devices_ = nullptr;
    QPushButton* refresh_ = nullptr;
    QPushButton* switch_ = nullptr;
    QWidget* usb_buttons_container_ = nullptr;
    QLineEdit* lan_host_ = nullptr;
    QLineEdit* lan_pin_ = nullptr;
    QComboBox* hw_decode_ = nullptr;
    QLineEdit* output_ = nullptr;
    QButtonGroup* rotation_group_ = nullptr;
    QCheckBox* mirror_ = nullptr;
    QCheckBox* vertical_flip_ = nullptr;
    QToolButton* log_toggle_ = nullptr;
    QPushButton* start_ = nullptr;
    QPushButton* stop_ = nullptr;
    QPlainTextEdit* log_ = nullptr;
    QProcess control_;
    QProcess bridge_;
    QByteArray control_output_;
    QByteArray bridge_output_;
    ControlAction control_action_ = ControlAction::None;
    int rotation_degrees_ = 0;
    bool stop_requested_ = false;
};

}  // namespace

int main(int argc, char** argv) {
    QApplication app(argc, argv);
    QCoreApplication::setApplicationName("Mobile Webcam");
    QCoreApplication::setApplicationVersion(AMB_VERSION);
    QCoreApplication::setOrganizationName("nekomario28");
    BridgeWindow window;
    window.show();
    return app.exec();
}
