# console の検証用 stub と実物との差

`TASK-8`（親 Issue #71）の成果物のうち「実物との差を書いた文書」。本段は Issue #79。
対象は `crates/console-stub`（`fandhe-auth-console-stub`）。

この文書は stub の作りと、実物の console との差を書く。判定・「確定」への変更・差の受け入れは人が行う（`ACPT-7`）。
「書き手の読み」は spec に定めが無く、書き手が置いた仮定であり、確定ではない。

## 1. 位置づけ

- console は未作成で、契約は console の文書ができるまでの仮定・契約候補である（`A-2026-10-28-12`。
  根拠は Hub `DELEG-9`・Hub `DELEG-14`・Hub `DELEG-20`、`D-2026-10-17-5`、`D-2026-10-17-49` の (3)）。
  Hub の ID は、この repo から Hub の原文を読めないため、Hub の原文との照合は人の確認待ちである。
- 値は設定で受け、stub の値で進める（`A-2026-10-28-14`・`TASK-451`）。この文書にも数値を書かない。
- stub の合格を実物の console の合格に数えない（`ACPT-7`・`CG-2`）。stub での測定を数値の根拠にしない
  （`TASK-30`・`THRT-29`）。
- 本番で console の `aud` を登録しない方針は変えない（`OCL-58`）。
- `TASK-8` の 2026-11-05 追記で決まった 2 項目（`D-2026-11-05-2`・`D-2026-11-05-5`）も、実物の console との接続は未検証である。
- 試験専用で、本体・Plugin など他の crate から依存しない（`LIB-2`・`LIB-4`。`scripts/check-crate-deps.sh`）。
- spec の本文は写さない。ID で引く。この repo は public である。
- 実物への差し替え先の TASK は §5 に書く。

## 2. 全体に共通する差

| 項目 | stub | 実物との差 |
| --- | --- | --- |
| 実行モデル | sans-IO・同期。HTTP・TLS・ネットワーク・環境変数・ファイルを持たない（`DPOL-32`） | 実物は通信を伴う。通信の失敗・遅延・再接続の挙動は stub に無い |
| 時刻 | 引数 `now`（UNIX 秒）で受け、時計を読まない。記録にも時刻を持たない | 実物は時計を読む。時刻の境界は stub で確かめられない |
| URL・ホスト | 文字列で受けるだけで、参照も取得もしない | 実物は名前解決・接続・証明書の検証を行う |
| 相手側の口 | 試験が実装する trait（`DeliveryPorts`・`CredentialPorts`）と、introspection の送る口のクロージャ | `fandhe-auth` の実物の口ではない。実物の口との結線は後続のタスクの担当 |
| 設定 | 既定値を持たない。値の範囲を検査しない | 実物の値は console 側の文書が正。stub の値を本番の値として扱わない |
| 秘密値 | 鍵・proof を持たない。token は借りて渡すだけで保持しない（`SECR-23`〜`SECR-27`） | 実物は鍵を持ち、署名・DPoP の proof を生成する |
| 判定の主体 | 受理・拒否の規則をコードに固定する | 実物の console の規則と一致する保証は無い |

## 3. 模擬の口ごとの差

各節は「stub がすること」「実物との差・判定しないもの」「書き手の読み」の順に書く。

### 3.1 introspection の呼び出し（`introspection::ConsoleIntrospector`、#73）

- stub がすること: 委任の現在の権限の確認として、試験が送る口のクロージャの応答から `active`・`cnf.jkt`・scope を
  確かめる（`ADLG-15` ①・`OPRV-54`・`AGCR-32`）。
- 実物との差・判定しないもの:
  - クライアント認証（`private_key_jwt` の assertion）を付けない。呼び出し元の認証は 2026-11-05 の決定で決まった
    （`ADLG-29`・`D-2026-11-05-2`）が、stub は鍵を持たないためそこへ届いていない。決定に合わせたとは書かず、未対応の差とする。
    実物の合格の根拠にしない。
  - 応答の `aud` を判定に使わない（`OCL-57`・`OCL-58`）。`sid` で照会する口は作らない（`OPRV-56`）。
  - 応答の登録 ID（`AGCR-57`）を扱わない。追跡先は `TASK-808`。
- 書き手の読み: 有効と判定するのは、`200` かつ `active` が真偽値 `true` で要件を満たすときだけとした
  （`AVLB-18`・`AVLB-22` の向き）。401・503・本文の誤り・送れないことはすべて無効側に倒す。

### 3.2 配送の時点の確認（`delivery::ConsoleDeliveryChecker`、#74）

- stub がすること: 配送のたびに ①②③ を 1 回ずつ呼ぶ。① は introspection、② は決めた本人・委任の記録の有効性の照会、
  ③ は Hub への現在の有効性の問い合わせ（`ADLG-15`・`ADLG-16`・`ADLG-29`・`ADLG-31`）。
  成功は 3 つとも有効のときだけである。
- 実物との差・判定しないもの:
  - 静的な確認（署名・DPoP の proof・ローカルの `exp`）を持たない。呼び出し元が済ませた前提である。
  - ① の拒否理由を 1 種類にまとめた（`OPRV-54`）。実物の理由の区別は模さない。
  - ③ の口の URL を設定に持たない。②③ の本文の形は抽象の型で受け、有効か無効かだけを返す。
    口の正本は Hub `ARUN-60` 等で、Hub の原文との照合は人の確認待ちである。
  - 窓（`t_done − t_commit`）を計測せず、上限も当てはめない（`ADLG-26`・`TASK-451`・`TASK-30`）。
- 書き手の読み: ① が無効でも ②③ を省かない（`ADLG-16` の構成 B の読み）。

### 3.3 資格情報の発行と失効（`credential::ConsoleCredentialCaller`、#75）

- stub がすること: console 側のエージェントが `fandhe-auth` を呼ぶ側を模し、Device Authorization の開始と
  `device_code` grant の交換、RFC 7009 の失効、エージェントの登録の失効を呼ぶ
  （`DGRT-1`・`DGRT-7`・`DGRT-8`・`AGCR-1`・`AGCR-8`・`AGCR-35`・`AGCR-36`・`AGCR-43`・`OPRV-53`）。
- 実物との差・判定しないもの:
  - DPoP の proof を生成しない。proof は引数で借りて口へ渡すだけである。
  - 答えは型付きで受け、HTTP の本文を解釈しない。
  - refresh の grant を模さない。
  - ポーリングの間隔を数えない。
  - 登録の失効は、本人の画面操作を試験が代わりに起こす呼び出しである。判定・承認・自動失効を行う経路ではない（`HAPR-1`）。
- 書き手の読み:
  - 呼び出しの向き（console 側のエージェントが `fandhe-auth` を呼ぶ）は書き手の読みである（`D-2026-11-05-6`）。
  - 経路名は最終ではない（`D-2026-10-17-84`）。登録の失効の受付の正本は Hub `ARUN-58`。

### 3.4 呼び出しの記録（`record`、#75）

- stub がすること: 発行・失効・配送の確認の呼び出しを、結果に関わらず 1 件ずつ呼んだ順に記録する。
- 実物との差: 記録は種別と結果だけで、時刻・値・識別子・URL を持たない（`SECR-23`〜`SECR-27`）。
  実物の監査の記録とは別物で、監査の根拠にしない。

### 3.5 障害注入（`fault`、#76）

- stub がすること: 配送の時点の確認の口と資格情報の口の応答に、遅れ・送れない・拒否を設定で注入する。
  `TASK-13` のハーネスが使う。
- 実物との差・判定しないもの:
  - 遅れは `Wait` のフックへ渡すだけで、実際に待たない。遅れの値は仮で、数値の根拠にしない（`THRT-29`・`A-2026-10-28-14`）。
  - 注入できるのは失敗の向きだけ。成功を作る経路・確認を省く経路を持たない（`AVLB-18`・`AVLB-22`・`DPOL-30`〜`DPOL-32`）。
  - ②③ の拒否は「送れない」だけである。introspection の `Status(200)` は本文が空のため `MalformedResponse` になり、成功にならない。
  - 注入の仕組みは本体の設定に持ち込まない。stub の中だけに置く。
- 書き手の読み:
  - 拒否を HTTP の状態コード・接続の失敗で表すのは仮定である。
  - 「適用されたが応答だけが失われる」形は作らない。実物で起きうるこの形は stub で再現できない。

### 3.6 仮定の契約の印（`contract`、#77）

- stub がすること: 契約の項目が仮定か、spec の決定で決まっているかを、静的な ID と印だけで残す。
  項目と印の一覧の正は `contract::contract_record()` で、この文書には写さない。
- 実物との差・判定しないもの: 印は「仮定」と「決定（実物の console との接続は未検証）」の 2 種類だけで、
  「確定」「検証済み」を持たない（`ACPT-7`）。
- 書き手の読み: 決定の印を付けるのは 2 項目だけ（`D-2026-11-05-2`・`D-2026-11-05-5`）。印の割り当ては書き手の読みで、
  見直しと「確定」への変更は人が行う。

## 4. 設定の欄と引く ID

`ConsoleStubConfig`・`AgentCredentialConfig`・`DeliveryFaultConfig`・`CredentialFaultConfig` の欄。ここには値の列を作らない。
欄は `crates/console-stub/src/config.rs` の doc コメントの ID を引く。

| 欄 | 用途 | 引く ID |
| --- | --- | --- |
| `ConsoleStubConfig::issuer` | 試験対象の `fandhe-auth` の `iss` | `OCL-58` |
| `ConsoleStubConfig::audience` | console のクライアントの `aud`。受けるだけで組み立て・登録をしない | `OCL-58`・`D-2026-11-05-5` |
| `ConsoleStubConfig::client_id` | console のクライアントの `client_id` | `ADLG-15`・`D-2026-11-05-5` |
| `ConsoleStubConfig::introspection_endpoint` | introspection の口の URL（取得しない） | `AGCR-32`・`OPRV-54` |
| `ConsoleStubConfig::validity_endpoint` | 有効性の照会の口の URL（パスは写さない） | `ADLG-29`・Hub `ARUN-60` |
| `ConsoleStubConfig::delivery_window_max` | 進行中の配送の窓の上限 | `ADLG-26`・`TASK-451`・`A-2026-10-28-14` |
| `AgentCredentialConfig` の各 URL | 発行・交換・失効・登録の失効の口の URL | `DGRT-1`・`DGRT-7`・`DGRT-8`・`AGCR-35`・`AGCR-36`・`AGCR-43`・`OPRV-53`・Hub `ARUN-58` |
| `AgentCredentialConfig::client_id` | エージェントの公開クライアントの `client_id` | `AGCR-1`・`AGCR-2` |
| `DeliveryFaultConfig`・`CredentialFaultConfig` | 注入する規則（口ごとの呼び出し回数の範囲・遅れ・拒否） | `AVLB-18`・`AVLB-22`・`THRT-29` |

- `issuer`・`audience`・`delivery_window_max` は設定として保持されるだけで、stub の判定には使われない（`src/` で参照しない）。
  実物でこれらを判定に使う段で当てはめる（§5）。
- 値の範囲の検査は持たない。
- 秘密値は持たない。鍵などを足す段は `TODO(TASK-511)` で追う。

## 5. 実物への差し替え先の TASK

`TASK-8` の 2026-10-28 追記は、実物の console が提供されたときの差し替えと再試験を、MS-10 の既存の 6 タスクで行うとしている
（前提の仮定は `A-2026-10-28-12`・`A-2026-10-28-14`）。
項目と TASK の対応づけは spec が集合で示しているだけで、下の行ごとの割り当ては書き手の読みである。

| TASK | Issue | roadmap 上の所属 | stub の差し替え・再試験の対象（書き手の読み） |
| --- | --- | --- | --- |
| `TASK-62` | #4880 | MS-10 | console のクライアントが照会できる `aud`（`OCL-58`）。§4 の `audience` |
| `TASK-63` | #4883 | MS-10 | console が質問・回答・配送を保存する形（`ADLG-10`） |
| `TASK-64` | #4886 | MS-10 | 配送の取り消し・訂正の通知（`ADLG-27`。契約の候補 4。`A-2026-10-28-13`） |
| `TASK-88` | #4898 | MS-10 | 実物の console との照合（`ACPT-15`）。§3.1〜§3.3 の再試験 |
| `TASK-449` | #5060 | MS-10 | 実物の console・本番の Hub での `ADLG-6`・`ADLG-9`・`ADLG-11`〜`ADLG-16` の流し直しと `sid` の一致（`ADLG-23`・`ADLG-30`） |
| `TASK-30` | #4807 | MS-10 | 窓の上限の数値。§4 の `delivery_window_max`。stub の測定は根拠にしない |

関連（差し替え先ではない）:

- `TASK-451`（#4347、MS-8）: 窓の上限の受け口と計測点。
- `TASK-808`（#5855、MS-8）: introspection の応答の登録 ID（`AGCR-57`）。§3.1 の扱わない点の追跡先。
- `TASK-13`（#122）: 障害注入のハーネス。§3.5 の `fault` の利用者。
- `TASK-511`（#582）: 秘密値の型の crate。鍵・proof を足す段の追跡先。

差し替え・再試験を終えても、「確定」への変更は人が記録する（`ACPT-7`）。

## 6. 参照した spec

- `fandhe-auth-spec` の `547e305`（この repo の `main` が固定している版）の `05-tasks.md`・`06-roadmap.md`。
- Hub の ID は Hub の原文を読めないため、原文との照合は人の確認待ちである。
