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
(
  set -euo pipefail
  cd crates/trial-deps/fuzz
  cargo +nightly metadata --locked --format-version 1 > /dev/null
  cargo +nightly fuzz build
  cargo +nightly fuzz run secret_debug -- -max_total_time=10
  git diff --exit-code Cargo.lock
)
```

サブシェルで `set -euo pipefail` を掛けるので、どの段階の失敗もブロックの終了コードに伝わる（対話シェルに素の `set -e` を貼るとシェルごと落ちるため、サブシェルにしてある）。
`metadata --locked` が失敗したら、fuzz の build と run は始まらない。

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
3. 変数を決める。advisory DB の保存先は、空の専用ディレクトリにする。取得・照合・後片付けで同じ変数を使うので、同じシェルで最後の手順まで使う。

    ```bash
    SRC_CONFIG=/path/to/deny.toml  # 手順 1・2 で作った写しのパスに替える
    DENY_DB_DIR="$(mktemp -d)"
    DENY_CONFIG="$(mktemp -d)/deny.toml"
    ```

4. 一時の設定を作り、その設定で `cargo deny fetch` を 1 回流す。取得の直後に取得日時と、`db-path` の下にある全 DB の URL と commit SHA を控える（`DPOL-33`）。
    設定には `[advisories]` の `db-path` と `db-urls` を足す（cargo-deny 0.19.4 で、指定した `db-path` の下に DB が取得されることを確かめた）。
    写しにこの 2 つのキーが既にあるときは、重複させずに値を替える。commit の日付は取得日ではないので、取得日時は別に取る。DB のディレクトリを当て推量で選ばず、全部を列挙する。
    サブシェルで `set -euo pipefail` を掛けるので、どの段階の失敗もブロックの終了コードに伝わる。

    ```bash
    (
      set -euo pipefail
      awk -v d="$DENY_DB_DIR" '{ print } /^\[advisories\]/ { print "db-path = \"" d "\""; print "db-urls = [\"https://github.com/rustsec/advisory-db\"]" }' "$SRC_CONFIG" > "$DENY_CONFIG"
      grep -q '^db-path' "$DENY_CONFIG"
      cargo deny fetch --config "$DENY_CONFIG" db
      date -u +%Y-%m-%dT%H:%M:%SZ
      for db in "$DENY_DB_DIR"/*/; do
        [ -d "$db/.git" ]
        echo "url=$(git -C "$db" remote get-url origin) sha=$(git -C "$db" rev-parse HEAD)"
      done
    )
    ```

    - 取得日時: `date` の出力。
    - 監査に使った DB の URL と commit SHA: ループの出力。`url` の表記は、git の設定（`url.<base>.insteadOf`）の書き換えを受けることがある。

5. ルートに対して流す。取得は済んでいるので `--disable-fetch` を付ける。

    ```bash
    cargo deny --locked check --config "$DENY_CONFIG" --disable-fetch
    ```

6. fuzz 側は `--manifest-path` を付けて流す。`libfuzzer-sys` の NCSA の通過を確かめるのは、こちらである。

    ```bash
    cargo deny --locked --manifest-path crates/trial-deps/fuzz/Cargo.toml check --config "$DENY_CONFIG" --disable-fetch
    ```

7. 照合が済んだら、一時のディレクトリを消す。この DB は手順 5・6 の `--disable-fetch` で使うので、照合より先には消さない。

    ```bash
    rm -rf "${DENY_DB_DIR:?}" "$(dirname "${DENY_CONFIG:?}")"
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

## K11 と feature の選び

- K11 の測り方・ターゲット・閾値は未定で、人が決める（`D-2026-11-20-7`）。この README には測定の手順も、値を入れる表も置かない。
- 測定手順の下書きは PR 本文で渡す。
- 口の feature の選び（`tracing`・`metrics`・dev の道具）は未定で、K11 を人が決めた後に確定する。いまの feature はすべて default のままの暫定である（上の「位置づけ」）。

## 確かめた事実

各 crate の `[features]` を読んだ事実:

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

## 観測した事実

- `syn` は、マクロ呼び出しの中に書いた構造体を見ない（`src/scan.rs` の `observation_struct_inside_macro_call_is_not_seen`）。合否ではなく観測の固定である。
  見落としの扱いは `TASK-514` で決める（main の読み）。
- `src/scan.rs` の走査が拾わない形（`TASK-514` への引き継ぎ事項。いずれも合否ではなく観測で、扱いは `TASK-514` で決める。`SECR-21`）:
  - マクロ呼び出しの中に書いた構造体（上の観測）。
  - 標準の `String` の別名（`type S = String;`、`use std::string::String as S;`）で参照する欄。逆に、独自型の `custom::String` は末尾が `String` なので拾う（誤検出。`observation_custom_string_is_found_and_alias_is_not`）。
  - tuple struct（例: `struct ApiToken(String);`）。名前付きの欄だけを見る。
  - enum の variant の欄。`struct` の項目だけを辿る。
  - `Option<String>` などで包んだ形。型の経路の末尾の識別子だけを見る。
  - `Vec<u8>` と `[u8; N]`。
  - 型名の側が秘密値らしいもの（例: 欄名が `value` で型が `SecretToken`）。見るのは欄名だけである。
  - 上の tuple struct・enum・`Option<String>`・`Vec<u8>`・`[u8; N]`・型名の 5 つは、scratch の写しで試験を足し、拾われないことを確かめた（拾われた件数 0）。この試験は repo に入れていない。
  - 試しで通ったことを、走査の検出力の根拠にしない。検出力は `TASK-500`（`AAUD-30`・`AAUD-31`）の陽性対照で確かめる。
- `syn` の置き場（xtask・専用 crate・別 workspace など）は spec に無く未定である。この crate の dev 依存は一時的な置き方で、置き場の決定ではない。
