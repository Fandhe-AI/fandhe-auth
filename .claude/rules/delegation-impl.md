---
paths:
  - "crates/**"
  - "Cargo.toml"
  - "**/Cargo.toml"
  - "deny.toml"
  - "scripts/**"
  - "Makefile"
  - ".github/**"
  - ".claude/agents/**"
  - ".claude/rules/**"
  - "CLAUDE.md"
  - "README.md"
description: >
  作成・編集フェーズの委譲ルール。crates/・Cargo 設定・scripts/・Makefile・.github/・.claude/agents|rules/・
  CLAUDE.md・README.md を変更する際に適用する。crate ごとの委譲先、委譲プロンプトの必須項目、
  実装後の流れ（試験 → 依存監査 → セキュリティ監査 → 完了ゲート）を規定する。
---

# 委譲ルール（作成・編集フェーズ）

調査・設計モードと model 配分は `./delegation.md` を参照。本ファイルは**作成・編集モード**に特化する。

## 委譲先（作成・編集系）

| 対象 | subagent_type | model | 適用ルール |
| --- | --- | --- | --- |
| `crates/core`・`webauthn`・`provider`・`keys`・`secret`・`types`・`client` | `core-builder` | sonnet | `./coding-rust.md`・`./secret-values.md`・`./security.md` |
| `crates/db-contract`・`db-tests`・`db-pg`・`db-migrate`（`db-fandhe` は移行判定時）・`migrations/` | `db-builder` | sonnet | `./db-layer.md`・`./coding-rust.md`・`./secret-values.md` |
| `crates/backend`（lib）・`crates/server`（bin） | `backend-builder` | sonnet | `./coding-rust.md`・`./security.md`・`./secret-values.md` |
| `crates/frontend`・`webauthn-wasm` | `frontend-builder` | sonnet | `./coding-rust.md`・`./security.md` |
| ルート `Cargo.toml`・`deny.toml`・`.cargo/`・`scripts/`・`Makefile`・`.github/`・`lefthook.yml` | `infra-builder` | sonnet | `./dependency-policy.md`・`./rust-build-profile.md`・`./makefile.md`・`./git-hooks.md`・`./github-actions.md` |
| 試験（`#[cfg(test)]`・`crates/*/tests/`・`crates/db-tests` の試験ケース・property test・fuzz） | `test-writer` | sonnet | `./coding-rust.md`・`./db-layer.md`・`./verification.md` |
| `.claude/agents/` | `agent-author` | sonnet | `./dotclaude-via-temp.md` |
| `.claude/rules/` | `rules-author` | sonnet | `./dotclaude-via-temp.md` |
| `CLAUDE.md`・`README.md` | `docs-writer` | sonnet | `./japanese-style.md` |

適用の注意:

- crate と委譲先の対応は暫定である。各 Agent の `description` と食い違う場合は Agent 側を確かめて本表を直す
- `crates/db-tests` は crate の構成を `db-builder`、試験ケースの追加を `test-writer` が担う
- `crates/backend`（ハンドラ・ルーティング・Cookie/CSRF の配線・外向き HTTP・outbox）は採用（利用者の決定 2026-11-13。spec への記録〔DPOL-41 の表・`D-`〕は未反映で、spec repo への PR は利用者の指示を待つ）
  （`D-2026-10-29-1` (5)）。`crates/server` は組み立てだけにする（`./coding-rust.md`）
- crate の `Cargo.toml` の依存を足す・変える場合は、担当の builder が書き、`./dependency-policy.md` に従う
- 複数 crate にまたがる変更は crate ごとに分けて委譲する。依存の向き（DPOL-1・DPOL-7・DPOL-39・DPOL-40）を
  変える設計判断は main が利用者と決めてから委譲する
- 試験は実装と別の Agent（`test-writer`）に書かせる。契約テストを通すために期待を緩めない（AUDB-6・AUDB-69）
- 依存を変更しない純粋な文書・ルールの変更では、下の「依存監査」の段は省いてよい

## 委譲プロンプトの必須項目

1. **目的**: 何を作成・編集するか（対応する TASK-N）
2. **根拠**: 守るべき spec ID・決定 ID（本文を写さず ID で示す。未決は「未定（TASK-n で決める）」）
3. **入力**: 参照すべきファイルパス・既存の実装
4. **出力先**: 作成・編集するファイルのパス（`.claude/` は `_/dotclaude/` 経由のパス）
5. **観点・適用ルール**: 上表の適用ルールと、完了の確認に使うコマンド

例:

```
subagent_type: core-builder
prompt: |
  目的: TASK-3 の fandhe-auth-secret の雛形を作る
  根拠: SECR-11・SECR-12・SECR-16・SECR-31。#![forbid(unsafe_code)]（利用者決定 2026-11-13）
  入力: docs/spec/04-behavior/secret-handling.md、ルートの Cargo.toml
  出力先: crates/secret/
  観点・適用ルール: ./coding-rust.md・./secret-values.md・./code-comment-style.md。完了確認は ./verification.md のコマンド
```

## 実装後の流れ

1. **試験**: `test-writer` が振る舞いの試験を追加する（前提・操作・期待の形で spec ID を引く）
2. **依存監査**（`Cargo.toml`・`Cargo.lock`・`deny.toml` を変えたとき）: `dependency-auditor` が
    `./dependency-policy.md` の検査結果を集計・照合する
3. **セキュリティ監査**: `security-auditor`（読み取り専用）が差分を `./security.md` の観点で監査する
4. **完了ゲート**: `./verification.md` のコマンドを新規実行し、出力を引用して報告する
5. **人の判定**: 監査・試験の結果は判断材料として利用者へ示す。合否・「確定」への変更・例外の承認は人が行う
    （ACPT-7、CG-2）。コミットは `./conventional-commits.md` に従う

指摘が出たら担当の builder に差し戻し、2〜4 をやり直す。

## .claude/ 配下の編集

`.claude/` 配下（`agents/`・`rules/`・`settings.json` 等）を作成・編集する Agent は `./dotclaude-via-temp.md` に従い
**直接書き込まず `_/dotclaude/` を経由**する。配置（`mv`）は main がまとめて行う。
`.claude/skills/` は外部取得物のため編集しない（`./delegation.md`）。
