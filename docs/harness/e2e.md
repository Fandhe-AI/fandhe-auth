# passkey の E2E ハーネス: 構成の下書き

`TASK-11`（passkey の E2E ハーネス。MS-1）の構成の下書き。1/6（#101）で範囲と枠を置き、4/6（#104）で登録の後の認証と失敗の場合を足した。
仕様は spec の `TASK-11` で引く（本文は写さない）。この文書は下書きで、crate の名前・置き場は仮称、人が確定する。

## 目的と範囲

passkey の登録と認証（`api-passkey.md`・`PKEY`）の 2 操作を、Chrome・Firefox の WebDriver の仮想 authenticator で通す
E2E のハーネス。2026-11-04 の追随で、画面の E2E は Hub の `ASCR` に移り、本タスクは passkey の E2E に絞られた。

## 含めないもの

- 画面の E2E と画面の識別子: Hub の `ASCR`（`LGUI-35` → `ASCR-65`）
- 適合性試験の実行環境: Hub の `ARUN-20`（`DPOL-27`）
- Safari・実機: `TASK-784`・`TASK-75`〜`TASK-78`
- 結合試験: `TASK-10`（`docs/harness/integration.md`）
- 計測と負荷: `TASK-12`。障害注入: `TASK-13`

## 構成要素

| 種類（`E2eComponentKind`） | 担当の段 | 実体 | 待ち |
| --- | --- | --- | --- |
| `BrowserDriver` | 2/6（Chrome）・3/6（Firefox）。4/6 で確立したセッションの `browserName` を照合する | chromedriver・geckodriver（外部プロセス）。2/6 で Chrome の実体 `chrome::ChromeDriverComponent` を置いた | Firefox は 3/6 |
| `RelyingParty` | 未定 | options を出し応答を検証する側 | `TASK-716` と PKEY の実装タスク |

2/6 で置いた実体は Chrome の `BrowserDriver` だけ。RelyingParty の実体は 0 件。操作は起動前に取る `ChromeHandle`
（it-harness の handle と同じ形）で行い、起動前・停止後は `NotRunning`。WebDriver のクライアントは std だけの自前
（loopback 限定・同期・応答の上限あり・chunked は拒否）で、外部の HTTP クライアントと非同期ランタイムは足していない。
JSON は `serde_json` で扱う（下の「依存」）。Chrome の実体と登録の手順は unix（Linux・macOS 等）に限る（`cfg(unix)`）。

## 起動と停止の契約

- 入口: `E2eHarness::start(E2eConfig) -> RunningE2e`、`RunningE2e::stop()`。`E2eConfig` は対象の操作とブラウザだけを持つ。
- 起動順は RelyingParty → BrowserDriver、停止はその逆。渡した順には依らない。同じ種類の重複は起動前に拒否する。
- 途中で失敗したら起動済みを逆順に停止してから誤りを返す。`Drop` でも停止を試みる。
- 誤りは種類だけを持ち、パス・URL・ポート・秘密値を載せない（`SECR-11`・`SECR-23`）。環境変数・ファイルは読まない（`DPOL-32`）。
- 試験の Fake は枠の順序を確かめるためのもので、E2E の合格に数えない（`ACPT-7`）。

## 仮想の扱い

- `Evidence` は `Virtual` だけで、`counts_as_real_device`・`counts_as_safari` は常に `false`（`ACPT-7`・`ACPT-8`）。
- `Browser` に Safari の変種を置かない。Safari の結果はこのハーネスの型に入る経路が無い。
- 仮想 authenticator の options は `VirtualAuthenticatorOptions`（`Default` なし）で受け、値は試験が明示で渡す。
  2/6 の試験の値（`ctap2`・`internal`・resident key あり・UV あり・UV 成功）は PoC の値（QP-1）で、要件の値ではない。
  `AuthenticatorKind::WebDriverVirtual` に対応し、結果は〔仮想〕。

## 登録 1 件の定義（2/6）

登録 1 件が通るとは、次の 1・2 を `register_once` が検査し、3 を登録試験の側が検査することをいう。
サーバー側の登録応答の検証（`webauthn-rs`）は RelyingParty の実体（`TASK-716`）待ちで含めない。
PKEY の振る舞いを満たしたとは主張しない。

`register_once` が検査する範囲（`RegistrationOutcome::succeeded`）:

1. 試験用 origin で `navigator.credentials.create()` が resolve し、`type` が `public-key`、attestation が空でない
2. `clientDataJSON` の `type`・`origin`・`challenge` が期待と一致（ページ内で比べ、真偽と長さだけを返す）

失敗したときの `DOMException.name` は既知の名前の列挙（`WebAuthnErrorName`。`RegistrationErrorName` は別名）に写し、未知の値は `Other` に畳む（`SECR-23`）。

登録は `credentialId`（`rawId` の base64url）も返す。不透明な型 `CredentialRef` で持ち、`Debug` は `<redacted>` の固定の文字列、
`Display` は実装せず、値は crate 内からだけ借りる。公開鍵・attestation の中身・署名は返さない。

登録試験の側（`tests/chrome_registration.rs`）が検査する範囲。`register_once` は検査しない:

3. `handle.credentials()` の要約で、仮想 authenticator の credential がちょうど 1 件で、`rpId` が `localhost`

## 認証 1 件の定義（4/6）

認証 1 件が通るとは、`authenticate_once` が登録で得た credential ID を `allowCredentials` に入れて `navigator.credentials.get()` を呼び、
次を検査することをいう（`AuthenticationOutcome::succeeded`）。サーバー側の認証応答の検証（`webauthn-rs`）は RelyingParty の実体
（`TASK-716`）待ちで含めず、PKEY の振る舞いを満たしたとは主張しない。結果は〔仮想〕（`ACPT-7`・`ACPT-8`）。

1. `get()` が resolve し、`type` が `public-key`、`rawId` が渡した credential ID と一致
2. `clientDataJSON` の `type`（`webauthn.get`）・`origin`・`challenge` が期待と一致
3. `authenticatorData` の先頭 32 バイトが RP ID の SHA-256 と一致（ページ内の `crypto.subtle`）、flags の UP・UV が立ち、長さが 37 以上、署名の長さが 1 以上

署名・`authenticatorData` の中身・`userHandle` は返さない。認証試験の側は、登録に成功すること・`credentials()` がちょうど 1 件で `rpId` が
`localhost` であることを検査する。options の形は `PKEY-4` に合わせるが確定扱いにしない。

### 失敗の場合（4/6）

- 採ったもの: 登録の後に仮想 authenticator の UV を失敗にし（WebDriver の Set User Verified、`isUserVerified: false`）、
  `userVerification: required` で認証すると `NotAllowedError` になる。判定は authenticator 側で完結し、ネットワークに出ない。
- 採らなかったもの: RP ID を origin の suffix でない値にする案（Chrome は Related Origin Requests で `https://<rpId>/.well-known/webauthn` を
  取りに行くため、外への通信が起きる。A10）。credential を消してから認証する案（timeout まで待つかが確かめられていない）。
- 時間の上限: 認証スクリプトはページ内の `AbortController` で 10 秒の期限を明示的に設ける（`publicKey.timeout` はブラウザが調整し得る
  ヒントで、打ち切りの保証にならない）。期限切れは `HarnessTimeout`（`NotAllowedError` とは別の名前）で返す。試験側の script timeout は 20 秒、
  command timeout は 30 秒。拒否が遅れても WebDriver の誤りにならず結果として観測できる。値はハーネスの安全上の値で、要件の値ではない。
- RP ID: `authenticate_once` は RP ID が試験用 origin のホストと完全一致しなければ、ブラウザへ渡す前に拒否する（A10）。
- Firefox で期待と違う結果が出ても、期待を緩めず別の案に黙って差し替えない。結果を記録して人に確かめる。

### browserName の照合（4/6）

`new_session` は応答の `capabilities.browserName` を期待の値（Chrome は `chrome`、Firefox は `firefox`）と完全一致で照合し、
欠落・不一致ならセッションを最善で閉じて `CommandFailed` にする（照合しない経路は残さない）。失敗時は handle に公開せず、`start()` の後片付けが
プロセスグループごと止める。Firefox の `firefox` はこの環境では実測していない（W3C の名前）。

### 暫定の結果行（4/6。正式な形式は 5/6）

試験が標準出力に 1 行出す（`tests/support/passkey.rs`。crate の公開 API にしない）:
`e2e-result: operation=… browser=… browser_version=… authenticator=WebDriverVirtual evidence=Virtual counts_as_real_device=false counts_as_safari=false result=succeeded|rejected:<名前>`。
ブラウザの版は文字種と長さで絞り、URL・ポート・セッション ID・authenticator ID・credential ID は出さない（`SECR-11`・`SECR-23`）。
実行の回数の記録と正式な形は 5/6（#105）。入口のスクリプトは `--show-output` で成功した試験の出力も見せ、`--test-threads=1` でブラウザを同時に複数起動しない。

## 試験用ページと origin の置き場（人が決める）

本体は画面も HTTP の受付も持たない（`library-boundary.md`、Hub の `ASCR`・`ARUN-20`）。候補は次の 3 つ。

- (a) `TASK-716` の最小の組込み先が配る。
- (b) E2E のハーネスの試験コードの中にだけ置く試験用ページ。本番の画面には持ち込まない。
- (c) Hub の試験へ寄せる。

2/6 は暫定で (b) を採り、`crates/e2e-harness/tests/support/` に置いた（loopback のみ・`GET /` のみ・`Host` が
`localhost:<port>` 以外は 400）。本番の画面・本体の crate には何も足していない。origin は `http://localhost:<port>`、
`rp.id` は `localhost`（PoC-2 の開発・`localhost` の構成を ID で引く）。採否は人の確認待ち。

WebAuthn の secure context と RP ID は PoC-2 の結果を ID で引き、試験用の仮の値で受ける（`A-2026-10-28-1`〜`14`）。

## ドライバの入手と CI（2/6 以降の案）

2/6 の入口は暫定の `make e2e-chrome`（`scripts/e2e-chrome.sh`。`#[ignore]` の `chrome_registration`・`chrome_authentication` を流す。CI 外）。
chromedriver が無ければ失敗する。Chrome の起動 flag は `--headless=new` だけで、`--disable-*`・`--no-sandbox` は足さない。
前提は unix（Linux・macOS 等）。非 unix では登録試験が 0 件になり、入口のスクリプトが失敗にする。

### Firefox（3/6）

- 入口は暫定の `make e2e-firefox`（`scripts/e2e-firefox.sh`。`#[ignore]` の `firefox_registration`・`firefox_authentication` を流す。CI 外）。geckodriver が無ければ失敗する。
- 起動・停止は Chrome と共通の `driver_process` に切り出した（振る舞いは 2/6 のまま）。geckodriver は `--port <n>` で起動する。
- セッションの要求は引数 `-headless` と pref 2 つだけ。Firefox の既定の設定では仮想 authenticator の作成が `ConstraintError` になるため
  softtoken を有効・usbtoken を無効にする（PoC-3 の値で要件の値ではない。人の確認待ち）。検証を外す意味の引数・pref は足さない（DPOL-30）。
- 要素の識別（`LGUI-35` → `ASCR-65`）: ハーネスが引く要素は 0 件。登録は `navigator.credentials.create()` をスクリプトで呼ぶだけで、
  試験用ページはスクリプトを持たず、WebDriver クライアントに要素を探す命令が無い。規則の値はここに写さない。
  将来要素を引く段を足すときは `LGUI-35`（→ Hub `ASCR-65`）を ID で引いて従い、本番の画面に試験用の要素を持ち込まない。
- 受け入れた制限（Chrome と同じ）: 強制終了で geckodriver の一時プロファイルが残り得る。送信先は flag で制限しない。
- 2 台の authenticator を同時に有効にすると Firefox が拒否することがある（PoC-3）ため、1 台だけを使う。
- 動作の記録がある組み合わせ（spec の `03-poc/` の記録。〔仮想〕）: Firefox 156〜158、geckodriver 0.37.1、pref は上の 2 つだけ
  （capabilities の `moz:firefoxOptions.prefs` で渡す。geckodriver が起動する Firefox に届く）。この repo の `make e2e-firefox` では未実行。
  geckodriver 0.34.0 で対応が入ったが、0.36.0 の変更履歴は使用を避けるよう勧める（`02-poc-plan.md`）ため、上の組み合わせを試す。
  これより古い版・上の 2 つで足りない版の可否は未確認で、追加の設定を推測で足さない（足すなら人に確かめる）。

### 起動・停止の契約（2/6）

- chromedriver は新しいプロセスグループ（グループ ID = 子の PID）で起動する。
- 起動待ち（`with_ready_timeout`）は probe と待機を含めた全体の期限で、期限後の ready は受理しない。
- WebDriver の 1 命令（`with_command_timeout`）は、接続・書き込み・読み取りを通した全体の期限で打ち切る。
  期限を表せない値は誤りにする（panic しない）。応答の `Content-Length` が上限を超えたら読む前に誤りにする。
- 停止・起動の失敗・`Drop` は同じ手順を通る: セッションの削除（最善）→ 子を回収する前にグループへ `kill -KILL -<pgid>` →
  子の終了を期限付き（5 秒。安全上の値で要件の値ではない）で待って回収する。セッションの削除に失敗しても、
  セッション ID が取れていなくても、配下の Chrome をグループごと止める。期限内に終わらなければ子を保持して `StopFailed` を返し、
  `stop()`・`Drop` で再試行する。セッションの削除に失敗したときは後片付けを最後まで続けたうえで `StopFailed` を返す。
- 起動待ちの間に子の終了を観測した（回収した）ときは、子を手放してからグループへは送らない（PID の再利用を宛先にしない）。
- Chrome が chromedriver と同じプロセスグループに入ることは、Linux・Chromium 157（snap）・ChromeDriver 157 で
  `ps -eo pid,pgid,ppid` により確かめた。他の環境・版では確かめていない。
- 受け入れた制限（人の確認待ち）:
  - 空きポートを取って手放してから子に渡すため、その間に別のプロセスが同じポートを取ると取り違え得る
    （試験の環境で許容する）。ポートの所有の照合はしない。
  - ブラウザの送信先は flag で制限しない。試験用ページは loopback の試験用 origin だけを開く。
  - 強制終了のため、Chrome の一時プロファイルが残り得る。
  - 登録結果を解釈できなかった後のセッションの再利用は API では止めていない。拒否された経路は 4/6 の失敗の試験で、解釈できない応答
    （`CommandFailed`）は単体の試験で確かめる。試験は誤りで panic し、`Drop` の後片付けでブラウザを止めるため再利用は起きない。

- chromedriver・geckodriver は外部の実行ファイル。`#[ignore]` と明示の make ターゲット（`it-db` と同じ形）で CI の外に置く案。
- 環境の有無で自動に飛ばす分岐は作らない（`DPOL-30`〜`32`）。ブラウザの起動 flag（`--disable-*` 等）は `DPOL-30` の走査語に掛かる。
- geckodriver の Virtual Authenticator の安定性は `02-poc-plan.md` の確認状況を 3/6 の懸念として引く。
- ドライバの起動は引数を配列で渡しシェルを通さない。通信はループバックのドライバと試験用 origin だけにする（A03・A10）。

## 段ごとの担当

| 段 | 内容 |
| --- | --- |
| 1/6（#101） | 範囲と構成の下書き、枠（本段） |
| 2/6（#102） | Chrome の仮想 authenticator の結線と登録 1 件 |
| 3/6（#103） | Firefox の結線と識別子の規則（`LGUI-35` → `ASCR-65`） |
| 4/6（#104） | 登録の後の認証を両ブラウザで通す。失敗する場合の試験。暫定の結果行。3/6 のレビューの持ち越し（`browserName` の照合など） |
| 5/6（#105） | 実行の記録の形式（ブラウザの版・authenticator の種類・回数・仮想の印） |
| 6/6（#106） | 使い方の文書と、実機の確認は別タスクである旨 |

## 未決・人の確認待ち

- crate の追加・名前 `fandhe-auth-e2e-harness`・置き場 `crates/e2e-harness`・許可表の行
- `LIB-14`・`LIB-15` との関係（段階 1 のリリースのメンバーに含めない扱い）
- std の自前の WebDriver クライアント（外部の WebDriver クライアント crate・非同期ランタイムを足さない判断。`dependency-policy.md`）
- 試験用ページと origin の置き場（上の (a)〜(c)。2/6 は (b) を暫定採用）
- 仮想 authenticator の options の値（2/6 の値は PoC の値）
- make ターゲット名 `e2e-chrome`・`e2e-firefox`
- 3/6: Firefox の pref 2 つ（`security.webauth.webauthn_enable_softtoken`=true・`..._usbtoken`=false。PoC-3 の値で要件の値ではない）と、
  引数を `-headless` だけにしたこと
- 3/6: 起動・停止の共通部の切り出しの形（共通の `DriverHandle` を包む独立した型 `ChromeHandle`・`FirefoxHandle`〔`Deref` で操作を使う。取り違えは型で弾き、`register_once` へは `Deref` で渡す〕、共通の設定型 `DriverConfig`〔`ChromeDriverConfig`・`GeckoDriverConfig` はその別名〕）
- 3/6: 受け入れ条件 3（要素の識別が `LGUI-35` → `ASCR-65` に従う）を「ハーネスが引く要素は 0 件」で満たすとする解釈
- 3/6: geckodriver・Firefox のある環境での `make e2e-firefox` の実行は未検証（作業環境に無かった）。初回の実行で、
  `POST /webauthn/authenticator` の応答の形・`Content-Length` の有無・Firefox が geckodriver と同じプロセスグループに入るか・
  `credentials` の欄が揃うかを確かめる。違いが出たら検査を緩めずに人へ確かめる
- 4/6: credential ID を不透明な型 `CredentialRef` で返すこと。失敗の場合の選び方（UV の失敗）。WebAuthn の timeout 10 秒（ハーネスの値）。
  `--test-threads=1`。暫定の結果行
- 4/6: Chrome（Chromium 157）では登録・認証・失敗の場合を実行して通った（〔仮想〕）。Firefox での認証と Set User Verified は未検証
  （作業環境に geckodriver・Firefox が無かった）。geckodriver・Firefox のある環境での `make e2e-firefox` の実行と記録は人の作業
- Chrome の起動 flag は `--headless=new` だけ（`DPOL-30` の走査語に掛かる flag は足していない）
- it-harness・最小の組込み先への辺（足す段で許可表を人に確かめる）

## 依存

外部依存は `serde_json` だけ。2026-11-20 の利用者の決定（試験専用の crate に限り直接依存として採用）による。
`Cargo.lock` に既存で package は増えない（hub-stub・console-stub と同じ宣言）。版の固定方針と `deny.toml` は `TASK-668`（未整備）。
