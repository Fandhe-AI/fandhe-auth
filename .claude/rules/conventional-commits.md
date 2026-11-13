---
description: >
  Conventional Commits 規約の正本。commit-msg フック（scripts/hooks/commit-msg-check.sh）が検証する形式・
  type 一覧と、フックが検証しない慣習（scope の候補・要約の長さ・本文・footer）を分けて定める。
  PR タイトルも同じ検証を通る（squash merge）。--no-verify 禁止。
---

# Conventional Commits 規約

## 形式（フックが検証する）

```
<type>[(<scope>)][!]: <要約>

[本文]

[footer]
```

`scripts/hooks/commit-msg-check.sh` が件名（コメント行・空行を除いた最初の行）を次で検証する。
これが形式の正であり、本ファイルと食い違う場合はフックを読んで本ファイルを直す。

| 項目 | フックの判定 |
| --- | --- |
| type | `feat` `fix` `docs` `style` `refactor` `perf` `test` `build` `ci` `chore` `revert` のいずれか |
| scope | 任意。1 つだけ、英小文字・数字・ハイフンのみ（`[a-z0-9-]+`）。カンマ区切りの複数指定は不可 |
| `!` | 任意。破壊的変更の印 |
| 区切りと要約 | `: `（コロン + 半角空白）の後に 1 文字以上 |
| 素通り | `Merge `・`Revert `（git 既定の文言）・`fixup! `・`squash! ` で始まる件名 |

PR タイトルも CI の `pr-title` ジョブで同じスクリプトに通す。squash merge では PR タイトルがそのまま
`main` のコミットの件名になるため、PR タイトルも同じ形式で書く。

## type の使い分け

| type | 用途 |
| --- | --- |
| `feat` | 振る舞いの追加（spec の振る舞い ID を満たす実装） |
| `fix` | 不具合の修正 |
| `docs` | 文書のみの変更（README・rustdoc のみ） |
| `style` | 振る舞いを変えない書式のみの変更（rustfmt・空白） |
| `refactor` | 振る舞いを変えない構造の変更 |
| `perf` | 性能の改善 |
| `test` | 試験の追加・修正のみ |
| `build` | Cargo 設定・依存・`deny.toml`・ビルド手順の変更 |
| `ci` | `.github/workflows/` の変更 |
| `chore` | 上記に当たらない補助的な変更（Makefile・scripts・`.claude/` の整備等） |
| `revert` | 取り消し |

## scope の候補（慣習。フックは検証しない）

- crate のディレクトリ名: `core` `webauthn` `provider` `keys` `db-contract` `db-pg` `db-migrate` `client`
  `secret` `types` `frontend` `webauthn-wasm` `backend` `server`
- それ以外: `skills`（外部取得スキルの更新）・`claude`（`.claude/` の agents・rules 等）・`ci`・`hooks`
  （`lefthook.yml`・`scripts/hooks/`）・`deps`（依存の更新）
- 複数 crate にまたがる場合は scope を省略する。新しい crate を足したら同じ変更で本リストに足す

## 慣習（フックは検証しない）

- 要約は **72 文字以下**、末尾に句点・ピリオドを付けない。日本語可（`./japanese-style.md`）
- 要約は「〜を追加」「〜を修正」のように何をしたかを短く書く
- 本文は件名から 1 行空け、背景・理由と根拠の spec ID・決定 ID（例: `SECR-11`・`D-2026-10-29-1`）・
  対応する `TASK-N` を書く
- footer: `Refs #<issue>` で Issue を参照する。共同著者の行はその時点の attribution の指示に従う
- 秘密値（`./secret-values.md`）・実機の名前・ホームのパス・内部のアドレスを書かない

例:

```
feat(secret): 秘密値の型の Debug を固定文字列にする
build(deps): webauthn-rs を 0.5.5 に固定する
chore(claude): 委譲ルールを追加
fix(db-pg)!: 時刻の列を UTC ミリ秒の整数に変更
```

## 破壊的変更

件名の `type!` / `type(scope)!`、または本文の `BREAKING CHANGE:` footer で表す。両方書いてもよい。
公開 crate（`client`・`secret`・`types` 等）の公開 API・設定の型・DB の列の形を変える場合が該当する。

## フックの迂回禁止

**`--no-verify`・`LEFTHOOK=0` によるフックの迂回は禁止**（詳細は `./git-hooks.md`）。フックが失敗したら
原因を調べて直し、新しいコミットを作り直す。フック自体の不具合は `scripts/hooks/` を直して `make test-hooks` を通す。

## コミットの実行

HEREDOC で渡し、引用符・特殊文字を安全に扱う。コミット・push・PR 作成は利用者の指示があるときだけ行う。

```bash
git commit -m "$(cat <<'EOF'
feat(secret): 秘密値の型の Debug を固定文字列にする

SECR-11・SECR-12 に従い、値・長さ・ハッシュを出さない。TASK-N。
EOF
)"
```
