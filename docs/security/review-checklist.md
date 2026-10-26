# 人間レビュアー向けチェックリスト（R4 事前）

AI が `main` `791568606368da074529701ac3a2bdcf78dfc2e4` を読み、この PR の2修正を足した所見である。人間レビューの合格にはしない。

所見の意味:

| 印 | 意味 |
|---|---|
| 確認済み | 記載したファイルを読み、その振る舞いがソースにある |
| 要確認 | ソースは読んだが、実行・別実装・見落としがあり、人間が同じ箇所を見るべき |
| 問題 | ソース上の欠陥。この PR で直したものは「この PR で修正」と書く |
| 要判断 | 欠陥と断定せず、製品の選択として残す。根拠を書く |
| 未確認 | 実行もソースも足りない |

## 1. TLS 1.3 と証明書ピン

見る場所: `host-rs/src/secure.rs` の `PinnedCertificate`、`SecureTcp::connect`。`android/.../SecureLanIdentity.kt` の `listener`、`keyStore`。`android/.../LanServer.kt` の `authenticate`。

| 項目 | 所見 |
|---|---|
| ホストのプロトコル版 | 確認済み。`with_protocol_versions(&[&rustls::version::TLS13])` のみ |
| スマホのプロトコル版 | 確認済み。リスナと受理ソケットの `enabledProtocols = arrayOf("TLSv1.3")` |
| ピン | 確認済み。証明書 DER の SHA-256 が一致しないと `verify_server_cert` が失敗する。チェーンと名前は捨てている |
| ハンドシェイク署名 | 確認済み。`verify_tls13_signature` を呼ぶ。`verify_tls12_signature` はトレイト用に残る。設定した版は 1.3 のみ |
| 失敗後の平文 | 確認済み。`SecureTcp::connect` にフォールバックはない。本番 LAN は `host-rs/src/video.rs` の `receive` が `SecureTcp::connect` だけを使う |
| 平文 `Tcp::connect` | 確認済み。空 HELLO を送る。呼び出しは `host-rs/src/transport.rs` のテスト。本番 LAN 分岐ではない |
| 暗号スイートのさらなる制限 | 要確認。版は 1.3 のみ。スイートの明示的な絞り込みはソースにない |
| TLS 1.2 ClientHello を実際に拒否するか | 未確認。このレビューでは送っていない |
| セッション再開 | 確認済み（ソース）。rustls 0.23.46 の `client/tls13.rs` は再開時に証明書を再検証せず `assertion()` を置く。`SecureTcp::connect` は呼び出しごとに新しい `ClientConfig` を作るので、前回の接続のチケットは次の `connect` に残らない。同一 `connect` 内の `config.clone()` はメモリ上のストアを共有する。別ピアはそのチケットを復号できない。同一 `dial` で同じサーバがチケットを受理したときだけ、その試行ではピン関数が再実行されない。再開そのものの実行は未確認 |
| 証明書の期限 | 要確認。ピン関数は `notAfter` を見ない。生成時の期限は `SecureLanIdentity.keyStore` の `Date(4_000_000_000_000L)` |

## 2. 128 bit アクセスキー

見る場所: `SecureLanIdentity.authenticates` / `newSecret` / `persist`。`LanServer.authenticate`。`host-rs/src/transport.rs` の `pair_until`。`host-rs/src/settings.rs` の `Debug` と `read_file`。`host-rs/src/settings/registry.rs`。

| 項目 | 所見 |
|---|---|
| 長さ | 確認済み。生成は 16 バイト。HELLO は 16 バイト以外を拒否（`readHelloWithDeadline`）。ホストの `parse_hex(..., 16)` |
| 比較 | 確認済み。`MessageDigest.isEqual`。呼び出しは `credential.size == 16 && authenticates`。長さが違うときの `isEqual` の実装は AOSP を読んでいない（未確認） |
| 単回性 | 確認済み。ローテーションまで再利用される。HELLO の再送は、キーを持つ者には新しい TLS 上でも通る形である。PC 証明の nonce とは別 |
| スマホの保存 | 確認済み。`encrypted_secret_v1` は AES-GCM。平文キー `secret` は `persist` の commit で消す。Keystore 鍵がないバックアップからの復元は、このレビューでは実行していない（未確認） |
| `allowBackup` | 確認済み。`AndroidManifest.xml` は `false` |
| Linux の設定ファイル | 確認済み。`read_file` は `lanKey=` を含み mode の group/other ビットが立つファイルを拒否する。0600 は暗号化ではない（`docs/security.md` と同じ） |
| システム全体のフォールバック | 確認済み。`settings.rs` の読込はフォールバックから `lanKey` と `lanFingerprint` を採用しない、とコメントと `system_fallback_never_supplies_pairing_credentials` がある。このレビューではそのテストを実行する（PR 本文） |
| Windows | 確認済み（ソース）。`lanKey` は `lanKeyProtected` の DPAPI を優先し、HKLM からは読まない。`lanFingerprint` も HKLM を飛ばす。レガシー平文 `lanKey` は HKCU からのみ読み、保存成功後に消す。DPAPI 呼び出しはこの環境では未確認 |
| ローテーション | 確認済み。`rotate` は新しいキーを保存し、Wi-Fi サーバを閉じる。認可レコードは消さない（ダイアログ文言と `per-pc-authorization.md`） |

## 3. PC ごとの認可 v2

見る場所: `protocol/client-authorization-v2.md`。`PhoneAuthorization.kt`。`ClientAuthorization.kt` の `evaluate` / `validLabel`。`host-rs/src/client_auth.rs` の `exchange`。`host-rs/src/wire.rs` の `client_auth_message` / `valid_label`。`AuthorizedMediaSession.readMedia`。

| 項目 | 所見 |
|---|---|
| 映像の前に認可 | 確認済み。`video.rs` は `Outcome::Authorized` のあとでデコーダを開く。スマホは `permitted()` まで capabilities と映像を出さない |
| チャレンジは 1 回 | 確認済み。`receive` は `phase == PROOF` だけ。nonce はセッション生成時に一度。署名入力に含まれる |
| 再送 | 確認済み（ソース）。同じセッションの 2 通目は phase 検査で落ちる。別セッションは別 nonce。キャプチャ再送の実行は未確認 |
| 署名を先に見る | 確認済み。`ClientAuthorization.evaluate` は署名と ID の不一致を、一覧を許可にする前に拒否する |
| LAN binding | 確認済み。ホストは自分のピンと比較し、違えば `"authorization phone certificate binding mismatch"`。スマホの USB binding は全ゼロ必須、LAN は非ゼロ必須（`Wire.requirePhoneBinding`） |
| ラベル | 確認済み（この PR で修正）。修正前、`Wire.strictLabel` は `isISOControl()` だけで、Rust の `valid_label` と `ClientAuthorization.validLabel` が拒否する Format / 行区切り / 段落区切りをデコードが通していた。認可経路は `PhoneAuthorization.receive` が `validLabel` を再度呼ぶので、その経路では許可になっていなかった。デコーダ単体は仕様より弱かった。今は `Wire.labelScalarsAreSafe` をデコーダと `validLabel` が共有する。テストは `QualityWireTest.auth_labels_reject_format_and_line_separators_at_the_wire_decoder` |
| 期限 | 確認済み。証明待ち 5 秒、承認待ち 60 秒、未知要求のクールダウン 30 秒。`tickLocked` は期限を延ばさない。壁時計はスマホの `currentTimeMillis`。時計を戻したときの実時間は未確認 |
| 拒否のあと | 確認済み。`client_auth::exchange` は `Rejected` を返し、`video.rs` はそこで終了する。同じチャレンジで証明をやり直さない |
| ホストの Pending | 確認済み。Pending は 1 回だけ人間待ち 60 秒に切り替える。2 回目は `"repeated pending authorization outcome"` |

## 4. USB でのペアリング情報

見る場所: `MainActivity` の `sendPairingAction`。`Wire.encodePairingInfo`。`AuthorizedMediaSession.sendPairingInfo`。`host-rs/src/wire.rs` の `PairingInfo`。`gui.rs` の `take_pairing_info` 付近。

| 項目 | 所見 |
|---|---|
| 送る条件 | 確認済み。詳細を開いた認可済み USB セッションで、ボタンを押したとき。未認可の `sendPairingInfo` は書かない |
| 中身 | 確認済み。IPv4、証明書 SHA-256、16 バイトキー。認可フレームではない |
| ケーブルの信頼 | 要判断。プロトコルは「ケーブルを信頼してキーを運ぶ」と書いている。暗号的なスマホ証明はない。この前提を受け入れるかがレビューの判断 |
| PC が保存するタイミング | 要判断。`gui.rs` はフレームを受け取った時点で `lan_host` / `lan_fingerprint` / `lan_key` を保存する。バナーの「Wi-Fiに切り替える」はそのあと。「あとで」はバナーだけ消す。指紋を PC 画面で突き合わせるステップは、この経路にはない |
| アドレスが 5 件以上 | 要判断。スマホは列挙順の先頭 4 件を送り、余りは捨てる。テスト `addresses_keep_their_order_and_four_are_carried` がその順序を固定している。PC は届いた中でプライベートまたはリンクローカルを優先する。プライベートが 5 件目以降だと送られない |
| 切り替えたあとの接続 | 確認済み。`pending_switch_to_lan` のあと `auto_connect` が通常の TLS 接続を始める。認可交換は省略されない |

## 5. ホストの入力検証

見る場所: `host-rs/src/wire.rs` の `Header::decode` / `validate_payload_size`。`host-rs/src/transport/framing.rs`。`PairingInfo::decode`。`client_auth::exchange`。

| 項目 | 所見 |
|---|---|
| ヘッダ | 確認済み。マジック `AMB1`、版 1、16 MiB 超は拒否。型ごとの長さは `validate_payload_size` |
| 認可フレーム | 確認済み。チャレンジは 66 バイト。証明は 131–163。結果は 6。ラベル長と実長が一致しないと `ClientAuth::decode` が拒否する（`wire.rs` の該当関数） |
| `PAIRING_INFO` | 確認済み。版、件数 1..=4、末尾の余りを拒否。`fits` と `decode` が同じ長さ |
| 未知の型 | 要確認。`validate_payload_size` の `_ => true` は、16 MiB 以下の未知型をヘッダとして受理する。認可後の `receive_stream` は映像と既知制御以外を無視して読み続ける。拒否に変えると、将来の型を黙って捨てる今の挙動が変わる |
| 認可中の余計なフレーム | 確認済み。`exchange` は最初が `CLIENT_CHALLENGE`、その後は `CLIENT_AUTH_RESULT` 以外をエラーにする。未認可の `PAIRING_INFO` はここで落ちる |
| fuzz | 要確認。`host-rs/fuzz/fuzz_targets/wire_decode.rs` と corpus がある。このレビューでは fuzzer を回していない（未確認: 実行） |

## 6. スマホの受信

見る場所: `Wire.readHostFrame` / `readFrame`。`AuthorizedMediaSession.readMedia`。`SessionControl.dispatch`。`PhoneAuthorization.receive`。

| 項目 | 所見 |
|---|---|
| 確保の前に長さ | 確認済み。`readFrame` は `payloadBounds` の範囲外ならペイロードを読まない。ホスト向け未知型は `readHostFrame` がエラー |
| 未認可の制御 | 確認済み。`isAuthorized()` が偽のあいだは `gate.receive` だけ。`CLIENT_AUTH` 以外はセッション終了 |
| 認可後の制御 | 確認済み。`SessionControl` は PING、HOST_HELLO、IDR、PREPARE、COMMIT だけ。他は `"unexpected authenticated host command"` |
| PING | 確認済み。1 秒あたり 64 を超えると `require` が失敗し、セッションが落ちる |
| IDR と HOST_HELLO | 確認済み。超過分はエラーにせず捨てる。上限は各 8/秒 |
| 制御キュー | 確認済み（ソース）。`FramedOutbound` の制御上限は、溢れたとき `onError`。このレビューでは負荷をかけていない（未確認） |

## 7. ログと秘密

| 箇所 | 所見 |
|---|---|
| `Settings` の `Debug` | 確認済み。`lan_key` は `<redacted>`。`lan_fingerprint` は全文。本番の `println` / `eprintln` で `Settings` を `{:?}` している箇所は見つからなかった |
| `PairingInfo` の `Debug` | 要確認。導出 `Debug` は `key` を含む。本番ログで `{:?}` している箇所は見つからなかった。テスト失敗時の表示には出る |
| `client_auth` の `AMB_AUTH_TRACE` | 確認済み。環境変数があるとき nonce、binding、ラベル、公開鍵長を `eprintln` する。アクセスキーは出さない。既定では出さない |
| `usb.rs` の `AMB_USB_TRACE` | 確認済み。エンドポイント番号とスクラッチサイズだけ |
| `--usb-dump` | 要判断。`usb_probe.rs` は最大 512 バイトの生バイトを 16 進で出す。その窓に `PAIRING_INFO` があればキーのバイトが含まれる。コマンドの目的がダンプなので、この PR では伏せていない |
| `--headless` | 確認済み（この PR で修正）。修正前は worker の JSON 行をそのまま `println` していた。`Event::PairingInfo` は `key` を持つ。今は `printed_event_line` が `key` を `<redacted>` にする。パースできない行に `pairing_info` が含まれるときは本文を出さない。GUI へのパイプは変えていない。テストは `a_printed_pairing_event_does_not_contain_the_access_key` |
| Android `Log` | 確認済み。`SecureWifiGate` はリセット成否と、認可ストレージ不能の例外。キー文字列の補間は、読んだ呼び出しにはない。例外メッセージがキーを含む実装は見つからなかった。全例外型のメッセージは未確認 |
| 画面 | 確認済み。キー表示中だけ `FLAG_SECURE`。`onPause` でテキストを空にしフラグを外す |
| worker パイプ | 要判断。GUI はキーを受け取る必要があるので、パイプ上の JSON にはキーがある。同じ利用者のデバッガからは読める。ログファイルへは書いていない |

## 8. 既定の LAN 公開範囲

見る場所: `SecureLanIdentity.listener`。`LanServer.localIpv4Addresses` / `endpoints` / `acceptLoop`。

| 項目 | 所見 |
|---|---|
| 待受 | 確認済み。ポートのみ。アドレス引数はない。Java の `createServerSocket(int)` はワイルドカード待受と文書化されている。端末で `ss` 等は取っていない（未確認） |
| 列挙 | 確認済み。非ループバックの上がっている IPv4 全部。Wi-Fi 限定のフィルタはない |
| 同時接続 | 確認済み。未認証ハンドシェイクは 4。アクティブセッションがいるあいだの新規 accept は閉じる。ハンドシェイク期限は 5 秒でソケットを閉じる |
| Wi-Fi だけに縛るか | 要判断。テザリング、VPN、ホットスポットを切ると、今の「上がっている IPv4」では届かなくなる。ソースは切っていない |

## 9. 再接続、再送、平文への退行

| 項目 | 所見 |
|---|---|
| 再接続（USB） | 確認済み（ソース）。`HOST_HELLO` は使用済みセッションを終わらせ、次のチャレンジは新しい nonce。実機の AOA 残留はこのレビューでは未確認 |
| 再接続（Wi-Fi） | 確認済み。保存済みピンとキーで `auto_connect`。毎回 `SecureTcp::connect` と `exchange`。キーだけの再送は通り、証明の再送は nonce が新しいので通らない、という形 |
| 証明の再送 | 確認済み（ソース）。実行は未確認。上記 §3 |
| 平文退行 | 確認済み（ソース）。本番 Wi-Fi に退行分岐はない。スマホは 16 バイト HELLO 以外を拒否する。旧ホストとの相互運用試験は未確認 |
| 片側だけダウングレード | 確認済み（文書とソースの一致）。`docs/migration-tls-wifi.md` は両端を同じ世代へ戻す、と書く。現行ホストが古い平文スマホへ平文で繋ぎに行く分岐は、`video.rs` にはない |

## レビュアーが最初に開くファイル

1. `host-rs/src/secure.rs` — ピンと TLS 1.3、フォールバックの有無
2. `android/.../LanServer.kt` と `SecureLanIdentity.kt` — 待受、HELLO、キー比較
3. `android/.../PhoneAuthorization.kt` と `AuthorizedMediaSession.kt` — 1 チャレンジ、未認可の制御
4. `host-rs/src/client_auth.rs` と `host-rs/src/wire.rs` の認可デコード — binding と長さ
5. `android/.../Wire.kt` の `encodePairingInfo` / `labelScalarsAreSafe` と `gui.rs` の保存 — USB でキーがどこへ入るか
6. `host-rs/src/headless.rs` の `printed_event_line` — 標準出力からキーが消えているか
7. `host-rs/src/settings.rs` と `settings/registry.rs` — キーの保存先

この一覧を読んでいない領域（依存クレートの中、実機の待受アドレス、fuzzer の再実行、DPAPI の実行）は上の「未確認」のままである。
