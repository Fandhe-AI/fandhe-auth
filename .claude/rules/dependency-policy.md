---
paths:
  - "Cargo.toml"
  - "**/Cargo.toml"
  - "Cargo.lock"
  - "deny.toml"
  - ".cargo/**"
description: >
  依存 crate の追加・更新・feature 選択の規約。PoC で確かめた版と相手側の版の固定（DPOL-9・CSRF-13）、暗号の自前実装の
  禁止と JWT の backend（DPOL-4・17・47）、rsa・openssl・native-tls の bans（DPOL-10〜13・18・21・46）、cargo-deny と
  cargo-audit の 5 検査とライセンスの許可リスト（DPOL-20〜25・33〜35）、試験用の feature を作らないこと、依存を足すときの
  確認手順（cargo deny / cargo audit / cargo tree -i）を定める。プロファイルと target/ の掃除は rust-build-profile。
---

# 依存の方針（Cargo）

正本は `docs/spec/04-behavior/dependency-policy.md`（DPOL）。多くの DPOL は本番の crate が無く検査が未実施のため「検討中」だが、
守る規則として扱う。ここに無い判断（依存・例外の追加）は原文を確かめ、決まっていなければ利用者に伺う。
`[profile]`・`target/` の掃除・CI キャッシュへの影響は `.claude/rules/rust-build-profile.md` に従い、ここでは繰り返さない。crate の構成と依存の向きは `.claude/rules/coding-rust.md`。

## 版

- 下の版は PoC・受け取りで確かめた版。spec は本番の版を要件にしていない。
  - `webauthn-rs` 0.5.5: 既定の options を実測した版（PKEY-4）。既定の options に頼る前提を変える更新は原文と突き合わせる。
    state の期限は検査しないので、core 側で `now < expires_at` を判定する（PKEY-11）。
  - `fandhe-backend` 0.6.0: 2026-10-27 公開の版を受け取った記録（`D-2026-10-28-2`）。固定する版は部品の差し替えのときに決める（同）。
  - `sqlx` 0.9.0: `AssertSqlSafe` を前提にした版（AUDB-43。`.claude/rules/db-layer.md`）。
- 相手側（`fandhe-backend`・`fandhe-frontend`）が提供した部品は、使う前に版を `Cargo.lock` で固定し、固定した版で PoC-12 の
  手順 3〜7・10 と PoC-10 の手順 12 を流し直してから差し替える。流し直す前に差し替えない（DPOL-9・CSRF-13・`A-2026-10-16-5`）。
  DPOL-9・CSRF-13 の対象は相手側の版で、`webauthn-rs`・`sqlx` は含まない。`Cargo.lock` はコミットする。
- 外向き HTTP は `reqwest` の 1 つだけ。TLS の feature は `rustls-tls-manual-roots` だけで、OS の信頼ストアを使わず、
  渡された CA だけを信頼する（DPOL-46。backend の統一は TASK-32）。

## 暗号

- 暗号・署名・乱数・パスワードハッシュ・WebAuthn の検証は保守されている既存 crate に任せ、自前の実装 0 件（DPOL-4・DPOL-47）。
- JWT の署名・検証は `aws-lc-rs` か `ring`（DPOL-10・DPOL-11）。
- `jsonwebtoken` は `aws_lc_rs` feature だけ。`rust_crypto` は `rsa` を引くので有効にしない（DPOL-17）。

## bans

| 対象 | 規則 | 根拠 |
| --- | --- | --- |
| `rsa`（RUSTSEC-2023-0071） | 本番の依存（RP 向けの `fandhe-auth-client` を含む）に 0 件 | DPOL-18・DPOL-21 |
| `openssl` | 直接の宣言 0 件。逆依存の根は `webauthn-rs` 系だけ。`vendored` 不可・配備先の OpenSSL に動的リンク | DPOL-10〜13 |
| `native-tls` | 使わない（`reqwest` の TLS は rustls） | DPOL-46 |
| `web-sys` | サーバーのビルドに 0 件。引いてよいのは `fandhe-auth-webauthn-wasm` だけ（画面の crate もサーバーのビルドに入るため不可） | DPOL-40・`D-2026-10-17-35` の (2) |

- RP 向け crate の JWT 検証部は自前の薄い検証部で、`rsa` を入れない（DPOL-21）。`fandhe-backend` の `REQ-16` は 0.5.0 で提供済みで、
  版を固定し、再試験（ACPT-16・ACPT-23 等）の後に差し替える（DPOL-21・`A-2026-10-16-5`）。

## cargo-deny と cargo-audit

- `deny.toml` はまだ無い。本番の厳格版は TASK-668 で作る。TASK-2 は `[licenses.private] ignore = false` の設定の下書きまでで、
  TASK-683（DPOL-44）で `fandhe-backend` の現行の `deny.toml` と照合する。作るまで下の基準を手元の確認に使う。
- advisories・bans・licenses・sources の 4 検査と `cargo-audit` の 5 つが全て成功しない変更はマージしない（DPOL-34）。
- 依存の変更のたびと、変更が無い期間も 1 日 1 回流す。1 日 1 回は初期値で、本番の開始から 3 か月の後に見直す。新しい advisory が
  見つかったら運用の監視の警報の経路で運用担当（人）へ知らせる（DPOL-35）。
- advisory DB は実行の前に 1 回だけ取得し、全ての実行に `--disable-fetch`（cargo deny）・`--no-fetch`（cargo audit）を付ける。
  使った DB の commit と取得日を記録し、`Cargo.lock` を書き換えない（DPOL-33）。
- licenses の許可リスト: MIT・Apache-2.0・Apache-2.0 WITH LLVM-exception・Unicode-3.0・BSD-3-Clause・ISC・BSD-2-Clause・
  MPL-2.0・BSL-1.0・CDLA-Permissive-2.0・Zlib。LGPL-2.1-or-later は `r-efi` に限った例外で、単独の LGPL は通さない（DPOL-22・DPOL-24）。
- 自身の crate は `license = "MIT OR Apache-2.0"` を宣言する（DPOL-26・`D-2026-10-23-8`）。
- sources は crates.io だけ。git 依存・他の registry 0 件（DPOL-25）。
- 例外（`ignore`・`skip`・許可の追加）には理由・期限・対象 crate の範囲を同じ場所に書く（DPOL-20）。
  例外を足すか、advisory をどう扱うかは人が決める。AI は案と根拠を示すところまで（DPOL-35）。

## feature

- 試験用の経路・ログインを省く分岐・検証を外す feature を `[features]` に作らない（DPOL-28・DPOL-30〜32）。
  DPOL-29（試験用のログインを feature で足す次点の案。Could・検討中）は採っていない。
- 例外は次の 2 つだけで、どちらも本番のビルドで有効にならない。
  - 契約テストの変異スイッチ。既定で無効（`default` に入れない）の feature か別 crate に閉じる（DPOL-31）。
  - 運用・検査用の `Ops`。運用の bin（`-migrate` 相当）と試験用のビルドだけが依存する別 crate または feature に出し、
    認証サーバーの本番の bin の `cargo tree -e features` に現れない（DPOL-38 の (イ)）。
- `reqwest`（DPOL-46）と `jsonwebtoken`（DPOL-17）は、決まった feature 以外が有効になっていないかを `cargo tree -e features` で確かめる。

## 依存を足す・更新するときの確認手順

ルートで次を実行し（`dependency-auditor` Agent も同じ手順で流す）、結果（合否と、例外を足したならその理由）を PR 本文に書く。

```bash
# advisory DB の取得（実行の前に 1 回だけ。DPOL-33）。使った DB の commit と取得日を記録する
cargo deny fetch
cargo audit --file Cargo.lock        # cargo-audit の DB の取得を兼ねる（QP-2 の手順）
git -C ~/.cargo/advisory-db log -1 --format='%H %cI'                  # cargo-audit の DB の commit と日時
git -C ~/.cargo/advisory-dbs/advisory-db-<hash> log -1 --format='%H %cI'  # cargo-deny の DB の commit と日時

# 取得なしで流す（DPOL-33・DPOL-34）
cargo deny --locked check --disable-fetch   # advisories・bans・licenses・sources
cargo audit --no-fetch --file Cargo.lock    # cargo-audit 0.22.2 には --locked が無いため、下の git diff で書き換えが無いことを確かめる
git diff --exit-code Cargo.lock      # 意図しない Cargo.lock の書き換えが無いこと（依存を足した変更では差分を目視で確かめる）

# PROD_FEATURES: 本番のビルドで有効にする feature を `<crate>/<feature>` のカンマ区切りで明示する（無ければ空）。--all-features は使わない
# SERVER_FEATURES: fandhe-auth-server の本番の feature だけ（-p で選んだ package に無い `<crate>/<feature>` を渡すと誤りになる）
cargo tree --locked --workspace -e normal,build --target all --features "$PROD_FEATURES" -i rsa         # 合格: 「did not match any packages」（一致なし）
cargo tree --locked --workspace -e normal,build --target all --features "$PROD_FEATURES" -i native-tls  # 合格: 同上
cargo tree --locked --workspace -e normal,build --target all --features "$PROD_FEATURES" -i openssl     # 合格: 一致なし、または逆依存の根が webauthn-rs 系だけ
cargo tree --locked -p fandhe-auth-server -e normal,build --features "$SERVER_FEATURES" -i web-sys      # 合格: 一致なし（サーバーのビルドだけを見る。DPOL-40）
```

- `cargo tree -i` は一致が無いとき `error: package ID specification ... did not match any packages` を出し、終了コードは
  0 にならない（cargo 1.100.0 で 101 を確認）。`--workspace` を付けないと、ルートが virtual manifest でない場合にルートの package の木しか見ない。他の誤りでも非 0 になるので、終了コードだけで合否を決めず、出力の文言を確かめる。
- `-e normal` だけでは build-dependencies が抜けるので `-e normal,build` で見る。dev-dependencies は除く。試験だけで使う依存の
  監査は `-e dev` を足して別に見る。
- `--all-features` は DPOL-31 の変異スイッチなど試験用の feature も有効にし、本番のビルドの木と変わるので使わない。
  feature を明示しないと既定の feature だけを見る。
- `fandhe-auth-server` が無い間（TASK-3 の前）は、`web-sys` の確認はサーバー側の crate ごとに `-p` で行う。
- 新しい依存の要否は、TASK-4 の選定対象（メール送信・構文木の走査・property test と fuzz・構造化ログ）なら選定の記録を待つ。

## 関連 rule

- プロファイル・target/: `.claude/rules/rust-build-profile.md`
- crate 構成・試験用の分岐: `.claude/rules/coding-rust.md`
- 秘密値の型の crate の依存制約: `.claude/rules/secret-values.md`
