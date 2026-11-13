---
description: >
  調査・設計フェーズで常に適用する委譲ルール。main は対話・計画・委譲・報告に徹し、token を消費する
  調査・作成・編集・レビューはサブエージェントへ委譲する。パスによるモード切り替え、model 配分、
  調査系の委譲先、並列化、main がやってよいこと・いけないことを規定する。
---

# 委譲ルール（調査・設計フェーズ）

## 委譲の原則

main の役割は **対話・計画・委譲・報告** に徹することで、token を消費する作業（spec・コードの調査、
ファイルの作成・編集、レビュー、監査）はサブエージェントへ委譲する。結果の判定・値の決定・例外の承認・
「確定」への変更は人が行う（ACPT-7）。main もサブエージェントも、それらを自分で済ませない。

## パスによるモード切り替え

| 操作 | モード | 適用ルール |
| --- | --- | --- |
| リポジトリ内の**閲覧のみ**（`docs/spec/`・`crates/`・`scripts/`・`.claude/`・`_/` 等） | 調査・設計モード | 本ファイル |
| `crates/`・`Cargo.toml`・`scripts/`・`Makefile`・`.github/`・`.claude/agents/`・`.claude/rules/`・`CLAUDE.md`・`README.md` の**作成・編集** | 作成・編集モード | `./delegation-impl.md` |

作成・編集モードの正確な対象は `./delegation-impl.md` の `paths` を正とする（`deny.toml` 等を含む）。

編集しないもの:

- `docs/spec/` — spec repo（private）の submodule。この repo では編集しない。spec を直す手順は
  spec に定めが無く未定のため、利用者の明示の指示を待つ（読み方は `./spec-reference.md`）
- `.agents/skills/`・`.claude/skills/` — `skills-lock.json` で管理する外部取得物（`.claude/skills/` は
  `.agents/skills/` への symlink）。更新は `update-external.yml` の自動 PR に任せる

## model 配分

| tier | model | 用途 |
| --- | --- | --- |
| 最上位 | `opus` | セキュリティ監査、crate 横断の設計判断 |
| 標準 | `sonnet` | 調査・実装・試験・ルール/Agent/文書の作成・計画の検証 |
| 軽量 | `haiku` | 機械的な集計、lint・`cargo deny` 等の結果の照合 |

opus は判断の誤りが直接リスクになるもの（認証・秘密値・依存の監査）と横断設計に限る。
haiku は判断を伴わない処理に限り、セキュリティの合否判断には使わない。

## 委譲先（調査系）

| やりたいこと | subagent_type | model |
| --- | --- | --- |
| `docs/spec/` の ID 引き・原文確認・決定 ID の追跡 | `spec-researcher` | sonnet |
| gh / git / cargo / lefthook / CI 失敗の原因調査 | `sub-investigator` | sonnet |
| 計画・指示書に対する作業完了の検証 | `plan-verifier` | sonnet |
| セキュリティ監査（読み取り専用。OWASP・spec の安全要求） | `security-auditor` | opus |
| 依存の監査結果の集計・照合（`cargo deny`・`cargo audit`・依存ツリー） | `dependency-auditor` | haiku |
| コードベースの横断検索（ファイル・シンボルの所在） | `Explore`（組み込み） | 既定 |

呼び出し例:

```
subagent_type: spec-researcher
prompt: |
  目的: Cookie の属性要求を確認する
  入力: docs/spec/04-behavior/ の SESS-1・CSRF-49・CSRF-50
  出力: ID ごとの要求・ステータス（確定/検討中）・確定の範囲を一覧で返す。本文は写さず ID で引く
```

## 並列化

独立したタスクは**同一メッセージ内で複数 Agent を起動**して並列実行する。依存関係がある場合のみ逐次実行する。

例: 「spec-researcher で spec の要求を確認」と「Explore で既存 crate の該当箇所を探す」は並列起動できる。
「security-auditor の監査」は実装の完了を待ってから起動する。

## main が直接やってよいこと

- ユーザーとの対話・要件と前提（未決の決定・仮定 ID）の確認
- 計画立案と `_/local-plans/` への計画ファイル作成
- サブエージェントへの委譲プロンプト作成（必須項目は `./delegation-impl.md`）
- サブエージェントの報告を統合してユーザーへ返答する（判定はせず、判断材料として示す）
- 短いコマンド（`git status`・`ls`・`make help` 程度）の直接実行

## main がやってはいけないこと

- `./delegation-impl.md` の対象パスの直接編集（→ 作成・編集系の Agent へ委譲）
- `docs/spec/` 本文の大量の逐次 Read（→ spec-researcher へ委譲）
- セキュリティ監査・依存の監査を自分で行うこと（→ security-auditor・dependency-auditor）
- サブエージェントや AI レビューの合格だけで安全・完了と確定すること（CG-2、`./verification.md`）
- 「単純だから」「数行だから」を理由に委譲を省くこと

## 関連ルール

- `./delegation-impl.md` — 作成・編集系の委譲先と実装後の流れ
- `./spec-reference.md` — spec の読み方・ID の引き方
- `./verification.md` — 完了ゲート
