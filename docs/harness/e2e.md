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

`register_once` が次を満たすことを「登録 1 件が通る」とする。サーバー側の登録応答の検証（`webauthn-rs`）は
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
chromedriver が無ければ失敗する。Chrome の起動 flag は `--headless=new` だけで、`--disable-*`・`--no-sandbox` は足さない。

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
| 4/6（#104） | 登録の後の認証を両ブラウザで通す。失敗する場合の試験 |
| 5/6（#105） | 実行の記録の形式（ブラウザの版・authenticator の種類・回数・仮想の印） |
| 6/6（#106） | 使い方の文書と、実機の確認は別タスクである旨 |

## 未決・人の確認待ち

- crate の追加・名前 `fandhe-auth-e2e-harness`・置き場 `crates/e2e-harness`・許可表の行
- `LIB-14`・`LIB-15` との関係（段階 1 のリリースのメンバーに含めない扱い）
- `serde_json` の暫定採用（Cargo.lock に既存。hub-stub・console-stub と同じ）と、std の自前の WebDriver クライアント
  （外部の WebDriver クライアント crate・非同期ランタイムを足さない判断。`dependency-policy.md`）
- 試験用ページと origin の置き場（上の (a)〜(c)。2/6 は (b) を暫定採用）
- 仮想 authenticator の options の値（2/6 の値は PoC の値）
- make ターゲット名 `e2e-chrome`
- Chrome の起動 flag は `--headless=new` だけ（`DPOL-30` の走査語に掛かる flag は足していない）
- it-harness・最小の組込み先への辺（足す段で許可表を人に確かめる）
