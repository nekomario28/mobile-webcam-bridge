# Mobile Webcam

[English](README.md) | **日本語**

Android スマートフォンを USB または Wi-Fi 経由で PC の Web カメラとして使用します。USB は Android Open Accessory を使用するため **USB デバッグ（ADB）は不要**、Wi-Fi はスマートフォン証明書のピン留めを伴う TLS 1.3（Android 10 以上）で動作します。表示言語（英語／日本語）はシステム言語に追従し、PC 側ウィンドウにはライブプレビューが表示されるため、カメラ利用アプリを開く前に画角を確認できます。

## ダウンロードと環境準備

[Releases](https://github.com/nekomario28/mobile-webcam-bridge/releases/latest) から Android APK とお使いの OS 向けデスクトップアプリをダウンロードします。

- **Windows:** 初回のみ **Setup.exe** を実行して仮想カメラを登録します（DX12 対応グラフィックスドライバが必要。USB 接続には libusb 互換のアクセサリドライバも必要です）。登録後は ZIP 版をポータブルに利用できます。
- **Linux:** AppImage に実行権限を付与して起動します（ディストリビューションの `v4l2loopback` カーネルモジュール、Vulkan ドライバ／ローダー、glibc 2.39 以上が必要です。ソースビルド時に `--features pipewire` を指定した場合は `v4l2loopback` の代わりに PipeWire ビデオソースを使用します）。

## 使い方（クイックスタート）

1. スマートフォンと PC の両方で **Mobile Webcam** を開きます（起動時は USB モードです）。
2. **USB 接続:** ケーブルを接続し、PC 側で **接続**（Connect）を押します（1 台のみ接続されている場合は自動選択されます）。スマートフォン側で USB アクセサリのアクセスを許可（「常に許可」にチェック）し、この PC を許可（90 日間有効）した上で **カメラ開始**（Start camera）を押してカメラ権限を許可します。PC の承認だけではカメラ配信は開始されません。
3. **Wi-Fi 接続（初回・Android 10 以上）:** 手順 2 で USB 経由の PC 承認を済ませた状態で、スマートフォン側の **Wi-Fi設定をPCへ送る**（Send Wi-Fi pairing to PC）を押し、スマートフォンの **Wi-Fi** をタップしてから PC 側の **Wi-Fiに切り替える**（Switch to Wi-Fi）を押します（手入力は不要です）。USB ケーブルを使えない場合は、スマートフォン側の **詳細 → 接続キーを表示**（Details → Show pairing key）を開き、画面上の SHA-256 証明書フィンガープリントを目視確認した上で、スマートフォンのアドレス・フィンガープリント・128-bit キーを PC に入力して **接続** を押します。
4. カメラアプリや Web 会議アプリでカメラとして **Mobile Webcam** を選択します。

## セキュリティと互換性

現行リリース（`v0.3.x`）では、Wi-Fi で証明書ピン留め付き TLS 1.3 と 128-bit アクセスキーを強制し、USB・Wi-Fi の両方で [PC ごとの承認（per-PC authorization）](docs/per-pc-authorization.md) を要求します。Android アプリと Rust 製デスクトップホストは必ずセットで更新してください。旧ビルド（`v0.2.0` 以前および classic Qt `v0.1.2`）は認証なしの平文 Wi-Fi を使用しており、新しい PC 承認ゲートとも互換性がありません（`v0.3.0` 未満の Wi-Fi ビルドは信頼できないネットワークに晒さないでください）。ペアリング／承認プロトコルの第三者による独立セキュリティレビューおよび Windows 実機での DirectShow コンシューマ検証は未完了です。詳細は [セキュリティ仕様](docs/security.md)、[TLS Wi-Fi への移行](docs/migration-tls-wifi.md)、[ロードマップ](docs/roadmap.md) を参照してください。

## ドキュメント・ライセンス

[接続・ヘッドレス CLI・セットアップ詳細](host-rs/README.md) ·
[アーキテクチャ](docs/architecture.md) ·
[セキュリティ](docs/security.md) ·
[ロードマップと検証記録](docs/roadmap.md) ·
[ビルドと検証](specs/rust-host-rewrite/implementation.md) ·
[Classic Qt v0.1.2](docs/classic-qt.md)

ソースコード: [Apache 2.0](LICENSE)。同梱コンポーネントには個別のライセンスが適用されます。[第三者ライセンス通知](THIRD_PARTY_NOTICES.md)、[FFmpeg ソース提供案内](docs/licenses/FFMPEG_SOURCE_OFFER.md)、および各配布パッケージ内のライセンスファイルを参照してください。
