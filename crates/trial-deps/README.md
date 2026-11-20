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
| `tracing` + `tracing-subscriber` | `tests/secr23_tracing.rs` | `SecretString` を `?` 形式で出しても、派生 `Debug` の入れ物（std の tuple・`Option`。自前の構造体の試験は `TASK-513`）に入れても、捕獲した出力に値が無い。`tests/secr23_tracing.rs` の中で `expose()` の値を直接出す経路は検出できる（陽性対照。lib には置かない） | `SECR-23`・`SECR-11`・`SECR-13` |
| `metrics` | `tests/metrics_probe.rs` | 自前の最小 `Recorder` を `with_local_recorder` で当て、counter の key・ラベル・増分を受けられる | `SECR-26`・`SECR-27` |
| `proptest` | `tests/prop_debug.rs` | 任意の `String` から作った `SecretString` の `Debug` が常に固定文字列で、値を含まない。`expose()` が往復する。K8 の形を試すもので、対象の P-n・F-n ではない | `SECR-11`（K8 の形。`ACPT-37` の対象外） |
| `syn` | `src/scan.rs` の `#[cfg(test)]` | `syn::parse_file` で、秘密値らしい欄名で型が生の `String` の欄を拾う。edition 2024 の構文を読める | `SECR-21` |
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

`cargo fuzz` の `build`・`run` は `--locked` を受けない（`cargo +nightly fuzz build --help` で確かめた）。
先に `cargo metadata --locked` で lock が最新であることを確かめてから流す。lock の更新が要るときは、`--locked` が失敗する。
その場合は、依存の差分を監査してから lock を更新する（`./.claude/rules/dependency-policy.md` の手順）。更新後に `Cargo.lock` の差分が無いことを確かめて流す。

```bash
cd crates/trial-deps/fuzz
cargo +nightly metadata --locked --format-version 1 > /dev/null
cargo +nightly fuzz build && cargo +nightly fuzz run secret_debug -- -max_total_time=10
git diff --exit-code Cargo.lock
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
3. 一時の設定のパスを変数に入れる（`DENY_CONFIG`）。取得と照合で同じ設定を使う。
4. 先にその設定で `cargo deny fetch` を 1 回流し、取得の直後に次の 2 つを控える（`DPOL-33`）。commit の日付は取得日ではないので、取得日時は別に取る。

    ```bash
    DENY_CONFIG=/path/to/deny.toml  # 手順 1・2 で作った写しのパスに替える
    cargo deny fetch --config "$DENY_CONFIG" db
    date -u +%Y-%m-%dT%H:%M:%SZ
    ADVISORY_DB_DIR="$(ls -d ~/.cargo/advisory-dbs/advisory-db-* | head -1)"
    git -C "$ADVISORY_DB_DIR" rev-parse HEAD
    ```

    - 取得日時: 上の `date` の出力。
    - 監査に使った DB の commit SHA: 上の `rev-parse` の出力。`ls` が複数の DB を返すときは、その設定で使う DB のディレクトリを選ぶ。
5. ルートに対して流す。取得は済んでいるので `--disable-fetch` を付ける。

    ```bash
    cargo deny --locked check --config "$DENY_CONFIG" --disable-fetch
    ```

6. fuzz 側は `--manifest-path` を付けて流す。`libfuzzer-sys` の NCSA の通過を確かめるのは、こちらである。

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

2. 取得の直後に、取得日時と DB の commit SHA を控える。`git log` の `%cd` は DB の最新 commit の日付で、取得した日ではないので使わない。

    ```bash
    date -u +%Y-%m-%dT%H:%M:%SZ
    CARGO_AUDIT_DB_DIR=~/.cargo/advisory-db
    git -C "$CARGO_AUDIT_DB_DIR" rev-parse HEAD
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

測定は、repo の外の一時ディレクトリに作る単独の package で行う。ルートの `Cargo.toml` は触らない。
次のブロックは、新しい bash にそのまま貼って最後まで流れる。先頭の変数だけを組ごとに替える。

- `CRATE`・`VERSION`・`DEFAULT_FEATURES`・`FEATURES`: 測る crate、版（`=VERSION` で固定する）、既定の feature を使うか、足す feature（引用符付きのカンマ区切り。例: `'"std","attributes"'`）。
- `KIND`: 実際の利用が `normal` か `dev` か。dev の道具（`tracing-subscriber`・`proptest`・`syn`）も、測定用の manifest では `[dependencies]` に置く。
  こうすると、全組を同じ `normal,build` の値で測れる。dev であること（本番の木に入らない）は K7 として別に扱い、数には含めない。`KIND` は記録の表示にだけ使う。
- `TARGET`: 全組で同じにする。例は `x86_64-unknown-linux-gnu`。`all` も選べる。どちらを使ったかを記録する。
  `all` のとき、MSRV は `--filter-platform` を省いて全ターゲットの依存から引く（`all` は `--filter-platform` の値として全ターゲットを意味しない）。
- edition と resolver はルートの `Cargo.toml` に合わせる（edition は `[workspace.package]` の `2024`、resolver は `[workspace]` の `"3"`。どちらも暫定で、ルートが変われば合わせ直す）。
  測定用の manifest は単独の package なので、`resolver` は `[package]` に書く。
- 比べる組は、同じ日に同じ環境（同じ toolchain・同じ `TARGET`・同じ registry の状態）でまとめて測る。
  各組の `Cargo.lock` を保存し、共通する推移依存の解決版が組の間でそろっているかを照合する。そろっていなければ測り直す。

```bash
set -euo pipefail
PROBE_NAME=probe-k11
CRATE=tracing
VERSION=0.1.44
DEFAULT_FEATURES=true
FEATURES=''
KIND=normal
TARGET=x86_64-unknown-linux-gnu

WORK="$(mktemp -d)"
mkdir -p "$WORK/src" "$WORK/record"
cd "$WORK"
cat > Cargo.toml <<EOF
[package]
name = "$PROBE_NAME"
version = "0.0.0"
edition = "2024"
resolver = "3"

[dependencies]
$CRATE = { version = "=$VERSION", default-features = $DEFAULT_FEATURES, features = [$FEATURES] }
EOF
: > src/lib.rs
cargo generate-lockfile

# 推移依存の数。cargo tree が失敗したら、ここで止まる。
cargo tree -e normal,build --target "$TARGET" --prefix none --format '{p}' > tree.txt
sed 's/ (\*)$//' tree.txt | sort -u > uniq.txt
DEPS="$({ grep -v -e "^$PROBE_NAME " -e "^$CRATE " uniq.txt || true; } | wc -l | tr -d ' ')"

# MSRV。TARGET が all のときは --filter-platform を省く。
if [ "$TARGET" = all ]; then
  cargo metadata --locked --format-version 1 > meta.json
else
  cargo metadata --locked --format-version 1 --filter-platform "$TARGET" > meta.json
fi
jq -r '[.resolve.nodes[].id] as $ids | .packages[] | select(.id as $i | $ids | index($i)) | "\(.name) \(.version) \(.rust_version // "未宣言")"' meta.json > msrv.txt

# 記録。保存先は未定なので、控えを record/ に集めて場所を表示する。
cp Cargo.lock tree.txt msrv.txt record/
{ date -u +%Y-%m-%dT%H:%M:%SZ; rustc -V; } > record/env.txt
echo "crate=$CRATE version=$VERSION kind=$KIND target=$TARGET deps=$DEPS"
echo "record: $WORK/record"
```

`cargo tree` は 2 回目以降に出る依存の行に ` (*)` を付ける。`sort -u` は行全体で比べるので、印を `sed` で除いてから重複を除く。
除かないと、複数の経路で届く依存を 2 回数える。`tracing` と `tracing-subscriber` を両方 normal に置いた一時の crate で流したところ、
印を除かない場合は 21、除いた場合は 18 で、`--no-dedupe` で数えた 18 と一致した。
`grep -v` は全行が除かれると終了コード 1 を返すので、`|| true` で数 0 を許す。`cargo tree` や `cargo metadata` が失敗すると、`set -e` でその行で止まる。

- 推移依存の数は、測定用のルートのパッケージと測る crate 自身を除いた件数である。
- 組ごとに、測った crate の版と feature の組・`TARGET`・測った日・`rustc -V` の出力・`Cargo.lock`（または `tree.txt`）を記録する。保存先は未定で、人が決める。
- MSRV は、`msrv.txt` の `未宣言` の依存を MSRV の toolchain（1.100）でビルドして確かめる。構成の MSRV は、各依存の宣言の最大値を人が読み取る。
  `packages` 単独には他のターゲットだけの依存も入る。この workspace で `cargo metadata` の `packages` が 101 件のところ、`x86_64-unknown-linux-gnu` で絞ると 85 件になる。
  表の「MSRV」の列には、対象の `TARGET` を添えて記入する。
- ライセンスは `cargo deny list` の結果を使う。

組の候補（各 crate の `[features]` を読んだ事実による）:

- `tracing`: default は `std`・`attributes`。`attributes` は proc-macro の `tracing-attributes` を引く。`#[instrument]` を本体が使うかが論点になる。
  ほかに `max_level_*`・`release_max_level_*` がある。
- `tracing` の default の `attributes` は、`tracing-attributes`（proc-macro）を経由して `syn` 2.0.119 を normal で引く。
  `cargo tree --locked -p fandhe-auth-trial-deps -e normal,build --target all -i` で確かめた事実は次のとおり。
  - この crate の normal・build の木には `syn` 2.0.119 だけが入る（`tracing-attributes` 経由）。`syn` 3.0.10 は入らない（出力は `nothing to print`）。
  - `-e normal,build,dev` に替えると、この crate の木には `syn` 2.0.119 と `syn` 3.0.10 の両方が入る。`syn` 3 は、この crate の dev 依存（構文の走査用）である。
  - workspace 全体の normal・build では、`syn` 3.0.10 は `serde_derive` を経由して `hub-stub` と `console-stub` に入っている。この crate の変更によるものではない。
  - 並ぶかどうかの判断はここでは書かない。本体の木に `syn` が何を引くかは、feature の組の選びと併せて人が決める。
- `metrics`: `[features]` が無い。`default-features = false` の組は成り立たない。
- `tracing-subscriber`: default は `smallvec`・`fmt`・`ansi`・`tracing-log`・`std`。
- `proptest`: default は `std`・`fork`・`timeout`・`bit-set`。
- `syn`: default は `derive`・`parsing`・`printing`・`clone-impls`・`proc-macro`。`full`・`visit` は別の feature。いまの試し実装は `default-features = false` + `full`・`parsing`。

| crate | 版 | ターゲット | 測った日 | feature の組 | 推移依存の数（`normal,build`） | MSRV（対象の `--target` の依存グラフ） | ライセンス | 人の判定 |
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
