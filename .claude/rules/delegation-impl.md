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
  - "AGENTS.md"
description: >
  作成・編集フェーズの委譲ルール。crates/・Cargo 設定・scripts/・Makefile・.github/・.claude/agents|rules/・
  CLAUDE.md・README.md・AGENTS.md を変更する際に適用する。段階ごとの crate の委譲先（Hub の担当の crate は委譲先なし）、委譲プロンプトの必須項目、
  実装後の流れ（試験 → 依存監査 → セキュリティ監査 → 完了ゲート）を規定する。
---

# 委譲ルール（作成・編集フェーズ）

調査・設計モードと model 配分は `./delegation.md` を参照。本ファイルは**作成・編集モード**に特化する。

## 委譲先（作成・編集系）

| 対象 | subagent_type | model | 適用ルール |
| --- | --- | --- | --- |
| 段階 1: `crates/core`・`secret`・`client`・`plugin-oidc`（外部ログイン Plugin の OIDC 共通部品。`D-2026-11-20-2`）。位置が未定の `webauthn`・`provider`・`keys`・`types`。試験専用の `hub-stub`・`console-stub`・`it-harness`・`e2e-harness`・`load-harness` | `core-builder` | sonnet | `./coding-rust.md`・`./library-boundary.md`・`./secret-values.md`・`./security.md` |
| 段階 1: `crates/db-tests`。位置が未定の `db-contract`（契約 trait。`-core` の中なら `core-builder` と調整） | `db-builder` | sonnet | `./db-layer.md`・`./coding-rust.md`・`./secret-values.md` |
| 段階 2: `crates/backend`（任意の接続部品。`LIB-15` の判定の記録まで `interim` に中身を入れない＝委譲しない。下記） | `backend-builder` | sonnet | `./coding-rust.md`・`./library-boundary.md`・`./security.md`・`./secret-values.md` |
| 段階 2: `crates/frontend`・`webauthn-wasm`（任意の部品） | `frontend-builder` | sonnet | `./coding-rust.md`・`./library-boundary.md`・`./security.md` |
| ルート `Cargo.toml`・`deny.toml`・`.cargo/`・`scripts/`・`Makefile`・`.github/`・`lefthook.yml` | `infra-builder` | sonnet | `./dependency-policy.md`・`./rust-build-profile.md`・`./makefile.md`・`./git-hooks.md`・`./github-actions.md` |
| 試験（`#[cfg(test)]`・`crates/*/tests/`・`crates/db-tests` の試験ケース・property test・fuzz） | `test-writer` | sonnet | `./coding-rust.md`・`./db-layer.md`・`./verification.md` |
| `.claude/agents/` | `agent-author` | sonnet | `./dotclaude-via-temp.md` |
| `.claude/rules/` | `rules-author` | sonnet | `./dotclaude-via-temp.md` |
| `CLAUDE.md`・`README.md`・`AGENTS.md` | `docs-writer` | sonnet | `./japanese-style.md` |

適用の注意:

- crate と委譲先の対応は暫定である。各 Agent の `description` と食い違う場合は Agent 側を確かめて本表を直す
- crate の段階は spec 上〔提案〕で、名前だけが利用者の決定済み（`./coding-rust.md`）。段階 2 の crate は作る前に利用者に確かめてから委譲する
- Hub の担当の crate（`db-pg`・`db-fandhe`・`db-migrate`・`server`）と migration は、この repo では作らない（`LIB-14`。`./library-boundary.md`）。委譲先は無い
- `crates/db-tests` は crate の構成を `db-builder`、試験ケースの追加を `test-writer` が担う
- `crates/backend` は名前の採用が `D-2026-10-29-2` で spec に記録済み。役割は段階 2 の任意の接続部品で、`crates/server`・outbox の配送・Gate の登録は Hub が持つ
  （利用者の決定 2026-11-19。`./coding-rust.md`）
- `crates/backend` は `TASK-9` の暫定の置き場で、最初のリリースに含めない扱い（`D-2026-11-20-1`）。`LIB-15` の判定を人が記録するまで、
  `crates/backend` のどこにも（`interim` の外も含めて）中身を足す作業・`fandhe-backend` への依存を足す作業を `backend-builder` 等へ委譲しない（`D-2026-11-20-3`）。
  進めてよいのは `docs/interim/README.md` の「backend の切り出しの記録」の判定 (1)〜(4) の 4 欄すべてに「満たす」という人の記入があるときだけで、「未記入」・「満たさない」の欄があれば止めて報告する。
  `form_limit`・`trusted_proxy` の移管分は作らない（→ Hub `ARUN-46`・`ARUN-47`・`ARUN-43`・`ARUN-44`）。
  `CSRF-43`・`CSRF-52`・`CSRF-45`・`CSRF-46` の本体の部分は `trusted_proxy` の枠の外で作る。本体に残る部分は `LIB-1` で行ごとに判定し、置き場が未決の部分は `LIB-1` の判定で利用者が決めるまで委譲しない
- crate の `Cargo.toml` の依存を足す・変える場合は、担当の builder が書き、`./dependency-policy.md` に従う
- 複数 crate にまたがる変更は crate ごとに分けて委譲する。依存の向き（DPOL-1・DPOL-7・DPOL-39・DPOL-40）を
  変える設計判断は main が利用者と決めてから委譲する
- 試験は実装と別の Agent（`test-writer`）に書かせる。契約テストを通すために期待を緩めない（AUDB-6。同趣旨の AUDB-69 は → Hub `ASTORE-2`）
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
  根拠: SECR-11・SECR-12・SECR-16・SECR-31。#![forbid(unsafe_code)]（利用者の決定 2026-11-13。spec に記録なし）
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
