# CLAUDE.md

## Overview

`fandhe-auth` は Keycloak を使わない Rust 製の共通認証基盤の実装リポジトリ（public、`MIT OR Apache-2.0`）。
passkey・認証セッション・失効・復旧・登録済み自社クライアント向けの OIDC・ローカルエージェント認可を担う。

- 仕様は private の `Fandhe-AI/fandhe-auth-spec` を `docs/spec/` に submodule で置いている（`update = none`。
  取得手順と読み方は `.claude/rules/spec-reference.md`）。**仕様の正は `docs/spec/04-behavior/`**
- spec 中の「`fandhe-auth-backend` の repo／workspace」は本リポジトリを指す（`D-2026-10-29-1` の読み替え）
- 現在地: Phase 1〜6（仕様・PoC）完了。次は `06-roadmap.md` の MS-1「決定と土台」（TASK-3 雛形・TASK-4 技術選定・
  TASK-5 開発 DB と migration の骨組み）。`Cargo.toml`・crate はまだ無い。実装の着手と Issue の起票は利用者の指示を待つ
- 判定・値の決定・例外の承認・「確定」への変更は人が行う。Claude Code は手順・仕組み・試験を作って流すところまで（ACPT-7）

開発基盤（Makefile・lefthook・CI・ツール導入）の説明は [README.md](README.md) を参照する。

## Repository Structure

```text
.
├── CLAUDE.md                       -- 本ファイル
├── README.md                       -- 開発基盤・コマンド・CI の説明
├── Makefile / scripts/             -- 薄い入口と処理の実体（make help）。scripts/hooks/ は lefthook の実体
├── lefthook.yml                    -- pre-commit（シークレット検知・editorconfig・shellcheck）/ commit-msg
├── skills-lock.json                -- 外部スキルの lock（update-external.yml が日次追従）
├── crates/                         -- （TASK-3 で作成予定）crate は crates/<接頭辞 fandhe-auth- を除いた名前>
├── docs/spec/                      -- 仕様（private submodule）
│   ├── 04-behavior/                -- 振る舞いの正本（ID 付き）
│   ├── 05-tasks.md                 -- TASK-N（追跡の正）
│   └── 06-roadmap.md               -- マイルストーン（MS）
├── .agents/skills/                 -- 外部取得のスキル実体（編集しない）
├── .github/workflows/              -- ci.yml / ai-review.yml / update-external.yml
└── .claude/
    ├── agents/                     -- サブエージェント（research / implement / testing / quality / author）
    ├── rules/                      -- ルール（下表）
    ├── hooks/rustfmt-on-edit.sh    -- PostToolUse: 編集した .rs に rustfmt
    ├── skills/                     -- .agents/skills/ への symlink
    ├── workflows/implement-issue-tree.js  -- symlink: ../skills/implement-issue-tree/scripts/implement-issue-tree.js
    └── settings.json               -- hooks（SessionStart / PostToolUse）
```

## 委譲方針（必読）

main の役割は **対話・計画・委譲・報告** に徹する。token を消費する作業（調査・実装・試験・レビュー・文書作成）は
サブエージェントへ委譲する。詳細は `.claude/rules/delegation.md`（調査）・`.claude/rules/delegation-impl.md`（作成・編集）。

### パスベースの目安

| 操作対象パス | モード | 適用ルール |
|---|---|---|
| `docs/`・`_/`・`.claude/`・`crates/` の**閲覧のみ** | 調査・設計 | `delegation.md` |
| `crates/`・`Cargo.toml`・`deny.toml`・`scripts/`・`Makefile`・`.github/`・`.claude/agents/`・`.claude/rules/`・`CLAUDE.md`・`README.md` の**作成・編集** | 作成・編集 | `delegation-impl.md` |

### model 配分

| tier | alias | 用途 |
|---|---|---|
| 最上位 | `opus` | セキュリティ監査・横断的な設計判断 |
| 標準 | `sonnet` | 調査・実装・試験・文書作成・レビュー |
| 軽量 | `haiku` | 機械的な集計・ツール出力と規約の照合 |

独立したタスクは同一メッセージ内で複数 Agent を起動して並列実行する。

## Sub-agents

`.claude/agents/<category>/<name>.md`。`subagent_type: <name>` で呼び出す。

### research/ — 調査（読み取り専用）

| name | model | 概要 |
|---|---|---|
| `spec-researcher` | sonnet | `docs/spec` から振る舞い ID・決定 ID・ステータス・確定の範囲を引く |
| `sub-investigator` | sonnet | gh / git / cargo / lefthook / CI の失敗調査 |

### implement/ — 実装

| name | model | 担当 |
|---|---|---|
| `core-builder` | sonnet | `crates/core`・`webauthn`・`provider`・`keys`・`secret`・`types`・`client`（I/O を持たない層と RP 向け） |
| `db-builder` | sonnet | `crates/db-contract`・`db-tests`・`db-pg`・`db-migrate`（・`db-fandhe`） |
| `backend-builder` | sonnet | `crates/backend`（lib。`fandhe-backend` 上のハンドラ・Gate・Cookie・ヘッダ）・`crates/server`（bin。組み立てのみ）。分割は利用者の決定（2026-11-13、spec 未記録。`D-2026-10-29-1` (5)） |
| `frontend-builder` | sonnet | `crates/frontend`・`webauthn-wasm`（`fandhe-frontend`・CSP・wasm） |
| `infra-builder` | sonnet | Makefile・scripts・lefthook・CI・`deny.toml`・workspace 設定 |

### testing/

| name | model | 概要 |
|---|---|---|
| `test-writer` | sonnet | 単体・契約テスト・property test・fuzz（期待や安全条件を緩めない） |

### quality/ — 品質・検証（読み取り専用）

| name | model | 概要 |
|---|---|---|
| `security-auditor` | opus | OWASP Top 10 と spec のセキュリティ要求で差分を監査（安全の確定ではない） |
| `dependency-auditor` | haiku | `cargo deny`・`cargo audit`・`cargo tree -i` の結果を依存方針と照合 |
| `plan-verifier` | sonnet | 計画ファイルに対する作業完了の検証 |

### author/ — `.claude/` と文書

| name | model | 概要 |
|---|---|---|
| `agent-author` | sonnet | `.claude/agents/` の作成・編集（`_/dotclaude/` 経由） |
| `rules-author` | sonnet | `.claude/rules/` の作成・編集（`_/dotclaude/` 経由） |
| `docs-writer` | sonnet | `README.md`・`CLAUDE.md`・`docs/`（`docs/spec` を除く） |

## Rules

`paths` を持つルールは対象ファイルの編集時のみ読み込まれる。

| ファイル | 読み込み | 概要 |
|---|---|---|
| `delegation.md` | 常時 | 調査・設計フェーズの委譲原則・model 配分・委譲先 |
| `delegation-impl.md` | 作成・編集対象 | 作成・編集の委譲先と実装後の流れ |
| `spec-reference.md` | 常時 | spec の取得・読み替え・ID 引用・AI が確定にしない |
| `security.md` | 常時 | OWASP Top 10 と認証基盤固有の要求（CSRF・Cookie・CSP・署名・人の判定） |
| `japanese-style.md` | 常時 | 日本語スタイル |
| `conventional-commits.md` | 常時 | コミット・PR タイトルの形式（commit-msg フックと同じ） |
| `out-of-scope-tracking.md` | 常時 | スコープ外事項の Issue 化（承認後） |
| `verification.md` | 常時 | 証拠なき完了宣言の禁止 |
| `coding-rust.md` | `crates/**`・`*.rs` | crate 構成・依存の向き・unsafe 禁止・エラー応答の形 |
| `secret-values.md` | `crates/**`・`*.rs` | 秘密値の型と出力・保存の規則（SECR） |
| `code-comment-style.md` | `crates/**`・`*.rs`・`scripts/**` | `//!`・`///` に役割・契約・spec ID を書く |
| `db-layer.md` | `crates/db-*`・migrations | 契約 trait・時刻・SQL・migration の規則（AUDB・AUDP） |
| `dependency-policy.md` | `Cargo.toml`・`Cargo.lock`・`deny.toml` | 固定版・bans・ライセンス・cargo-deny（DPOL） |
| `rust-build-profile.md` | `Cargo.toml`・`.cargo/`・toolchain | dev プロファイル・`target/` の掃除・CI キャッシュ |
| `makefile.md` | `Makefile`・`scripts/**` | 薄い入口・ターゲット追加手順 |
| `git-hooks.md` | `lefthook.yml`・`scripts/hooks/**` | フックの時間予算と CI との役割分担 |
| `github-actions.md` | `.github/workflows/**` | runner・必須チェック・キャッシュ方針 |
| `dotclaude-via-temp.md` | `.claude/**` | `.claude/` は `_/dotclaude/` で作って `mv` |

## Current Skills (29)

`skills-lock.json` で管理（`.agents/skills/` に実体、`.claude/skills/` は symlink）。編集せず、改修は `contribute-skill` で upstream へ。

### 開発ワークフロー（Fandhe-AI/agent-cli-skills, 16）

comment-code, contribute-skill, create-commit, create-issue, create-issue-tree, create-plan, create-pr,
implement-issue, implement-issue-tree, implement-review, implement-review-pr, init-claude, setup-repo-guards,
update-claude, update-docs, update-issue-tree

### 参照（Fandhe-AI/agent-reference-skills, 13）

- 実装: rust, fandhe-backend, fandhe-frontend, fandhe-db, docker
- 開発基盤: make, lefthook, commitlint, editorconfig, github-docs
- エージェント: anthropic-claude-code, anthropic-claude-code-extend, openai-codex

## Conventions

- 日本語でやりとりする（技術用語・識別子は原語）
- Conventional Commits（`<type>[(<scope>)][!]: <要約>`）。`--no-verify` は使わない
- 実装は `implement-issue` で計画（`_/local-plans/`）を作り、**利用者の承認後**に着手する
- コミット・PR の前にセキュリティ確認（`security-auditor`）。AI のレビュー合格を安全の確定として扱わない（CG-2・TASK-83）
- 根拠は spec の ID（`SECR-11`・`D-2026-10-29-1` 等）で示し、他所有者の値は写さない
- repo の設定変更・外部への投稿・spec repo への PR は利用者の明示の指示を待つ
- `_/` は git 管理外の作業領域（計画・`.claude/` の一時作成）

## hooks（.claude/settings.json）

- **SessionStart**: 日本語・委譲・`_/dotclaude` 経由・spec の扱い・Conventional Commits のリマインダー
- **PostToolUse**（`Edit|Write|MultiEdit`）: `.claude/hooks/rustfmt-on-edit.sh` が編集された `.rs` 1 ファイルだけに
  `rustfmt` をかける（`rustfmt.toml` が無い間は edition 2024。`.agents/`・`docs/spec/`・`target/` は対象外。
  失敗しても編集は止めない。整形の最終判定は CI）
