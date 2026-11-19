# 暫定の実装の置き場と差し替え対象の台帳（TASK-9）

`fandhe-backend`・`fandhe-frontend`・`fandhe-db` の部品は、提供されるまで自前の暫定の実装で代える。
暫定の実装は多くの TASK に散らばるため、どの部品を、どの TASK で作り、どの TASK で差し替えるかを
このディレクトリの台帳 1 か所で追えるようにする。対象は `TASK-9`（Issue #80・#81）。

根拠の ID: `A-2026-10-16-5`（版の固定）・`CG-6`（差し替え前の再試験）・`D-2026-10-30-1`（Gate の担当）。
spec の本文・版の数値は写さず、ID で引く（`.claude/rules/spec-reference.md`）。
差し替えの完了や各項目の「確定」の判定は人が行う（ACPT-7）。

## 置き場の規則

- 本ディレクトリは文書だけを置く。暫定の実装のコードは、その部品を使う crate の中に置き、下の台帳の行から引く。
  `backend` の配置は `crates/backend` で決定済み（利用者の決定 2026-11-13。`.claude/rules/coding-rust.md`）。
  それ以外のコードの具体のパスはここで決めない。
- 暫定の実装と提供版を、feature・環境変数・設定で実行時に切り替える経路は作らない（DPOL-30〜32・CSRF-7）。
  差し替えはコードの変更と版の固定で行う。
- 差し替えの前に再試験を流す（CG-6）。
- 暫定コードの module doc と `TODO(TASK-n)` には、中身を作る TASK と差し替える TASK を書く
  （`.claude/rules/code-comment-style.md`・`.claude/rules/out-of-scope-tracking.md`）。
- `backend`（`fandhe-auth-backend`・`crates/backend`）は #82 で枠（`crates/backend/src/interim/`）を作った。
  暫定の実装の中身は未作成。module の構成は暫定で、人が確定する。
- `frontend`（`fandhe-auth-frontend`・`crates/frontend`）の枠の形は #83 で本文書の末尾の節に案として記録した。
  crate は未作成で、作る前に利用者に確かめる（LIB-14・LIB-15）。`frontend` 以外の段階 2 の crate も同じ。

## 差し替え対象の一覧

各行の内容は spec の原文で確かめること。ステータスはいずれも「暫定の実装は未作成」「差し替えは未実施」。

| # | 提供元の部品 | 暫定の実装を作る TASK | 差し替え・再試験の TASK | コードの置き場 | この repo での扱い |
| --- | --- | --- | --- | --- | --- |
| 1 | `fandhe-backend` CSRF の Gate | TASK-523 | TASK-527 | `crates/backend/src/interim/csrf_gate.rs`（枠は #82 で作成） | TASK-527 は置き換え（→ Hub `ARUN-17`）。Gate の登録・実行は Hub（`csrf-ssr.md` の適用範囲・`D-2026-10-30-1`） |
| 2 | `fandhe-backend` 安全な既定値の `SetCookie` | TASK-214（目安。認証 Cookie の発行・消去の TASK で、部品そのものの TASK ではない） | TASK-527 | `crates/backend/src/interim/set_cookie.rs`（枠は #82 で作成） | 同上（差し替えの実施は Hub の担当） |
| 3 | `fandhe-backend` 同一オリジンのリダイレクト先 | TASK-533 | TASK-527 | `crates/backend/src/interim/local_redirect.rs`（枠は #82 で作成） | 同上（差し替えの実施は Hub の担当） |
| 4 | `fandhe-backend` `no-store` の補助 | TASK-530 | TASK-527 | `crates/backend/src/interim/no_store.rs`（枠は #82 で作成） | 同上（差し替えの実施は Hub の担当） |
| 5 | `fandhe-backend` フォームの上限 | TASK-534 | TASK-527 | `crates/backend/src/interim/form_limit.rs`（枠は #82 で作成） | TASK-534・TASK-61 は移管（→ Hub `ARUN-46`・`ARUN-47`） |
| 6 | `fandhe-backend` 信頼するプロキシ | TASK-535・TASK-536 | TASK-527（提供された場合）。現時点は提供なし（`D-2026-10-27-2`） | `crates/backend/src/interim/trusted_proxy.rs`（枠は #82 で作成） | 提供されるまで自前の accept ループで代える（`D-2026-10-16-19` の (1)・`D-2026-10-27-2`）。差し替えの実施は Hub の担当（TASK-527 は置き換え → Hub `ARUN-17`） |
| 7 | `fandhe-backend` 下流の JWT の検証（`REQ-16`） | TASK-370 | TASK-372（再試験 TASK-89） | `crates/client`（段階 1・既存） | 本 repo の範囲。#82 では触らない（中身は TASK-370） |
| 8 | `fandhe-frontend` の builder（`REQ-17`） | TASK-532 | TASK-90（TASK-152 は移管 → `UISF-10`・`UISF-22`） | `crates/frontend/src/interim/builder.rs`（案。crate の作成は利用者の確認待ち。#83 で形だけ記録） | 画面は Hub。TASK-664 は移管（→ Hub `ASCR-1`・`ARUN-17`） |
| 9 | `fandhe-db` | PostgreSQL の参照実装（`dev/postgres/`・TASK-5） | TASK-628（契約 trait は TASK-584） | 契約 trait の置き場は未定（TASK-584・TASK-684）。契約テストは `crates/db-tests` | 本 repo は契約と契約テストまで（DPOL-37・DPOL-38）。`fandhe-db` 向けの実装は Hub の担当（LIB-14） |

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
- 契約 trait の置き場（TASK-584・TASK-684 で決める）
- 固定する版の値（#85 と各差し替え TASK で決める。`A-2026-10-16-5`）

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
  `fandhe-frontend` への依存と版の固定は、差し替えの TASK と #85 の形式で行う。
- `src/lib.rs` は `#![forbid(unsafe_code)]` と `pub mod interim;`。
- `src/interim/mod.rs` を差し替えの境界にし、台帳の行 8 の 1 部品だけを扱う。
- 子 module `src/interim/builder.rs` は module doc と `// TODO(TASK-532): …` だけにし、中身を持たない。

境界の規則（`crates/backend/src/interim/` と同じ）:

- builder は `interim` の子 module からだけ参照する。
- 暫定版と提供版を feature・環境変数・設定で切り替える経路は作らない（DPOL-30〜32・CSRF-7）。
- 差し替えの前に再試験を流す（CG-6）。手順は #86、空の実装での確認は #88。

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
