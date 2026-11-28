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
| `cargo tree --locked --workspace -e normal,build,dev --target all -i openssl`（lock に無い名前） | 101 | ``error: package ID specification `openssl` did not match any packages``（期待した未解決エラー。lock に一致が無いことの根拠は次行の grep の標準出力と終了コードに置く） |
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
| `ring`（依存先に `untrusted`・`getrandom 0.2.17` を持つ） | 1/4 の記録時点（基準 `c040dbe`）: `ring` の直接の依存元は `hub-stub`（normal）で、そこから各ハーネスへ経路が続く（`hub-stub` ← `it-harness` ← `load-harness`、`hub-stub` ← `load-harness`）。`untrusted`・`getrandom 0.2.17` は `ring` の依存先で、同じ経路で依存元へ至る。2/4 の基準（`2d38a7e`）以降は `it-harness` が `ring` に dev で直接依存する辺も加わった（2/4 の結果 1 を参照） |
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

1. 1/4 の測り方を再実行し、lock が動いていないことを確かめた（`cargo metadata --format-version 1 --locked`、`cargo tree --locked --workspace -e normal,build,dev --target all -i <名前>`、`grep -c '^name = "<名前>"' Cargo.lock`）。1/4 と同じだった項目: 4 crate の `dependencies[]` は 0 件、lock は 69 crate、`openssl`・`rsa`・`webauthn-rs`・`aws-lc-rs`・`jsonwebtoken` は 0 件。1/4 と異なる項目: `ring`・`untrusted`・`getrandom 0.2.17` の逆依存は、`it-harness` から `ring` への直接の dev 依存が加わった分だけ経路が増えた（結果 1 と「基準間の `Cargo.lock` の差分」を参照）。それ以外の根は 1/4 の結果 3 と同じ。
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
| 実行後の作業ツリーの状態確認（`status --porcelain`） | 0 | 出力なし（実行後に作業ツリーがクリーンだった。実行前の状態は記録しておらず、調査の前後で `Cargo.lock` が同じだったとは主張しない） |

### 4. 結果 1: lock にある暗号の crate の出所（事実のみ）

すべて `source` は crates.io の registry（`DPOL-25`）。`checksum` は lock と cache の `.crate` の sha256 が一致した。`cargo audit` の該当は 8 件とも 0 件。
逆依存の根は、`ring`・`untrusted`・`getrandom 0.2.17` が 2 経路、他は 1/4 の結果 3 と同じ（`trial-deps` の dev 依存 `proptest`）。
`ring` の 2 経路は、(i) `ring` ← `hub-stub` ← `it-harness`・`load-harness`（`hub-stub` は `ring` に normal で依存）と、(ii) `ring` ← `it-harness`（dev。`crates/it-harness/Cargo.toml` の `[dev-dependencies]` に `ring = "0.17.14"`）← `load-harness`（dev）。
`untrusted`・`getrandom 0.2.17` は `ring` の依存先のため、同じ 2 経路で及ぶ。1/4 の記録（基準 `c040dbe`）の `it-harness` の外部の直接の依存「なし」は、その時点の事実として残す。(ii) は基準 `2d38a7e` 以降の変更で、下の「基準間の `Cargo.lock` の差分」に記録した。

基準間の `Cargo.lock` の差分（実行: `git diff c040dbe 2d38a7e -- Cargo.lock`、終了コード 0。変更は `Cargo.lock` の 3 行の追加だけ）: `fandhe-auth-it-harness` の `dependencies` に `base64`・`ring`・`serde_json` が加わった。`[[package]]` の追加・削除はなく、`name = ` 行は 69 のまま（`base64`・`ring`・`serde_json` は `hub-stub` 経由で既に lock にある）。
これは基準コミット間の差分であり、2/4 の調査コマンドの実行前後に確認した範囲（作業ツリーの状態確認、測り方 5 の表の最終行）とは別である。後者は実行後に作業ツリーがクリーンだったことだけを示し、実行前後の比較ではない（調査コマンドが `Cargo.lock` を書き換えなかったことの証拠にはならない。未検証）。

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

「本体への導入」と「workspace の `Cargo.lock` への収録」は別である。候補はどれも本体の 4 crate には未導入で、`ring` だけが試験専用の `hub-stub` 経由と `it-harness` の dev 依存（直接）で lock に収録されている。他の候補は lock に無く、lock での出所・checksum・advisory は測れない。

| 区分 | 候補 | spec にある記録の ID | lock での出所 | 最新（`cargo search`） | 検証済みかの判定 |
|---|---|---|---|---|---|
| 署名 | `aws-lc-rs`・`ring`・`jsonwebtoken`（`aws_lc_rs` の feature のみ） | `DPOL-4`・`DPOL-10`・`DPOL-11`・`DPOL-17`・`PoC-5` | `aws-lc-rs`・`jsonwebtoken` は lock に無く測れない。`ring` 0.17.14 は本体には未導入だが workspace の `Cargo.lock` に `hub-stub` 経由と `it-harness` の dev 依存（直接）で収録済みで、出所・checksum・advisory は結果 1 を参照する | `aws-lc-rs` 1.19.0、`jsonwebtoken` 12.0.0 | （空） |
| 検証 | 署名と同じ既存の crate | `DPOL-18`・`DPOL-21` | 署名と同じ。`ring` は結果 1 を参照し、他は lock に無く測れない | 同上 | （空） |
| 乱数 | 本体は持たず組込み先が実装（`Rng`） | `HOST-2`・`HOST-22` | 本体の依存は 0 件 | 該当なし | （空） |
| WebAuthn | `webauthn-rs` | `PKEY-4`・`DPOL-10` | 測れない | 0.6.2（PoC で実測した 0.5.5 とは別の版。選定は別に行う） | （空） |
| TOTP | 本番の crate は未定 | `AMTH-3`・`PoC-18`・`TASK-762` | 測れない | 該当なし | （空） |

### 6. 結果 3: 出所や検証の記録を確かめられない部品（受入基準 (b)）

- `aws-lc-rs`・`jsonwebtoken`・`webauthn-rs`: 版ごとの配布元と上流リポジトリは調査済みで 6a に記録した。未確認なのは、採用版とその版を含む `Cargo.lock` に対する source・checksum・advisory の照合である（lock に無く測れない。`ring` は本体に未導入だが lock に収録済みで結果 1 に記録がある）。
  あわせて、3 候補それぞれの外部の監査・形式検証・認証・保守状況（メンテナの活動、既知の advisory の有無、yanked の有無）は未調査である。確認した資料は 6a の crates.io の配布ページ・上流リポジトリの URL の記録だけで、監査報告・RustSec の記録・リリース履歴は読んでいない（`DPOL-47` の判定材料としては不足。人が判定する前に〔資料〕として URL と確認日を添えて別に記録する）。
- 署名の実装の選び（`aws-lc-rs` か `ring` か）: 未定。
- TOTP の本番の crate: 未定（`TASK-762`）。
- 乱数の実装（`Rng`）: 組込み先が実装し、本体からは見えない（`HOST-2`）。
- 結果 1 の 8 crate と、未導入の 3 候補（`aws-lc-rs`・`jsonwebtoken`・`webauthn-rs`）それぞれの、外部の監査・形式検証・認証・保守状況の記録: 本節では調べていない（11 crate すべて未調査）。確認するなら〔資料〕として URL と確認日を添えて別に記録する。
- `untrusted`・`getrandom`・`rand_core`・`rand_xorshift`・`chacha20` の spec 上の個別の記録: 確かめた範囲では見つからない（他の crate の依存先として現れるだけ）。
- `getrandom` 0.2 系の最新の版: `cargo search` は最新の 1 件（0.4.3）しか返さず確かめていない。
- `cargo deny` の結果（ライセンス・bans・sources）: 未実行（`deny.toml` 未作成。`TASK-668`）。
- 最新安定版の照合（K11）と、3 候補の採用版・lock の checksum: 未確認（6a・6c）。
- 「検証済み」の基準そのもの: 未定。

### 6a. 未導入の 3 候補の公開元（版ごと。事実のみ）

`cargo info <名前>`（終了コード 0。crates.io の index を読む）が返した版と公開元。これは検索で返った版の記録で、採用版ではない。
採用版と `Cargo.lock` の `checksum` は未確定（lock に無いため測れない。導入時に別に決める。`D-2026-11-21-2`）。
`.crate` の取得物の sha256 と lock の突き合わせも、導入前のため未実施。

| crate | 検索で返った版 | crates.io の配布ページ | 上流リポジトリ | license | `rust-version` |
|---|---|---|---|---|---|
| `aws-lc-rs` | 1.19.0 | `https://crates.io/crates/aws-lc-rs/1.19.0` | `https://github.com/aws/aws-lc-rs` | ISC AND (Apache-2.0 OR ISC) | 1.71.0 |
| `jsonwebtoken` | 12.0.0 | `https://crates.io/crates/jsonwebtoken/12.0.0` | `https://github.com/Keats/jsonwebtoken` | MIT | 1.88.0 |
| `webauthn-rs` | 0.6.2 | `https://crates.io/crates/webauthn-rs/0.6.2` | `https://github.com/kanidm/webauthn-rs` | MPL-2.0 | 1.89 |

### 6b. 2/4 の記録時点の再計測（HEAD `98ab1c3`、`origin/main` `ce28b5a`）

レビュー指摘への対応として、本節の記録後に同じコマンドを再実行した（cargo 1.100.0）。終了コードを併記し、一致 0 件とコマンド失敗を区別する。

| コマンド | 終了コード | 出力の抜粋 |
|---|---|---|
| `cargo metadata --format-version 1 --locked` | 0 | `core`・`secret`・`db-tests`・`client` の `dependencies[]` はいずれも 0 件 |
| `cargo tree --locked -p fandhe-auth-<名前> -e normal,build --depth 1 --target all`（4 crate） | 0（4 件とも） | 4 件とも出力 1 行 |
| `cargo tree --locked --workspace -e normal,build,dev --target all -i <名前>`（`ring`・`untrusted`・`rand`・`getrandom@0.2.17`・`getrandom@0.4.3`） | 0（5 件とも） | 1 行目はそれぞれ `ring v0.17.14`・`untrusted v0.9.0`・`rand v0.10.4`・`getrandom v0.2.17`・`getrandom v0.4.3` |
| 同上 `-i openssl` | 101 | ``error: package ID specification `openssl` did not match any packages``（期待した未解決エラー。`openssl` は package ID として解決されない。lock に一致が無いことの根拠は次行の grep の標準出力と終了コードに置く） |
| `grep -c '^name = "<名前>"' Cargo.lock`（`openssl`・`rsa`・`webauthn-rs`・`aws-lc-rs`・`jsonwebtoken`） | 1（5 名とも） | 標準出力 `0`（一致 0 件。grep は一致 0 件で 1 を返す。2 以上のコマンド失敗ではない） |
| `grep -c '^name = ' Cargo.lock` | 0 | `69` |
| `cargo info aws-lc-rs`・`cargo info jsonwebtoken`・`cargo info webauthn-rs` | 0 | 6a の表のとおり |

この表で確認できたのは、4 crate の `dependencies[]` が 0 件であること、表に載せた 5 件（`ring`・`untrusted`・`rand`・`getrandom@0.2.17`・`getrandom@0.4.3`）の逆依存の 1 行目、5 名の `name` 一致 0 件、`name` 行数 69 の範囲である。`rand_core`・`rand_xorshift`・`chacha20` はこの再計測の表に無く、ここでは確認していない（測り方 1 の再実行の範囲に限る）。
`Cargo.lock` 全体の同一性、および結果 1 の source・checksum・advisory の再確認は、本表では示していない（比較したコミットと比較コマンドの出力を記録していない。未検証）。

### 6c. 最新安定版の確認状況（K11）

- 結果 1・2 の「最新」「同版」は、`cargo search <名前> --limit 1` が返した版との一致を示す記録である。安定版（プレリリースでない版）かの区別は付けておらず、K11 が求める最新安定版の照合は未実施。
- 未確認事項: 各 crate の最新安定版、yanked の有無、`getrandom` 0.2 系の最新の版。導入時に crates.io の公式の一覧で確かめる（人が行う）。

### 7. 人が決めること

- 「検証済み」の基準（K11 の観点で足りるか、監査の記録を要るとするか）と、各部品の判定。
- 試験専用の crate の依存（`ring`・`rand` 系など）を検査の対象に含めるか。
- advisory に該当が出た場合の扱い（`DPOL-35`。今回は該当 0 件）。
- 署名・WebAuthn・TOTP の crate の選定（`TASK-762` 等）。

### 8. 判定

（人が記入する。空欄）

## 3/4 自作の暗号処理の走査の仕組み（#5169）

### 1. 目的と範囲

- 受入基準は 3 点: (a) 自作の暗号処理を探す走査の仕組みがある、(b) 範囲（対象・規則・限界）が文書にある、(c) 今の main で検出 0 件で通る。根拠は `LIB-21`・`DPOL-4`・`DPOL-47`。
- 仕組み: `scripts/check-crypto-primitives.sh`（`make crypto-scan`。名前は暫定）。make check 相当への組み込みと CI への搭載は 4/4（#5170）の範囲で、本節では行わない。
- 対象: `git ls-files --cached --others --exclude-standard -- crates/` のファイル。`crates/` の外（外部取得物・`scripts/`）は対象外。
- 合否の範囲: 本番メンバー（core・secret・db-tests・client・plugin-oidc・backend）。参考の範囲: 試験専用メンバー（hub-stub・console-stub・it-harness・e2e-harness・load-harness・trial-deps と配下の fuzz）。
- 出力中の絶対パスは出さない設計で、記録にも書かない。

### 2. 基準

- base: `origin/main` の `1181a68`。
- 実行環境: GNU bash 5.3.9、ugrep 7.8.4（`grep`）。

### 3. 規則と限界

| 規則 | 扱い | 内容 |
|---|---|---|
| C1 | 合否 | 既知の暗号定数（SHA-1/2・MD5 の IV と K[0]、ChaCha/Salsa の sigma、黄金比、Poly1305 の clamp、Keccak の RC、P-256 の素数の断片、AES の S-box の先頭）。`_` を除き大小文字を区別しない |
| C2 | 合否 | インラインアセンブリ・アーキテクチャ固有の命令 |
| C3 | 合否 | Rust 以外のネイティブのソース（拡張子 `.c .h .cc .cpp .cxx .hh .hpp .hxx .inc .cu .m .mm .s .S .asm`。一覧に無い拡張子は当たらない） |
| C4 | 合否 | 暗号 trait の自前の実装 |
| C5 | 合否 | 同じファイルでの rotate と wrapping 演算の併存 |
| N1 | 参考 | 定義の名前に暗号の基本処理の語が入ること |
| N2 | 参考 | 本番メンバーの `build.rs` |

- 限界: 発見的な規則である。名前を変えた実装・10 進や別表現の定数・マクロで生成したコードは見落としうる。XML 署名は SAML の Plugin（段階 2）の側でまだ crate が無い。人のレビュー（TASK-83）の代わりにならない（CG-2）。 バイナリと判定されたファイルの grep 出力（行番号なし）は検出が落ちうる。ファイル名の `|`・`&` は出力の整形に混ざりうる。C3 は拡張子の一覧にある範囲だけを見る。
- fail-closed: メンバーが分類表に無い・表の重複・対象 0 件・制御文字を含むパス・grep の失敗は検査不能（2）。実行のたびに自己試験（陽性 S1〜S8・陰性 S9・S10）を先に流す。検査を外す引数・環境変数・例外の許可リストは無い。

### 4. 実行したコマンド

- `scripts/check-crypto-primitives.sh`: 終了コード 0。`selftest ok`。C1〜C5 は各 0。N1・N2（本番）は各 0。
- `make crypto-scan`: 終了コード 0。
- `scripts/check-crypto-primitives.sh foo`: usage を出して終了コード 2。
- 陽性の実地確認: 未追跡の一時ファイル `crates/core/src/zz_probe.rs` に C1 の定数を書いて実行 → C1 が 1 件、終了コード 1。確認後に削除した。

### 5. 結果

- 合否の範囲の検出: 0 件（走査したファイル: 本番 19・試験専用 127）。
- 参考の警告: 試験専用メンバーで 1 件（N1: `crates/hub-stub/tests/assertion.rs:277`。関数名に `p256` の語が入る）。

### 6. 人が決めること

- 試験専用の crate を合否に含めるか、N1・N2 を合否に上げるか。
- 例外を許す仕組みの要否（本段では作っていない）。
- 規則の集合が十分か。

### 7. 判定

（人が記入する。空欄）

## 4/4 検査に載せて記録する（#5170）

### 1. 目的と範囲

- 受入基準は 3 点: (a) `make check` に当たる検査の中で走査が必ず走る、(b) 結線後の実行結果を記録する、(c) `DPOL-4`・`DPOL-47` を緩めない（走査を外す道・例外の仕組みを足さない）。根拠は `LIB-21`・`DPOL-4`・`DPOL-47`。
- 結線先: `scripts/check.sh` の末尾から `scripts/check-crypto-primitives.sh` を呼ぶ。CI では必須の `check` ジョブが `make check` を呼ぶため、`ci.yml` とジョブ名は変えていない。
- 走査の規則・分類・自己試験は変えていない（3/4 の節のとおり）。

### 2. 基準

- base: `origin/main` の `8fbed4b`。
- 実行環境: GNU bash 5.3.9、ugrep 7.8.4（`grep`）。ローカルは linux。

### 3. 結線の内容

- `check.sh` は `set -e` の下で走査を引数なしで呼ぶ。`|| true`・条件分岐・環境変数による切り替えは無い。
- 走査の 1（違反）・2（検査不能）は `check.sh` の非 0 として伝わる。
- 走査を外す引数・環境変数・許可リストは足していない。

### 4. 実行したコマンド

- `make check`: 終了コード 0。ログは editorconfig-checker、shellcheck、crypto-scan の順。`selftest ok`、C1〜C5 は各 0、N1・N2（本番）は各 0。
- `scripts/check.sh foo`: 終了コード 2（引数を拒む）。
- 陽性の実地確認: 未追跡の一時ファイル `crates/core/src/zz_probe.rs` に C1 の定数を書いて `make check` → C1 が 1 件、make の終了コード 2。`scripts/check.sh` の直接実行は終了コード 1。確認後に削除した。
- `make crypto-scan`: 終了コード 0。
- `make test-hooks`: 終了コード 0。`# passed: 771, failed: 0`。
- `make help` と `scripts/help.sh` の `check`・`crypto-scan` の説明の内容が一致することを目で確認した。

### 5. 結果

- 合否の範囲の検出: 0 件（走査したファイル: 本番 19・試験専用 131）。
- 参考の警告: 試験専用メンバーで 4 件（N1: `crates/hub-stub/tests/assertion.rs:277`、`crates/trial-deps/tests/dpol14_rsa_pkcs8.rs` の 20・59・77 行）。3/4 の 1 件から増えた事実だけを書き、評価はしない。
- CI の `check` ジョブ（ubuntu・GNU grep）での実行結果: 未検証（PR の CI で確かめる）。macOS（BSD grep）でのローカル実行: 未検証。

### 6. 人が決めること

- 載せ先（`check` か `lint` か）の最終の判定。
- ジョブ名 `make check (editorconfig-checker + shellcheck)` に走査を出すか（必須チェックの登録名の変更を伴う。設定は変えていない）。
- 試験専用の crate を合否に含めるか、N1・N2 を合否に上げるか、例外を許す仕組みの要否（3/4 から持ち越し）。
- CI は PR の head のスクリプトを走らせるため、PR 自身が走査や `check.sh` を書き換えうる。信頼済みの検出器の方式が要るか。

### 7. 判定

（人が記入する。空欄）

## 追記（2026-11-27）: trial-deps の dev への導入

上の記録の「`aws-lc-rs`・`jsonwebtoken` は lock に無い」（48・95・181・189・223 行付近）は、記録した時点の事実で書き換えない。
その後 #6164 で `crates/trial-deps` の dev 依存として `jsonwebtoken` 12.0.0・`aws-lc-rs` 1.19.0・`pem` が `Cargo.lock` に入った（`DPOL-14` の試験用）。
これは試験専用（dev）として受け入れたもので、本体の crate の選定ではない（利用者の決定。署名の実装の選びと `jsonwebtoken` の採否は上の記録のとおり未定）。
`rsa` は引き続き lock に入らない（`cargo tree --locked -i rsa` が 0 件）。
