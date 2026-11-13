---
description: >
  完了ゲート規約。コマンドの新規実行の出力・終了コードなどの証拠なしに「完了した」「動作する」と宣言しない。
  この repo の検証コマンド（make check・make test-hooks、Cargo 導入後の fmt・clippy・test・deny）と、
  ゲートの通過は判断材料であって spec の「確定」ではないこと（ACPT-7）を定める。
---

# 完了ゲート規約

コマンドの出力・終了コード等の証拠なしに「完了した」「動作する」と宣言しない。完了の主張は
5 段階のゲートをすべて通過した後にだけ行う。

## 適用対象

実装・修正・生成など副作用を伴う作業をする全ての Agent とスキル（implement-issue・implement-issue-tree・
implement-review・implement-review-pr・create-pr 等）。サブエージェントの報告を受けて報告する main も含む。

## 5 段階の完了ゲート

| # | フェーズ | 内容 |
| --- | --- | --- |
| 1 | **特定** | 主張を証明するコマンドを決める（下の「この repo のコマンド」から選ぶ） |
| 2 | **実行** | そのコマンドを新規に実行する（前回のログ・キャッシュの結果を流用しない） |
| 3 | **読取** | 出力全体・終了コード・失敗数・警告を確認する（一部だけ読まない） |
| 4 | **検証** | 出力が主張を実際に裏付けているかを判断する（失敗 0・警告の扱い等） |
| 5 | **宣言** | 証拠を引用して完了を報告する |

## この repo のコマンド

現時点で存在するもの:

| 変更の対象 | コマンド | 内容 |
| --- | --- | --- |
| 全般（文書・ルール・scripts を含む） | `make check` | editorconfig-checker + shellcheck（CI の `check` ジョブと同一） |
| `scripts/hooks/`・`lefthook.yml` | `make test-hooks` | フックスクリプト自体の回帰テスト（CI の `test-hooks` ジョブと同一） |
| `lefthook.yml`・`scripts/hooks/` の時間予算 | `time lefthook run pre-commit --all-files` | `./git-hooks.md` の時間予算の確認 |
| `Makefile`・`scripts/*.sh` | `make help` と `scripts/help.sh` の一致 | `./makefile.md` の手順 5 |
| `.github/workflows/` | `actionlint`・PR 上での実際の CI の実行 | `./github-actions.md` |

Cargo の導入後（TASK-3 の雛形で make ターゲット化し、TASK-6 で CI に載せる予定。ターゲット名は未定）:

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`
- `cargo deny check`・`cargo audit`（`--locked` での実行・許可リスト・例外の扱いは `./dependency-policy.md`）

make ターゲットができた後は、上の個別コマンドではなく make ターゲットを正とする（ローカル・CI・hooks で
同じ契約を使う。`./makefile.md`）。DB を使う結合試験・契約テストの実行方法は TASK-5・TASK-6 で決まるまで未定。

## ゲートの通過と spec の「確定」

- ゲートの通過は「このコマンドがこの範囲で通った」という証拠であり、spec の振る舞いを「確定」にするものではない
- 「確定」への変更は人が記録する。AI・自動処理が「確定」にした件数は 0 でなければならない（ACPT-7）
- 〔仮想〕〔モック〕の合格で〔実機〕〔人の操作〕の項目を確定扱いしない（ACPT-7）。stub は実物ではない
- AI レビューの合格だけで安全と確定しない（CG-2）。人のセキュリティレビューは TASK-83
- 「標準準拠済み」と書けるのは適合性テストで確かめた範囲だけ（ACPT-33）

## 禁止事項

- 「〜のはず」「たぶん」「おそらく」等の推測語での完了の主張
- 新規に実行していない結果（前回のログ・他 worktree の結果）を根拠にすること
- サブエージェントの報告だけを根拠にした完了の宣言（報告が出力の引用を含む場合を除く）
- 終了コードを確認せずに成功と判断すること
- 試験が存在しないまま「試験が通る」と主張すること
- 失敗を通すために検査・試験の期待を緩めること（AUDB-6・AUDB-69）、フックを迂回すること（`./git-hooks.md`）

## 完了の書き方

```
検証結果: `make check` を実行。editorconfig-checker・shellcheck とも指摘 0 件（終了コード 0）。
```

```
検証結果: `make test-hooks` を実行。`# passed: N, failed: 0`（終了コード 0）。
```

証拠が足りない場合は「未検証」と明記し、実行が必要なコマンドを列挙する。判定は利用者に委ねる。

## 関連ルール

- `./delegation-impl.md` — 実装後の流れ（試験 → 依存監査 → セキュリティ監査 → 完了ゲート）
- `./security.md` — セキュリティ観点の完了条件
- `./conventional-commits.md` — コミット前の確認
