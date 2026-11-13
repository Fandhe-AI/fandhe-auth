---
description: >
  実装・レビュー中に見つけたスコープ外の事項を放置せず追跡する規約。追跡の正は spec の 05-tasks.md の
  TASK-N（と 06-roadmap.md の MS）であり、GitHub Issue は TASK-N に対応付けて起票する。
  起票・追記は利用者の承認後に限る。
---

# 実装対象外の追跡規約

## 目的

実装・レビュー中にスコープ外と判断した事項を放置しない。埋もれるのを防ぐため、見つけた時点で
既存の追跡先（TASK-N・Issue）に結び付け、追跡できる状態にする。

## 追跡の正と Issue の関係

- 追跡の正は spec の `05-tasks.md` の `TASK-N` と `06-roadmap.md` の MS（`D-2026-10-28-4`）
- GitHub Issue は TASK-N の実施単位として扱う。Issue の本文に対応する `TASK-N`（複数可）と対象の振る舞い ID を書く
- 1 つの TASK を複数 Issue に分けてよい。Issue に対応する TASK が無い事項は、TASK の追加が要る（下記）
- 2026-10-29 時点で Issue は未起票。起票の開始は利用者の明示の指示を待つ
- この repo は public、spec repo は private。Issue の本文には spec の本文を写さず、`TASK-N`・振る舞い ID・
  決定 ID で引く（`./spec-reference.md`）。秘密値・実機の名前・内部のアドレスを書かない

## フロー

1. **検出**: スコープ外と判断した事項と、その根拠（関係する spec ID・ファイル）をメモする
2. **既存の TASK の確認**: `spec-researcher` に `05-tasks.md` から該当する `TASK-N` を探させる
3. **既存 Issue の確認**:

    ```bash
    gh issue list --search "${KEYWORD}" --state open
    ```

    キーワードは `"${KEYWORD}"` でクォートして渡す。`TASK-N` でも検索する
4. **利用者の承認を得る**: 既存 Issue への追記か、新規起票か、TASK の追加が要るかを利用者と確認する
5. **記録**:
    - 既存 Issue がある場合:

        ```bash
        gh issue comment "${ISSUE_NUMBER}" --body "$(cat <<'EOF'
        （本文。対応する TASK-N と根拠の ID を書く）
        EOF
        )"
        ```

    - 無い場合: `create-issue-tree` または `create-issue` で、対応する TASK・MS の親 Issue 配下に起票する
    - 対応する TASK が spec に無い場合: spec の変更が要る。`docs/spec/` はこの repo では編集しない
      （submodule）。spec repo への反映の手順は spec に定めが無く未定のため、利用者の判断を待つ
6. **元 PR・レビューに切り出し先を記録**: PR 本文かコメントに、切り出し先の Issue 番号と `TASK-N` を書く

## コード内の印

- 後で対処する箇所の `TODO` には追跡先を付ける（例: `// TODO(TASK-4): 構造化ログの部品を選定後に置き換える`）
- 追跡先の無い `TODO` を残さない（`./code-comment-style.md`）

## 注意

- 利用者の承認なしに Issue を起票・追記しない。TASK の追加・優先度・担当の決定は人が行う
- スコープ外の修正を現在の PR に混ぜない（別 Issue・別 PR で対処する）
- 相手側（Hub・fandhe-backend・fandhe-frontend・fandhe-db）の待ちは仮定 ID（`A-2026-10-28-n`）と
  相手側の TASK（repo 名付き）で引き、相手側の repo へ勝手に起票しない
