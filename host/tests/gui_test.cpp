#define AMB_GUI_TEST
#include "../src/gui_main.cpp"
#include <QFile>
#include <QTemporaryDir>
#include <QtTest>

class GuiTest final : public QObject {
    Q_OBJECT
  private:
    QTemporaryDir temporary_;

  private slots:
    void initTestCase() {
        QVERIFY(temporary_.isValid());
#ifndef Q_OS_WIN
        QSettings::setDefaultFormat(QSettings::IniFormat);
        QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, temporary_.path());
#else
        QCoreApplication::setApplicationName("Mobile Webcam GUI tests " + QFileInfo(temporary_.path()).fileName());
#endif
#ifdef Q_OS_WIN
        const QString suffix = ".exe";
#else
        const QString suffix;
#endif
        const QString directory = QCoreApplication::applicationDirPath();
        for (const QString& name : {QString("amb-aoa-probe"),
#ifdef Q_OS_WIN
                                   QString("amb-windows-sink")
#else
                                   QString("amb-v4l2-sink")
#endif
        }) {
            const QString destination = directory + "/" + name + suffix;
            QFile::remove(destination);
            QVERIFY(QFile::copy(directory + "/amb-gui-helper" + suffix, destination));
        }
    }

    void init() {
        QSettings().clear();
        const QString state = temporary_.filePath("usb");
        qputenv("AMB_GUI_FIXTURE_STATE", state.toUtf8());
        QFile::remove(state);
        QFile::remove(state + ".args");
    }

    void savedWifiSurvivesNewProcess() {
        {
            BridgeWindow window;
            window.findChild<QComboBox*>("transport")->setCurrentIndex(1);
            window.findChild<QLineEdit*>("lanHost")->setText(" 192.168.4.42 ");
            window.findChild<QCheckBox*>("mirror")->setChecked(false);
            window.findChild<QCheckBox*>("verticalFlip")->setChecked(true);
            window.findChild<QButtonGroup*>()->button(90)->click();
            window.show();
            QTest::qWait(50);
            if (!qgetenv("AMB_GUI_CAPTURE_DIR").isEmpty())
                QVERIFY(window.grab().save(QString::fromUtf8(qgetenv("AMB_GUI_CAPTURE_DIR")) + "/wifi.png"));
            window.close();
        }
        QProcess child;
        auto environment = QProcessEnvironment::systemEnvironment();
        environment.insert("AMB_GUI_TEST_SETTINGS", temporary_.path());
        environment.insert("AMB_GUI_TEST_APPLICATION", QCoreApplication::applicationName());
        child.setProcessEnvironment(environment);
        child.start(QCoreApplication::applicationFilePath(), {"--check-persisted"});
        QTRY_COMPARE_WITH_TIMEOUT(child.state(), QProcess::NotRunning, 5000);
        QCOMPARE(child.exitStatus(), QProcess::NormalExit);
        QCOMPARE(child.exitCode(), 0);
    }

    void usbStartSwitchesAndUsesSelectedDevice() {
        const QString output = temporary_.filePath("camera");
        QFile file(output);
        QVERIFY(file.open(QIODevice::WriteOnly));
        file.close();
        BridgeWindow window;
        window.findChild<QComboBox*>("transport")->setCurrentIndex(0);
        window.findChild<QLineEdit*>("output")->setText(output);
        auto* start = window.findChild<QPushButton*>("start");
        auto* stop = window.findChild<QPushButton*>("stop");
        QTRY_VERIFY_WITH_TIMEOUT(start->isEnabled(), 5000);
        start->click();
        const QString arguments = temporary_.filePath("usb.args");
        QTRY_VERIFY_WITH_TIMEOUT(QFile::exists(arguments), 5000);
        QFile recorded(arguments);
        QVERIFY(recorded.open(QIODevice::ReadOnly | QIODevice::Text));
        QVERIFY(recorded.readAll().contains("--bus-address\n1:3\n"));
        QTRY_VERIFY_WITH_TIMEOUT(window.findChild<QPlainTextEdit*>()->toPlainText().contains("transform rotation="), 5000);
        const QString status = window.findChild<QLabel*>("status")->text();
        QVERIFY(!status.contains("Streaming") && !status.contains("配信中"));
        QVERIFY(status.contains("Waiting for USB") || status.contains("USB受信待機"));
        window.show();
        QTest::qWait(50);
        if (!qgetenv("AMB_GUI_CAPTURE_DIR").isEmpty())
            QVERIFY(window.grab().save(QString::fromUtf8(qgetenv("AMB_GUI_CAPTURE_DIR")) + "/usb.png"));
        QVERIFY(stop->isEnabled());
        stop->click();
        QTRY_VERIFY_WITH_TIMEOUT(start->isEnabled(), 3000);
        QVERIFY(!stop->isEnabled());
        window.close();
    }

    void cancelDuringSwitchNeverStartsReceiver() {
        const QString output = temporary_.filePath("camera");
        QFile file(output);
        QVERIFY(file.open(QIODevice::WriteOnly));
        file.close();
        BridgeWindow window;
        window.findChild<QComboBox*>("transport")->setCurrentIndex(0);
        window.findChild<QLineEdit*>("output")->setText(output);
        auto* start = window.findChild<QPushButton*>("start");
        QTRY_VERIFY_WITH_TIMEOUT(start->isEnabled(), 5000);
        start->click();
        window.findChild<QPushButton*>("stop")->click();
        QTest::qWait(500);
        QVERIFY(!QFile::exists(temporary_.filePath("usb.args")));
        window.close();
    }

    void cleanupTestCase() { QSettings().clear(); }
};

int main(int argc, char** argv) {
    QApplication app(argc, argv);
    QCoreApplication::setApplicationName("Mobile Webcam GUI tests");
    QCoreApplication::setOrganizationName("mobile-webcam-tests");
    if (argc > 1 && QString::fromLocal8Bit(argv[1]) == "--check-persisted") {
        QCoreApplication::setApplicationName(QString::fromUtf8(qgetenv("AMB_GUI_TEST_APPLICATION")));
#ifndef Q_OS_WIN
        QSettings::setDefaultFormat(QSettings::IniFormat);
        QSettings::setPath(QSettings::IniFormat, QSettings::UserScope,
                           QString::fromUtf8(qgetenv("AMB_GUI_TEST_SETTINGS")));
#endif
        BridgeWindow window;
        return window.findChild<QComboBox*>("transport")->currentData() == "lan" &&
               window.findChild<QLineEdit*>("lanHost")->text() == "192.168.4.42" &&
               !window.findChild<QCheckBox*>("mirror")->isChecked() &&
               window.findChild<QCheckBox*>("verticalFlip")->isChecked() &&
               window.findChild<QButtonGroup*>()->button(90)->isChecked() ? 0 : 1;
    }
    GuiTest test;
    return QTest::qExec(&test, argc, argv);
}
#include "gui_test.moc"
