---
paths:
  - "Cargo.toml"
  - "**/Cargo.toml"
  - "Cargo.lock"
  - "deny.toml"
  - ".cargo/**"
description: >
  本体の依存 crate の追加・更新・feature 選択の規約。PoC で確かめた版と版の記録（相手側の部品の版の固定と照合は Hub の ARUN-17）、
  暗号の自前実装の禁止と JWT の backend（DPOL-4・17・47）、rsa・openssl・native-tls の bans（DPOL-10・11・13・18・21・46）、cargo-deny と
  cargo-audit の 5 検査とライセンスの許可リスト（DPOL-20〜25・33〜35）、試験用の feature を作らないこと、依存を足すときの
  確認手順（cargo deny / cargo audit / cargo tree -i。rsa・native-tls・openssl・web-sys・HTTP クライアントの具象 crate の逆引き）を定める。プロファイルと target/ の掃除は rust-build-profile。
---

# 依存の方針（Cargo）

正本は `docs/spec/04-behavior/dependency-policy.md`（DPOL）。多くの DPOL は本番の crate が無く検査が未実施のため「検討中」だが、
守る規則として扱う。ここに無い判断（依存・例外の追加）は原文を確かめ、決まっていなければ利用者に伺う。
`[profile]`・`target/` の掃除・CI キャッシュへの影響は `./rust-build-profile.md` に従い、ここでは繰り返さない。crate の構成と依存の向きは `./coding-rust.md`、本体と Hub の境界は `./library-boundary.md`。
Hub の本番のバイナリの依存監査・advisory の運用は Hub が持ち、本体の禁止の依存を下限として含める（`AOPS-1`。DPOL-11・DPOL-18・DPOL-34 の改訂）。

## 版

- 下の版は PoC・受け取りで確かめた版。spec は本番の版を要件にしていない。
  - `webauthn-rs` 0.5.5: 既定の options を実測した版（PKEY-4）。既定の options に頼る前提を変える更新は原文と突き合わせる。
    state の期限は検査しないので、core 側で `now < expires_at` を判定する（PKEY-11）。
  - `fandhe-backend` 0.6.0: 2026-10-27 公開の版を受け取った記録（`D-2026-10-28-2`）。固定する版は部品の差し替えのときに決める（同）。
  - `sqlx` 0.9.0 と `AssertSqlSafe`（AUDB-43 → Hub `ASTORE-91`）は Hub の `-db-pg` の担当。本体は `sqlx` に依存しない（DPOL-37。`./db-layer.md`）。
- 相手側（`fandhe-backend`・`fandhe-frontend`）の部品の版の固定と、固定した版での流し直しは Hub の担当（DPOL-9・CSRF-13 → Hub `ARUN-17`。`A-2026-10-16-5`）。
  本体に残るのは `Cargo.lock` のコミットと、`webauthn-rs` 0.5.5 の版の記録（PKEY-4。固定の根拠は DPOL-4・DPOL-10）。
- 外向き通信は、本体が組込み先に要求する契約（信頼する CA は設定で渡されたものだけ・OS の信頼ストアを使わない・リダイレクトを追わない・打ち切り時間を持つ）を持つ
  （DPOL-46・HOST-7）。契約は `OutboundHttp` で受け、実装は組込み先（Hub）が持つ。本体の依存に HTTP クライアントの具象 crate（`reqwest` 等）を持たない
  （利用者の決定 `D-2026-11-20-4`。根拠は HOST-7 の期待欄。HOST-9 は依存に現れてよいのを組込み先の実装側に限る。HOST-7・HOST-9 はいずれも検討中・〔提案〕。`D-2026-11-20-4` 自体は利用者の決定）。
  実クライアント（`reqwest` の `rustls-tls-manual-roots` 等）の選定と本番の依存解決での検査は Hub の `ARUN-14`（DPOL-46 の行内分割）。

## 暗号

- 暗号・署名・乱数・パスワードハッシュ・WebAuthn の検証は保守されている既存 crate に任せ、自前の実装 0 件（DPOL-4・DPOL-47）。
- JWT の署名・検証は `aws-lc-rs` か `ring`（DPOL-10・DPOL-11）。
- `jsonwebtoken` は `aws_lc_rs` feature だけ。`rust_crypto` は `rsa` を引くので有効にしない（DPOL-17）。

## bans

| 対象 | 規則 | 根拠 |
| --- | --- | --- |
| `rsa`（RUSTSEC-2023-0071） | 本番の依存（RP 向けの `fandhe-auth-client` を含む）に 0 件 | DPOL-18（未評価。逆引きだけでは満たさない）・DPOL-21 |
| `openssl` | 直接の宣言 0 件。逆依存の根は `webauthn-rs` 系だけ。`vendored` 不可。配備物は配備先の OpenSSL に動的にリンクする（版の記録 DPOL-12 は → Hub `AOPS-2`） | DPOL-10・DPOL-11・DPOL-13（`vendored` は `scripts/check-openssl-vendored.sh`〔`make lint`・`make no-vendored`〕で機械的に検査する。合否の対象は試験専用メンバーを除いた本番メンバーの normal・build 依存で、既定 feature と `PROD_FEATURES` で明示した feature だけ。全 feature を有効にする走査は警告だけの参考） |
| `native-tls` | 使わない。外向き通信の TLS は OS の信頼ストアに頼らない（DPOL-46。実クライアントの選定は Hub の `ARUN-14`） | DPOL-46 |
| `web-sys` | サーバー側のビルドに 0 件。引いてよいのは `fandhe-auth-webauthn-wasm`（段階 2）だけ。サーバーのビルドの確認は Hub のビルドにも課される（→ Hub `ARUN-16`） | DPOL-40・`D-2026-10-17-35` の (2) |

- RP 向け crate の JWT 検証部は自前の薄い検証部で、`rsa` を入れない（DPOL-21）。`fandhe-backend` の `REQ-16` は 0.5.0 で提供済みで、
  版を固定し、再試験の後に差し替える（DPOL-21・`A-2026-10-16-5`）。版の固定と再試験（ACPT-16・ACPT-23 等）の実施は Hub が行う（DPOL-9 → Hub `ARUN-17`）。
  本体側は `Cargo.lock` と版の記録を持つ。

## cargo-deny と cargo-audit

- `deny.toml` はまだ無い。本番の厳格版は TASK-668 で作る。TASK-2 は `[licenses.private] ignore = false` の設定の下書きまでで、
  `fandhe-backend` の `deny.toml` との照合は Hub の担当（DPOL-44・TASK-683 → Hub `AOPS-4`・`AOPS-5`）で、本体は TASK-668（厳格版）と TASK-2 の残りだけを持つ。
  作るまで下の基準を手元の確認に使う。MPL-2.0 を `fandhe-backend` の許可リストに足さない条件は緩めない（DPOL-45 → Hub `AOPS-5`）。
- advisories・bans・licenses・sources の 4 検査と `cargo-audit` の 5 つが全て成功しない変更はマージしない（DPOL-34）。
  CI の枠は `ci.yml` の `deps-audit`（道具の導入だけ。TASK-6 6/10）で、検査の結線は TASK-680。契約は `./github-actions.md` の「依存の検査の拡張点」。
- 依存の変更のたびと、変更が無い期間も 1 日 1 回流す。1 日 1 回は初期値で、本番の開始から 3 か月の後に見直す。新しい advisory が
  見つかったら運用の監視の警報の経路（DPOL-34 の改訂では Hub の `AOPS-3`）で運用担当（人）へ知らせる（DPOL-35）。
- advisory DB は実行の前に 1 回だけ取得し、全ての実行に `--disable-fetch`（cargo deny）・`--no-fetch`（cargo audit）を付ける。
  `Cargo.lock` は書き換えない（DPOL-33）。使った DB の commit と取得日は、今は記録していない（下の記録の項）。
- licenses の許可リスト: MIT・Apache-2.0・Apache-2.0 WITH LLVM-exception・Unicode-3.0・BSD-3-Clause・ISC・BSD-2-Clause・
  MPL-2.0・BSL-1.0・CDLA-Permissive-2.0・Zlib・NCSA。NCSA は `libfuzzer-sys`（`(MIT OR Apache-2.0) AND NCSA`）のために足した（許可リスト全体に足す選択〔main の読み〕。確定の範囲（QP-2 の 18 ワークスペース）は広げない。crate 限りの例外（`[[licenses.exceptions]]`）にするかは、`deny.toml`（TASK-668）を作るときに決める未定。DPOL-22 の 2026-11-20 追記・`D-2026-11-20-9`。足した後の厳格版の通過は本番の依存解決では未確認）。LGPL-2.1-or-later は `r-efi` に限った例外で、単独の LGPL は通さない（DPOL-22・DPOL-24）。
- 自身の crate は `license = "MIT OR Apache-2.0"` を宣言する（DPOL-26・`D-2026-10-23-8`）。
- sources は crates.io だけ。git 依存・他の registry 0 件（DPOL-25）。
- 例外（`ignore`・`skip`・許可の追加）には理由・期限・対象 crate の範囲を同じ場所に書く（DPOL-20）。
  例外を足すか、advisory をどう扱うかは人が決める。AI は案と根拠を示すところまで（DPOL-35）。

## feature

- 試験用の経路・ログインを省く分岐・検証を外す feature を `[features]` に作らない（DPOL-28・DPOL-30〜32）。
  DPOL-29（試験用のログインを feature で足す次点の案。Could・検討中）は採っていない。
- 試験用の仕組みで、本番のビルドに現れない例外は次の 2 つだけ。
  - 契約テストの変異スイッチ。既定で無効（`default` に入れない）の feature か別 crate に閉じ、環境変数だけで有効にならない（DPOL-31）。
  - メモリの代替アダプター。試験用 feature の後ろに置き、本番の `cargo tree -e features` に現れない（LIB-6）。
  運用・検査用の `Ops` は Hub の担当（DPOL-38 の改訂 → Hub `ASTORE-5`）で、本体は作らない。
- `jsonwebtoken`（DPOL-17）は、決まった feature 以外が有効になっていないかを `cargo tree -e features` で確かめる。
- HTTP クライアントの具象 crate（`reqwest` 等）は本体の依存に現れないことを `cargo tree --locked --workspace -e normal,build --target all --features "$PROD_FEATURES" -i reqwest` で確かめる（合格: 一致なし。HOST-7・HOST-9）。下の確認手順の bash ブロックにも入れてある。
  限界: 機械の検査は `reqwest` の 1 名前だけを見る。他の具象 crate（`hyper` の client・`ureq` 等）はレビューで見る。
  検査する名前の一覧と `deny.toml` の bans への追加は未定（TASK-668 で決める。利用者が決めるまで名前を bans に足さない）。

## 依存を足す・更新するときの確認手順

この手順は暫定で、cargo-audit の DB の取得が監査を兼ねており DPOL-33 の取得と監査の分離を満たさない。正式な手順と記録の様式は TASK-679（DPOL-33）、`deny.toml` は TASK-668 で決める。
この手順の合格を DPOL-34 の合格とは扱わない。
cargo-deny・cargo-audit の結果は、どの設定が読まれたかを確かめる手順が決まるまで（TASK-668・TASK-679）、`deny.toml` の有無にかかわらず参考結果（規約の合否ではない）とする。DPOL-18・DPOL-20・DPOL-22・DPOL-24・DPOL-25・DPOL-33・DPOL-34 は PR 本文に「未評価」と書く（`dependency-auditor` Agent の手順と同じ）。

ルートで次を実行し（`dependency-auditor` Agent も同じ手順で流す）、結果（参考結果。DPOL-18・DPOL-20・DPOL-22・DPOL-24・DPOL-25・DPOL-33・DPOL-34 は未評価）と、例外を足したならその理由を PR 本文に書く。

```bash
# advisory DB の取得（実行の前に 1 回だけ。DPOL-33）。取得日・DB の版は記録しない（下の記録の項。TASK-679 で決める）
cargo deny fetch
cargo audit --file Cargo.lock   # cargo-audit の DB の取得を兼ねる（QP-2 の手順）

# 取得なしで流す（DPOL-33・DPOL-34）
cargo deny --locked check --disable-fetch   # advisories・bans・licenses・sources
cargo audit --no-fetch --file Cargo.lock    # cargo-audit 0.22.2 には --locked が無いため、下の git diff で書き換えが無いことを確かめる
git diff --exit-code Cargo.lock      # 意図しない Cargo.lock の書き換えが無いこと（依存を足した変更では差分を目視で確かめる）

# PROD_FEATURES: 本番のビルドで有効にする feature を `<crate>/<feature>` のカンマ区切りで明示する（無ければ空）。--all-features は使わない
# SERVER_FEATURES: サーバー側の本番の feature だけを `<crate>/<feature>` のカンマ区切りで渡す（--workspace と併用する feature は必ずこの形。無ければ空）
cargo tree --locked --workspace -e normal,build --target all --features "$PROD_FEATURES" -i rsa         # 合格: 「did not match any packages」（一致なし）。DPOL-21 の観点に限る。DPOL-18 は厳格版の bans も要るので未評価（逆引きだけでは満たさない）
cargo tree --locked --workspace -e normal,build --target all --features "$PROD_FEATURES" -i native-tls  # 合格: 同上
cargo tree --locked --workspace -e normal,build --target all --features "$PROD_FEATURES" -i openssl     # 合格: 一致なし、または逆依存の根が webauthn-rs 系だけ
cargo tree --locked --workspace --exclude fandhe-auth-webauthn-wasm -e normal,build --target all --features "$SERVER_FEATURES" -i web-sys      # 合格: 一致なし（DPOL-40。`fandhe-auth-server` は Hub の担当で、ここでは確かめない）
cargo tree --locked --workspace -e normal,build --target all --features "$PROD_FEATURES" -i reqwest     # 合格: 一致なし（HOST-7・HOST-9。名前は reqwest だけ。上の「feature」の限界を参照）
```

- advisory DB の記録（DPOL-33）: DPOL-33 は Should で、使った DB の commit と取得日の両方の記録を求めるが、今は記録していない。取得日も DB の版も「未記録（TASK-679 で決める）」と書く。取得の開始ログ（`Fetching advisory database from` 等）・終了コード・DB の HEAD・取得元との照合（`ls-remote` 等）は、どれも取得や DB の版の根拠にしない。記録の手順は TASK-679 で決める。
- cargo-deny・cargo-audit の結果は、どの設定が読まれたかを確かめる手順が決まるまで（TASK-668・TASK-679）、すべて参考結果とする。DPOL-18・DPOL-20・DPOL-22・DPOL-24・DPOL-25・DPOL-33・DPOL-34 の判定は未評価とする。読まれ得る設定ファイルの名前は未確認の例として、`deny.toml`・`.deny.toml`・`.cargo/deny.toml`・`audit.toml` がある（確かめ方は決めていない。親ディレクトリの設定を探すかも未確認）。
- `cargo tree -i` は一致が無いとき `error: package ID specification ... did not match any packages` を出し、終了コードは
  0 にならない（cargo 1.100.0 で 101 を確認）。対象が `Cargo.lock` にあっても指定した依存の種類・feature のグラフに無いときは、終了コード 0 で `nothing to print` を出す。`--workspace` を付けないと、ルートが virtual manifest でない場合にルートの package の木しか見ない。他の誤りでも非 0 になるので、終了コードだけで合否を決めず、出力の文言を確かめる。
  「一致なし」と読むのは、stderr を取得でき、指定した `-i` の対象（例: `rsa`）についての `did not match any packages` の診断だけのとき、
  または終了コード 0 で `nothing to print` の他に木の出力が無いときで、後者を実行失敗と書かない。
  stderr を取得できない・別の誤り（lockfile・manifest・fetch・未知の feature 等）・別の対象への診断は「実行失敗」で不合格とし、「依存なし」と読まない。
  `absent`（一致なし）と、`--exclude` 等で絞り込んだ結果の 0 件（`filtered`）は区別して書く。集計行でも混ぜず、禁止依存が見つからなかったことだけを示すなら絞り込みの有無を併記する。
  `openssl` が見つかったときは、手動で確認した結果（逆依存の根と、DPOL-10・DPOL-11 への適合の判定）を PR 本文に記録する。
- `-e normal` だけでは build-dependencies が抜けるので `-e normal,build` で見る。dev-dependencies は除く。試験だけで使う依存の
  監査は `-e dev` を足して別に見る。
- `--all-features` は DPOL-31 の変異スイッチなど試験用の feature も有効にし、本番のビルドの木と変わるので使わない。
  feature を明示しないと既定の feature だけを見る。
- 本番のビルドで有効にする非既定 feature を足す変更では、同じ変更で `PROD_FEATURES` も更新する。更新しないと、その feature が引く経路（例: `openssl-sys/vendored`）が合否の検査（`make no-vendored`・DPOL-13）から外れる。
  `scripts/check-openssl-vendored.sh` は環境変数 `PROD_FEATURES` を同じ形式で受け、合否の走査に含める。
- `web-sys` の確認は、`fandhe-auth-webauthn-wasm` を `--exclude` で除いた workspace 全体に `--target all` を付けて行う（crate ごとの `-p` は使わない）。`--exclude` の crate が workspace に無い間（段階 2 の前）は `--exclude` を外して流す。Hub の本番のバイナリでの確認は Hub が持つ（`ARUN-16`）。
- TASK-4 の選定基準は K1〜K12（`D-2026-11-20-7`）。K7: 試験の道具は dev 依存か別の workspace に置き、本番の依存の木に入れない。K11（推移依存の少なさ・保守）は、数の閾値を置かず、次の 4 つで見る（利用者の決定。`D-2026-11-20-14` の (4)(d)）:
  最新の安定版・既知の advisory 0 件・feature は必要最小・依存の `rust-version` が MSRV 以下（MSRV は `D-2026-11-20-12`）。新しい crate を足すときも同じ 4 つで確かめる。
- 選定の結果（`D-2026-11-20-6`〜`10`・`14`）。版は spec の記録時点のもので、版の固定の手順は spec に記録が無く未定:
  - 版と feature の選び方（`D-2026-11-20-14` の (4)(a)(b)(c)）: 版は、可能な限り安全な最新の安定版（pre-release を除く）。最新の安定版が依存の `rust-version` で MSRV（`D-2026-11-20-12`）を超える、または既知の advisory があるときは、条件を満たす旧版を勝手に選ばず、その crate を上げずに利用者へ確かめる（この場合の扱いは `D-2026-11-20-14` に定めが無く未定。決める人は利用者）。feature は `default-features = false` から要るものだけを足す。
    口は `tracing` が `std` だけ・`metrics` が既定。試験の道具は `tracing-subscriber` が `fmt` だけ・`proptest` が `std` だけ（`fork`・`timeout` を外す）・`syn` が `full`・`parsing` だけ・`libfuzzer-sys` が既定
    試験の道具の feature は試し実装（`crates/trial-deps`）で選んだ組で、`log` の記録は `fmt` だけでは捕獲に届かない。`TASK-518` で `log` を捕まえるために要る feature（例: `tracing-subscriber` の `tracing-log`。または試験専用の別の捕獲の経路）は、(4)(a) の「要るものだけを足す」として足してよい。足すときは K11 の 4 つで確かめる。`tracing-log` を採る場合、feature を足すだけでは `log` から `tracing` への転送は始まらない（試し実装の捕獲は `with_default` の局所の subscriber）。試験の中で転送を初期化し、`log` を経由した陽性対照が捕獲先に届くことを確かめる（SECR-23・AAUD-30・AAUD-31。経路ごとの陽性対照は `./coding-rust.md` の TASK-518 の箇条）
  - ログの出す口 `tracing`・計測の出す口 `metrics`（本体の依存）。`tracing-subscriber` は dev-dependencies だけ（`D-2026-11-20-8`）
  - property test は `proptest`、fuzz は `cargo-fuzz`（`libfuzzer-sys` を使う）。いずれも本番の依存の木に入れない（K7）。
    fuzz の nightly は日付で固定せず最新を追う。実行の記録に nightly の版を残すかは **未定**（`D-2026-11-20-9`）
  - `SECR-21` の構文木の走査は `syn` を使う自前の検査（`D-2026-11-20-10`）。置き場は spec に記録が無く **未定**。
    マクロ呼び出しの中の型を見落とすかは `TASK-514` の試し実装で確かめる（main の読み）
  - 最小の試し実装（TASK-4 の成果物）は実装 repo に入った（`crates/trial-deps`。`D-2026-11-20-14` の (1)）。引き継ぎ先の TASK は spec の `TASK-4` の追記で引く
  上の対象以外の依存は選定の記録に無いので、足す前に利用者へ確かめる。メール送信と外向き HTTP クライアントは選定の対象から外れた。本体は口と契約テストだけを持ち、具象は組込み先が選ぶ（`D-2026-11-20-4`・`HOST-7`・`AMTH-14`・`AAUD-40`・`DPOL-46`）ので、本体の依存に足さない。

## 関連 rule

- プロファイル・target/: `./rust-build-profile.md`
- crate 構成・試験用の分岐: `./coding-rust.md`
- 秘密値の型の crate の依存制約: `./secret-values.md`
