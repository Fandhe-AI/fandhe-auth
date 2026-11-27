# passkey の E2E ハーネス: 構成の下書き

`TASK-11`（passkey の E2E ハーネス。MS-1）の構成の下書き。1/6（#101）で範囲と枠を置き、4/6（#104）で登録の後の認証と失敗の場合を足し、6/6（#106）で使い方と実機の確認との境目を足した。
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

登録 1 件が通るとは、次の 1〜3 を `register_once` が検査し、4 を登録試験の側が検査することをいう。
サーバー側の登録応答の検証（`webauthn-rs`）は RelyingParty の実体（`TASK-716`）待ちで含めない。
PKEY の振る舞いを満たしたとは主張しない。

`register_once` が検査する範囲（`RegistrationOutcome::succeeded`）:

1. 試験用 origin で `navigator.credentials.create()` が resolve し、`type` が `public-key`、attestation が空でない
2. `clientDataJSON` の `type`・`origin`・`challenge` が期待と一致（ページ内で比べ、真偽と長さだけを返す）
3. `cred.id` と `rawId` の base64url 表現が一致し、その値から `CredentialRef` を構築できる（形式の検証に通る）

失敗したときの `DOMException.name` は既知の名前の列挙（`WebAuthnErrorName`。`RegistrationErrorName` は別名）に写し、未知の値は `Other` に畳む（`SECR-23`）。

登録は `credentialId`（`rawId` の base64url）も返す。不透明な型 `CredentialRef` で持ち、`Debug` は `<redacted>` の固定の文字列、
`Display` は実装せず、値は crate 内からだけ借りる。公開鍵・attestation の中身・署名は返さない。

登録試験の側（`tests/chrome_registration.rs`）が検査する範囲。`register_once` は検査しない:

4. `handle.credentials()` の要約で、仮想 authenticator の credential がちょうど 1 件で、`rpId` が `localhost`

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
- 対照: 失敗の試験は、同じセッション・credential で UV が成功の状態の認証を先に通し（対照）、UV だけを失敗に変えて `NotAllowedError` を
  観測する。`NotAllowedError` は期限切れ等でも返るため、名前だけでは UV による拒否と断定できない。対照が通った同一条件から UV だけを変えた
  結果として読み、結果の主張は `NotAllowedError` の観測までにとどめる（原因の判別はブラウザが公開しない）。
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

### 暫定の結果行（4/6。5/6 で実行の記録に置き換え済み）

> 5/6 で下の `e2e-result:` の行は廃止し、「実行の記録の形式（5/6）」の節の記録に移した。以下は 4/6 時点の記録として残す。

試験が標準出力に 1 行出す（`tests/support/passkey.rs`。crate の公開 API にしない）:
`e2e-result: operation=… browser=… authenticator=WebDriverVirtual evidence=Virtual counts_as_real_device=false counts_as_safari=false result=succeeded|rejected:<名前>`。
ドライバが返す文字列（ブラウザの版を含む）は結果行に転記せず（5/6 で版は主番号の数値だけを型付きで出すよう改めた。下の節）、URL・ポート・セッション ID・authenticator ID・credential ID は出さない（`SECR-11`・`SECR-23`）。
実行の回数の記録と正式な形は 5/6（#105）。入口のスクリプトは `--show-output` で成功した試験の出力も見せ、`--test-threads=1` でブラウザを同時に複数起動しない。

## 実行の記録の形式（5/6）

`fandhe_auth_e2e_harness::record::E2eRunRecord`。1 つのブラウザのセッション（`E2eHarness::start` 1 回、つまり E2E の試験の関数 1 つ）で
行った登録・認証の結果を、1 行 1 項目の `key: value`（UTF-8・LF）で出す。試験は `tests/support/passkey.rs` の `RecordSink` を使い、
終了時（panic の巻き戻りを含む）に本文を標準出力へ出し、`CARGO_TARGET_TMPDIR/e2e-records/`（Cargo の既定の設定なら `target/` の下。`CARGO_TARGET_DIR` を変えた場合は保存先もビルド時の `CARGO_TARGET_TMPDIR` に従って変わる）へ
`e2e-run-<browser>-<unix秒>-<pid>-<通し番号>.txt` として新規に書く（既存のファイルは上書きしない）。

| キー | 内容 |
| --- | --- |
| `format` | 形式の識別子（`fandhe-auth-e2e-run-record/1`） |
| `harness_version` | この crate の版 |
| `browser` | `Chrome` または `Firefox` |
| `browser_version_major` | ブラウザの版の主番号（数字）。解釈できなければ `unknown` |
| `authenticator` | `WebDriverVirtual` |
| `evidence` | `Virtual` |
| `counts_as_real_device`・`counts_as_safari` | 常に `false`（`ACPT-7`・`ACPT-8`。型から導き、外から上書きできない） |
| `target_operation` | その試験が対象とする操作 |
| `completed` | 試験が最後まで到達したら `true`。到達しなければ `false` で、実行した分の回数は残る |
| `run` | 実行の順に 1 行ずつ。`<通し番号> <操作> succeeded｜rejected:<名前>｜check_failed｜harness_error:<種類>｜preflight_failed｜harness_timeout`。`preflight_failed`（呼び出し前の origin 確認で止めた）と `harness_timeout`（ハーネス自身の期限切れ）はブラウザの拒否（`rejected`）に数えない |
| `count` | 操作ごと（登録・認証の 2 つ。0 件でも出す）の実行の回数と結果ごとの件数（`succeeded`・`rejected`・`check_failed`・`harness_error`・`preflight_failed`・`harness_timeout`） |
| `note` | 仮想の結果を実機・Safari に数えない旨 |

- 回数の単位は 1 セッション内の `register_once`・`authenticate_once` の呼び出し（認証の試験の中の登録、UV 失敗の試験の対照の認証も数える）。
  試験の関数・target をまたぐ合算は crate では行わず、ファイルが 1 セッションに 1 つなので読み手が合算する。
- ブラウザの版は主番号（1〜4 桁の数字）だけを出す。ドライバの文字列は保持せず、空・文字種外・32 バイト超・文法（1〜4 桁の主番号 + 1 つ以上の `.` 区間。各区間は数字で始まり `[0-9a-z]` のみで空でない。`157.`・`157.invalid` は不可）に合わない・
  セッション ID と一致するものは `unknown` にする。記録に出る値は列挙の名前・数値・定数だけで、URL・ポート・セッション ID・各種 ID・署名・パスは入らない（`SECR-11`・`SECR-23`）。
- 記録は実行の事実の出力で、合否の判定でも spec の「確定」でもない。結果は〔仮想〕で、実機・Safari の結果は別タスク（`TASK-784`・`TASK-75`〜`78`）である。
- 呼び出しの前後のハーネスのスクリプトの例外（応答の `JSON.parse` 等）は WebAuthn の拒否ではなく、固定名 `HarnessScript` で受けて `check_failed` に数える（`rejected` に入れない）。
  登録後の credential の取得は 1 回で、取得の誤りは `harness_error:<種類>`、取得後の条件の不一致だけを `check_failed` に数える。
- ファイルの公開は、同じ出力先の一時ファイルへ書いて `fs::hard_link` で最終名へ公開する（既存の最終ファイルは上書きしない・不完全なファイルを最終名で見せない）。
  このため出力先はハードリンクに対応するファイルシステムである必要があり、非対応なら `WriteFailed` を返す。
- 出力先 `e2e-records/` は起動をまたいで追記され、過去の起動や並行する起動のファイルが残る。ファイル名の unix 秒は秒単位で、
  開始時刻による絞り込みでは同じ秒の別の記録を拾うため使わない。集計は、その起動の標準出力（`RecordSink` が本文を出す）だけを正とするか、
  起動ごとに空の専用の出力先を使い、他の起動と共有しない。
- 偽のドライバを使う単体の試験は記録をファイルに書かない（E2E の実行回数に数えない）。
- 日時はファイル名の秒で代え、source の revision・ドライバの版は入れていない（未決）。

## 使い方（6/6）

形式・名前・節立ては仮置きで、人が確認する。正本の見本は `crates/e2e-harness/tests/chrome_registration.rs`・`tests/chrome_authentication.rs`
（Firefox は `tests/firefox_registration.rs`・`tests/firefox_authentication.rs`）で、ここには手順の骨格だけを書く（値の詳細は見本を見る。ずれを防ぐため）。

### 前提

- unix（Linux・macOS 等）に限る。非 unix では対象の試験が 0 件になり、入口のスクリプトが失敗にする。
- Chrome は chromedriver と Chrome、Firefox は geckodriver と Firefox（仮想 authenticator の WebDriver のエンドポイントを使える組み合わせ。
  版の値はここに写さず、参照先を分ける）。
  - Firefox: 動作の記録のある組み合わせは `scripts/e2e-firefox.sh` と Makefile の `e2e-firefox` の説明を見る（スクリプトが書く範囲は、記録のある組み合わせと、それより古い版・pref が足りない版は未確認という点）。
  - Chrome: 同じ文書の「起動・停止の契約（2/6）」にある ChromeDriver を含む確認環境を見る。書かれている確認は chromedriver と Chrome が同じプロセスグループに入ること（特定の環境・版で `ps` により確認。他は未確認）で、仮想 authenticator の動作の確認ではない。
- ドライバが無ければ黙って飛ばさず失敗する（`DPOL-30`〜`32`）。環境の有無で自動に飛ばす分岐は作らない。

### 試験の置き場

ドライバを使う試験は `crates/e2e-harness/tests/*.rs` に置き、`#[ignore]` を付けて `cfg(unix)` に限る。共通の流れと記録は
`tests/support/passkey.rs`（`register`・`authentication_run_result`・`RecordSink`。crate の公開 API ではない）と `tests/support/`（試験用 origin の `TestOrigin`）にある。
ハーネスは試験専用で、どの crate からも依存しない（`LIB-2`・`LIB-4`）。他の crate からの辺を足すときは、許可表（`scripts/check-crate-deps.sh`）の人の確認が要る。

### 組み立ての手順（見本の流れと対応）

1. `TestOrigin::start()` で試験用 origin を起こす。
2. `ChromeDriverComponent::new(ChromeDriverConfig::new("chromedriver"))`（Firefox は `GeckoDriverComponent`・`GeckoDriverConfig`）を作り、`Box` に入れる前に `handle()` を取る。
3. `E2eHarness::start(E2eConfig::new(PasskeyOperation::.., Browser::..).with(Box::new(driver)))` で起動する。
    起動順・逆順の停止・`Drop` の停止は「起動と停止の契約」の節を見る。
4. `RecordSink::new(&handle, Browser::.., PasskeyOperation::..)` で記録を開く。
5. 登録は `support::passkey::register`（仮想 authenticator を `VirtualAuthenticatorOptions` で足し、`register_once` を呼び、戻り値は `(AuthenticatorId, CredentialRef)`）。
    ヘルパーは内部で `registration_run_result` により分類した登録の結果を `sink.record(..)` に足すので、呼び出し側は登録を記録しない（二重に数えない）。
    検査の担当を分ける。ヘルパーを使うときは、登録の成否と登録の `check_ok` の検査をヘルパーが内部で行い、呼び出し側へ返るのは `(AuthenticatorId, CredentialRef)` だけで `check_ok` は返らない。
    呼び出し側が登録の `check_ok` を再検査する必要はなく、できない。ヘルパーが担わない検査（手順 6 の認証の `check_ok`、`evidence()` の確認）だけを呼び出し側が行う。
    ヘルパーは登録結果の `counts_as_real_device()`・`counts_as_safari()` がともに `false` であることも内部で検査する（`ACPT-7`・`ACPT-8`）。戻り値から登録結果の `evidence()` は取れないので、
    直接呼び出した操作（認証など）の証跡の確認は呼び出し側が行う。
    `registration_run_result`・`authentication_run_result` を直接使うときは、得た `(run, check_ok)` を呼び出し側が検査する。`run` は操作の結果と後続の検査を合成した記録用の分類で、`check_ok` の意味は登録と認証で違う。登録の `check_ok` は、登録が成功し、かつ後続の検査（credential の件数・RP ID）にも通ったときだけ真になる。認証の `check_ok` は、操作の成否にかかわらず、後続の検査だけの結果を表す。
    操作の成否は `run` ではなく元の `result` から決める。`run` を記録した後に `result.expect(..)` で `outcome` を取り出し、成功を期待するなら `outcome.succeeded()`、拒否を期待するなら `!outcome.succeeded()` に加えて、`outcome.error_name()` が対象の WebAuthn のエラー名（期待したブラウザの拒否の名前）であることを照合する。`expect(..)` を通っても `error_name()` には `HarnessTimeout`（ハーネス自身の期限切れ）・`HarnessScript`（ハーネスのスクリプトの例外）・`OriginMismatch`（呼び出し前の origin 確認）が入り得て、これらはブラウザの拒否ではないので拒否の根拠にしない（記録の `run` ではそれぞれ `harness_timeout`・`check_failed`・`preflight_failed` に分類され、`rejected` に数えない。「実行の記録の形式（5/6）」の節）。`!outcome.succeeded()` だけで拒否と見なさない。`check_ok` はそれとは別に確かめる。違いの詳細は次のとおり。`registration_run_result` は、登録が成功しなかったとき（`Err` を含む）は credential を見ずに `(run, false)` を返し、成功したときだけ credential の件数と RP ID を見て真偽を決める。偽になる条件は「登録が成功しなかった」「credential の取得に失敗した」「件数が 1 でない・RP ID が `localhost` でない」で、登録が予期せず成功して件数と RP ID が合えば拒否を期待する試験でも真になる。そのため `check_ok` は拒否の判定に使えず、登録の拒否を期待する試験は `!outcome.succeeded()` と `error_name()` で確かめ、`assert!(check_ok, ..)` を拒否の根拠に置かない。登録の成功を期待するときは `assert!(check_ok, ..)` で確かめる。`authentication_run_result` は操作の成否によらず credential を検査するので、拒否を期待する認証でも `assert!(check_ok, ..)` で確かめる（`check_ok` が真でも、認証が成功したり別の理由で拒否されたりし得る）。
    登録だけを見本にする試験でヘルパーを使わないときは、`registration_run_result` の戻り値を `(run, check_ok)` に分け、`run` を `sink.record(PasskeyOperation::Registration, run).expect("record")` で一度だけ記録する。
    `RecordSink::record` は件数の上限を超えると `Err` を返すので、`sink.record(..).expect("record")` のように成功を確かめる。
    エラーを無視すると、記録が欠けたまま `finish()` で完了にできてしまう。登録・認証のどちらの直接の記録も同じ。
    記録のあと、元の `result` から取り出した `outcome` で登録の成否を、`check_ok` を別に、それぞれ呼び出し側で検査する（手順 6 の `check_ok` はここで得た値）。
    ヘルパーを使わないときは、`register_once` の前の準備も自分で行う（見本は `support::passkey::register`、`crates/e2e-harness/tests/support/passkey.rs`）。
    準備は 3 つ。(1) `add_virtual_authenticator(&VirtualAuthenticatorOptions)` で仮想 authenticator を足す、(2) `navigate(&origin.origin())` で試験用 origin へ移動する、
    (3) `set_script_timeout(..)` でスクリプトのタイムアウトを command timeout より短く設定する（時間切れを script timeout として観測するため）。
    認証は `authenticate_once(&handle, &origin.origin(), "localhost", &cred)` を呼び、`authentication_run_result(&handle, &aid, &result)` で
    後続の検査（credential がちょうど 1 件・RP ID が `localhost` と完全一致）を反映した `(run, check_ok)` を得て、`run` を `sink.record(PasskeyOperation::Authentication, run).expect("record")` で一度だけ記録し、成功を確かめる。
    `authenticate_once` の結果をそのまま記録しない（検査を反映できない）。options の値は PoC の値（`QP-1`）で要件の値ではない。
6. 登録をヘルパーで行った場合、登録の検査はヘルパーが済ませている。呼び出し側は、認証（または直接使った登録）の成否（`run` ではなく元の `result` の `outcome`。拒否を期待する場合は `error_name()` を対象の WebAuthn のエラー名と照合し、`HarnessTimeout`・`HarnessScript`・`OriginMismatch` は拒否の根拠から外す）と `check_ok`（認証は常に、直接使った登録は成功を期待するときだけ `assert!(check_ok, ..)`。登録の拒否を期待する試験では置かない）と、`evidence().counts_as_real_device()`・`counts_as_safari()` が `false` であることを確かめる。
7. `running.stop()` で停止し、その成功を確かめてから `sink.finish()` で記録を出し、`finish()` の戻り値も確かめる。停止に失敗した実行を完了扱いで記録しない。

失敗の場合（`set_user_verified` と対照の認証）の作り方は「認証 1 件の定義（4/6）」の節を見る。

### 実行の方法

- `make test`（`cargo test --workspace --locked`）: workspace 全体の試験を流す。ドライバ不要で、この crate では実ブラウザ・実ドライバを要さない試験だけが流れ（例えば、偽のドライバを使う試験、`crates/e2e-harness/tests/` の `scope.rs`・`test_origin.rs`・`lifecycle.rs`・`webdriver_client.rs`・`run_record.rs`〔記録形式・保存処理〕、`crates/e2e-harness/src/record.rs` 内の単体試験。一覧は `tests/` の実際の構成に従う）（`#[ignore]` のブラウザ E2E は対象外）、ブラウザ E2E の実行回数・合格に数えない（`ACPT-7`）。CI の build-test ジョブと同じ契約。
- `make e2e-chrome`（`scripts/e2e-chrome.sh`）・`make e2e-firefox`（`scripts/e2e-firefox.sh`）: `#[ignore]` の登録・認証の試験を target ごとに流す。
  引数を拒否するのは各スクリプトの `reject_args`（`scripts/lib.sh`）で、スクリプトを直接実行して位置引数を渡すと usage を出して終了コード 2 で止まる。
  Makefile のレシピは `make` に渡した引数をスクリプトへ転送しないので、登録・認証の対象やテストのオプションを入口（`make e2e-chrome`・`make e2e-firefox`）から指定することはできない。
  target ごとに 1 件以上通らなければ失敗する。CI の外（runner のブラウザは前提にしない。`TASK-6`）。ターゲット名は暫定。

### 記録の読み方

形式は「実行の記録の形式（5/6）」の節を見る。出力は標準出力と `CARGO_TARGET_TMPDIR/e2e-records/`（`RecordSink` の出力先。`target/` の下になるのは Cargo の既定の設定の場合だけで、固定の `target/e2e-records/` ではない。`CARGO_TARGET_DIR` を変えた場合はビルド時の `CARGO_TARGET_TMPDIR` に従う）。
出力先は起動をまたいで追記されるので、集計はその起動の標準出力だけを正とするか、起動ごとに空の専用の出力先を使う。
出力先は `env!("CARGO_TARGET_TMPDIR")` でビルド時に取り込まれるため、実行時に環境変数を設定しても変わらない。
起動ごとに分けるには、`make e2e-chrome`・`make e2e-firefox` を呼ぶときに `CARGO_TARGET_DIR` を起動ごとに異なる空のディレクトリへ設定する
（例: `dir=$(mktemp -d) && CARGO_TARGET_DIR="$dir" make e2e-chrome`。`mktemp` が失敗したときは make を起動しない。cargo が環境変数を引き継ぐ。ビルドはその出力先で一からやり直しになる）。

### 守ること

- URL・ポート・セッション ID・authenticator ID・credential ID・署名・パスを記録・誤り・ログに出さない（`SECR-11`・`SECR-23`）。試験で値を出す行を足さない。
- 検証を外す意味の起動 flag・pref・feature を足さない（`DPOL-30`〜`32`）。Firefox で足りない場合も推測で足さず、人に確かめる。
- 通信はループバックのドライバと試験用 origin だけにする。RP ID の完全一致の検査を外さない（A10）。
- 期待と違う結果は期待を緩めず、記録して人に確かめる（`AUDB-6` の趣旨）。
- 記録は実行の事実で、合否の判定でも spec の「確定」でもない（`ACPT-7`）。

### この段で使えないもの

- RelyingParty の実体（サーバー側の登録・認証応答の検証）: `TASK-716` の完了待ち。このため `PKEY` の振る舞いを満たしたとは主張しない。
- 画面の E2E（Hub の `ASCR`）、適合性試験の実行環境（Hub の `ARUN-20`）、Safari（`Browser` に変種が無い）。

## 実機の確認との境目（6/6）

- このハーネスが扱うのは Chrome・Firefox の WebDriver の仮想 authenticator（`AuthenticatorKind::WebDriverVirtual`・`Evidence::Virtual`）だけで、結果は〔仮想〕。
- Safari と実機の authenticator（同期 passkey・セキュリティキーを含む）での確認は**別のタスク**で、このハーネスの範囲外。
  `TASK-784`（`CG2-2`）と、旧構成の手順を受ける `TASK-75`〜`TASK-78` で扱う。手順・機材・記録票の中身はここに写さず、spec を ID で引く。
- 仮想の合格を実機・Safari の合格に数えない（`ACPT-7`・`ACPT-8`）。記録の `counts_as_real_device`・`counts_as_safari` は型から導く常に `false` で、外から上書きできない。
  このハーネスの記録を、実機のタスクの記録票の結果の欄に入れる材料にしない。
- 実機の確認の実施と結果の判定・記録は人が行う。〔仮想〕の合格で〔実機〕〔人の操作〕の項目を確定にしない（`ACPT-7`）。
- この repo の状況: Chrome（Chromium 157）では登録・認証・失敗の場合を実行して通った〔仮想〕。Firefox は作業環境に geckodriver・Firefox が無く未実行（未検証）。
  どちらも実機・Safari の結果ではない。

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
| 5/6（#105） | 実行の記録の形式（ブラウザの版の主番号・authenticator の種類・回数・仮想の印）。暫定の結果行を置き換え |
| 6/6（#106） | 使い方の文書と、実機の確認は別タスクである旨（本段。「使い方（6/6）」「実機の確認との境目（6/6）」の節） |

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
- 5/6: 形式の識別子・キー名・文言・ファイル名の形（仮置き）。ブラウザの版を主番号だけにした読み（代わりの案は、完全な版を文法と IPv4 様の値の除外の下で出すこと。
  4/6 の「転記しない」方針を改めた範囲を含む）。回数の単位を 1 セッションとし合算は読み手が行うこと。上限 10,000 件の値。
  日時・source の revision・ドライバの版を入れていないこと（it-harness への辺、または `DPOL-32` の読みが要る）。`Drop` で巻き戻る間に記録を書く作り。
  Firefox での実行と記録（geckodriver と Firefox のある環境での `make e2e-firefox`）は人の作業
- 6/6 の追加（すべて仮置き。人が確認する）: 使い方の節立て・「実機の確認との境目」の節の置き方、見本を既存の `tests/chrome_*.rs`・`tests/firefox_*.rs` とし専用のサンプルの試験を足さなかったこと
- 6/6 の未達: Firefox での `make e2e-firefox` の実行と記録は人の作業のまま。RelyingParty の実体は `TASK-716` 待ちで、`TASK-11` はそれまで未達の部分を持つ
- Chrome の起動 flag は `--headless=new` だけ（`DPOL-30` の走査語に掛かる flag は足していない）
- it-harness・最小の組込み先への辺（足す段で許可表を人に確かめる）

## 依存

外部依存は `serde_json` だけ。2026-11-20 の利用者の決定（試験専用の crate に限り直接依存として採用）による。
`Cargo.lock` に既存で package は増えない（hub-stub・console-stub と同じ宣言）。版の固定方針と `deny.toml` は `TASK-668`（未整備）。
