# 02 — 能力通知と切替境界

解除する制約: PCから設定できないことと、受信途中の設定変更が接続を壊すこと。
境界: AMB1制御メッセージ、Androidの撮影所有者、Rust workerの映像世代。

## 対応確認

新APKからQualityCapabilitiesを通知する。Wi-Fiは従来HELLO_ACK後、USBはAccessorySession開始後。
カメラ権限がなければ権限待ちを通知し、既存のスマホ操作で許可された後に一覧を更新する。
旧PCは未知の受信種別を無視する既存挙動を使う。新PCは能力通知前に新コマンドを送らない。
通知がない旧APKも接続エラーにせず従来映像を受け取る。
能力は現在の選択カメラだけ。カメラIDをPCから自由指定する機能は追加しない。

拡張payloadの形式はlittle-endianの固定フィールド＋件数付き一覧。
拡張版、catalogRevision、requestId、streamEpoch、modeの寸法／fpsを共通fixtureで固定する。
種別番号と正確なバイト配置は、この工程の最初にWire定義と照合して決め、両言語で同じfixtureを読む。
制御payload上限16 KiB、一覧最大256組、進行中の適用1件。超過一覧を黙って切り捨てない。
上限を超える場合は機能利用不可の能力通知を返し、設定操作だけを無効にして従来の配信を続ける。
新しい制御種別の長さは確保前に検証する。GUIへの転送も既存の16 KiB制御行上限を超えない形にし、境界fixtureを持つ。
一覧の分割転送・汎用RPC・新しいポート・接続方式は作らない。

## 適用

1. PCが希望モードとcatalogRevisionをPrepareQualityで送る。
   Androidは許可・前面状態・対応モードを確認。拒否なら配信中のモードを維持して返す。
2. 配信中ならAndroidの既存撮影所有者がカメラ／エンコーダーを閉じ、古いcallbackを無効にする。
   FramedOutboundの古いAU・codec config・VIDEO_CONFIGを捨て、単一writerの順序上でPausedを返す。
   Pausedより後に古い映像が出ないことが契約。
3. PC workerはPausedを受けてデコーダー・変換領域・仮想カメラ出力を解放する。
   新しいstreamEpochを開始し、最後にCommitQualityを送る。USB／TCP接続は保つ。
4. Androidが新モードで開始し、適用結果と従来形式のVIDEO_CONFIG、設定付きキーフレームを送る。
   PCは新世代内で寸法とPTSを検証し、出力の再開を確認して適用済みを報告する。

カメラ未開始時は、Prepareで次の開始用モードを設定して応答する。
PCは撮影を開始せず、スマホの「カメラ開始」を待つ。既にユーザーが開始した配信だけを再開できる。
同一requestIdの重複は同じ結果を返し、二度停止／起動しない。古いrevision／epoch／応答は適用しない。
中断・権限取り消し・背景移行で撮影を再開しない。

適用全体の期限は5秒、Stopは既存1秒。画面描画と独立して受信・制御を処理する。
現在の120秒receive待ちにGUIのStop／Applyを閉じ込めず、期限と入力を処理できる読み取りにする。
期限切れは適用失敗として撮影を停止、接続を閉じて待機へ戻す。自動復旧は行わない。
Androidにも未Commitの準備状態の5秒期限を持たせ、PC消失時は停止を維持して準備状態を解放する。
出力の再作成が失敗した場合も停止し、旧設定への無断ロールバックはしない。
Apply後に失敗しても、希望値・最後の適用値・エラーを混同しない。

## 所有と確認

MainActivityのカメラ所有を再利用し、readerスレッドから直接Camera2を再起動しない。
AccessorySession／LanSessionは制御の配送、FramedOutboundはフレーム順序を担当する。
PC SessionControllerは要求、workerは受信順序と映像世代を所有する。もう一つの接続状態機械を作らない。

両言語のgolden fixtureとUSB／TCPの合成peerで、旧peer、新peer、拒否、重複、遅延ACK、
古いcallback、旧フレーム混入、切替なしのVIDEO_CONFIG変更、PTSリセット、Stop／終了／切断を確認する。
既存のwire・worker・キーフレーム復旧・単調PTS・親プロセス終了チェックを維持する。
観察入口は制御順序とepochを表示するfixture。映像を見た証明とは分ける。

未解決: USB接続を保った切替の実機確認、消費アプリ使用中の出力再作成。
委任する判断: 種別番号・フィールド配置・内部名・期限付き読み取りの実装方式。
状態とwriter境界が単体fixtureで証明できない場合、UI工程へ進まない。
