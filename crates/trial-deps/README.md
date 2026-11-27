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
  - `TASK-667` (3/4): `DPOL-14` の試験（`tests/dpol14_rsa_pkcs8.rs`）。引き取り先は RS256 の署名の実装を置く本番の crate とその TASK で、spec 上の番号は未定（置き場は人が確定する）。実装が PEM の形を使うことの確認は (4/4)・#320
  - `SECR-29` の例外: この crate の試験の型（`prop_debug` の入力の型・`SECR-23` の試験の捕獲の型）は、crate を消すまで `SECR-10`・`SECR-29` の一覧の対象外（`D-2026-11-20-14` の (4)(e)）。
    条件は `.claude/rules/secret-values.md` の例外の箇条に従う: 秘密値の型を包まない。`prop_debug` の `Input` は `Debug` が固定の文字列である間。捕獲の型 5 つは `Debug` を実装しない間。条件を外れた型は一覧に戻す
  - `TASK-518`: `log` crate を経由した記録も本番の `SECR-23` の試験で捕まえる（`D-2026-11-20-14` の (4)(f)。この crate は直さない）
- 消す条件: 採った道具と試験の形を、本番の TASK（500・511・513・514・518・116・117）と `DPOL-14` の試験の引き取り先が引き取ったら、この crate は消す。
- feature は、利用者の方針（2026-11-20。安全な最新の安定版・必要最小の feature）で必要最小に絞った。この組は `D-2026-11-20-14` の (4)(b)(c) で決まった（試し実装の時点の組）。
  - `tracing`: `default-features = false` + `std`
  - `metrics`: 既定（`[features]` が無い）
  - `tracing-subscriber`: `default-features = false` + `fmt`（明示）。`fmt` が `registry`・`std` を暗黙に有効にする
  - `proptest`: `default-features = false` + `std`
  - `syn`: `default-features = false` + `full`・`parsing`
  - `jsonwebtoken`（dev）: `default-features = false` + `aws_lc_rs`・`use_pem`。`rust_crypto` は有効にしない（`DPOL-17`）。採否の決定ではない
  - `aws-lc-rs`（dev）: `default-features = false` + `aws-lc-sys`。鍵の実行時の生成だけに使う
  - `pem`（dev）: `default-features = false`
  - `libfuzzer-sys`: 既定（`fuzz/Cargo.toml` の `0.4.13`。fuzz は別の workspace）
- 採用 crate の根拠は `D-2026-11-20-8`〜`10`。
- ここでの合格は〔仮想〕の確認で、spec の「確定」ではない（`ACPT-7`）。

## 試験と fuzz の対象

| 対象 | 場所 | 確かめること | ID |
| --- | --- | --- | --- |
| `tracing` + `tracing-subscriber` | `tests/secr23_tracing.rs` | `SecretString` を `?` 形式で出しても、派生 `Debug` の入れ物（std の tuple・`Option`。自前の構造体の試験は `TASK-513`）に入れても、捕獲した出力に値が無い。`tests/secr23_tracing.rs` の中で `expose()` の値を直接出す経路は検出できる（陽性対照。lib には置かない） | `SECR-23`・`SECR-11`・`SECR-13` |
| `metrics` | `tests/metrics_probe.rs` | 自前の最小 `Recorder` を `with_local_recorder` で当て、counter の key・ラベル・増分を受けられる | `SECR-26`・`SECR-27` |
| `proptest` | `tests/prop_debug.rs` | 任意の `String` から作った `SecretString` の `Debug` が常に固定文字列で、値を含まない。`expose()` が往復する。K8 の形を試すもので、対象の P-n・F-n ではない | `SECR-11`（K8 の形。`ACPT-37` の対象外） |
| `syn` | `src/scan.rs` の `#[cfg(test)]` | `syn::parse_file` で、秘密値らしい欄名で型が生の `String` の欄を拾う。edition 2024 の構文を読める | `SECR-21` |
| `jsonwebtoken`（`aws_lc_rs`・`use_pem`）+ `aws-lc-rs` | `tests/dpol14_rsa_pkcs8.rs` | 実行時に生成した RS256 の鍵が PKCS#8 で PKCS#1 の読み手に拒否される（陽性対照）。`from_rsa_der` に PKCS#8 の DER を渡すと `encode` が署名の誤りで失敗する。`from_rsa_pem` に PKCS#8 の PEM を渡すと署名でき、公開鍵で検証できる。〔仮想〕で、確定ではない | `DPOL-14` |
| `cargo-fuzz` / `libfuzzer-sys` | `fuzz/fuzz_targets/secret_debug.rs` | 任意のバイト列を UTF-8 として読めたら `SecretString` にし、`Debug` が固定で `expose()` が往復する。K8 の形を試すもので、対象の P-n・F-n ではない | `SECR-11`（K8 の形。`ACPT-38` の対象外） |

範囲外: `SECR-23` の本番の試験（`TASK-518`）、`SECR-21` の本番の走査と CI への組み込み（`TASK-514`）、本番の property test と fuzz の継続（`TASK-116`・`TASK-117`）。
`metrics` のラベルに秘密値を載せない型の制約も範囲外である。

## 実行手順

試験:

```bash
cargo test -p fandhe-auth-trial-deps --locked
```

fuzz は別 workspace（`fuzz/`）で、`make build`・`make test`・`make lint`・`make crate-deps`・CI のどれの対象にもならない。CI に載せるかは `TASK-117` で決める。
手で流すときは、空の専用のコーパスか合成の入力だけを使う。crash のときの stderr と `fuzz/artifacts/` を PR・Issue・記録に貼らない（`SECR-23`・`SECR-26`）。
手で流す手順は、PR #5980 の本文にある折りたたみ「試しの実行の手順」に置いた。作業ディレクトリは `crates/trial-deps/fuzz`、対象は `secret_debug` である。
手順には、lock の確認（`cargo metadata --locked`）と、空の専用コーパスの指定を含む。正式な手順は `TASK-117` が作る。
nightly は最新を追う（`D-2026-11-20-9`）。実行の記録に nightly の版を残すかは未定（`D-2026-11-20-9`）で、記録の保存先も未定である。どちらも人が決める。

## 依存の監査

- `deny.toml`（厳格版）は `TASK-668`、正式な手順と記録の様式は `TASK-679`（`DPOL-33`）が作る。repo に `deny.toml` は置かない。
- 試しの実行の手順と結果は、PR #5980 の本文に記録した。
- 合否は PR の head で、全検査が成功することで決める（`DPOL-34`）。結果の判定と例外の追加は人が決める（`DPOL-35`）。
- `fuzz/` は別 workspace（ルートの `exclude`）で、独自の `Cargo.lock` を持つ。このため、ルートの `cargo deny`・`cargo audit` は `libfuzzer-sys` とその推移依存を見ない。合否には、ルートと fuzz 側の両方の結果を含める（`DPOL-33`・`DPOL-34`）。fuzz 側の対象は、manifest が `fuzz/Cargo.toml`、lockfile が `fuzz/Cargo.lock` である。fuzz 側を含めた監査の正式な手順は `TASK-679`（`DPOL-33`）、厳格版の設定は `TASK-668`（#321）で決める。

## K11 と feature の選び

- K11 の見方は `D-2026-11-20-14` の (4)(d) で決まった（`D-2026-11-20-7` の時点では未定だった）。この README には測定の手順も、値を入れる表も置かない。
- K11 の測定の記録は、spec の `D-2026-11-20-14` の (1) にある。
- 口の feature の選び（`tracing`・`metrics`・dev の道具）は、`D-2026-11-20-14` の (4)(b)(c) で決まった。組は上の「位置づけ」に列挙している（試し実装の時点の組）。

## 確かめた事実

各 crate の `[features]` を読んだ事実:

- `tracing`: default は `std`・`attributes`。`attributes` は proc-macro の `tracing-attributes` を引く。`#[instrument]` を本体が使うかは試し実装の時点の論点で、`D-2026-11-20-14` の (4)(b) で口は `std` だけ（`#[instrument]` は使わない）と決まった。
  ほかに `max_level_*`・`release_max_level_*` がある。
- `tracing` の default のままなら、`attributes` が `tracing-attributes`（proc-macro）を経由して `syn` 2.x を normal で引く。
  この crate は `default-features = false` + `std` にして `attributes` を外した（マクロだけを使い、`#[instrument]` は使わない）。
  lock から `syn` 2.0.119 と `tracing-attributes` は消えた。
  repo のルートで、次のコマンドで確かめた事実は次のとおり（`-i` の引数は版つきの `syn@3.0.10`）。

  ```bash
  cargo tree --locked -p fandhe-auth-trial-deps -e normal,build --target all -i syn@3.0.10
  cargo tree --locked -p fandhe-auth-trial-deps -e normal,build,dev --target all -i syn@3.0.10
  cargo tree --locked --workspace -e normal,build --target all -i syn@3.0.10
  ```

  - この crate の normal・build の木には `syn` が入らない（1 本目の出力は `nothing to print`）。`syn` 2.x は lock にも無く、`-i syn@2.0.119` は `did not match any packages` になる。
  - `-e normal,build,dev` に替えると（2 本目）、`syn` 3.0.10 が `[dev-dependencies]` の直接依存として入る。構文の走査用である。
  - workspace 全体の normal・build では（3 本目）、`syn` 3.0.10 は `serde_derive` を経由して `hub-stub` と `console-stub` に入っている（どちらも `serde_json` 経由）。この crate の変更によるものではない。
  - 並ぶかどうかの判断はここでは書かない。本体の木に何を引くかは、feature の組の選びと併せて人が決める。
- `metrics`: `[features]` が無い。既定の feature が無いので、`default-features = false` を指定しても差は出ない。
- `tracing-subscriber`: default は `smallvec`・`fmt`・`ansi`・`tracing-log`・`std`。いまの試し実装は `default-features = false` + `fmt` だけを明示する。`fmt` が `registry`・`std` を暗黙に有効にする（crate の `[features]` の `fmt = ["registry", "std"]`）。
- `proptest`: default は `std`・`fork`・`timeout`・`bit-set`。いまの試し実装は `default-features = false` + `std`。
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
  - 括弧で囲んだ型（`(String)`）。型の経路の末尾の識別子だけを見るので、括弧（`Type::Paren`）の中は辿らない。
  - ファイルとインラインのモジュールの項目だけを辿るので、関数の本体などのブロックの中の `struct` は見ない（`collect` が辿るのは `Item::Struct` と、本文のあるインラインの `mod` だけ）。
  - 型名の側が秘密値らしいもの（例: 欄名が `value` で型が `SecretToken`）。見るのは欄名だけである。
  - 上の tuple struct・enum・`Option<String>`・`Vec<u8>`・`[u8; N]`・型名の 5 つは、scratch の写しで試験を足し、拾われないことを確かめた（拾われた件数 0）。この試験は repo に入れていない。
  - 試しで通ったことを、走査の検出力の根拠にしない。検出力は `TASK-500`（`AAUD-30`・`AAUD-31`）の陽性対照で確かめる。
- `syn` の置き場（xtask・専用 crate・別 workspace など）は spec に無く未定である。この crate の dev 依存は一時的な置き方で、置き場の決定ではない。
