# TASK-715 暗号の基本処理の検査の記録

`TASK-715`（親 Issue #5166）の記録の置き場。根拠は `LIB-21`（`DPOL-4`・`DPOL-47` を緩めない）。
1/4（Issue #5167）・2/4（Issue #5168）の節を埋め、3/4〜4/4 は見出しだけ置く。

この文書は実行したコマンドと出力の事実だけを書く。判定・「確定」への変更・例外の承認は人が行う（`ACPT-7`）。
「検証済み」とは書かない（確認の材料集めは 2/4 の範囲で、判定は人が行う）。判定の欄は空けてある。spec の本文は転記せず ID で引く。

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
| `cargo tree --locked --workspace -e normal,build,dev --target all -i ring` | 0 | `ring v0.17.14` ← `fandhe-auth-hub-stub` ← `fandhe-auth-it-harness` ← `fandhe-auth-load-harness`（`load-harness` が `it-harness` に dev 依存する）。`load-harness` は `hub-stub` にも dev で直接依存（`←` は依存先から依存元へ向かう逆引きの経路。根は 1 行目から最後の行） |
| `cargo tree --locked --workspace -e normal,build,dev --target all -i untrusted` | 0 | `untrusted v0.9.0` ← `ring v0.17.14` ← `fandhe-auth-hub-stub` ← `it-harness`・`load-harness`（`←` は依存先から依存元へ向かう逆引きの経路。根は 1 行目から最後の行） |
| `cargo tree --locked --workspace -e normal,build,dev --target all -i rand` | 0 | `rand v0.10.4` ← `proptest v1.12.0` ← `fandhe-auth-trial-deps`（dev）（`←` は依存先から依存元へ向かう逆引きの経路。根は 1 行目から最後の行） |
| `cargo tree --locked --workspace -e normal,build,dev --target all -i rand_core` | 0 | `rand_core v0.10.1` ← `chacha20`・`getrandom v0.4.3`・`rand`・`rand_xorshift`（`getrandom v0.4.3` の依存元は `rand` で、`rand` ← `proptest`。`chacha20`・`rand_xorshift` は `proptest` が直接引く）← `proptest v1.12.0` ← `trial-deps`（dev）（`←` は依存先から依存元へ向かう逆引きの経路。根は 1 行目から最後の行） |
| `cargo tree --locked --workspace -e normal,build,dev --target all -i rand_xorshift` | 0 | `rand_xorshift v0.5.0` ← `proptest v1.12.0` ← `trial-deps`（dev）（`←` は依存先から依存元へ向かう逆引きの経路。根は 1 行目から最後の行） |
| `cargo tree --locked --workspace -e normal,build,dev --target all -i chacha20` | 0 | `chacha20 v0.10.2` ← `proptest v1.12.0` ← `trial-deps`（dev）（`←` は依存先から依存元へ向かう逆引きの経路。根は 1 行目から最後の行） |
| `cargo tree --locked --workspace -e normal,build,dev --target all -i getrandom@0.2.17` | 0 | `getrandom v0.2.17` ← `ring v0.17.14` ← `hub-stub` ← `it-harness`・`load-harness`（`←` は依存先から依存元へ向かう逆引きの経路。根は 1 行目から最後の行） |
| `cargo tree --locked --workspace -e normal,build,dev --target all -i getrandom@0.4.3` | 0 | `getrandom v0.4.3` ← `rand v0.10.4` ← `proptest v1.12.0` ← `trial-deps`（dev）（`←` は依存先から依存元へ向かう逆引きの経路。根は 1 行目から最後の行） |
| `cargo tree --locked --workspace -e normal,build,dev --target all -i openssl`（lock に無い名前） | 101 | ``error: package ID specification `openssl` did not match any packages``（実行失敗ではなく「lock に無い」の意味） |
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
| `ring`（依存先に `untrusted`・`getrandom 0.2.17` を持つ） | `ring` の直接の依存元は `hub-stub`（normal）で、そこから各ハーネスへ経路が続く（`hub-stub` ← `it-harness` ← `load-harness`、`hub-stub` ← `load-harness`）。`it-harness`・`load-harness` はどちらも `hub-stub` に直接依存する（`load-harness` は `it-harness` 経由の dev 依存もある）。`untrusted`・`getrandom 0.2.17` は `ring` の依存先で、`ring` → `hub-stub` → 各 harness の順に依存元へ至る |
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

### 1. 目的と範囲

- 受入基準は 3 点: (a) 一覧の各部品に出所の記録がある、(b) 出所や検証の記録を確かめられない部品が一覧になっている、(c) 「検証済みか」の判定は人が行う段として分けてある（`ACPT-7`）。根拠は `LIB-21`・`DPOL-4`・`DPOL-47`。
- 本節は 1/4 の結果 2・3 の「検証済みかの確認」欄に対応する節。1/4 の節は記録した時点の事実として書き換えず、その欄も空のまま残す。
- 「検証済み」の基準は spec に数で測れる形では定まっていない（`DPOL-47` は「保守されている既存 crate」と書く）。新しい crate を足すときの観点は K11（`D-2026-11-20-14` の (4)(d)）、版の選び方は `D-2026-11-21-2`。本節は基準を決めず、観点に沿った事実を集めるだけにとどめる。基準そのものは未定（人が決める）。
- 本体の 4 crate の直接の依存は 0 件（1/4）。確かめる対象は「lock にあり本体の外から引かれる暗号の crate（参考）」と「5 区分の候補（未導入）」の 2 種類になる。
- 出力中の絶対パスは `<repo>` に置き換え、registry cache のパスは書かない。

### 2. 基準

- 実行時の base: `origin/main` の `2d38a7e`。
- spec の pin: `2f25a1f54ceefe8c2a10b2b9834364b3828bfa0d`（`docs/spec`。この時点の `origin/main` が指す値）。
- ツールの版: cargo 1.100.0 (3acc79c0c 2026-11-05)、rustc 1.100.0 (0ef1bc386 2026-11-09)、cargo-audit 0.22.2、cargo-deny 0.19.8（1/4 の記録時は cargo・rustc 1.99.1）。
- advisory DB: 取得後の commit `f291af9b1cc0d21dfcea1709d6b037963cef262b`（commit 日時 2026-11-20T13:47:55+01:00）。取得は 2026-11-21、1555 件を読み込んだ。

### 3. 測り方

1. 1/4 の測り方を再実行し、lock が動いていないことを確かめた（`cargo metadata --format-version 1 --locked`、`cargo tree --locked --workspace -e normal,build,dev --target all -i <名前>`、`grep -c '^name = "<名前>"' Cargo.lock`）。結果は 1/4 と同じ: 4 crate の `dependencies[]` は 0 件、lock は 69 crate、`openssl`・`rsa`・`webauthn-rs`・`aws-lc-rs`・`jsonwebtoken` は 0 件、逆依存の根も 1/4 の結果 3 と同じ。
2. `cargo metadata --format-version 1 --locked --offline` の `packages[]` から `source`・`license`・`repository`・`rust_version` を取った。
3. `Cargo.lock` の `checksum` と、cargo の registry cache にある `<名前>-<版>.crate` の `sha256sum` を比べた。
4. `cargo search <名前> --limit 1` で crates.io の最新の版を得た（読み取りのみ。安定版かの区別は検索結果からは付けていない）。
5. `cargo audit --file Cargo.lock`（advisory DB を取得して実行。`DPOL-33`・`DPOL-34`）。取得後の DB の commit（基準節の `f291af9b…`）を固定したまま、`cargo audit --no-fetch --file Cargo.lock` を再実行して同じ DB に対する結果を記録した（`DPOL-33`）。
6. `cargo deny` は流していない（`deny.toml` が未作成のため。`TASK-668`）。

| コマンド | 終了コード | 出力の抜粋 |
|---|---|---|
| `cargo metadata --format-version 1 --locked --offline` | 0 | 対象 8 件の `source` はすべて crates.io の registry |
| `sha256sum <名前>-<版>.crate`（8 件） | 0 | 8 件とも lock の `checksum` と一致 |
| `cargo search <名前> --limit 1`（重複を除いた 10 名） | 0 | 結果 1・2 の「最新」欄のとおり。10 名は `ring`・`untrusted`・`getrandom`・`rand`・`rand_core`・`rand_xorshift`・`chacha20`（結果 1 の 7 名）と `aws-lc-rs`・`jsonwebtoken`・`webauthn-rs`（結果 2 のみの 3 名）。結果 1 は `getrandom` が 0.2.17・0.4.3 の 2 行になるため 8 行で、検索は 1 名 1 回（`getrandom` は 1 回）。`ring` は結果 2 にも載るが同じ検索結果を使う |
| `cargo audit --no-fetch --file Cargo.lock`（DB は commit `f291af9b1cc0d21dfcea1709d6b037963cef262b` に固定） | 0 | `Loaded 1555 security advisories`、`Scanning Cargo.lock for vulnerabilities (69 crate dependencies)`。警告・脆弱性の出力なし（該当 0 件） |
| 実行後の作業ツリーの状態確認（`status --porcelain`） | 0 | 出力なし（`Cargo.lock` に変更なし） |

### 4. 結果 1: lock にある暗号の crate の出所（事実のみ）

すべて `source` は crates.io の registry（`DPOL-25`）。`checksum` は lock と cache の `.crate` の sha256 が一致した。`cargo audit` の該当は 8 件とも 0 件。
逆依存の根は 1/4 の結果 3 と同じ（`ring`・`untrusted`・`getrandom 0.2.17` は `hub-stub` 経由の各 harness、他は `trial-deps` の dev 依存 `proptest`）。

| crate・版 | license | repository | `rust-version` | 最新（`cargo search`） | spec にある記録の ID | 検証済みかの判定 |
|---|---|---|---|---|---|---|
| `ring` 0.17.14 | Apache-2.0 AND ISC | `briansmith/ring` | 1.66.0 | 0.17.14（同版） | `DPOL-4`・`DPOL-46`・`PoC-10` | （空） |
| `untrusted` 0.9.0 | ISC | `briansmith/untrusted` | 宣言なし | 0.9.0（同版） | `ring` の依存先。個別の記録は確かめていない | （空） |
| `getrandom` 0.2.17 | MIT OR Apache-2.0 | `rust-random/getrandom` | 宣言なし | 0.4.3（0.2 系の最新かは未確認） | `ring` の依存先。個別の記録は確かめていない | （空） |
| `getrandom` 0.4.3 | MIT OR Apache-2.0 | `rust-random/getrandom` | 1.85 | 0.4.3（同版） | 個別の記録は確かめていない | （空） |
| `rand` 0.10.4 | MIT OR Apache-2.0 | `rust-random/rand` | 1.85 | 0.10.4（同版） | `DPOL-4` | （空） |
| `rand_core` 0.10.1 | MIT OR Apache-2.0 | `rust-random/rand_core` | 1.85 | 0.10.1（同版） | 個別の記録は確かめていない | （空） |
| `rand_xorshift` 0.5.0 | MIT OR Apache-2.0 | `rust-random/rngs` | 1.85 | 0.5.0（同版） | 個別の記録は確かめていない | （空） |
| `chacha20` 0.10.2 | MIT OR Apache-2.0 | `RustCrypto/stream-ciphers` | 1.85 | 0.10.2（同版） | 個別の記録は確かめていない | （空） |

- 「`rust-version` が MSRV 以下か」（K11 の観点）は、MSRV の値を本節では再掲しない（`Cargo.toml` の値と `D-2026-11-20-14` を見て人が比べる）。
- `ring` を `hub-stub`（試験専用の stub）が引くことと `DPOL-4` の PoC の一覧との関係は、人が確かめる。

### 5. 結果 2: 5 区分の候補（未導入）の出所

「本体への導入」と「workspace の `Cargo.lock` への収録」は別である。候補はどれも本体の 4 crate には未導入で、`ring` だけが試験専用の `hub-stub` 経由で lock に収録されている。他の候補は lock に無く、lock での出所・checksum・advisory は測れない。

| 区分 | 候補 | spec にある記録の ID | lock での出所 | 最新（`cargo search`） | 検証済みかの判定 |
|---|---|---|---|---|---|
| 署名 | `aws-lc-rs`・`ring`・`jsonwebtoken`（`aws_lc_rs` の feature のみ） | `DPOL-4`・`DPOL-10`・`DPOL-11`・`DPOL-17`・`PoC-5` | `aws-lc-rs`・`jsonwebtoken` は lock に無く測れない。`ring` 0.17.14 は本体には未導入だが workspace の `Cargo.lock` に `hub-stub` 経由で収録済みで、出所・checksum・advisory は結果 1 を参照する | `aws-lc-rs` 1.19.0、`jsonwebtoken` 12.0.0 | （空） |
| 検証 | 署名と同じ既存の crate | `DPOL-18`・`DPOL-21` | 署名と同じ。`ring` は結果 1 を参照し、他は lock に無く測れない | 同上 | （空） |
| 乱数 | 本体は持たず組込み先が実装（`Rng`） | `HOST-2`・`HOST-22` | 本体の依存は 0 件 | 該当なし | （空） |
| WebAuthn | `webauthn-rs` | `PKEY-4`・`DPOL-10` | 測れない | 0.6.2（PoC で実測した 0.5.5 とは別の版。選定は別に行う） | （空） |
| TOTP | 本番の crate は未定 | `AMTH-3`・`PoC-18`・`TASK-762` | 測れない | 該当なし | （空） |

### 6. 結果 3: 出所や検証の記録を確かめられない部品（受入基準 (b)）

- `aws-lc-rs`・`jsonwebtoken`・`webauthn-rs`: lock に無く（`ring` は本体に未導入だが lock に収録済みで結果 1 に記録がある）、出所・checksum・advisory を測れない。
- 署名の実装の選び（`aws-lc-rs` か `ring` か）: 未定。
- TOTP の本番の crate: 未定（`TASK-762`）。
- 乱数の実装（`Rng`）: 組込み先が実装し、本体からは見えない（`HOST-2`）。
- 結果 1 の 8 crate それぞれの、外部の監査・形式検証・認証・保守状況の記録: 本節では調べていない。確認するなら〔資料〕として URL と確認日を添えて別に記録する。
- `untrusted`・`getrandom`・`rand_core`・`rand_xorshift`・`chacha20` の spec 上の個別の記録: 確かめた範囲では見つからない（他の crate の依存先として現れるだけ）。
- `getrandom` 0.2 系の最新の版: `cargo search` は最新の 1 件（0.4.3）しか返さず確かめていない。
- `cargo deny` の結果（ライセンス・bans・sources）: 未実行（`deny.toml` 未作成。`TASK-668`）。
- 「検証済み」の基準そのもの: 未定。

### 7. 人が決めること

- 「検証済み」の基準（K11 の観点で足りるか、監査の記録を要るとするか）と、各部品の判定。
- 試験専用の crate の依存（`ring`・`rand` 系など）を検査の対象に含めるか。
- advisory に該当が出た場合の扱い（`DPOL-35`。今回は該当 0 件）。
- 署名・WebAuthn・TOTP の crate の選定（`TASK-762` 等）。

### 8. 判定

（人が記入する。空欄）

## 3/4 自作の暗号処理の走査の仕組み（#5169）

未記入。

## 4/4 検査に載せて記録する（#5170）

未記入。
