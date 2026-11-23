# 人のセキュリティレビューの資料（範囲の各行の参照先と確認の観点）

`TASK-82`（人のセキュリティレビューの資料と記録票を整える）の 2/4。対象ビヘイビアは `ACPT-12`（CG-2）。
`TASK-83` で利用者本人が行う人のセキュリティレビューで、範囲の各行について「どこを読むか」と「何を確かめるか」を示す。
範囲の行と対応ビヘイビア ID は [scope-ids.md](scope-ids.md) の表を出典とし、ここでは ID の一覧を写さない。

## AI の結果は参考であり、判定ではない

- 本資料は **AI が作った資料であり、判定ではない**。判定（確認済み・指摘あり・未対応）・日付・判定者の記録は、
  この資料には書かない。記録は 3/4 の記録票（[record-form.md](record-form.md)）に、人が書く（`TASK-83`・ACPT-7）。
- QP-1 自体が AI のレビューのみである（人のレビューは行っていない。`D-2026-10-16-21` の (1)）。
  QP-1 の T 表の「判定」列と、4 節の「推奨」は **参考** として読む。
- `security-auditor` など、この repo で AI が行う監査・レビューの結果も参考である。
  AI のレビューの合格を安全性の根拠にしない（CG-2・`D-2026-10-15-20`・`ACPT-12` の操作と期待・`ACPT-6`）。
- 候補 ID の採否、判定、spec のステータスを「確定」にする変更は人が行う。AI・自動処理が確定にした件数は 0（ACPT-7）。
- spec の誤り・欠けを見つけたら、ID と根拠を添えて利用者に知らせる。この repo の `docs/spec` は直さない
  （`.claude/rules/spec-reference.md`）。

## 導出元と写さないもの

| 項目 | 内容 |
|---|---|
| spec repo と commit | `Fandhe-AI/fandhe-auth-spec`（private）の `4987777db3db59064e7fe2d1d5c7302d0f719847`（[scope-ids.md](scope-ids.md) と同じ。2026-11-21 に利用者が `TASK-83` の着手を指示した時点の spec の commit。`D-2026-11-21-22`） |
| 範囲と対応 ID | [scope-ids.md](scope-ids.md)。範囲の定義は QP-1 README の「6. 利用者の判断が要る点」の判断 3 |
| `ACPT-12` の行 | `04-behavior/acceptance-retest.md`。2026-11-21 の追記（`D-2026-11-21-20`）が、T-16 の対応 ID の行き先を訂正している（`DVUI-20` は Hub `ASCR-42`、`DVUI-21` は `UISF-37`・Hub `ASCR-37`）。既存の文は変わっていない。`DGRT-29` の実施の担当は Hub `ASCR-84` と注記されている |
| 移管した ID | 各ファイル末尾の「移管・置き換えた ID」の表で移管先を確かめる。Hub の ID は、この repo のレビューの範囲外（記録票に「移管 → Hub の ID・この repo のレビューの範囲外」と書く）。Hub の本文・値は写さない |
| 取得手順 | [README.md](../../README.md) の「仕様」節。この repo は public で spec は private のため、権限のある環境で取得する |

この repo は public である。次のものは写さず、`T-N`・`R-N`・節の番号・ID で引き、本文は spec の原文で読む。

- 脅威の名前・指摘の本文・推奨の本文
- PoC の数値・件数、PoC のコードのファイルパスと行番号
- 実機の名前（環境の記録は引かない）

行ごとの表の PoC の手順番号・`ADB-N` は、本文ではなく読む位置を示す参照であり、許容する。
下の観点は、spec の本文を言い換えず、人への問いの形で書く。

## レビューの準備と読む順

1. spec を固定の commit で取得する。この repo の `docs/spec`（submodule）が指す commit は、固定の commit と違うことがある（2026-11-21 の時点で、固定は `4987777`、submodule は `5207d5e`）。固定の commit `4987777` は、比べる元と、範囲・対応 ID・区分の出典として残す（`D-2026-11-24-4` の (3)(c)。出典の位置づけは同 (4)(d)〔書き手の読み〕）。証拠と決定を読む版は、同 (3)(c)(d) に従う。固定の commit の原文は、C-3 のために、[`.claude/rules/spec-reference.md`](../../.claude/rules/spec-reference.md) の手順で取得した後に `git -C docs/spec show 4987777db3db59064e7fe2d1d5c7302d0f719847:<ファイルのパス>` で開く（submodule の作業ツリーは変えない。commit が無いときは先に `git -C docs/spec fetch` で取る）。
2. 記録票の判定語「確認済み」「指摘あり」「未対応」の定義と付け方は `D-2026-11-24-3` の (3) で、記入の前に伺う行と歯止めは同 (5)(a)(b)(c) で確かめる。記録票が引く決定 `D-2026-11-21-20`・`D-2026-11-21-22`・`D-2026-11-21-24`・`D-2026-11-24-2`・`D-2026-11-24-3`・`D-2026-11-24-4` のうち、`D-2026-11-21-22`・`D-2026-11-21-24`・`D-2026-11-24-2`・`D-2026-11-24-3`・`D-2026-11-24-4` と、`D-2026-11-21-20` の日付付きの追記は固定の commit に無い（固定の commit の `01-brainstorm.md` で確かめた範囲）。spec repo の `01-brainstorm.md` で読む。読む版は `D-2026-11-24-4` の (3)(d)。開始日の後に決まった決定は同 (5)(b)（未決）。決定を読んだ spec の commit は、C-3 の「比べた commit」とは別の欄である、記録票の前提の表の「判定語・記録の扱いの決定を読んだ spec の commit」に書く（欄は書き手の様式）。`scope-ids.md`・`gap-check.md` の `5207d5e` は記録した時点の commit である。R-1〜R-4 は、C-2 の反映だけで語を選ばず、修正と再試験の結果の記録を `D-2026-11-24-3` の (3)・(5)(a)(b) に照らして確かめる。QP-1 の README が引く、実行前に固定した記録の 3 節（T 表の判定の定義）と 5 節（重大度と「未対応」の数え方）は、T 表の判定の定義と、QP-1 での重大度・数え方である（記録票の判定語の定義ではない。件数を減らす読み替えをしない点は変わらない。3 節の T 表の「確認済み」は別の語）。記録の所在は QP-1 の README から辿る。
3. 下の「行ごとの表」に沿って、QP-1 の README、PoC の README、`04-behavior/` の順に読む。PoC-N のディレクトリは `docs/spec/03-poc/` の一覧から番号で引く（ディレクトリ名は写さない）。読む版は `D-2026-11-24-4` の (3)(c) に従う。
4. 対応 ID のステータス（確定／検討中）と、移管の有無を原文で確かめ直す。C-3 では、固定の commit `4987777` と、「現在の spec」としてレビューの日の spec repo の `main` の先頭を比べる（`D-2026-11-21-24` の (4)(e)）。比べる直前に `git -C docs/spec fetch origin refs/heads/main && git -C docs/spec rev-parse --verify 'FETCH_HEAD^{commit}'` を実行し、返った commit（この `fetch` が取得したもの）を「比べた commit」とする（submodule の commit や、手元に前から残っている `origin/main` を使わない）。`fetch` が終了コード 0 で終わらなければ、C-3 の比較をしない。比べた 2 つの commit と、参考の submodule の commit を記録票の前提の表の役割ごとの欄に書き、比べた結果を記録票の「C-3 の結果」の表に書く（点検 C が比べたのは `1df12ad` と `4987777` で、C-3 の比べ直しの代わりにはならない）。比べる対象は `D-2026-11-24-4` の (3)(b)、レビューの日の読みは同 (3)(e)、比べた後に `main` が進んだ場合と集計の直前の比較は同 (3)(a) に従う。同 (4)(a)〜(f) は書き手の読みである。決めていないことは同 (5) のとおりで、そこに書かれた時点で利用者へ伺う。[scope-ids.md](scope-ids.md) の値は commit 時点の写しである。差分の記録は [gap-check.md](gap-check.md) の点検 C にある。

QP-1 の README は `docs/spec/03-poc/` の一覧から QP-1 で引く（ディレクトリ名は写さない。以下「QP-1」）。
T 表は 1.2 節、重大は 4.1 節、中は 4.2 節、RFC 9700 の照合は 2 節にある。

## 全行に共通の観点

| 番号 | 観点（人への問い） |
|---|---|
| C-1 | QP-1 の該当行が引く「確かめた PoC・手順・ADB」と「結果」は、出典の PoC README の該当の手順・節と合っているか。AI の読みは参考として扱う |
| C-2 | T 行は「未確認の残り」の各項目、R 行は「方針」の決定 ID ごとに、`04-behavior/` のどの ID の前提・操作・期待で受けられているか。または残りとして記録されているか |
| C-3 | 対応 ID のステータス（確定／検討中）は、固定の commit と現在の spec の両方で変わっていないか。〔仮想〕の合格で〔実機〕の項目が確定になっていないか（ACPT-7）。比べる対象は `D-2026-11-24-4` の (3)(b) |
| C-4 | （候補の行のみ）AI が挙げた ID で足りるか、広すぎないか。足し引きは `TASK-83` の担当者が決める |
| C-5 | 指摘が出たら、まず `04-behavior/` の該当ビヘイビアの変更から始める（SSOT。`ACPT-12` の期待）。記録は 3/4 の記録票（[record-form.md](record-form.md)）に書く |

## 行ごとの表

「未確認の残り」は QP-1 の 1.2 節（T 表）の該当列で確かめる。項目の本文・件数は写さず、
(1)〜(n) のそれぞれに C-2 を当てる。区分は [scope-ids.md](scope-ids.md) の表に従う。

### T 行（14 行。QP-1 の 1.2 節）

| 範囲の行 | 区分 | 読む PoC の README（`03-poc/<dir>/README.md`）と手順 | `04-behavior/` のファイル | 確認の観点 |
|---|---|---|---|---|
| T-1 | 候補 | PoC-8 手順 4・6、PoC-1 ADB-6・ADB-25 | `lifecycle.md` | C-1〜C-4。残りの各項目を C-2 で確かめる |
| T-2 | 候補 | PoC-8 手順 4、PoC-3 手順 5・発見事項 10、PoC-12 手順 10、PoC-1 ADB-24、PoC-10 手順 7 | `rate-limit.md` | C-1〜C-4。残りの各項目を C-2 で確かめる。判定と加算の単位が 1 文であることを `THRT-6`・`AUDB-27`（`db-contract.md`）まで辿る |
| T-4 | 候補 | PoC-8 手順 3・4 | `api-session.md`・`lifecycle.md`・`rate-limit.md` | C-1〜C-4。残りの各項目を C-2 で確かめる |
| T-6 | 原文指定 | PoC-2 手順 4・5、PoC-3 手順 5、PoC-9 手順 6・7 | `api-device-grant.md`（`DGRT-29` は Hub へ移管。末尾の表で確かめる） | C-1〜C-3、C-5。対応 ID の `DGRT-29` は Hub `ASCR-84` へ移管し、この repo のレビューの範囲外（`D-2026-11-21-20` の (4)(b)）。本体側では、C-1（QP-1 の該当行と PoC の README の照合）、C-3（`DGRT-29` が固定の commit と現在の spec の両方で移管のままか）、移管先が移管表と合うことの確認、C-5 を行う。C-2（残りの各項目がどの ID で受けられているか）は、本体の ID で受けるか残りとして記録するかを人が確かめる。`4987777` の本体の spec に `DGRT-29` の行が無く、照らす行が無いと読める（書き手の読み。人が C-2 で確かめる）。C-2 で確かめるときに読む版は `D-2026-11-24-4` の (3)(c) に従う。Hub `ASCR-84` での受けの確認は Hub の受け先へ渡す（`D-2026-11-21-22` の (4)(c)）。対応 ID が全て Hub へ移管した行で、判定の書き方は `D-2026-11-24-2` の (3)(a) に従う（同 (5)(d) に利用者が答えるまでは保留。記入は利用者が行う） |
| T-7 | 候補 | PoC-8 手順 4・5・9、PoC-10 手順 4 | `api-session.md`・`lifecycle.md` | C-1〜C-4。残りの各項目を C-2 で確かめる |
| T-10 | 原文指定 | PoC-3 手順 7、PoC-12 手順 5、PoC-4 手順 4、PoC-6 の足した機能 4 | `oidc-clients.md`（関連: `csrf-ssr.md`） | C-1〜C-3、C-5。残りの各項目を C-2 で確かめる。Gate の例外が 1 パスだけである点は `.claude/rules/security.md` の CSRF の節と照らす |
| T-13 | 候補 | PoC-5 手順 3・5、PoC-4 手順 5・9 | `agent-credential.md`・`oidc-clients.md`・`api-oidc-provider.md` | C-1〜C-4。残りの各項目を C-2 で確かめる。トークン種別の区別は `token-signing.md` の `typ` の項と照らす。`OCL-14` は `1df12ad` の後に本文が変わった（[gap-check.md](gap-check.md) の点検 C）。その追記を人が確かめる |
| T-16 | 原文指定 | PoC-9 手順 3・4・6 | `screen-device.md`・`ui-safety.md`（`DVUI-21` の本体側の移管先 `UISF-37`。`DVUI-20` は Hub へ移管） | C-1〜C-3、C-5。残りの各項目を C-2 で確かめる。`UISF-37` は範囲に残す。`ACPT-12` の 2026-11-21 の追記（T-16 の移管先の訂正。[gap-check.md](gap-check.md) の点検 C）を、`screen-device.md` の末尾の移管表と照らす |
| T-17 | 候補 | PoC-9 手順 5・8・9、DPoP の検査 | `agent-credential.md` | C-1〜C-4。残りの各項目を C-2 で確かめる |
| T-19 | 候補 | PoC-5 手順 2・6、PoC-10 手順 2・3・12、PoC-1 ADB-17・ADB-25 | `token-signing.md`・`ops-restore-runbook.md` | C-1〜C-4。残りの各項目を C-2 で確かめる。`ORST-27` は Hub へ移管（この repo の範囲外） |
| T-20 | 候補 | PoC-10 手順 4、PoC-1 ADB-18・ADB-19、PoC-8 の台帳 | `ops-restore-runbook.md`・`availability.md` | C-1〜C-4。残りの各項目を C-2 で確かめる。R-2 と対応 ID は重ならないが、どちらも PoC-10 手順 4 を引くため、R-2 の観点と合わせて確かめる（対応 ID は行ごとに照らす）。`ORST-18` は Hub へ移管（この repo の範囲外） |
| T-21 | 候補 | PoC-10 手順 9、PoC-1 ADB-25、PoC-3・4・7・8・9・11 の走査 | `secret-handling.md`・`audit.md` | C-1〜C-4。残りの各項目を C-2 で確かめる。秘密値の保存の形は `.claude/rules/secret-values.md` と照らす |
| T-22 | 候補 | PoC-1 ADB-15・ADB-16、PoC-10 手順 5・6、PoC-8 | `db-contract.md`・`availability.md` | C-1〜C-4。残りの各項目を C-2 で確かめる。`AUDB-35`・`AVLB-34` は Hub へ移管（この repo の範囲外） |
| T-23 | 候補 | PoC-11 手順 3・4、PoC-7 手順 4・8、PoC-9 手順 7 | `agent-delegation.md` | C-1〜C-4。残りの各項目を C-2 で確かめる。PoC-11 手順 4 は R-1 と同じ手順を引くため、R-1 の観点と合わせて確かめる |

### R 行（6 行。QP-1 の 4.1 節・4.2 節）

| 範囲の行 | 区分 | 所在 | 読む PoC の README と節 | `04-behavior/` のファイル | 関係する決定 ID | 確認の観点 |
|---|---|---|---|---|---|---|
| R-1 | 原文指定 | 4.1 節、6 節の判断 1 | PoC-11 手順 4 | `agent-delegation.md` | `D-2026-10-16-16` の (1)(2)。`ADLG-26` の持ち越しは `D-2026-10-17-2`・`D-2026-10-17-84` | C-1〜C-3、C-5。固定した基準を緩める読み替えの承認は利用者が持つ（AI が承認の有無を決めない）。`ADLG-26` が持ち越しで連動する点を原文で確かめる |
| R-2 | 原文指定 | 4.1 節、6 節の判断 2 | PoC-10 手順 4 | `ops-restore-runbook.md`・`lifecycle.md`・`availability.md` | `D-2026-10-16-15` の (1)(2)(3)(5)(7) | C-1〜C-3、C-5。方針の各項目がどの ID の受け入れ条件になっているか。T-20 と同じく PoC-10 手順 4 を引く点にも触れる（対応 ID は重ならないので、行ごとに照らす）。`ORST-23` は一部移管（Hub `AOPS-25`）。本体に残る部分は範囲に残し、Hub の部分は範囲外（`D-2026-11-21-22` の (4)(b)） |
| R-3 | 原文指定 | 4.1 節、6 節の判断 5 | PoC-1 の README | `dependency-policy.md`・`agent-delegation.md` | `D-2026-10-16-16` の (4) | C-1〜C-3、C-5。検証を外す設定・feature を作らない要求（`.claude/rules/security.md` の A05。`DPOL-30`〜`DPOL-32`・`CSRF-7`）と照らす |
| R-4 | 原文指定 | 4.1 節、6 節の判断 5 | PoC-8 の README | `human-approval.md`・`agent-delegation.md` | `D-2026-10-16-16` の (5) | C-1〜C-3、C-5。人の判定に自由文・LLM を通さない要求（`HAPR-1`・`SECR-28`）が `HAPR-9`・`HAPR-10` で受けられているか |
| R-5 | 原文指定 | 4.2 節 | PoC-5 手順 5 | `token-signing.md`・`acceptance-retest.md` | なし（QP-1 の該当行で確かめる） | C-1〜C-3。`ACPT-12` の期待どおり、ビヘイビアへの反映（P4）が済んでいるか。下流（`fandhe-backend`）の値は写さず、`ACPT-45`・`SIGN-49`・`SIGN-50` と相手側の ID で引く |
| R-6 | 原文指定 | 4.2 節、2 節（RFC 9700 の 2.6 の照合のうち表の 22・23 行） | PoC-4 の逸脱欄、PoC-6 | `api-oidc-provider.md` | なし | C-1〜C-3。`ACPT-12` の期待どおり、反映（P4）が済んでいるか。`OPRV-8` の原文が RFC 9700 の該当の要求を受けているか |

### 別枠（1 行）

| 範囲の行 | 区分 | 読む PoC の README | `04-behavior/` のファイル | 確認の観点 |
|---|---|---|---|---|
| 別枠 | 原文指定 | PoC-9 の README（DPoP の検査）、QP-1 の R の表 | `agent-credential.md` | C-1〜C-3。対応 ID は `AGCR-29`。`ACPT-12` が R の行と別に挙げた DPoP の検証部で、特定の T・R の行には結び付けない |

## ビヘイビア ID の接頭辞と `04-behavior/` のファイル

| 接頭辞 | ファイル | 接頭辞 | ファイル |
|---|---|---|---|
| LIFE | `lifecycle.md` | SIGN | `token-signing.md` |
| THRT | `rate-limit.md` | ORST | `ops-restore-runbook.md` |
| SESS | `api-session.md` | AVLB | `availability.md` |
| DGRT | `api-device-grant.md` | SECR | `secret-handling.md` |
| OCL | `oidc-clients.md` | AAUD | `audit.md` |
| AGCR | `agent-credential.md` | AUDB | `db-contract.md` |
| DVUI | `screen-device.md` | ADLG | `agent-delegation.md` |
| DPOL | `dependency-policy.md` | HAPR | `human-approval.md` |
| OPRV | `api-oidc-provider.md` | ACPT | `acceptance-retest.md` |
| UISF | `ui-safety.md` |  |  |

## 本資料で扱わないこと

- 判定と記録票の様式（3/4。`TASK-82`。判定は `TASK-83` で人が記録する）
- 資料と記録票の欠けの点検（4/4。記録は [gap-check.md](gap-check.md)）
- 候補 ID の採否（`TASK-83`）
- spec の修正（利用者の指示を待つ）
