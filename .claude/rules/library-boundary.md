---
paths:
  - "crates/**"
  - "Cargo.toml"
  - "**/Cargo.toml"
description: >
  fandhe-auth が「Hub が組み込むライブラリ」であることの境界の規約（D-2026-10-30-1）。本体の責務と公開 API・依存方向（LIB-1〜3・LIB-14・LIB-15）、
  組込み先が渡す依存（HOST）、外部ログイン Plugin の拡張契約（PLG）、iss を引数で受けること（DPOL-48）、画面の安全条件の正本（UISF）を定める。
  crate の表と名前は coding-rust.md、依存の bans は dependency-policy.md、HTTP・Cookie 等の要求値は security.md が持つ。
---

# ライブラリの境界（fandhe-auth）

`fandhe-auth` は Hub が組み込むライブラリで、HTTP の受付・永続状態の実装・配備・監視・復旧・画面の描画などの実行と運用は Hub が持つ（`D-2026-10-30-1`）。
この repo が持つのは、型・判定・拡張契約・ストレージ契約・安全条件である。正本は `docs/spec/04-behavior/library-boundary.md`（LIB）。
LIB は全行が検討中〔提案〕〔未検証〕。HOST・PLG・UISF はステータスが行ごとに異なる（UISF には旧構成の証拠で確定のまま残す行がある。確定の根拠は旧構成の PoC で、
新しい構成では確かめていない）。実装は無い。守る規則として扱うが、AI が確定と読み替えない（ACPT-7）。ステータスは引く前に原文で確かめる。

## 適用対象

`crates/**` と `Cargo.toml` を書く・直す Agent。新しい公開 API・crate・依存を足すときに、本ファイルの区分で「本体に置くか」を先に確かめる。

## 本体が持つもの・持たないもの（LIB-1）

| 区分 | 内容 |
| --- | --- |
| 本体が持つ | 型・判定・自社発行のプロトコル処理・ストレージ契約・秘密値の型・外部ログイン Plugin の拡張契約・組込み先への依存の契約・設定の検査関数・試行制限の判定・失効の意味・`return_to` の検査 |
| 本体が持たない（組込み先 = Hub） | HTTP の受付・TLS・DB の接続・秘密の保管と更新・イベントの永続化と配送・監査の集約と検索・画面の描画・接続先ごとの SDK |

- 公開 API に「持たない」側の実装を置かない。分類できない公開項目を作らない（LIB-1）。責務の分類を変えるときは、先に spec（`04-behavior/`）を直す必要がある。
  spec の編集はこの repo ではせず、利用者の指示を待つ（`./spec-reference.md`）。
- 本体の型・trait の引数と戻り値に、Hub の組織・課金の型、DB の具象型（接続・トランザクション）、特定の HTTP の枠組みの型を出さない（LIB-3）。
- 暗号の基本処理を自前で実装しない（LIB-21・DPOL-4・DPOL-47。`./dependency-policy.md`）。
- 配備の形（同居・分離）・プロセス境界・ドメインを前提にしない（LIB-17）。

## 依存方向（LIB-2・LIB-4）

- 依存の向きは「Plugin → 本体」「組込み先 → 本体・Plugin」だけ。本体から Plugin・組込み先への依存を作らない。試験の都合でも足さない（LIB-2・LIB-4）。
- 本体の段階 1 の crate は、外部ログイン Plugin・Hub の crate・接続先ごとの SDK・SAML の実装・サーバー側の HTTP の枠組み・DB ドライバー・
  非同期ランタイムに依存しない（LIB-2。DPOL-1 を維持する）。
- `fandhe-auth-client` の依存は秘密値の型と共有型に限り、本体のサーバー側へは依存しない（LIB-2・LIB-16）。
- 本体の試験は本体の中の模擬 Plugin とメモリの代替アダプターで流す（LIB-4・LIB-5・LIB-6）。メモリ実装は試験用 feature の後ろに置き、
  本番のビルドの `cargo tree -e features` に現れない（LIB-6）。

## 組込み先が渡す依存（HOST）

- 時刻・乱数・秘密・外向き通信・イベント・認可・ストレージは、本体が引数（trait）で受ける。渡す側の実装は組込み先（`host-ports.md`。HOST-1〜3・HOST-7・HOST-21）。
- 組込み先の依存ごとに、メモリ・固定値の代替アダプターと失敗を返す模擬を用意する（LIB-7）。名前（`Clock`・`Rng`・`OutboundHttp` 等）は spec 上の仮称で、
  実装の名前は未定。
- 外向き通信の契約（信頼する CA は設定で渡されたものだけ・リダイレクトを追わない・打ち切り時間を持つ・接続ごとの宛先の許可リスト）は本体、実クライアントの選定は Hub（DPOL-46・HOST-7。実行は Hub の `ARUN-14`）。
  許可リストは接続の登録時に確定し、外部 IdP の応答で広げない。許可リストと独立に、ループバック・リンクローカル・私設のアドレス・クラウドのメタデータの宛先へ送らない（HOST-8）。
  許可リストと宛先の拒否（HOST-7 の (4)・HOST-8）は Plugin に渡す口だけに掛かる。失効の通知の送出の口は (1)〜(3) だけを共有し、宛先は登録済みと https で限る（RVPG-24・RVPG-26・RVPG-27。送出の実行は Hub の `AEVT-19`・`AEVT-21`）。
- 秘密は `fandhe-auth-secret` の型で保持し、出力に値を出さない（HOST-4。`./secret-values.md`）。ログ・イベントに出してよい情報の規則は HOST-10〜13。

## 外部ログイン Plugin（PLG・IDLK・AMTH）

- Plugin は同じプロセスの信頼済みコードで、本体の拡張契約（`plugin-contract.md`）を実装する。外部 IdP との通信は本体の外向き通信の口を介し、独自の HTTP クライアントを使わない（PLG-22・HOST-9）。
- 契約の版は `PluginDescriptor`（仮称）で持ち、範囲外の版は登録を拒否する（LIB-10）。外部 Identity の連結は `identity-link.md`、追加の認証方式は `auth-methods.md` が正本。
- OIDC の共通部品は本体に含めず、段階 1 で作る同梱の Plugin として扱う（LIB-1・PLG-26）。crate 名は `fandhe-auth-plugin-oidc`（`crates/plugin-oidc`）に決定済み（`D-2026-11-20-2`。crates.io の空きは未照会で、公開するかは別に決める）。
- 自社の Provider・RP 向け crate・外部ログイン Plugin の 3 役割は別の型・別の設定にし、取り違えて渡せないようにする（LIB-19）。

## 公開 API の版と契約テスト

- semver に従い、公開 API の変更は `cargo-semver-checks` 相当で CI に掛ける。破壊的変更は major と移行手順を伴い、削除は `deprecated` の版を経る（LIB-8・LIB-9）。
  公開の列挙型・構造体は `#[non_exhaustive]` を基本とする（LIB-22）。検査の道具・CI への載せ方は未定（CI の枠は TASK-6）。
- 契約テスト（ストレージ・Plugin・組込み先の依存）は組込み先が `dev-dependencies` で流せる公開 crate とする（LIB-11・LIB-12。ストレージは `./db-layer.md`）。
- 依存監査・互換性検査・契約テストは、Hub の repo・環境なしで本体の repo だけで流せる形を保つ（LIB-18）。

## `iss` は引数で受ける（DPOL-48）

- 発行者の識別子 `iss` は設定の型で受け取り、本体のソースに固定値を置かない（0 件）。1 つの組み立ての中では discovery・ID トークン・アクセストークン・JWKS・
  introspection の `iss` が一致する（DPOL-48。契約テストで比較する）。

## crate の段階（LIB-14・LIB-15）

段階の区分と crate の表は `./coding-rust.md`。ここでは扱い方だけを書く。

- 段階は〔提案〕で、利用者の決定は crate の名前（`D-2026-10-27-8`・`D-2026-10-28-1`・`D-2026-10-29-2`）までである。段階を確定した構成として書かない。
- 最初のリリースのメンバーは段階 1 に限る。段階 2 は `LIB-15` の 4 基準（2 つ以上の利用側・契約の安定・Hub への非依存・暗号を自作しない）を記録で満たしたものだけ。
- `fandhe-auth-backend`（`crates/backend`）は `TASK-9` の暫定の置き場として残すが、最初のリリースに含めない扱い（`D-2026-11-20-1`）。
  今の `LIB-14` の操作では段階 2 のメンバーとして見つかり、除き方は未決（`Q-2026-11-20-1`）。
  今の `crates/backend`（空の枠）は `LIB-15` の期待（記録のない切り出しが 0 件）を満たしておらず、利用者がそれを判定の時点までの外れとして受け入れた（`D-2026-11-20-3` の (ア)）。
- `LIB-15` の判定は `interim` に最初の中身を足す前、または `fandhe-backend` への依存を足す前の早い方で記録する。
  **判定を記録するまで、`crates/backend` のどこにも（`interim` の外の新しい module・ファイルも含めて）中身を足さない（自動の実装を含む）**。判定は人が行う（`D-2026-11-20-3`・ACPT-7）。進めてよいのは、記録欄の (1)〜(4) すべてに「満たす」という人の記入があるときだけで、「未記入」・「満たさない」の欄があれば止めて利用者に報告する。
  判定の記録欄は `docs/interim/README.md` の「backend の切り出しの記録」で、判定の有無はここで確かめる。欄は人だけが記入し、Agent は書かない。`Cargo.toml` の注記は判定の根拠にしない（ACPT-7）。
- `interim` で作らないもの・置き場が未決のものの区分は `./coding-rust.md` の `crates/backend` の項（`D-2026-11-20-3`）。
- Hub が持つ crate（`-db-pg`・`-db-fandhe`・`-db-migrate`・`-server`）は作らない。作る前に利用者に確かめる。

## 画面の安全条件（UISF）

本体は画面を描画・稼働しない。画面が守る安全条件（XSS の無効化・`return_to`・列挙できない同じ形の失敗・招待なしの登録の導線 0・CSP と `no-store` の要求値など）の
契約の正本は `ui-safety.md`（UISF）で、描画・文言・E2E は Hub（`ASCR`）。CSRF・Cookie・CSP の要求値の扱いは `./security.md`。

## 関連 rule

- crate の表・依存の向き: `./coding-rust.md` / bans・版: `./dependency-policy.md`
- 要求値と組込み先の分担: `./security.md` / spec の読み方・移管 ID: `./spec-reference.md`
- 秘密値: `./secret-values.md` / ストレージ契約: `./db-layer.md`
