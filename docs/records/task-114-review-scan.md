# TASK-114 REVIEW 4 件の走査の記録

`TASK-114`（`ACPT-35`）の 1/2 段目（Issue #260）。PoC-6 の Basic OP の集計で REVIEW の 4 モジュールが合否に
数えられていないかの数え直しと、REVIEW のモジュールを PASSED と書いた記述の走査の結果を残す。
2/2 段目（Issue #261）の確認の結果の整理は、7 節（判定の欄）・8 節（申請者の欄）・9 節（判定のための整理）に
追記した。1/2 段目の事実（1〜6 節）は書き換えていない。

この文書は走査と集計の事実だけを書く。判定・「確定」への変更・例外の承認は人が行う（`ACPT-7`）。
「0 件だから合格」とは書かない。判定の欄は空けてある。

## 1. 目的と範囲

- 対象のビヘイビア: `ACPT-35`（確定の範囲は PoC-6 の Basic OP の REVIEW 4 件。ローカルの検証）。
- 公式の認証は v1 では申請しない（`D-2026-10-17-71`・`D-2026-10-23-10`）。REVIEW の画面を人が確かめる作業は範囲外。
- 本文（spec）は転記しない。path:line・ID・件数・suite の公開のモジュール名だけを書く。

## 2. 走査の対象

- spec: `Fandhe-AI/fandhe-auth-spec` のコミット `eb418ff0084bf5d5704dbd06ddbc790401980b45`
  （本 repo の submodule `docs/spec` が pin している値。HEAD が動いても再現できるよう固定した）。
- 集計の入力: spec の `03-poc/oidc-conformance-local/results/run2/{es256-chrome,rs256-chrome,es256-firefox}/basic-op/summary.json`
  と同 `basic-op.txt`。
- 記述の走査の範囲: 上記コミットの全 `*.md` と、本 repo の `README.md`・`AGENTS.md`・`CLAUDE.md`・
  `.claude/rules/`・`.claude/agents/`。モジュール名の検索 (a) だけは `results/` 以下を除く。
- 本 repo の走査時点の base: `origin/main` の `a5c1c84`。

## 3. 手順

spec を worktree の外（一時ディレクトリ）へ clone して pin のコミットを checkout し、clone の外に置いた
スクリプトを `python3 -I` で実行した（clone 内のコードは実行していない）。

集計（`modules[].result` と `modules[].conditions.REVIEW` を数える）:

```bash
python3 -I count.py <clone>/03-poc/oidc-conformance-local/results/run2
```

記述の走査:

```bash
# (a) 4 モジュール名（results/ を除く）
grep -rnE 'oidcc-prompt-login|oidcc-max-age-1\b|oidcc-ensure-registered-redirect-uri|oidcc-ensure-request-object-with-redirect-uri' --include='*.md' . | grep -v '/results/'
# (b) 全体の主張
grep -rnE '全モジュール|35 ?件 ?PASSED|PASSED ?3[345]|すべて合格|全て合格|REVIEW.*合格|合格.*REVIEW|REVIEW.*PASSED.*(と数|に数|扱)' --include='*.md' . | grep -v '/results/'
# (c) REVIEW の全件
grep -rnw 'REVIEW' --include='*.md' .
# (d) (c) のうち同じ行に PASSED・合格・通 を含むもの
grep -rnw 'REVIEW' --include='*.md' . | grep -E 'PASSED|合格|通'
# 本 repo
grep -rnwE 'REVIEW|oidcc-prompt-login|oidcc-max-age-1' README.md AGENTS.md CLAUDE.md .claude/rules .claude/agents
```

(d) の行は、REVIEW の前 45 文字・後 75 文字の窓を python で取り出して文脈を確かめた。

## 4. AC-1 集計の数え直し

### 4.1 モジュールの段（`modules[].result`）

| 構成 | モジュール数 | PASSED | REVIEW | SKIPPED | WARNING | FAILED |
| --- | --- | --- | --- | --- | --- | --- |
| ES256・Chrome | 35 | 29 | 4 | 1 | 1 | 0 |
| RS256・Chrome | 35 | 29 | 4 | 1 | 1 | 0 |
| ES256・Firefox（4 回に数えない追加の確認） | 35 | 29 | 4 | 1 | 1 | 0 |

- 3 構成とも合計は 35。`basic-op.txt` の `result=` の数え直しも `summary.json` と一致した
  （各構成 PASSED 29・REVIEW 4・SKIPPED 1・WARNING 1）。
- REVIEW のモジュール（3 構成とも同じ）: `oidcc-prompt-login`・`oidcc-max-age-1`・
  `oidcc-ensure-registered-redirect-uri`・`oidcc-ensure-request-object-with-redirect-uri`。
- SKIPPED: `oidcc-unsigned-request-object-supported-correctly-or-rejected-as-unsupported`。
  WARNING: `oidcc-claims-essential`。

### 4.2 REVIEW の 4 モジュールの数えられ方

| 確認 | 結果（3 構成とも） |
| --- | --- |
| 4 モジュールの名前が、PASSED の 29 モジュールの名前の集合に含まれるか | 含まれない（共通部分 0 件） |
| FAILED のモジュール | 0 件（4 モジュールを含まない） |

### 4.3 条件の段（`conditions.REVIEW`）

| モジュール | モジュールの result | 条件の REVIEW |
| --- | --- | --- |
| `oidcc-response-type-missing` | PASSED | 1 |
| `oidcc-prompt-login` | REVIEW | 1 |
| `oidcc-max-age-1` | REVIEW | 1 |
| `oidcc-ensure-registered-redirect-uri` | REVIEW | 1 |
| `oidcc-ensure-request-object-with-redirect-uri` | REVIEW | 1 |

条件の REVIEW の合計は 5（3 構成とも）。モジュールの段は 4、条件の段は 5 で、差の 1 件が
`oidcc-response-type-missing`（result は PASSED）である。

### 4.4 既存の記述との照合

| 照合先 | 記述の件数 | 数え直しとの関係 |
| --- | --- | --- |
| PoC-6 README（`03-poc/oidc-conformance-local/README.md`）の Basic OP 集計表（352 行付近）の 3 構成 | PASSED 29・REVIEW 4・SKIPPED 1・WARNING 1・FAILED 0 | 一致 |
| `04-behavior/acceptance-retest.md:163`（ACPT-33） | PASSED 29・REVIEW 4・SKIPPED 1・WARNING 1 | 一致 |
| `04-behavior/README.md:545`・`spec.md:8240`・`02-poc-plan.md:555`・`01-brainstorm.md:2774` | 同上 | 一致 |
| PoC-6 README の 306・421・500 行 | 「REVIEW 5」 | 条件の段の件数（4.3）と一致。モジュールの段（4.1）とは異なる（6 節） |

## 5. AC-2 記述の走査

### 5.1 件数

| 走査 | 件数 |
| --- | --- |
| (a) 4 モジュール名を含む行（`results/` を除く `*.md`） | 23 行 |
| (b) 全体の主張の正規表現に当たった行 | 5.2 の分類 2 と重なる行のほか、PASSED と無関係な「全モジュールが `FINISHED`」等の行を含む |
| (c) `REVIEW`（単語）を含む行（`results/` を含む `*.md`） | 80 行 |
| (d) (c) のうち PASSED・合格・通 も含む行 | 37 行 |
| 本 repo（`README.md`・`AGENTS.md`・`CLAUDE.md`・`.claude/`） | 0 行 |

### 5.2 分類（Claude Code の読み。参考であり判定ではない）

分類 1: REVIEW のモジュールを PASSED と書いた記述の候補

- 候補として挙がった行: 0 件。(a)〜(d) の行を読み、4 モジュールを PASSED と読める文は見つからなかった。
- 近接する記述（候補としない理由つき）: `03-poc/oidc-conformance-local/README.md:408`・`01-brainstorm.md:150` の
  `oidcc-response-type-missing` は「result は PASSED」で、条件に REVIEW を 1 件持つ。4 モジュールには含まれない。

分類 2: REVIEW を合否に数えない・主張しないと書いた記述（path:line）

- `03-poc/oidc-conformance-local/README.md`: 414・479・506・594
- `03-poc/oidc-conformance-local/results/fixed-before-run-2.md`: 58・76
- `04-behavior/acceptance-retest.md`: 163・164・165
- `04-behavior/README.md`: 452・545
- `spec.md`: 8240・8246・8251・8252・8671
- `05-tasks.md`: 928・936・944
- `02-poc-plan.md`: 547・602・605・613
- `01-brainstorm.md`: 150・753・2661

分類 3: 件数の表し方が 2 通りある記述（6 節）

- `03-poc/oidc-conformance-local/README.md`: 306・421・500（条件の段で「REVIEW 5」）
- `01-brainstorm.md:150`（モジュール 4 件と条件 5 件の差を明記している）

分類 1 の 0 件は走査の範囲での事実であり、範囲外（他の repo・口頭の説明・今後の文書）には及ばない。

## 6. 観察した事実（4 件と 5 件）

- モジュールの段の REVIEW は 4、条件の段の REVIEW は 5。5 件目は `oidcc-response-type-missing` の条件で、
  このモジュールの result は PASSED（4.3）。
- PoC-6 README の 306・421・500 行は「REVIEW 5」と書き、`ACPT-33`・`ACPT-35` は「REVIEW 4」と書く。
  段が違う件数だが、段を示していない行がある。
- これを `ACPT-35` の該当・非該当のどちらと読むかは決めない。spec は直さない（本 repo では編集しない）。
  扱いは #261 で利用者の判断材料として整理する。


## 7. 判定と記録の欄

判定は人が記入する（`ACPT-7`・`ACPT-35`）。Claude Code は記入しない。根拠の節は読む場所の案内であり、結論ではない。

| 項目 | 根拠の節 | 記入 |
| --- | --- | --- |
| AC-1 の判定（集計表で REVIEW 4 件が PASSED・FAILED に入っていないか） | 4.1・4.2・9.1 | 未記入 |
| AC-2 の判定（REVIEW のモジュールを PASSED と書いた記述の有無） | 5.1・5.2・9.1 | 未記入 |
| 6 節（4 件と 5 件）の扱い | 6・9.3 | 未記入 |
| 走査の範囲の限界の扱い | 5.2 末尾・9.3 | 未記入 |
| 記録した人・日付 | 9 節 | 未記入 |

## 8. 申請を行う場合の担当（申請者）の記録欄

公式の認証は v1 では申請しない（`D-2026-10-17-71`・`D-2026-10-23-10`）。申請を行う場合に備えた欄で、
値は人が記入する。

| 項目 | 記入 |
| --- | --- |
| 申請の有無 | 未記入 |
| 申請者（人） | 未記入 |
| REVIEW の画面の確認の記録（申請するときに申請者が行う） | 未記入 |
| 確認した日付 | 未記入 |
| 記録した人・日付 | 未記入 |

再検討の条件の状況（事実。ID で引く）:

- 1 つ目の条件は `D-2026-10-23-7`・`D-2026-10-23-10` で再検討済みで、決定は維持された。
- 2 つ目の条件（外部の RP を受け入れる判断のとき）は残っている。

## 9. 利用者の判定のための整理（2/2 段目、Issue #261）

この節は 1/2 段目で集めた事実を、判定する人が読む順に並べ直したもの。再走査はしていない。
判定・推奨・「確定」への変更は書かない。

### 9.1 `ACPT-35` の期待と根拠の対応

spec の本文は写さず、ID と短い言い換えで引く。

| 期待 | 根拠の節 | 事実の要約 | 判定 |
| --- | --- | --- | --- |
| (1) 集計表で REVIEW 4 件が PASSED・FAILED に入っていない | 4.1・4.2 | 3 構成とも REVIEW 4。4 モジュールは PASSED の 29 の名前の集合と共通部分 0 件。FAILED は 0 件で 4 モジュールを含まない | 未記入 |
| (2) REVIEW のモジュールを PASSED と書いた記述が 0 件 | 5.1・5.2 | 分類 1 の候補 0 件（走査の範囲内） | 未記入 |

### 9.2 前提の確認（REVIEW のスクリーンショット）

`ACPT-35` の前提のスクリーンショット 4 件について、pin のコミット
`eb418ff0084bf5d5704dbd06ddbc790401980b45` の `03-poc/oidc-conformance-local/results/run2/<構成>/basic-op/` を
リモートの API（contents）で一覧し、ファイルの有無だけを確かめた。画像の中身は見ていない
（`D-2026-10-17-71` により REVIEW の画面の人の確認は v1 では要らない）。

| 構成 | モジュール | ファイル（有無） |
| --- | --- | --- |
| es256-chrome・rs256-chrome・es256-firefox | `oidcc-prompt-login` | `oidcc-prompt-login.login-page-1.png` あり（3 構成とも） |
| 同上 | `oidcc-max-age-1` | `oidcc-max-age-1.login-page-1.png` あり（3 構成とも） |
| 同上 | `oidcc-ensure-registered-redirect-uri` | `oidcc-ensure-registered-redirect-uri.error-page.png` あり（3 構成とも） |
| 同上 | `oidcc-ensure-request-object-with-redirect-uri` | `oidcc-ensure-request-object-with-redirect-uri.error-page.png` あり（3 構成とも） |

3 構成 x 4 モジュール = 12 ファイルが存在した。

### 9.3 利用者が判断する事項

推奨は書かない。選択肢は並べただけで、どれを採るかは人が決める。

- (i) AC-1 の判定（9.1 の期待 (1)）
- (ii) AC-2 の判定（9.1 の期待 (2)）
- (iii) 4 件（モジュールの段）と 5 件（条件の段）の扱い。事実は 6 節のとおり。取れる選択肢と、それぞれに要る作業:
  - 「`ACPT-35` に該当しないものとして記録だけ残す」: 本 repo では 6 節の記録の維持のみ。追加の作業なし
  - 「`ACPT-35` に該当するものとして扱う」: PoC-6 README の 306・421・500 行を期待に照らして
    扱う作業が要る。spec は本 repo で編集しない
  - 「PoC-6 README の 306・421・500 行に段の注記を足す提案をする」: `Fandhe-AI/fandhe-auth-spec` への PR になる。
    spec repo への PR は利用者の明示の指示を待つ
- (iv) 走査の範囲の限界（範囲外の repo・口頭の説明・今後の文書には及ばない。5.2 末尾）を判定でどう扱うか

判定の記入先は 7 節。

### 9.4 再現の情報

- spec の pin: `eb418ff0084bf5d5704dbd06ddbc790401980b45`
- 1/2 段目の本 repo の base: `a5c1c84`
- 2/2 段目の本 repo の base: `cdc74ab`
- 2/2 段目は再走査せず、1/2 段目の記録の整理と 9.2 のファイルの有無の確認だけを行った。

### 9.5 範囲外・持ち越し

- 公式の認証の申請: v1 では行わない（`D-2026-10-17-71`・`D-2026-10-23-10`）。
- 再検討の 2 つ目の条件（外部の RP を受け入れる判断のとき）: 8 節。
- spec の修正: 利用者の指示を待つ。
- security-auditor の監査: 1/2 段目では未実施。2/2 段目の差分の監査は 9.6。

### 9.6 2/2 段目の差分の監査

未実施（コミット時点。自動運転のため監査の Agent は起動していない）。AI の監査の合格は
安全の確定として扱わない（`CG-2`）。
