#include <QCoreApplication>
#include <QSettings>
#include <QString>
#include <iostream>
#include <memory>
int main(int argc,char** argv){
    QCoreApplication app(argc,argv);
    if(argc!=3)return 2;
    std::unique_ptr<QSettings> owner(QString::fromLocal8Bit(argv[2])=="--native" ? new QSettings("nekomario28", "Mobile Webcam") : new QSettings(QString::fromLocal8Bit(argv[2]),QSettings::IniFormat));
    QSettings& settings=*owner;
    if(QString::fromLocal8Bit(argv[1])=="write"){
        settings.setValue("transport","lan");settings.setValue("lanHost","192.168.4.42");
        settings.setValue("output",QString::fromUtf8("/tmp/カメラ, 1")+QChar(0x1f)+"ZA");
        settings.setValue("decode","off");settings.setValue("rotation",90);
        settings.setValue("mirror",false);settings.setValue("verticalFlip",true);
        settings.setValue("unknown",QByteArray("unchanged"));settings.sync();
        return settings.status()==QSettings::NoError?0:3;
    }
    if(settings.value("lanHost").toString()!="192.168.4.43"||settings.value("rotation").toInt()!=270||
       settings.value("mirror").toBool()||!settings.value("verticalFlip").toBool()||
       settings.value("unknown").toByteArray()!=QByteArray("unchanged")||
       settings.value("output").toString()!=QString::fromUtf8("/tmp/カメラ, 1")+QChar(0x1f)+"ZA")return 4;
    std::cout<<"PASS Qt -> Rust -> Qt settings, including Unicode and unknown values\n";
}
