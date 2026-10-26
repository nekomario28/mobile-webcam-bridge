# 脅威モデル（R4 事前の AI 自己レビュー）

これは人間によるセキュリティレビューの代替ではない。レビュアーに渡す材料である。
記述は、2026-10-24 時点の `main`（`791568606368da074529701ac3a2bdcf78dfc2e4`）を読んだ事実と、この変更で直した2点に限る。実行していないことは「未確認」と書く。

既存の [security.md](../security.md)、[per-pc authorization](../per-pc-authorization.md)、[wire-secure-lan](../../protocol/wire-secure-lan.md)、[client-authorization-v2](../../protocol/client-authorization-v2.md) が実装の説明である。ここはそれらを攻撃者と境界の形に並べ替えたもの。

## 資産

| 資産 | どこにあるか（読んだ場所） | 失ったときの影響 |
|---|---|---|
| スマホの TLS 秘密鍵（P-256、エイリアス `mobile_webcam_wifi_tls_v2`） | Android Keystore。`SecureLanIdentity.keyStore` | ピンと一致する証明書を、鍵を持つ者が提示できる |
| 証明書の SHA-256（ピン） | スマホ画面、PC の `lanFingerprint`、USB の `PAIRING_INFO` | 秘密ではない。間違った値を信じると別の証明書を信頼する |
| 128 bit アクセスキー | スマホは `encrypted_secret_v1`（Keystore AES-GCM）。PC は Linux の設定ファイルまたは Windows の DPAPI 値 `lanKeyProtected` | キーを持つ者は、ピンされたスマホへの TLS のあと HELLO を通過できる。PC ごとの認可は別 |
| PC の署名秘密鍵 | `Identity`。Unix は利用者のファイル（作成時 mode 0600）。Windows は利用者スコープ | その PC として認可証明を作れる。ハードウェア証明ではない |
| 認可レコード（公開鍵、表示名、期限） | スマホ `ClientStore`（別エイリアスの AES-GCM）。PC 側は秘密鍵から公開鍵を導出 | 改ざんに成功すると、誰を許可したかの記録が変わる |
| 映像 | 認可後の TLS または USB | セッションを持てる者にカメラが見える |

## 信頼境界

```text
[利用者] --画面で指紋を照合、または USB で PAIRING_INFO を送る指示-->
[スマホアプリ]
    | TLS 1.3（待受はポートのみ。アドレス指定なし）
    | USB AOA（ケーブル）
[PC ホスト]
    | 同じ利用者のプロセス（GUI と worker のパイプ、設定ファイル、Identity ファイル）
[仮想カメラを読むアプリ]
```

読んだコード上の境界は次のとおり。

- **画面。** 指紋とアクセスキーは「接続キーを表示」のあいだ `FLAG_SECURE` が付く（`MainActivity.setPairingKeyVisible`）。`onPause` で文言とフラグを外す。クリップボードや別カメラによる撮影までは、このフラグは止めていない（既存の static review もそう書いている。このレビューでは端末で再確認していない。未確認）。
- **LAN。** 本番の Wi-Fi 経路は `video.rs` の `SecureTcp::connect` のみ。平文の `Tcp::connect` は `transport.rs` のテストから呼ばれる。本番の LAN 分岐からは呼ばれない。
- **USB。** 認可の前に `PAIRING_INFO` は送られない。`AuthorizedMediaSession.sendPairingInfo` は `permitted()` のときだけ書き、ボタン表示も認可済み USB セッションに限る（`MainActivity.applySecondaryVisibility`）。
- **PC の利用者。** 設定と Identity は利用者のファイルまたは DPAPI。同一利用者の別プロセスは、Unix の 0600 では区別されない。
- **他の Android アプリ。** `allowBackup="false"`。`SessionService` は `exported="false"`。`MainActivity` はランチャーと USB アタッチのため `exported="true"`。停止インテントは Activity 側で資格情報ではない、と `SessionService` のコメントにある。このレビューは他アプリからの呼び出しを実行していない（未確認）。

## 攻撃者（この範囲で想定するもの）

| 攻撃者 | 持っているもの | 持っていないもの |
|---|---|---|
| 同じ LAN にいる第三者 | スマホの待受ポートへの TCP | ピン、アクセスキー、PC 秘密鍵 |
| アクセスキーを得た LAN 上の者 | ピンとキー（または USB で渡された同等の値） | 認可済み PC の秘密鍵。キーだけでは証明ゲートを越えない、と `ClientAuthorization` のコメントと `PhoneAuthorization` の流れが言う |
| ケーブルの先の機器、または物理的な中継 | USB 上のバイト | スマホの TLS 秘密鍵。USB の phone binding は全ゼロで、電話の証明にはならない（`protocol/client-authorization-v2.md` と `Route::Usb`） |
| 同じ PC 利用者のソフト | 設定ファイル、Identity、worker パイプ | スマホの Keystore 鍵。ただし保存済みのキーと PC 秘密鍵は使える |
| 古い平文ホスト、または平文へ戻そうとするピア | 旧プロトコルの実装 | 現行スマホの平文待受。現行ホストの本番 Wi-Fi に平文フォールバック分岐はない |

ローカルの root、Keystore の突破、端末の画面を見ている人は、この文書の保証の外である。既存の `docs/security.md` も同じ線を引いている。

## 想定する脅威

各項目は「コードがそうなっている」ことと「実行で見ていない」ことを分ける。手順は書かない。

### 証明書のすり替え

ホストは提示された証明書 DER の SHA-256 を、利用者が入れたピンと比較する（`PinnedCertificate.verify_server_cert`）。不一致ならハンドシェイクを失敗させる。ホスト名は見ない。サーバ名は固定の `mobile-webcam.local` である。証明書の有効期限はこの検証関数では見ていない。生成する証明書の `notAfter` は `Date(4_000_000_000_000L)` である。

ハンドシェイク署名は `verify_tls13_signature` に渡す。`ClientConfig` は `TLS13` だけを指定する。スマホ側はリスナとソケットの `enabledProtocols` を `TLSv1.3` だけにする。TLS 1.2 の署名検証関数はトレイト実装として残っている。バージョン配列に TLS 1.2 は入っていない。

実際の TLS 1.2 ClientHello をこのレビューでは送っていない（未確認）。

### アクセスキーの再利用

キーは設置ごとの 16 バイトで、ローテーションまで同じ値である（`SecureLanIdentity.newSecret` / `rotate`）。HELLO は TLS の中で定数時間比較（`MessageDigest.isEqual`）される。長さは 16 を要求してから比較する。

したがって、キーを持つ者は新しい TLS セッションでも HELLO を再送できる。これは単回トークンではない。PC ごとの証明は別で、nonce を含む（次項）。

### 証明の再送

`PhoneAuthorization.Session` はセッションごとに `SecureRandom` で 32 バイトの nonce を作り、`phase == PROOF` のときだけ 1 通の `CLIENT_AUTH` を受ける。署名メッセージは `Wire.clientAuthMessage` が transport、phone binding、nonce、公開鍵、ラベルを連結したもの。次の証明は phase が変わっているため `"unexpected authorization proof"` でセッションを終える。

新しい接続は新しい nonce を出す。古い署名は、その nonce に対する検証を通らない、という形になっている。このレビューではキャプチャした証明の再送を実行していない（未確認）。

LAN の binding は、その接続でピンした証明書の SHA-256 である（`video.rs` が `lan_fingerprint` から `Route::Lan` を作る）。USB は 32 バイトのゼロである。

### 平文への退行

スマホの `authenticate` は TLS ハンドシェイクのあと、ペイロード 16 バイトの HELLO だけを受ける（`readHelloWithDeadline` の `credentialBytes`）。空の HELLO は長さ検査で落ちる。失敗時の UI 文言は汎用で、平文でやり直す分岐はない。

ホストの `SecureTcp::connect` は失敗後に `Tcp::connect` を呼ばない。`Tcp::connect` は空の HELLO を送るテスト用である。

両端を古い平文世代へ戻す手順は [migration-tls-wifi.md](../migration-tls-wifi.md) にある。現行ソースに、片側だけの失敗から平文へ落ちる分岐は見つからなかった。相互運用をこのレビューでは実行していない（未確認）。

### 再接続

USB は切断を AOA が報告しない、と `video.rs` と `PhoneAuthorization.onHostHello` のコメントにある。ホストは失敗の種類によっては `HOST_HELLO` を 1 回送り、交換をやり直す（`video.rs` の `a_hello_would_help` 分岐）。スマホは、すでにチャレンジを出したセッションを終える。次のセッションが新しい nonce を出す。

認可済みのあいだに来た `HOST_HELLO` は `SessionControl` が `onHostHello` に渡し、セッションを終える。未認可のあいだは `gate.receive` が `CLIENT_AUTH` 以外を拒否してセッションを終える。

PC の Wi-Fi は、保存済みのピンとキーがあれば起動時に `auto_connect` する。その接続も `SecureTcp::connect` と `client_auth::exchange` を通る。保存済みセッションチケットをプロセスの外に書くコードは、`secure.rs` にはない。`connect` のたびに `ClientConfig::builder` で新しい設定を作る。

依存クレート rustls 0.23.46 のソース（この環境の cargo registry）では、再開ハンドシェイクは証明書を再検証しない。`client/tls13.rs` は再開枝で `ServerCertVerified::assertion()` を置き、コメントが「証明書鎖はここでは再検証しない」と書いている。既定の `Resumption` はメモリ上のストアで、`ClientConfig` は `Clone` であり `resumption` のストアは `Arc` である。`SecureTcp::connect` は `config.clone()` を各アドレス試行に渡すので、同一 `connect` の中ではストアを共有する。別の `connect` 呼び出しは新しい `ClientConfig` なので、前のチケットは持たない。別のサーバはこちらのチケットを復号できない。同一 `dial` の中で、直前にハンドシェイクまで成功した同じサーバがチケットを受理した場合だけ、その試行ではピン関数が再実行されない。この再開を実行してはいない（未確認: 実行）。

### LAN の公開範囲

`SecureLanIdentity.listener` は `createServerSocket(port)` でポートだけを渡す。バインドアドレスを渡すコードはない。既定ポートは 48527。同時ハンドシェイクは 4、絶対期限は 5 秒（`LanServer`）。

`localIpv4Addresses` は、上がっていてループバックでない全インターフェースの IPv4 を列挙する。Wi-Fi だけに絞っていない。セルラ、VPN、リンクローカルも条件を満たせば含まれる。UI の待受表示と `PAIRING_INFO` の材料は、この列挙である。

待受ソケットが IPv6 でも受け付けるかは、このレビューでは観測していない（未確認）。

`PAIRING_INFO` は列挙順のまま最大 4 アドレスを載せ、余りは捨てる（`Wire.encodePairingInfo` と `PairingInfoWireTest.addresses_keep_their_order_and_four_are_carried`）。PC は受け取った中からプライベートまたはリンクローカルを優先する（`PairingInfo::preferred_address`）。5 件目以降のプライベートアドレスは、列挙が後ろだと送られない。順序を変えるかは設計判断である（チェックリスト）。

### USB でのペアリング情報

利用者がボタンを押したとき、認可済み USB セッションに `TYPE_PAIRING_INFO` を送る。中身はバージョン、IPv4、証明書 SHA-256、アクセスキーである。ケーブル上は TLS ではない。

PC は認可後の受信ループでこれをデコードし、設定へ書き、保存する（`video.rs` の `PAIRING_INFO` 分岐と `gui.rs` の `take_pairing_info`）。バナーの「Wi-Fiに切り替える」より前に保存される。「あとで」はバナーを消すだけで、すでに書いたキーは戻さない。切り替えたあとの接続は通常の TLS と PC 認可を通る。

### 入力の上限

スマホの `Wire.readHostFrame` は、ホストから来る型ごとに長さを決めてからペイロードを確保する。未知の型はエラーである。制御の上限は `CLIENT_AUTH_MAX_BYTES`（ラベル 32 バイトを含む）。

ホストの `Header::validate_payload_size` は、知っている型は長さを検査する。未知の型は `_ => true` で、16 MiB 以下ならヘッダを受理する。認可後の `receive_stream` は、映像でも既知の制御でもないフレームを無視して継続する（`VIDEO_AU` 以外は `continue`）。悪意のあるピアが 16 MiB フレームを続けたときの実メモリは、このレビューでは測っていない（未確認）。

### ログと秘密

本番経路でアクセスキーを文字列に入れている箇所と、入れていない箇所はチェックリストに列挙した。この PR で直したのは、`--headless` がイベント JSON をそのまま標準出力へ写していた点である。worker から GUI へのパイプには、GUI が保存するためにキーが残る。

## このレビューがやっていないこと

- 実機、エミュレータ、パケットキャプチャ、敵対的なピア。
- Android TLS プロバイダのソース。ホスト側の rustls 0.23.46 は、再開時に証明書を再検証しない箇所まで読んだ。Android 側のプロバイダ実装は読んでいない。
- Windows DPAPI の実行。`dpapi.rs` は読んだ。この環境では実行していない。
- 依存クレートの中身の監査。`Cargo.toml` は rustls `0.23.46`、ring `0.17.14` をピンしている、という事実だけ。
