# CLAUDE.md

## Overview

`fandhe-auth` は Keycloak を使わない Rust 製の共通認証基盤の実装リポジトリ（public、`MIT OR Apache-2.0`）。
Hub が組み込むライブラリ（本体）で、型・判定・拡張契約・ストレージ契約・安全条件を持つ（`D-2026-10-30-1`・`LIB-1`）。
passkey・認証セッション・失効・復旧・登録済み自社クライアント向けの OIDC・ローカルエージェント認可を担う。
実行・運用（HTTP の受付・Gate の登録・Cookie の発行・DB の実装と migration・配送）は Hub の担当で、本体は作らない。

- 仕様は private の `Fandhe-AI/fandhe-auth-spec` を `docs/spec/` に submodule で置いている（`update = none`。
  取得手順と読み方は `.claude/rules/spec-reference.md`）。**仕様の正は `docs/spec/04-behavior/`**
- spec 中の「`fandhe-auth-backend` の repo／workspace」は本リポジトリを指す（`D-2026-10-29-1` の読み替え）
- 現在地: Phase 1〜6（仕様・PoC）は完了。2026-10-30 の方針変更（`D-2026-10-30-1`）で Phase 4〜6 をやり直し、
  2026-11-04 に着手判定（`D-2026-11-04-6`）。マイルストーンは MS-1〜MS-12（MS-12 はトークン交換の延期。`D-2026-11-06-4`）。
  次は `06-roadmap.md` の MS-1「決定と土台」（TASK-3 雛形・TASK-5 契約テスト用 DB）。
  TASK-4 の成果物（選定の記録 `D-2026-11-20-7`〜`13`・試し実装の結果 `D-2026-11-20-14`。feature の選び・K11）は spec に記録済みで、#29 は 2026-11-20 に閉じた。MSRV 1.100・edition 2024 の `Cargo.toml` への反映は別の PR
  ルートの `Cargo.toml`（workspace）と段階 1 の crate の雛形（TASK-3 等）は入っている。実装は利用者の指示で Issue ツリーから進める。`deny.toml` はまだ無い（TASK-668）
- Issue: 2026-11-15 に spec の全 712 TASK を Issue ツリーとして起票し、2026-11-18〜19 に最新の spec（`1df12ad`）へ再同期済み。
  ルートは #8、Phase N = MS-N（12 Phase。Phase 12 は延期したトークン交換の MS-12）。有効な TASK は 680 件で、
  各 TASK の下に 2h ごとの sub-issue があり合計 4,382 件（移管・置き換えの TASK は not planned で閉じてある）。
  担当が人間の TASK には `owner:human`、共同の TASK には `owner:joint` のラベルが付いている
- 利用者の指示で `implement-issue-tree` を回すときは、ルート #8 に `skipLabels: ["owner:human","owner:joint"]` を必ず付ける。
  sub-issue のタイトルと受け入れ条件は目安で、正は spec の原文
- crate は repo ルートの 1 つの workspace に置く。crate 名は接頭辞 `fandhe-auth-` 付き、ディレクトリは接頭辞を除いた
  `crates/<名前>`（`D-2026-10-27-8`・`D-2026-10-28-1`）。作る範囲は段階 1（`core`・`secret`・`db-tests`・`client`、
  OIDC 共通部品 `plugin-oidc`〔`D-2026-11-20-2`〕）、段階 2（`backend`・`webauthn-wasm`・`frontend` 等）は作る前に利用者へ確かめる。
  `db-pg`・`db-fandhe`・`db-migrate`・`server` は Hub の担当で作らない。段階は〔提案〕で、決定済みなのは名前
  （`LIB-14`・`D-2026-10-29-2`）。詳細は `.claude/rules/coding-rust.md`・`.claude/rules/library-boundary.md`
- `crates/backend` は TASK-9 の暫定の置き場（`D-2026-11-20-1`）。`interim` に最初の中身を足す前、または `fandhe-backend` への依存を足す前の
  早い方で、人が `LIB-15` の判定を記録する（`D-2026-11-20-3`）。記録するまでは、`crates/backend` のどこにも（`interim` の外も含めて）中身を足さない。最初のリリースから除く仕組みは未決（`Q-2026-11-20-1`）
- 判定・値の決定・例外の承認・「確定」への変更は人が行う。Claude Code は手順・仕組み・試験を作って流すところまで（ACPT-7）

開発基盤（Makefile・lefthook・CI・ツール導入）の説明は [README.md](README.md) を参照する。

## 委譲方針（必読）

main の役割は **対話・計画・委譲・報告** に徹する。token を消費する作業（調査・実装・試験・レビュー・文書作成）は
サブエージェントへ委譲する。詳細は `.claude/rules/delegation.md`（調査）・`.claude/rules/delegation-impl.md`（作成・編集）。

### パスベースの目安

| 操作対象パス | モード | 適用ルール |
|---|---|---|
| `docs/`・`_/`・`.claude/`・`crates/` の**閲覧のみ** | 調査・設計 | `delegation.md` |
| `crates/`・`Cargo.toml`・`deny.toml`・`scripts/`・`Makefile`・`.github/`・`.claude/agents/`・`.claude/rules/`・`CLAUDE.md`・`README.md`・`AGENTS.md` の**作成・編集** | 作成・編集 | `delegation-impl.md` |

### model 配分

| tier | alias | 用途 |
|---|---|---|
| 最上位 | `opus` | セキュリティ監査・横断的な設計判断 |
| 標準 | `sonnet` | 調査・実装・試験・文書作成・レビュー |
| 軽量 | `haiku` | 機械的な集計・ツール出力と規約の照合 |

独立したタスクは同一メッセージ内で複数 Agent を起動して並列実行する。

## Conventions

- 日本語でやりとりする（技術用語・識別子は原語）
- Conventional Commits（`<type>[(<scope>)][!]: <要約>`）。`--no-verify` は使わない
- 実装は `implement-issue` で計画（`_/local-plans/`）を作り、**利用者の承認後**に着手する
- コミット・PR の前にセキュリティ確認（`security-auditor`）。AI のレビュー合格を安全の確定として扱わない（CG-2・TASK-83）
- 根拠は spec の ID（`SECR-11`・`D-2026-10-29-1` 等）で示し、他所有者の値は写さない
- repo の設定変更・外部への投稿・spec repo への PR は利用者の明示の指示を待つ
- `_/` は git 管理外の作業領域（計画・`.claude/` の一時作成）
- 外部取得のスキル（`.agents/skills/`・`.claude/skills/`）は `skills-lock.json` で管理し、編集しない。改修は `contribute-skill` で upstream へ
- `.claude/workflows/implement-issue-tree.js` は `../skills/implement-issue-tree/scripts/implement-issue-tree.js` への symlink であり、実体を直接編集しない（更新は skills-lock の追従で入る）
