# TASK-715 暗号の基本処理の検査の記録

`TASK-715`（親 Issue #5166）の記録の置き場。根拠は `LIB-21`（`DPOL-4`・`DPOL-47` を緩めない）。
1/4（Issue #5167）の節だけを埋め、2/4〜4/4 は見出しだけ置く。

この文書は実行したコマンドと出力の事実だけを書く。判定・「確定」への変更・例外の承認は人が行う（`ACPT-7`）。
「検証済み」とは書かない（確認は 2/4 の範囲）。判定の欄は空けてある。spec の本文は転記せず ID で引く。

## 1/4 本体の直接の依存から暗号に関わる部品を洗い出す（#5167）

### 1. 目的と範囲

- 署名・検証・乱数・WebAuthn・TOTP の 5 区分について、本体の直接の依存と用途の一覧を作る（根拠 `LIB-21`）。
- 「本体」の範囲は書き手の読みで、人が確かめる。
  - 対象: `core`・`secret`・`db-tests`・`client`（`LIB-1`・`LIB-14`。段階は〔提案〕）。
  - 参考として別に数える: `plugin-oidc`（同梱の Plugin。`PLG-26`）、`backend`（暫定の置き場。`D-2026-11-20-1`）、
    試験専用の `hub-stub`・`console-stub`・`it-harness`・`e2e-harness`・`load-harness`・`trial-deps`。
- 出力中の絶対パスは `<repo>` に置き換えた。

### 2. 基準

- 実行時の base: `origin/main` の `c040dbe`。
- spec の pin: `9055ba79d71afdcaefc8c4ee0ae98324a3f4ba3b`（`docs/spec`）。
- ツールの版: cargo 1.99.1 (5f94df478 2026-08-27)、rustc 1.99.1 (58cf8f9c3 2026-10-13)。

### 3. 測り方

1. `cargo metadata --format-version 1 --locked` の `packages[]` から、workspace メンバーごとに `dependencies[]` の `name`・`kind`・`source`（path か registry か）を集めた。
2. `cargo tree --locked -p fandhe-auth-<名前> -e normal,build --depth 1 --target all`（本体の 4 crate）。dev は `-e dev` で別に見た。
3. `cargo tree --locked --workspace -e normal,build,dev --target all -i <名前>`（lock にある暗号に関わる名前の逆依存。`getrandom` は版が 2 つあり `@0.2.17`・`@0.4.3` で指定した）。

実行した各コマンドの終了コードと出力の抜粋（空の結果と実行失敗を区別するため、終了コードを併記する。パスは `<repo>` に置換）:

| コマンド | 終了コード | 出力の抜粋 |
|---|---|---|
| `cargo metadata --format-version 1 --locked` | 0 | `core`・`secret`・`db-tests`・`client` の `dependencies[]` の件数はいずれも 0 |
| `cargo tree --locked -p fandhe-auth-<名前> -e normal,build --depth 1 --target all`（4 crate それぞれ） | 0 | `fandhe-auth-<名前> v0.0.0 (<repo>/crates/<名前>)` の 1 行のみ |
| `cargo tree --locked -p fandhe-auth-<名前> -e dev --depth 1 --target all`（4 crate それぞれ） | 0 | 同上の 1 行のみ |
| `cargo tree --locked --workspace -e normal,build,dev --target all -i ring` | 0 | `ring v0.17.14` から `fandhe-auth-hub-stub` を経て `it-harness`・`load-harness` へ至る木 |
| `cargo tree --locked --workspace -e normal,build,dev --target all -i openssl`（lock に無い名前） | 101 | `error: package ID specification `openssl` did not match any packages`（実行失敗ではなく「lock に無い」の意味） |
| `grep -c '^name = "<名前>"' Cargo.lock`（`openssl`・`rsa`・`webauthn-rs`・`aws-lc-rs`・`jsonwebtoken` を 1 名ずつ） | 1（5 名とも） | 標準出力は `0`（一致 0 件。grep は一致 0 件で終了コード 1 を返す） |
| `grep -c '^name = ' Cargo.lock` | 0 | `69` |

`cargo tree -i` は一致が無いと非 0 で終わるため、合否は終了コードだけで決めず出力で確かめる（`.claude/rules/dependency-policy.md`）。

### 4. 結果 1: 本体の直接の依存

| crate | normal | build | dev | 暗号に関わる直接の依存 |
|---|---|---|---|---|
| `fandhe-auth-core` | 0 | 0 | 0 | 0 件 |
| `fandhe-auth-secret` | 0 | 0 | 0 | 0 件 |
| `fandhe-auth-db-tests` | 0 | 0 | 0 | 0 件 |
| `fandhe-auth-client` | 0 | 0 | 0 | 0 件 |

`cargo metadata` の `dependencies[]` は 4 crate とも空。`cargo tree` も 4 crate とも本体の 1 行のみ（normal・build・dev）。

参考（本体に含めない crate の直接の依存。`cargo metadata` の出力）:

| crate | 外部（registry）の直接の依存 |
|---|---|
| `fandhe-auth-plugin-oidc`・`fandhe-auth-backend` | なし |
| `fandhe-auth-console-stub`・`fandhe-auth-e2e-harness` | `serde_json` |
| `fandhe-auth-hub-stub` | `base64`・`ring`・`serde_json` |
| `fandhe-auth-it-harness`・`fandhe-auth-load-harness` | なし（path と dev の path のみ） |
| `fandhe-auth-trial-deps` | `metrics`・`tracing`（dev: `proptest`・`syn`・`tracing-subscriber`） |

### 5. 結果 2: 5 区分の一覧

「実測」は結果 1 の値（全区分 0 件）。「決まっている・候補」は公開の規約に書かれた範囲で、導入の決定ではない。
検証済みかの確認の欄は 2/4（#5168）で記入する。

| 区分 | 本体での用途 | 実測 | 決まっている・候補の実装 | 状態 | 根拠 ID | 検証済みかの確認 |
|---|---|---|---|---|---|---|
| 署名 | 自社発行の ID トークン・アクセストークン・Logout Token の署名 | 0 件 | `aws-lc-rs` または `ring`。`jsonwebtoken` を使うなら `aws_lc_rs` の feature だけ。どれにするかは未定 | 未導入 | `SIGN-1`・`SIGN-2`・`SIGN-8`・`DPOL-10`・`DPOL-11`・`DPOL-17`・`D-2026-10-16-4` | （空） |
| 検証 | JWT の検証（`client` の自前の薄い検証部。`rsa` を入れない） | 0 件 | 署名の検証そのものは署名と同じ既存の crate に任せる | 未導入 | `DPOL-18`・`DPOL-21`・`SIGN-6`・`SIGN-9`・`SIGN-10`・`SIGN-17` | （空） |
| 乱数 | state・nonce・PKCE・認可コード・TOTP の秘密などの生成 | 0 件 | 本体は乱数の crate を直接持たず、組込み先の口（仮称 `Rng`）で受ける | 直接の依存 0 が設計どおり | `HOST-2`・`HOST-22`・`AMTH-2` | （空） |
| WebAuthn | passkey の登録・認証の検証 | 0 件 | `webauthn-rs`（0.5.5 は PoC で既定 options を実測した版で、本番の導入候補として選定した版ではない。版は `dependency-policy.md`・`PKEY-4` に従い別に決める）。`openssl` を引く根は `webauthn-rs` 系に限る。置く crate は未定 | 未導入 | `PKEY-4`・`DPOL-10`・`DPOL-11` | （空） |
| TOTP | RFC 6238 の検証（定数時間の比較） | 0 件 | 検証済みの既存実装を使い自作しない。本番の crate は未定（`TASK-762`。評価は `PoC-18`） | 未導入 | `AMTH-2`・`AMTH-3`・`DPOL-47` | （空） |

補足（`LIB-21` が挙げる他の基本処理）:

- ハッシュ・HMAC の保存の形の根拠は `SECR-4`・`SECR-8`・`SECR-34`。直接の依存は 0 件。
- XML 署名は SAML の Plugin（段階 2。`Q-2026-10-30-3`）側で、本体の外。

### 6. 結果 3: lock にあり本体の外にある暗号に関わる crate

`Cargo.lock`（69 crate）に `ring`・`untrusted`・`rand`・`rand_core`・`rand_xorshift`・`chacha20`・`getrandom`（0.2.17・0.4.3）がある。
`openssl`・`rsa`・`webauthn-rs`・`aws-lc-rs`・`jsonwebtoken` は lock に無い（上の `grep -c '^name = "<名前>"' Cargo.lock` が 5 名とも `0` で終了コード 1）。
`cargo tree -i`（normal,build,dev）の逆依存の根は次のとおりで、段階 1 の本体の 4 crate は根にも経路にも現れない。

| 名前 | 根 |
|---|---|
| `ring` → `untrusted`・`getrandom 0.2.17` | `hub-stub`（normal）。`it-harness`・`load-harness` を経由 |
| `rand`・`rand_core`・`chacha20`・`rand_xorshift`・`getrandom 0.4.3` | `trial-deps` の dev 依存 `proptest` |

試験専用の crate の暗号の扱いを検査の対象にするかは本タスクの範囲外で、人が決める。

### 7. 未決・人が決めること

- 「本体」の範囲の読み（結果 1・2 の対象）。
- 署名の実装（`aws-lc-rs` か `ring` か、`jsonwebtoken` の採否）。
- WebAuthn を置く crate。
- TOTP の crate（`TASK-762`）。
- 試験専用の crate を検査の対象に含めるか。

### 8. 判定

（人が記入する。空欄）

## 2/4 各部品が検証済みの既存実装かの確認（#5168）

未記入。

## 3/4 自作の暗号処理の走査の仕組み（#5169）

未記入。

## 4/4 検査に載せて記録する（#5170）

未記入。
