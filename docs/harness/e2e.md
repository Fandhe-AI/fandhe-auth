# passkey の E2E ハーネス: 構成の下書き

`TASK-11`（passkey の E2E ハーネス。MS-1）の構成の下書き。1/6（#101）で範囲と枠を置いた。
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
| `BrowserDriver` | 2/6（Chrome）・3/6（Firefox） | chromedriver・geckodriver（外部プロセス）。2/6 で Chrome の実体 `chrome::ChromeDriverComponent` を置いた | Firefox は 3/6 |
| `RelyingParty` | 未定 | options を出し応答を検証する側 | `TASK-716` と PKEY の実装タスク |

2/6 で置いた実体は Chrome の `BrowserDriver` だけ。RelyingParty の実体は 0 件。操作は起動前に取る `ChromeHandle`
（it-harness の handle と同じ形）で行い、起動前・停止後は `NotRunning`。WebDriver のクライアントは std だけの自前
（loopback 限定・同期・応答の上限あり・chunked は拒否）で、外部の HTTP クライアントと非同期ランタイムは足していない。

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

`register_once` は次の 1・2 を、ページ内の結果（`ok`・`origin_ok`・`challenge_ok` 等の型付きの値）として返す。
3 は `register_once` の成功条件に含めず、`chrome_registration`（`#[ignore]`）の側が `credentials` で別に確かめる。
次を満たすことを「登録 1 件が通る」とする。サーバー側の登録応答の検証（`webauthn-rs`）は
RelyingParty の実体（`TASK-716`）待ちで含めない。PKEY の振る舞いを満たしたとは主張しない。

1. 試験用 origin で `navigator.credentials.create()` が resolve し、`type` が `public-key`、attestation が空でない
2. `clientDataJSON` の `type`・`origin`・`challenge` が期待と一致（ページ内で比べ、真偽と長さだけを返す）
3. 仮想 authenticator の credential がちょうど 1 件で、`rpId` が `localhost`

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

2/6 の入口は暫定の `make e2e-chrome`（`scripts/e2e-chrome.sh`。`#[ignore]` の `chrome_registration` を流す。CI 外）。
chromedriver が無ければ失敗する。Chrome の起動 flag は `--headless=new` と、外部への通信を禁じる `--host-resolver-rules`・
`--proxy-server`（解決できない proxy）・`--proxy-bypass-list`（`<-loopback>` と登録 origin の `http://host:port`（スキーム付き）だけ。許可外の
ループバックのポートも送信前に失敗させる。A10）の 4 つと、ハーネスが付ける `--user-data-dir`（一時領域）だけで、
`--disable-*`・`--no-sandbox` は足さない。`navigate` は移動後の URL も許可リストで確かめ、外れたら空白ページへ戻して誤りにする。
A10 の保証範囲は Chrome の HTTP(S) の送信先までで、proxy を通らない WebRTC の直接 UDP などは flag では制限できず保証しない（試験用ページは使わない。ネットワーク層の隔離は実行環境の担当で、ここでは主張しない）。
chromedriver の起動はプロセスグループを使うため、Chrome 側の実装・公開経路・試験は `cfg(unix)` に限る。
停止は、子を回収する前にグループへ SIGKILL を送り、そのあと wait する（回収後の PID を宛先にしない）。子の終了の観測は
`try_wait` を使わず `/proc`（無ければ `ps`）で状態（zombie）だけを読み、終了を観測しても回収しない。signal の失敗は、グループに生きた構成員が
居ないと確かめられた場合だけ消滅済みとみなし、それ以外は `StopFailed` で子・一時領域を保持して再試行する。
SIGKILL の後は、グループに生きた構成員（zombie 以外）が居なくなったことを期限付き（5 秒。安全上の値で要件の値ではない）で確かめてから子を回収する。
確かめられない・期限内に消えないときは子を回収せず、一時領域も消さずに `StopFailed` を返して再試行に残す。
命令の送信前には、子が生きていることに加え、宛先ポートの LISTEN socket をすべて所有する子が持つこと（Linux は `/proc/net/tcp{,6}` と `/proc/<pid>/fd` の照合）を確かめ、
確かめられなければ送らない（A10）。Linux 以外は照合手段が無いため常に送らない（fail-closed）。
WebDriver の応答の受信バッファは上限ぶんを先に確保して再確保を避け、本文を複製せずそのまま解析し、成功・誤りのどの経路でも破棄時に上書きする。
JSON の重複キー検査は集合で行い、解析中も命令の絶対期限を値・キーごとに確かめる。
credential の応答は `rpId`・`isResidentCredential` だけを読む専用の解析で読み、他の欄（`privateKey` 等。キー名のエスケープ表記を含む）は値を保持せず捨てる（SECR-10・SECR-29）。
WebDriver の応答は、ヘッダー名の token 検査・値と理由句の制御文字の拒否を行う。宣言した長さを超える余分なバイトの拒否は、
長さに届いた後の短い猶予（50ms と残り時間の小さい方）内に届いた分までを保証の範囲とする（接続は 1 命令で捨てる）。

- chromedriver・geckodriver は外部の実行ファイル。`#[ignore]` と明示の make ターゲット（`it-db` と同じ形）で CI の外に置く案。
- 環境の有無で自動に飛ばす分岐は作らない（`DPOL-30`〜`32`）。ブラウザの起動 flag（`--disable-*` 等）は `DPOL-30` の走査語に掛かる。
- geckodriver の Virtual Authenticator の安定性は `02-poc-plan.md` の確認状況を 3/6 の懸念として引く。
- ドライバの起動は引数を配列で渡しシェルを通さない。通信はループバックのドライバと試験用 origin だけにする（A03・A10）。

### 起動・停止の契約（2/6）

- 待ち受けポートは `--port=0` で子に確保させ、子の出力（`started successfully on port N`）から受け取る。空きポートを
  先に取って手放す方式は、別のドライバと取り違える恐れがあるため採らない（A10）。
- 起動待ちの期限は probe・待機を含めた全体の絶対期限で、期限後の ready は受理しない。命令の期限も書き込み・読み取りを
  通した全体の絶対期限で、時間切れの値が大きすぎて期限を作れない場合は誤りにする（panic しない）。
- 子は新しいプロセスグループで起動し、停止・失敗・`Drop` で、セッションの削除（最善）→ グループへ SIGKILL（
  Chrome の一時領域はハーネスが所有し回収後に消すため SIGTERM の猶予は置かない）→ wait の順に chromedriver と配下の Chrome を後片付けする。セッション ID が無い失敗でも同じ。Chrome の一時領域は `--user-data-dir` でハーネスが作った 0700 の `/tmp` 配下に固定し、セッション確立の失敗でも回収後に消す。
- ブラウザの `navigate` は `ChromeDriverConfig::with_allowed_origin` で起動前に登録した試験用 origin
  （`http://localhost:<port>`・`http://127.0.0.1:<port>`。ポートは先頭ゼロなしで 80 以外）の配下だけを許し、未登録なら全拒否（A10）。
  ブラウザ自体の通信（初回の移動・リダイレクト・サブリソース）も proxy の bypass list により登録 origin 以外は送信前に失敗する。
  移動の結果を確定できないとき（命令の時間切れ等）はセッションを使用不可にし、停止で回収する。命令と停止は同じロックで同期する。
- WebDriver のクライアントは crate 内に閉じる。メソッドは列挙型、path は許可文字のみ（A03）。応答のステータス行・
  ヘッダーの構文違反、Content-Length の重複・不正・本文との不一致、chunked は拒否する。credential は
  `rpId`・`isResidentCredential` だけを外へ出し、`privateKey` を含む生の応答を公開しない（SECR-10・SECR-11・SECR-23）。
- 登録の失敗は `RegistrationErrorKind`（既知の `DOMException.name` の列挙と `Other`）で返し、応答由来の文字列を出さない（SECR-23）。
- 試験用 origin のサーバーは同時接続数と 1 接続の処理時間に上限を置く（A03・A10）。

## 段ごとの担当

| 段 | 内容 |
| --- | --- |
| 1/6（#101） | 範囲と構成の下書き、枠（本段） |
| 2/6（#102） | Chrome の仮想 authenticator の結線と登録 1 件 |
| 3/6（#103） | Firefox の結線と識別子の規則（`LGUI-35` → `ASCR-65`） |
| 4/6（#104） | 登録の後の認証を両ブラウザで通す。失敗する場合の試験 |
| 5/6（#105） | 実行の記録の形式（ブラウザの版・authenticator の種類・回数・仮想の印） |
| 6/6（#106） | 使い方の文書と、実機の確認は別タスクである旨 |

## 未決・人の確認待ち

- crate の追加・名前 `fandhe-auth-e2e-harness`・置き場 `crates/e2e-harness`・許可表の行
- `LIB-14`・`LIB-15` との関係（段階 1 のリリースのメンバーに含めない扱い）
- std の自前の WebDriver クライアントと自前の最小の JSON（`src/json.rs`。外部の WebDriver クライアント crate・JSON の crate・
  非同期ランタイムを足さない判断。`dependency-policy.md`）
- 試験用ページと origin の置き場（上の (a)〜(c)。2/6 は (b) を暫定採用）
- 仮想 authenticator の options の値（2/6 の値は PoC の値）
- make ターゲット名 `e2e-chrome`
- Chrome の起動 flag は `--headless=new` と外部への通信を禁じる `--host-resolver-rules`・`--proxy-server`（解決できない proxy）・
  `--proxy-bypass-list`（登録 origin の `http://host:port`（スキーム付き）だけを直接接続にする）。検証を外す flag（`DPOL-30` の走査語）は足していない。採否は人の確認待ち
- it-harness・最小の組込み先への辺（足す段で許可表を人に確かめる）

## 依存の判断待ち（人が決める）

外部依存は 0 件。`serde_json` の採否が人の確認待ちのため、直接の依存にせず、JSON は `src/json.rs` の自前の最小実装（厳格な構文解析・
深さの上限・重複キーの拒否）で扱う。採否を人が決めたら、置き換えるかを改めて判断する。
