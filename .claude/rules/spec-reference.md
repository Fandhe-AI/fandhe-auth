---
description: >
  fandhe-auth の spec（docs/spec submodule、private の Fandhe-AI/fandhe-auth-spec）の取得手順と読み方。常に読み込む。
  読み替えの規則（D-2026-10-29-1）、正本（04-behavior/）と追随の順、ID 規約と出典の書き方、コード・コメント・PR に
  ID を書くこと、他所有者の値を写さないこと、「確定／検討中」を AI が確定にしないこと（ACPT-7）、spec を直すときの扱い、
  現在のフェーズとタスクの調べ方（06-roadmap.md・05-tasks.md）を定める。
---

# spec の参照ルール

この repo（`Fandhe-AI/fandhe-auth`、public）は実装 repo で、仕様は private の `Fandhe-AI/fandhe-auth-spec` を
`docs/spec` の submodule で参照する（`D-2026-10-29-1`）。

## 取得

`.gitmodules` は `update = none` のため、`git submodule update --init` だけでは取得しない。権限のある環境で次を実行する。

```bash
git -c submodule.docs/spec.update=checkout submodule update --init docs/spec
```

- spec repo は private。権限が無い環境（CI・外部の貢献者）では取得できない前提で手順・検査を組む。
- この repo は public。spec の本文をどこまで写してよいかは spec に規定が無いため、まとまった転記が要るときは利用者に確認する。

## 構成と正本

| 場所 | 中身 |
| --- | --- |
| `docs/spec/04-behavior/` | ビヘイビア定義。**唯一の正（SSOT）**。`README.md` に ID 規約・Conditional Go の条件（CG2-1〜CG2-5。旧 CG-1〜13 は旧構成の条件として保全されている） |
| `docs/spec/05-tasks.md` | `TASK-N`（担当・前提タスク・成果物） |
| `docs/spec/06-roadmap.md` | マイルストーン `MS-N`・着手判定・追記の履歴 |
| `docs/spec/01-brainstorm.md` | 決定 `D-`・仮定 `A-`・未決 `Q-` |
| `docs/spec/02-poc-plan.md`・`03-poc/` | PoC 計画と結果（PoC-1〜18、QP-1〜5。PoC-15〜18 は新しい構成の分） |
| `docs/spec/migration/library-boundary-map.md` | 各ビヘイビアの本体／Plugin／Hub の分類と移管先（`D-2026-10-30-1`） |
| `docs/spec/spec.md` | `04-behavior/` から `/generate-spec` で自動生成。手で直さない |

- 仕様の変更はまず `04-behavior/` を直し、`05-tasks.md` と `spec.md` を後から追随させる（04-behavior/README.md 冒頭、05-tasks.md の方針）。
- 新しい構成で足したファイル: `library-boundary.md`（LIB）・`host-ports.md`（HOST）・`plugin-contract.md`（PLG）・`identity-link.md`（IDLK）・
  `auth-methods.md`（AMTH）・`ui-safety.md`（UISF）。本体の境界の読み方は `./library-boundary.md`。
- 追跡の正は `05-tasks.md` の `TASK-N` と `06-roadmap.md` の MS（`D-2026-10-28-4`）。
- 引くときは原文を開いて確かめる。要約や記憶から値を書かない。

## 読み替え（`D-2026-10-29-1`）

- 利用者の決定は repo 名だけ: 実装 repo は `fandhe-auth`、spec repo は `fandhe-auth-spec`。
- 次は書き手の読み（06-roadmap.md の 2026-10-29 追記（19））:
  - 「`fandhe-auth-backend` の workspace／repo」→「`fandhe-auth` repo の workspace」
  - 「`fandhe-auth-backend`（のハンドラ・認証サービス等）」→「`fandhe-backend` の上に作る認証サーバー側の crate」
- 2026-10-30 の読み替え（`A-2026-10-30-12`。〔提案〕）: 本文の「`fandhe-auth` が〜を返す・拒否する・記録する」は「本体の判定が決め、組込み先が受付から返す・記録を実行する」と読む。
  「認証サーバー」は「組込み先の認証の実行」、「`fandhe-auth-backend` の DB」は「組込み先のストレージ実装」、`fandhe-auth-backend` のハンドラは Hub のハンドラと読む
  （`screen-invite-recovery.md` の適用範囲）。読み替えで意味が変わる項目は原文を確かめる。
- crate `fandhe-auth-backend`（`crates/backend`）の名前は、利用者の回答として `D-2026-10-29-2` に記録済み（`D-2026-11-06-6`）。上の読み替えは名前の記録とは別で、
  crate の役割は段階 2 の任意の接続部品（段階は〔提案〕。`LIB-14`・`A-2026-10-30-9`。`./coding-rust.md`）。2026-11-13 の利用者の決定（`unsafe` 禁止など）は spec に記録が無い。
- 記録した時点の文は書き換えず、日付付きの追記と読み替えの注記を足す運用（04-behavior/README.md）。

## ID と出典

- Hub に移った行は「X（→ Hub `Y`）」と読む。各ファイル末尾の「移管・置き換えた ID」の表が正（`A-2026-10-30-12`・`A-2026-10-30-13`）。移管・置き換えの ID は欠番として再利用されない。
  規約・コメントでこの ID を引くなら、付記するか引用から外す。Hub の行の値・本文は写さず、ID だけで引く。
- 行の 2026-10-30 の追記で「本体の部分／実行は Hub `X`」と分けた行（行内分割）は本体に残る。「本体の要求。実行は Hub（`X`）」と書き分ける。

- ビヘイビア ID は `<PREFIX>-<N>`（例: `PKEY-1`）。削除した番号は欠番のまま再利用せず、新規はファイル内の最大 +1（04-behavior/README.md の ID 規約）。
- 各項目は前提・操作・期待の形。優先度は Must／Should／Could。
- 出典: 決定 `D-YYYY-MM-DD-N`・仮定 `A-…`・未決 `Q-…`。Hub の決定は必ず `Hub D-…`（同じ日付で番号が重なる）。
- `TASK-N`・`MS-N`・`REQ-N` が他 repo のものなら repo 名を付ける（例: `fandhe-db` の `TASK-336`、`fandhe-backend-spec` の `REQ-16`）。
- `ADB-N` は認証 DB の契約テスト項目でビヘイビア ID ではない（ID 規約 (e)）。

## コード・PR での引き方

- 規則を実装・試験した箇所のコメント・doc コメント・試験名に、根拠の ID を書く（例: `// THRT-25: 失敗理由は login_failed の 1 種類`）。
- PR 本文に対象の `TASK-N` と、実装・試験したビヘイビア ID を書く。満たしていない期待があれば ID 付きで残りとして書く。
- spec に無いことを規約や仕様として断定しない。未定は「未定（TASK-n で決める）」と書き、決める人に伺う。

## 他所有者の値

- Hub・`fandhe-backend`・`fandhe-frontend`・`fandhe-db` が正本を持つ値・本文は写さず、相手の repo・commit・ID で引く（ID 規約 (b)、`D-2026-10-15-21`）。
- 相手側が未決の値は設定で受け、試験用の仮の値で実装する。本番の値を推測で置かない（`A-2026-10-28-1`〜`14`）。

## 確定と検討中

- ステータスは「確定／検討中」の 2 値。確定は PoC・QP で実際に確かめた範囲だけ。根拠欄に〔実機〕〔仮想〕〔モック〕〔資料〕等の印（ID 規約 (a)）。
- 冒頭に **確定の範囲** がある項目は、その範囲の外を確定と読まない。PoC のパラメータの値（60 秒・300 秒など）は要件の値ではない。
- 〔仮想〕の合格で〔実機〕〔人の操作〕の項目を確定にしない。検討中から確定への変更は人が記録する。AI・自動処理が確定にする件数 0。
  試験の実行と記録の収集は自動でよい（ACPT-7）。
- 結果の判定・値の決定・例外の承認も人が行う。Claude Code の担当は手順・仕組み・試験を作って流すところまで（06-roadmap.md の読み方）。

## spec を直したいとき

- 実装中に spec の誤り・欠けを見つけたら、ID と根拠を添えて利用者に知らせる。この repo の `docs/spec` の中を直接編集しない。
- 直すのは `Fandhe-AI/fandhe-auth-spec` への PR になると読めるが、具体の手順は spec に無い。PR の作成・push・他 repo への投稿は、利用者の明示の指示を待つ。
- 直すときも `04-behavior/` を先に直し、`05-tasks.md`・`spec.md` を追随させる。記録時点の文は書き換えず追記する。

## 現在のフェーズとタスクの調べ方

- `docs/spec/README.md` の「ステータス」表でフェーズ（Phase 1〜6 は、2026-10-30 の方針変更で Phase 2 へ逆戻りした後、Phase 3〜6 をやり直して完了している。着手判定は「着手」〔`D-2026-11-04-6`。
  本番の開始日は MS-1 の後に見直す〕。実装の開始は指示待ち。Issue ツリーは起票済みで、最新の spec へ再同期した〔`./out-of-scope-tracking.md`〕）。
- `docs/spec/06-roadmap.md` の「マイルストーン一覧」と各 `### MS-N` の「含まれるタスク」で、今の MS と関門を見る。
  MS-1（決定と土台）の完了が MS-2 の開始条件。基盤・CI・stub・`rsa` を入れない検証部（TASK-672）は関門の前に進めてよい。
- `docs/spec/05-tasks.md` で `#### TASK-N` を引き、内容・成果物・前提タスク・担当（Claude Code／共同／人間）を確かめる。
  担当が人間・共同のタスクは、Claude Code が下書き・材料までにとどめる。
- 最新の状況は 06-roadmap.md の末尾の日付付きの追記で確かめる。
