# 暫定の実装の置き場と差し替え対象の台帳（TASK-9）

`fandhe-backend`・`fandhe-frontend`・`fandhe-db` の部品は、提供されるまで自前の暫定の実装で代える。
暫定の実装は多くの TASK に散らばるため、どの部品を、どの TASK で作り、どの TASK で差し替えるかを
このディレクトリの台帳 1 か所で追えるようにする。対象は `TASK-9`（Issue #80・#81）。

根拠の ID: `A-2026-10-16-5`（版の固定）・`CG-6`（差し替え前の再試験）・`D-2026-10-30-1`（Gate の担当）。
spec の本文・版の数値は写さず、ID で引く（`.claude/rules/spec-reference.md`）。
差し替えの完了や各項目の「確定」の判定は人が行う（ACPT-7）。

## 置き場の規則

- 本ディレクトリは文書だけを置く。暫定の実装のコードは、その部品を使う crate の中に置き、下の台帳の行から引く。
  `backend` の配置は `crates/backend`（`c2b3f67` の時点の規約〔`D-2026-11-20-3` の (1)〕。その後 `D-2026-11-20-1` で、
  `TASK-9` の暫定の置き場として残すことになった）。
  それ以外のコードの具体のパスはここで決めない。
- 暫定の実装と提供版を、feature・環境変数・設定で実行時に切り替える経路は作らない（DPOL-30〜32・CSRF-7）。
  差し替えはコードの変更と版の固定で行う。
- 差し替えの前に再試験を流す（CG-6）。手順は末尾の「差し替えの手順」の節（#86）。
- 版の固定は、末尾の「版の固定の記録の形式」の節の形式で記録する（#85）。
- 暫定コードの module doc と `TODO(TASK-n)` には、中身を作る TASK と差し替える TASK を書く
  （`.claude/rules/code-comment-style.md`・`.claude/rules/out-of-scope-tracking.md`）。
- `backend`（`fandhe-auth-backend`・`crates/backend`）は #82 で枠（`crates/backend/src/interim/`）を作った。
  暫定の実装の中身は未作成。module の構成は暫定で、人が確定する。行 5（`form_limit`）・行 6（`trusted_proxy`）の枠は、
  本体で作らない移管分のため外した（`D-2026-11-20-3` の (イ)）。中身を足す前に、人が下の「backend の切り出しの記録」で
  `LIB-15` の判定を記録する（同 (ア)）。
- `frontend`（`fandhe-auth-frontend`・`crates/frontend`）の枠の形は #83 で本文書の末尾の節に案として記録した。
  crate は未作成で、作る前に利用者に確かめる（LIB-14・LIB-15）。`frontend` 以外の段階 2 の crate も同じ。
- `db` の差し替え点の形は #84 で本文書の末尾の節に記録した。差し替えの境界は契約 trait で、置き場（独立 crate か
  `fandhe-auth-core` の中か）は未定（DPOL-38・TASK-584・TASK-684）。この repo に crate は足していない。

## 差し替え対象の一覧

各行の内容は spec の原文で確かめること。ステータスはいずれも「暫定の実装は未作成」「差し替えは未実施」。

| # | 提供元の部品 | 暫定の実装を作る TASK | 差し替え・再試験の TASK | コードの置き場 | この repo での扱い |
| --- | --- | --- | --- | --- | --- |
| 1 | `fandhe-backend` CSRF の Gate | TASK-523 | TASK-527 | `crates/backend/src/interim/csrf_gate.rs`（枠は #82 で作成） | TASK-527 は置き換え（→ Hub `ARUN-17`）。Gate の登録・実行は Hub（`csrf-ssr.md` の適用範囲・`D-2026-10-30-1`） |
| 2 | `fandhe-backend` 安全な既定値の `SetCookie` | TASK-214（目安。認証 Cookie の発行・消去の TASK で、部品そのものの TASK ではない） | TASK-527 | `crates/backend/src/interim/set_cookie.rs`（枠は #82 で作成） | 同上（差し替えの実施は Hub の担当） |
| 3 | `fandhe-backend` 同一オリジンのリダイレクト先 | TASK-533 | TASK-527 | `crates/backend/src/interim/local_redirect.rs`（枠は #82 で作成） | 同上（差し替えの実施は Hub の担当） |
| 4 | `fandhe-backend` `no-store` の補助 | TASK-530 | TASK-527 | `crates/backend/src/interim/no_store.rs`（枠は #82 で作成） | 同上（差し替えの実施は Hub の担当） |
| 5 | `fandhe-backend` フォームの上限 | TASK-534 | TASK-527 | 枠なし（`crates/backend/src/interim/form_limit.rs` は外した） | TASK-534・TASK-61 は移管（→ Hub `ARUN-46`・`ARUN-47`）。本体では作らない（`D-2026-11-20-3` の (イ)）。枠は外した。`TODO(TASK-534)` は追跡先が本体に無いため移していない |
| 6 | `fandhe-backend` 信頼するプロキシ | TASK-535・TASK-536 | TASK-527（提供された場合）。現時点は提供なし（`D-2026-10-27-2`） | 枠なし（`crates/backend/src/interim/trusted_proxy.rs` は外した） | 移管の部分（`CSRF-40`〜`42`・`CSRF-47`〔自前の accept ループ〕→ Hub `ARUN-43`、`CSRF-44` → Hub `ARUN-44`）は本体では作らない（`D-2026-11-20-3` の (イ)）。本体の部分（`CSRF-43`・`CSRF-52` は TASK-535、`CSRF-45`・`CSRF-46` は TASK-536）は枠の外で作り、置き場は未決（`LIB-1` の判定で利用者が決める）。`TODO(TASK-535)`・`TODO(TASK-536)` は `crates/backend/src/interim/mod.rs` に移した。差し替えの実施は Hub の担当（TASK-527 は置き換え → Hub `ARUN-17`） |
| 7 | `fandhe-backend` 下流の JWT の検証（`REQ-16`） | TASK-370 | TASK-372（再試験 TASK-89） | `crates/client`（段階 1・既存） | 本 repo の範囲。#82 では触らない（中身は TASK-370） |
| 8 | `fandhe-frontend` の builder（`REQ-17`） | TASK-532 | TASK-90（TASK-152 は移管 → `UISF-10`・`UISF-22`） | `crates/frontend/src/interim/builder.rs`（案。crate の作成は利用者の確認待ち。#83 で形だけ記録） | 画面は Hub。TASK-664 は移管（→ Hub `ASCR-1`・`ARUN-17`） |
| 9 | `fandhe-db` | PostgreSQL の参照実装（`dev/postgres/`・TASK-5） | TASK-628（契約 trait は TASK-584） | 差し替えの境界は契約 trait（置き場は未定: DPOL-38・TASK-584・TASK-684）。契約テストは `crates/db-tests`。#84 で形だけ記録 | 本 repo は契約と契約テストまで（DPOL-37・DPOL-38）。`fandhe-db` 向けの実装は Hub の担当（LIB-14） |

## この置き場に足すもの

TASK-9 の sub-issue で順に足す。文書は本ディレクトリに、コードは各 crate に置く。

| Issue | 内容 |
| --- | --- |
| #81 | 本置き場と台帳（本文書） |
| #82 | backend の枠 |
| #83 | frontend の枠 |
| #84 | db の枠 |
| #85 | 版の固定の記録の形式 |
| #86 | 差し替えの手順 |
| #87 | 契約テストを複数の DB 実装で流す枠 |
| #88 | 空の実装での確認 |

## 未定の事項（人が決める）

- `backend` の中の module の構成。#82 で暫定の形を置いた。確定は人
- `frontend` の crate を作るか。中の置き場は #83 で案を置いた（末尾の節）。作るかは人が決める
- 契約 trait の置き場（TASK-584・TASK-684 で決める）。独立 crate `fandhe-auth-db-contract` か `fandhe-auth-core` の中かは、
  PoC-15 の後に LIB-15 の基準で人が決める（DPOL-38）
- 固定する版の値（#85 は形式だけを作る。値は各差し替え TASK で人が決める。`A-2026-10-16-5`）
- この repo の版の固定の記録と、`A-2026-10-16-5` が指す spec 側（private）の表との関係。spec への反映は利用者の指示を待つ。
  本形式で spec 側の記録を満たしたとは書かない
- 記入済みの版の固定の記録をこの public repo に置くか

## backend の切り出しの記録（LIB-15。判定前の材料）

判定は人が行う（ACPT-7）。基準 (1)〜(4) は LIB-15 の番号で引く（本文は写さない）。

- 材料: workspace 内の依存 0・外部の依存 0。暗号の処理・I/O・環境変数の読み込みを持たない。
  Gate の登録・実行・HTTP の受付は持たない（`D-2026-10-30-1`）。
- 判定 (1): 未記入（人が記入）
- 判定 (2): 未記入（人が記入）
- 判定 (3): 未記入（人が記入）
- 判定 (4): 未記入（人が記入）

## frontend の枠（作成前の形。#83）

crate は作っていない。`frontend` の追加は段階 2 で（LIB-14）、LIB-15 の記録が要り、利用者の決定の記録も
リポジトリに無いため、利用者の確認を待つ。下は #82 の backend と同じ形をとる案である。

形（案）:

- crate 名 `fandhe-auth-frontend`、ディレクトリ `crates/frontend`（`D-2026-10-27-8`。`.claude/rules/coding-rust.md` の表）。
- `Cargo.toml` は backend と同じく `*.workspace = true` と `[lints] workspace = true`。workspace 内・外部の依存は 0。
  `fandhe-frontend` への依存と版の固定は、差し替えの TASK と #85 の形式（末尾の「版の固定の記録の形式」の節）で行う。
- `src/lib.rs` は `#![forbid(unsafe_code)]` と `pub mod interim;`。
- `src/interim/mod.rs` を差し替えの境界にし、台帳の行 8 の 1 部品だけを扱う。
- 子 module `src/interim/builder.rs` は module doc と `// TODO(TASK-532): …` だけにし、中身を持たない。

境界の規則（`crates/backend/src/interim/` と同じ）:

- builder は `interim` の子 module からだけ参照する。
- 暫定版と提供版を feature・環境変数・設定で切り替える経路は作らない（DPOL-30〜32・CSRF-7）。
- 差し替えの前に再試験を流す（CG-6）。手順は末尾の「差し替えの手順」の節（#86）、空の実装での確認は #88。

frontend に固有の制約: サーバーのビルドに `web-sys` を入れない（DPOL-40）。`fandhe-auth-db*` に依存しない（DPOL-7）。

作るときの手順（利用者の確認後）:

- workspace の `members` への追加とコメント、`Cargo.lock` の更新。
- `scripts/check-crate-deps.sh` の許可表に依存なしの行を足す。
- 本文書の台帳の行 8 の「（案）」を外す。

LIB-15 の判定前の材料。判定は人が行う（ACPT-7）。基準 (1)〜(4) は LIB-15 の番号で引く（本文は写さない）。

- 材料（作成した場合の見込み）: workspace 内の依存 0・外部の依存 0。暗号の処理・I/O・環境変数の読み込みを持たない。
- 判定 (1): 未記入（人が記入）
- 判定 (2): 未記入（人が記入）
- 判定 (3): 未記入（人が記入）
- 判定 (4): 未記入（人が記入）

## db の枠（作成前の形。#84）

crate・trait は作っていない。契約 trait の置き場は DPOL-38 で未定（独立 crate か `fandhe-auth-core` の中か。
PoC-15 の後に LIB-15 の基準で人が決める）で、中身は TASK-584、crate の分け方の検査は TASK-684 の範囲のため、
先取りしない。下は差し替えの境界の考え方を文書で示すもので、trait の名前・シグネチャは定めない。

差し替えの境界（案）:

- 呼び出し側（`core`・将来の Hub 側のハンドラ）は契約 trait にだけ依存する。
- DB の実装は契約 trait を実装する側に置く。差し替えは実装の入れ替えと版の固定（`A-2026-10-16-5`）で行う。
- 契約 trait の置き場は DPOL-38 の 2 案のどちらでも成り立つように書く。どちらにするかはここで決めない。

実装の側の扱い:

- PostgreSQL の参照実装は台帳の行 9 のとおり（`dev/postgres/`・TASK-5）。
- `fandhe-db` 向けの実装・`-db-pg`・`-db-fandhe`・`-db-migrate` は Hub の担当で、本体の workspace のメンバーにしない
  （LIB-14・DPOL-38）。`fandhe-db` 向けの実装の枠と点検の記録の様式は TASK-628 の範囲で、ここでは作らない。

境界の規則（backend・frontend と同じ）:

- 実装を feature・環境変数・設定で実行時に切り替える経路は作らない（DPOL-30〜32）。
- 差し替えの前に再試験を流す（CG-6）。手順は末尾の「差し替えの手順」の節（#86）。
- 契約 trait の置き場は SQL・ロック・誤りコードを持たない（DPOL-37・AUDB-3）。契約テストの本体は PostgreSQL の実装にも
  `sqlx` にも依存しない（DPOL-37）。

db に固有の安全条件:

- `fandhe-db` への移行は ADB-1〜26 の実測まで行わない（CG-7）。
- PostgreSQL での合格を `fandhe-db` が契約を満たす根拠にしない（TASK-628）。
- 契約テストを通すために期待・安全条件を緩めない（AUDB-6・AUDB-69）。

兄弟の範囲: 版の固定の記録の形式は #85（末尾の「版の固定の記録の形式」の節）、差し替えの手順は #86、契約テストを複数の DB 実装で同じ trait から流す枠は #87、
空の実装での確認は #88。中身を先取りしない。

作るときの手順（利用者の決定の後。この Issue では実行しない）:

- 独立 crate に決まった場合: workspace の `members` への追加とコメント、`Cargo.lock` の更新、
  `scripts/check-crate-deps.sh` の許可表に `fandhe-auth-db-contract` の行を足す（`fandhe-auth-db-tests` からの辺を含めるかは
  そのとき確かめる）。
- `fandhe-auth-core` の中に決まった場合: 許可表の変更は不要の見込み。
- いずれも本文書の台帳の行 9 の「置き場は未定」の記述を直す。

LIB-15 の判定前の材料（独立 crate にする場合だけ要る）。判定は人が行う（ACPT-7）。基準 (1)〜(4) は LIB-15 の番号で引く
（本文は写さない）。

- 判定 (1): 未記入（人が記入）
- 判定 (2): 未記入（人が記入）
- 判定 (3): 未記入（人が記入）
- 判定 (4): 未記入（人が記入）

## 版の固定の記録の形式（#85）

相手側の部品を公開された版で固定し、どの版から使えるかを記録してから依存を上げるための形式である（`A-2026-10-16-5`）。
提供された版を使う前に、その版を固定したうえで再試験を流す（CG-6）。依存の版は `Cargo.lock` でコミットする
（`.claude/rules/dependency-policy.md` の「版」の節）。この節は欄と規則と空の表だけを置く。版の値は各差し替え TASK で
人が決める（ACPT-7）。

使う TASK: TASK-372（台帳の行 7）・TASK-90（行 8）・TASK-628（行 9）・TASK-527（行 1〜6）。行 1〜6 は TASK-527 が
置き換え（→ Hub `ARUN-17`）で、この repo が固定を行うとは読まない。台帳の「この repo での扱い」の列に従う。

### 欄の定義

| 欄 | 書く内容 | 記入者 |
| --- | --- | --- |
| 記録番号 | `V-1` から始める連番。削除した番号は欠番のままにして再利用しない | 人 |
| 台帳の行 | 差し替え対象の一覧の `#`（1〜9） | 人 |
| 差し替えの TASK | `TASK-N`（他 repo のものは repo 名を付ける） | 人 |
| 提供元 | repo 名だけ（例: `Fandhe-AI/<repo>`） | 人 |
| 部品 | crate 名と有効にする feature。`fandhe-db` はサーバーの成果物の種類 | 人 |
| crate の版 | 公開された crate の版。当てはまらなければ「—」 | 人 |
| tag | 公開された tag。当てはまらなければ「—」 | 人 |
| commit | 公開された版がどの commit に当たるかの記録。当てはまらなければ「—」 | 人 |
| 固定の場所 | crate は `Cargo.lock`。`fandhe-db` はサーバーの成果物の版（Cargo の依存ではない） | 人 |
| 使える版 | どの版から使えるか（`A-2026-10-16-5`） | 人 |
| 公開日と確かめた方法 | tag・CHANGELOG・crates.io の版の一覧など。所有者の回答か、こちらが確かめた事実かを区別する | 人 |
| 出典 | `D-` の ID、相手側の Issue・PR の番号。本文は写さない | 人 |
| BREAKING CHANGE | 有無と、影響する箇所の参照だけ | 人 |
| 記録日 | 記録した日。依存を上げた日より前でなければならない | 人 |
| 依存を上げた PR・commit | 記録日より後であることを、この欄で後から確かめる | 人 |
| 依存の監査 | `cargo deny`・`cargo audit` の合否と、advisory DB の commit・取得日（DPOL-33・DPOL-34）。結果の本文は書かない | 人 |
| 再試験の記録先 | 流した試験の範囲（PoC の手順の番号・ACPT の ID 等）と記録へのリンクだけ（CG-6）。中身は各差し替え TASK の範囲 | 人 |
| 判定 | 版を使ってよいかの判定、記入者、日付 | 人だけ |

commit の欄は、公開された版の識別のためだけに置く。リポジトリ直接の依存を許すものではない（DPOL-25: sources は crates.io だけ）。

### 記入の規則

- 1 回の固定につき 1 行を追記する。同じ部品を上げ直すときも新しい行を足し、古い行は書き換えない。
- 行を記録してから依存を上げる（`A-2026-10-16-5`）。再試験の前に差し替えない（CG-6）。手順は末尾の「差し替えの手順」の節（#86）。
- 実測の前の値を実測済みとして記録しない。`fandhe-db` の実測の記録には対象の版を残す（TASK-628・AUDB-6）。
- 試験・AI のレビューの合格で判定の欄を埋めない（ACPT-7・CG-2）。
- 秘密値・実機の名前・ホームのパス・内部のアドレスを書かない（`.claude/rules/security.md`）。
- 他所有者の本文・値は写さず、相手の repo・ID で引く（`.claude/rules/spec-reference.md`）。

### 記録表（空）

| 記録番号 | 台帳の行 | 差し替えの TASK | 提供元 | 部品 | crate の版 | tag | commit | 固定の場所 | 使える版 | 公開日と確かめた方法 | 出典 | BREAKING CHANGE | 記録日 | 依存を上げた PR・commit | 依存の監査 | 再試験の記録先 | 判定 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| （未記入） | | | | | | | | | | | | | | | | | |

## 差し替えの手順（#86）

暫定の実装を、相手側が公開した版に差し替えるときの手順である。根拠は `A-2026-10-16-5`（版の固定）・`CG-6`（差し替え前の再試験）・
`CG-7`（`fandhe-db` の実測）。差し替えの TASK は台帳の「差し替え・再試験の TASK」の列で引く。
版の値・試験の中身・相手側の本文は書かない。判定・確定・例外の承認は人が行う（ACPT-7・CG-2）。

### 適用範囲と担当

| 台帳の行 | 差し替えの TASK | この repo が行うこと | 相手側が行うこと |
| --- | --- | --- | --- |
| 1〜6 | TASK-527（置き換え → Hub `ARUN-17`） | 記録の参照だけ。差し替えも版の固定も行わない | 部品の版の固定と再試験（`.claude/rules/dependency-policy.md` の「版」の節） |
| 7 | TASK-372（再試験は TASK-89） | `crates/client` の差し替えと版の固定 | 提供元は公開の版を出すまで |
| 8 | TASK-90 | frontend の crate を作った後に、`interim` の差し替え（crate は未作成。作る前に利用者に確かめる） | 画面は Hub |
| 9 | TASK-628 | 契約テストを流す対象の差し替えまで（Cargo の依存ではない。枠は #87） | `fandhe-db` 向けの実装は Hub（LIB-14） |

未決: 差し替えのとき、どの再試験の範囲を必須にするか。#86 は手順の形だけを置き、範囲の確定は人が行う。

### 着手の条件（1 つでも欠けたら止めて人に知らせる）

- 相手側の公開の版（tag・crate の版・commit）がある（`A-2026-10-16-5`）。未公開の main・git 依存は使わない（DPOL-25）。
- `crates/backend` に依存や中身を足す場合は、「backend の切り出しの記録」の判定 (1)〜(4) の全欄に人の「満たす」の記入がある
  （`D-2026-11-20-3` の (ア)）。frontend・db を独立 crate にする場合も、それぞれの節の LIB-15 の判定が要る。
- 差し替えの TASK の担当が、台帳の「この repo での扱い」の列で本体側にある。

### 手順（順番どおり）

1. 台帳の行・差し替えの TASK・提供元を特定する。
2. 「版の固定の記録の形式」の表に新しい行（`V-n`）を追記する。依存を上げる前に記録日を入れる（`A-2026-10-16-5`）。
    判定の欄は空のままにする（記入は人）。
3. 差し替えの前の再試験（基準線）を暫定の実装で流し、記録先を控える。範囲は `CG-6` と各差し替え TASK が指す ID で引く
    （行 8 は ACPT-17、行 9 は ADB の契約テスト。試験の中身は各 TASK の範囲で、ここでは書かない）。
4. 作業ブランチで依存を足す。版と feature を明示し、`--all-features` は使わない（DPOL-31）。sources は crates.io だけ（DPOL-25）。
    `Cargo.lock` をコミットする。行 9 は Cargo の依存ではなく、契約テストを流す対象の実装を替える。
5. 書き換えは境界の子 module（例: `crates/backend/src/interim/<部品>.rs`）の中に限る。境界の外から提供元の crate を直接 `use` しない。
    暫定版と提供版を feature・環境変数・設定で切り替える経路を作らない（DPOL-30〜32・CSRF-7）。
    `TODO(TASK-n)` は消さずに移すか、片づいたものだけ消す。
6. 下の「差し替えの後の確認コマンド」を流す。
7. 差し替えの後に再試験を流し、基準線と同じ期待が成り立つかを比べる（CG-6）。期待・安全条件は緩めない（AUDB-6・AUDB-69）。
    行 9 では、PostgreSQL での合格を `fandhe-db` が契約を満たす根拠にしない（TASK-628）。ADB-1〜26 の実測までは移行せず（CG-7）、
    実測の前の値を実測済みとして書かない。
8. 版の固定の記録の行の「依存を上げた PR・commit」「依存の監査」「再試験の記録先」を埋める。「判定」の欄は人が記入する。
    AI・自動処理は埋めない（ACPT-7・CG-2）。
9. PR の本文に `TASK-N`・`V-n`・引いた ID を書く。満たしていない期待は ID 付きで残りとして書く。

### 差し替えの後の確認コマンド

| コマンド | 合格の条件 |
| --- | --- |
| `make check` | 終了コード 0（editorconfig-checker・shellcheck） |
| `make lint` | 終了コード 0（`cargo fmt --check`・`cargo clippy --workspace --all-targets --locked -- -D warnings`・crate 間の依存の検査） |
| `make build` と `make test` | 終了コード 0、失敗 0（`--locked`） |
| `git grep -nE 'fandhe_backend\|fandhe_frontend' -- 'crates/' ':!crates/*/src/interim/'` | 一致なし（境界の外から直接使っていない） |
| `cargo tree --locked --workspace -e normal,build --target all -i <提供元の crate>` | 逆依存の根が、境界を持つ crate だけ |
| `git diff origin/main -- '*.toml'` と `git grep -nE 'cfg\(feature' -- 'crates/'` | 暫定と提供の切り替えの feature が足されていないことを目視 |

- `make crate-deps` が見るのは `fandhe-auth-*` の間の辺だけである。外部 crate の検査は未実装（`TODO(TASK-714)`）で、外部依存の検査として数えない。
- 依存の監査は `.claude/rules/dependency-policy.md` の「依存を足す・更新するときの確認手順」のコマンドを正として流す。ここへ複製しない。
  `deny.toml` は未作成（TASK-668）で、現時点の `cargo deny` は本番の基準では流せない。
  `cargo tree -i` は一致が無いと非 0 で終わるため、合否は終了コードではなく出力の文言（`did not match any packages`）で決める。
  `Cargo.lock` は依存を足す差し替えでは必ず差分が出る。差分が意図した crate だけかを目視で確かめる。

### 戻し方

差し替えの PR を revert する。実行時の切り替えは作らない。戻した場合も版の固定の記録の古い行は書き換えず、
新しい行（`V-n`）を追記して戻したことを記録する。

### 書かないもの

版の値・試験の中身・相手側の本文や値・秘密値・実機の名前・ホームのパス・内部のアドレス（`.claude/rules/security.md`・`.claude/rules/spec-reference.md`）。
