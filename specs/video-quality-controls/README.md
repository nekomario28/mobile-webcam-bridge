# 解像度・フレームレートの設定

設計のみ。2026-10-06。調査基点: `f81da2c54b6adab430025e3b4b1742aa4e5e5ca0`。
ブランチ: `design/video-quality-controls`。アプリ・通信・リリースは未変更。

## Next Agent Prompt

実装開始の依頼を受けるまで、設計だけを扱ってください。
開始時は最新のmainとの差を確認し、[01](slices/01-modes.md)から進めてください。
端末の対応モード、USBでの切替境界、Windows利用アプリ側のfpsは実機未確認です。
各工程の終了時に、この状態と次の着手点を更新してください。

- [ ] [01: Androidの対応モードと設定解決](slices/01-modes.md)
- [ ] [02: 能力通知と安全な切替](slices/02-control.md)
- [ ] [03: PCの詳細UIと保存](slices/03-ui.md)
- [ ] [04: Linux／Windows・実機での確認](slices/04-integration.md)

## 目標と操作

PCの「詳細」に解像度とフレームレートの選択肢を追加する方針。
現時点の設計前提はPCを主操作面とすること。操作場所の希望が異なれば実装前に更新する。
Androidは対応モードの提供・撮影設定の適用を担当し、設定画面を二重に持たせない。
機種別の選択肢や初期設定ウィザードは追加しない。EN/JAは既存のOS言語判定を使う。

```text
詳細
  解像度           [自動             ▾]
  フレームレート   [自動             ▾]
  [適用]                  ← 接続中に変更がある時だけ
```

- 詳細は引き続き閉じて起動。未変更時の説明文・保存完了通知は出さない。
- 解像度は実寸 `1280×720`、fpsは `30 fps` と表示。対応する組み合わせだけを選べる。
- 初期値は自動。対応していれば従来の1280×720・30 fpsを優先する。
- 未接続時は保存済みの希望値を表示。端末の能力が不明なら自動以外を選べると断言しない。
- 接続中の変更は下書きに保持し、一度の「適用」で反映。選ぶたびに配信を切らない。
- 初回のカメラ開始・権限許可はスマホで行う。PC操作で撮影権限を迂回しない。
- 適用中は映像が一度止まる。停止・キャンセルは常に使え、勝手に再試行しない。
- 保存した希望値と適用済み値を分ける。再起動後も希望値を復元し、別のスマホで再検証する。

## 現状から必要になる変更

| 現状の根拠 | 必要な変更 | 工程 |
| --- | --- | --- |
| [MainActivity.kt](../../android/app/src/main/java/dev/nekomario/amb/MainActivity.kt) の `startCamera720p` が1280×720・30 fps・6 Mbpsを指定 | 固定値を対応モードから解決 | 01 |
| [CameraCatalog.kt](../../android/app/src/main/java/dev/nekomario/amb/CameraCatalog.kt) はサイズとAE範囲を別々に列挙。`chooseSize` は近いサイズへ代替 | サイズ・fps・同じエンコーダーの交差を求め、明示指定を無断で代替しない | 01 |
| [AccessorySession.kt](../../android/app/src/main/java/dev/nekomario/amb/AccessorySession.kt)／[LanServer.kt](../../android/app/src/main/java/dev/nekomario/amb/LanServer.kt) は未知のPCメッセージで失敗 | 新APKから先に対応通知。通知前はPCから新コマンドを送らない | 02 |
| [video.rs](../../host-rs/src/video.rs) は途中のVIDEO_CONFIG変更で失敗 | 切替を明示した境界でのみデコーダー・出力を閉じ直す | 02 |
| [gui.rs](../../host-rs/src/gui.rs)／[settings.rs](../../host-rs/src/settings.rs) に品質設定がない | 詳細に2項目を追加、既存の保存先を利用 | 03 |
| [Linux出力](../../host-rs/src/output/linux.rs) はS_PARMの戻り値を再検証せず、[Windows出力](../../host-rs/src/output/windows.rs) はfps引数を使わない | 撮影目標fps・受信fps・利用アプリの出力fpsを区別して確認 | 04 |

## 所有と契約

AndroidのCameraCatalogが撮影・エンコード可能な組み合わせを所有する。
PCはその一覧を現在の出力制約で絞り込み、端末の能力を独自に推測しない。
CameraEncoderSessionとH264Encoderは同じ解決済みモードを使う。
SessionControllerがユーザーの適用／停止要求を所有し、workerの応答が実際の適用状態を決める。
GUIの選択だけで「適用済み」にしない。USBとWi-Fiは同じ制御契約を使う。

既存の[映像契約](../rust-host-rewrite/contracts.md)を無条件に緩めない。
切替の合意がないVIDEO_CONFIG変更は引き続き失敗。合意した切替だけ新しい映像世代として開始し、
その世代内のPTS単調増加、CONFIG_INCLUDED付きキーフレーム、寸法一致を維持する。
接続自体の世代管理、Stopの1秒終了期限、キュー・ログの既存上限も維持する。

旧PC／APKとの組み合わせでは新機能を無効にし、従来の受信を続ける。
既存AMB1ヘッダー、HELLO/ACK、VIDEO_CONFIGを変更せず、別の互換アダプターや設定移行は作らない。
新機能の対応確認は必須で、旧APKへ探索用の未知コマンドを送る方式は採らない。

## 範囲と見送り

今回の対象は撮影・エンコードの解像度と**目標**fps。PCで拡大縮小しただけの映像を撮影設定と呼ばない。
初回は通常のCamera2セッションの整数fps、最大60 fps、3840×2160以内を対象にする。
これは新しい選択UIの製品上限であり、全端末での4K／60 fps対応を意味しない。
初期のfps候補は15／24／25／30／50／60。端末が対応しない候補は除く。
サイズは端末の実際の一覧を利用し、720p／1080pだけに固定しない。

ビットレートは自動で内部解決する。手入力、画質プリセット、カメラ切替、音声、iOS、高速撮影、
小数fps、新しい仮想カメラ、接続の自動再試行は別の計画。
既存Camera2→MediaCodec、FramedOutbound、Rust worker、Settings Store、FFmpeg、
v4l2loopback／Unity Captureを流用し、新しい実行環境・常駐プロセス・依存は原則追加しない。

Androidだけに選択UIを置く案は通信変更が少ないが、PCからの操作を満たさない。
PCだけでリサイズする案は撮影負荷や送信量を変えられない。
映像設定の変化を検出して勝手に再開する案は、意図しない設定変更と古いフレームを隠すため採らない。

## 調査と証明の境界

Android公式仕様では、サイズごとの最小フレーム時間とAE範囲が撮影速度を制約する。
最小時間が0なら未取得として扱い、無制限とは扱わない。
[StreamConfigurationMap](https://developer.android.com/reference/android/hardware/camera2/params/StreamConfigurationMap)、
[CONTROL_AE_TARGET_FPS_RANGE](https://developer.android.com/reference/android/hardware/camera2/CaptureRequest#CONTROL_AE_TARGET_FPS_RANGE)。

同じH.264エンコーダーのサイズ・速度・ビットレート制約も必要。
APIが対応を返しても、実際の持続速度の保証にはならない。
[VideoCapabilities](https://developer.android.com/reference/android/media/MediaCodecInfo.VideoCapabilities)。
KEY_FRAME_RATEはエンコーダーへの目標値なので、実測fpsと分ける。
[MediaFormat](https://developer.android.com/reference/android/media/MediaFormat#KEY_FRAME_RATE)。

今回の証拠はソース調査と設計だけ。設定変更・実機・UI・CIはすべて **NOT RUN**。
将来の画面確認では既存UIとのcompare-screenshots、最後に未誘導のscreenshot-critiqueを行う。
合成映像・Wine・単体テストを、実機撮影やWindows利用アプリの成功と取り違えない。
