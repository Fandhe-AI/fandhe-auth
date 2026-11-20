# fandhe-auth-trial-deps

依存の試し実装（TASK-4 の 6/6、Issue #35）。採用した crate が最小の形で成立するかを確かめる試験専用の crate である。

## 位置づけ

- 試験専用で、名前・置き場は仮称。人が確定する。
- `publish = false`。どの crate からも依存されない。
- 段階 1 のリリースのメンバーではない（`LIB-14`）。
- 本番の依存の木に入らない（`D-2026-11-20-7` の K7）。`tracing`・`metrics` を normal 依存にしているのは、本体が将来持つ出す口の形を測るためである。
- 引き取り先は次のとおり。
  - `TASK-518`（`SECR-23`）・`TASK-514`（`SECR-21`）・`TASK-116`・`TASK-117`
  - `TASK-513`: 派生 `Debug` の入れ物の試験（`SECR-13`）と、乱数の probe の本番版
  - `TASK-500`: 陽性対照、つまり検出力の確認（`AAUD-30`・`AAUD-31`）
  - `TASK-511`: `SECR-11` の property test（`prop_debug`）と fuzz
- 消す条件: 採った道具と試験の形を、本番の TASK（500・511・513・514・518・116・117）が引き取ったら、この crate は消す。
- feature はすべて default のままの暫定（`syn` だけ `default-features = false` + `full`・`parsing`）。K11 の記入後に人が確定する。
- 採用 crate の根拠は `D-2026-11-20-8`〜`10`。
- ここでの合格は〔仮想〕の確認で、spec の「確定」ではない（`ACPT-7`）。

## 試験と fuzz の対象

| 対象 | 場所 | 確かめること | ID |
| --- | --- | --- | --- |
| `tracing` + `tracing-subscriber` | | | | `tests/secr23_tracing.rs` | | | | |
| `metrics` | | | | `tests/metrics_probe.rs` | | | | |
| `proptest` | | | | `tests/prop_debug.rs` | | | | |
| `syn` | | | | `src/scan.rs` の `#[cfg(test)]` | | | | |
| `cargo-fuzz` / `libfuzzer-sys` | `fuzz/fuzz_targets/secret_debug.rs` | 任意のバイト列を UTF-8 として読めたら `SecretString` にし、`Debug` が固定で `expose()` が往復する。K8 の形を試すもので、対象の P-n・F-n ではない | `SECR-11`（K8 の形。`ACPT-38` の対象外） |

範囲外: `SECR-23` の本番の試験（`TASK-518`）、`SECR-21` の本番の走査と CI への組み込み（`TASK-514`）、本番の property test と fuzz の継続（`TASK-116`・`TASK-117`）。
`metrics` のラベルに秘密値を載せない型の制約も範囲外である。

## 実行手順

試験:

```bash
cargo test -p fandhe-auth-trial-deps --locked
```

fuzz は別 workspace（`fuzz/`）で、`make build`・`make test`・`make lint`・`make crate-deps`・CI のどれの対象にもならない。手作業の検査は下の手順で行う。nightly は最新を追う（`D-2026-11-20-9`）。
CI に載せるかは `TASK-117` で決める。

```bash
cd crates/trial-deps/fuzz && cargo +nightly fuzz build && cargo +nightly fuzz run secret_debug -- -max_total_time=10
```

`-max_total_time` の値は秒数で、`10` は例である。変えてよい。

この試し実装の手作業の実行では、`rustc +nightly -V` の出力を控えて PR 本文に書く。実行の記録に nightly の版を残すかは未定（`D-2026-11-20-9`）で、記録の保存先も未定である。どちらも人が決める。

整形（`make lint` の対象外なので手で流す）:

```bash
cargo fmt --manifest-path crates/trial-deps/fuzz/Cargo.toml --check
```

## cargo deny の一時の設定

repo に `deny.toml` は作らない。作成は `TASK-668` の担当で、例外の形や bans の一覧を先取りしてしまうためである。
`docs/spec/03-poc/quality-checks/qp2-deps/deny.toml` の厳格版を、repo の外の一時ディレクトリで使う。

1. 厳格版を repo の外へ写す（`docs/spec` は submodule なので書き換えない）。
2. 写しの許可リストに `"NCSA"` を足す（`libfuzzer-sys` 向け。`D-2026-11-20-9`）。他は変えない。
3. 先に `cargo deny fetch` を 1 回流し、advisory DB の commit と取得日を控える（`DPOL-33`）。
4. 一時の設定のパスを変数に入れ（`DENY_CONFIG`）、ルートに対して流す。

    ```bash
    cargo deny --locked check --config "$DENY_CONFIG" --disable-fetch
    ```

5. fuzz 側は `--manifest-path` を付けて流す。`libfuzzer-sys` の NCSA の通過を確かめるのは、こちらである。

    ```bash
    cargo deny --locked --manifest-path crates/trial-deps/fuzz/Cargo.toml check --config "$DENY_CONFIG" --disable-fetch
    ```

合否は PR の head で、`cargo deny` の 4 検査（advisories・bans・licenses・sources）の全部が成功することで決める（`DPOL-34`）。
基線との差で合格にしない。基線と PR の両方で同じ検査が失敗していても合格とは読まない。
基線との差は、失敗したときの原因の切り分けにだけ使う。依存を足す前の HEAD で同じ設定を流した出力と、足した後の出力を比べる。
通過は判断材料であり、例外の追加は人が決める（`DPOL-35`）。

## cargo audit

`cargo deny` の手順の後に流す（`DPOL-33`・`DPOL-34`）。

1. advisory DB を 1 回取得する。`cargo audit` には `fetch` のサブコマンドが無いので、`--no-fetch` を付けない 1 回目の実行で取得する。

    ```bash
    cargo audit --file Cargo.lock
    ```

2. DB の commit と日付を控える。

    ```bash
    git -C ~/.cargo/advisory-db log -1 --format='%h %cd'
    ```

3. ルートの `Cargo.lock` と fuzz 側の `Cargo.lock` の両方に流す。以降は取得を止めて、同じ DB で照合する。

    ```bash
    cargo audit --no-fetch --file Cargo.lock
    cargo audit --no-fetch --file crates/trial-deps/fuzz/Cargo.lock
    ```

fuzz 側は別 workspace なので、ルートの `Cargo.lock` の監査では拾えない。2 つの lock を別々に流す。
合否は PR の head で、ルートと fuzz の両方の lockfile への `cargo audit` が成功することで決める（`DPOL-34`）。基線との差は切り分けにだけ使う。
結果の判定と例外の追加は人が決める（`DPOL-35`）。

## K11 と feature の測定手順

K11 の閾値は spec に無く未定である。値の記入と判定は人が行う（`ACPT-7`）。ここは手順と空欄の表だけを置く。

1. repo の外に一時ディレクトリを作る。ルートの `Cargo.toml` は触らない。
2. 組ごとに、最小の `Cargo.toml` を 1 つ作る。依存は測る crate 1 つだけにし、版は `=x.y.z` で固定して、feature の組を指定する。
    dev の道具（`tracing-subscriber`・`proptest`・`syn`）も、測定用の manifest では `[dependencies]` に置く。
    こうすると、その crate を使ったときの推移依存を、全組で同じ `normal,build` の値として測れる。
    実際の利用が dev であること（本番の木に入らない）は、K7 として別に扱い、この数には含めない。
3. 次の手順で数える。`-e normal,build` を起点にし、`--target` を明示する。測定用のルートのパッケージ（手順 2 で作ったもの。`PROBE_NAME` に入れる名前）と、
    測る crate 自身（`CRATE` に入れる名前）は推移依存ではないので、数から除く。`cargo tree` が失敗したら集計に進まないよう、2 段に分ける。

    ```bash
    set -euo pipefail
    TARGET=x86_64-unknown-linux-gnu
    cargo tree -e normal,build --target "$TARGET" --prefix none --format '{p}' > tree.txt
    sed 's/ (\*)$//' tree.txt | sort -u > uniq.txt
    { grep -v -e "^$PROBE_NAME " -e "^$CRATE " uniq.txt || true; } | wc -l
    ```

    `cargo tree` が失敗すると（存在しない feature の指定など）、`cargo tree` の行で止まり、後の集計は流れない。
    `grep -v` は全行が除かれると終了コード 1 を返すので、`|| true` で数 0 を許す。
    `cargo tree` は 2 回目以降に出る依存の行に ` (*)` を付ける。`sort -u` は行全体で比べるので、印を `sed` で除いてから重複を除く。
    除かないと、複数の経路で届く依存を 2 回数える。`tracing` と `tracing-subscriber` を両方 normal に置いた一時の crate で流したところ、
    印を除かない場合は 21、除いた場合は 18 で、`--no-dedupe` で数えた 18 と一致した。

    `--target` は `x86_64-unknown-linux-gnu` に固定する（全組で同じにする）。`--target all` にする場合は、全組をそれに替え、どちらを使ったかを記録する。
    表の「推移依存の数」の列は、全行この `normal,build` の値である。

    組ごとに次を記録する。別の日・別のホストの結果と比べるための記録で、値の判定は人が行う。
    - 測った crate の版（`=x.y.z`）と feature の組
    - `--target` の値
    - 測った日と `rustc -V` の出力
    - 測定用の `Cargo.lock`（または `tree.txt`）の保存先。保存先は未定で、人が決める
4. MSRV は、解決したパッケージの `rust_version` を `cargo metadata` から引く。宣言の無い依存は `未宣言` と出る。

    ```bash
    cargo metadata --format-version 1 | jq -r '.packages[] | "\(.name) \(.version) \(.rust_version // "未宣言")"'
    ```

    `packages` には全プラットフォーム分の依存が入る。`未宣言` の依存は、MSRV の toolchain（1.100）でビルドして確かめる。
    構成の MSRV は、各依存の宣言の最大値を人が読み取る。
5. ライセンスは `cargo deny list` の結果を使う。

組の候補（各 crate の `[features]` を読んだ事実による）:

- `tracing`: default は `std`・`attributes`。`attributes` は proc-macro の `tracing-attributes` を引く。`#[instrument]` を本体が使うかが論点になる。
  ほかに `max_level_*`・`release_max_level_*` がある。
- `tracing` の default の `attributes` は、`tracing-attributes`（proc-macro）を経由して `syn` 2.0.119 を normal で引く。
  本体に default のまま入れると、本番のビルドに `syn` 2 が入り、`syn` 3 と 2 つの major が並ぶ。
- `metrics`: `[features]` が無い。`default-features = false` の組は成り立たない。
- `tracing-subscriber`: default は `smallvec`・`fmt`・`ansi`・`tracing-log`・`std`。
- `proptest`: default は `std`・`fork`・`timeout`・`bit-set`。
- `syn`: default は `derive`・`parsing`・`printing`・`clone-impls`・`proc-macro`。`full`・`visit` は別の feature。いまの試し実装は `default-features = false` + `full`・`parsing`。

| crate | 版 | ターゲット | 測った日 | feature の組 | 推移依存の数（`normal,build`） | MSRV | ライセンス | 人の判定 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `tracing` | | | | default | | | | |
| `tracing` | | | | `default-features = false` | | | | |
| `tracing` | | | | `default-features = false` + `std` | | | | |
| `tracing` | | | | 上記 + `max_level_*` | | | | |
| `metrics` | | | | default | | | | |
| `tracing-subscriber`（dev） | | | | default | | | | |
| `tracing-subscriber`（dev） | | | | `default-features = false` + `fmt`・`std` | | | | |
| `proptest`（dev） | | | | default | | | | |
| `proptest`（dev） | | | | `default-features = false` + `std` | | | | |
| `syn`（dev） | | | | default | | | | |
| `syn`（dev） | | | | `default-features = false` + `full`・`parsing`（現状） | | | | |
| `syn`（dev） | | | | 上記 + `visit` | | | | |

## 観測した事実

- `syn` は、マクロ呼び出しの中に書いた構造体を見ない（`src/scan.rs` の `observation_struct_inside_macro_call_is_not_seen`）。合否ではなく観測の固定である。
  見落としの扱いは `TASK-514` で決める（main の読み）。
- `syn` の置き場（xtask・専用 crate・別 workspace など）は spec に無く未定である。この crate の dev 依存は一時的な置き方で、置き場の決定ではない。
