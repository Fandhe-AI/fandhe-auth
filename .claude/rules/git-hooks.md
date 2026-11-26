---
paths:
  - "lefthook.yml"
  - "lefthook-local.yml"
  - "scripts/hooks/**"
description: >
  lefthook.yml・scripts/hooks/ を編集する際の方針。フックは staged 限定で一瞬で終わる検査と
  コミット単位でしか意味がない検査に限り、テスト・ビルド・全体 lint や CI と重複する検査は
  CI（.github/workflows/ci.yml）へ置く。時間予算・判断手順・計測方法を規定する。
---

# Git hooks（lefthook）編集ルール

## 目的

フックはコミットのたびに実行されるため、遅いフックはそのまま開発速度の低下になる。
フックはローカルでの早期検知専用であり、CI の代替ではない。**開発速度を落とさないことを最優先**し、
重い検査・網羅的な検査は CI（`.github/workflows/ci.yml`）に任せる。

## 時間予算

| フック | 予算 | 判定方法 |
| --- | --- | --- |
| pre-commit | 全ジョブ合計 1 秒未満 | `lefthook run pre-commit --all-files` の `summary: (done in N seconds)` |
| commit-msg | 即時（外部コマンド・ネットワーク不可） | 実コミット時に表示される commit-msg の summary（メッセージファイルを引数に取るため `--all-files` では計測しない） |
| pre-push・post-checkout・post-merge 等 | 原則追加しない | 追加する場合は理由と実測値を PR 本文に記載する |

`--all-files` は全追跡ファイルを対象にする最悪ケースであり、実際のコミット（staged のみ）はこれより速い。

## フックに置かないもの（→ `ci.yml` へ）

- テストスイート（unit / integration / e2e）・ビルド・型チェック
- リポジトリ全体を走査する lint / fmt チェック（`make check` 全体の呼び出しを含む）
- 依存パッケージのインストール・ネットワークアクセス
- `ci.yml` のジョブと同じコマンドを同じ範囲で実行するもの
- フックスクリプト（`scripts/hooks/*.sh`）自体のテスト — これも CI のジョブとして実行する
  （`ci.yml` の `test-hooks` ジョブ。実体は `scripts/hooks/test-hooks.sh`〔入口。本体は `test-hooks-N.sh`、共有部は `test-hooks-lib.sh`〕、ローカルでは `make test-hooks`）

## フックに置いてよいもの

- `{staged_files}` に限定し、`glob` / `exclude` で対象を絞った 1 秒未満の検査
- CI ではコミット単位で検知できない、または push 後では遅いもの
  - secret-scan（`scripts/hooks/secret-scan.sh`）: 秘密情報は push 前に止める必要がある。
    引数なしが staged 差分（フック用）で、`--tracked`（追跡ファイル全体）はフックから呼ばない（`make secret-scan`・CI 用）
  - commit-msg（`scripts/hooks/commit-msg-check.sh`）: CI の `pr-title` は PR タイトルしか検証しない

### 明示的な例外

- pre-commit の editorconfig-checker / shellcheck は CI の `check` ジョブ（`make check`）と重複するが、
  staged ファイル限定で合計 0.3 秒程度のため維持する（2026-10-11 計測・決定）
  - 2026-11-21 追記（実測。上の決定の文言は書き換えない）: 追跡している `*.sh` が 31 件・合計約 4.9 秒分の検査になり、
    `--all-files` では 0.3 秒に収まらない（staged のみの実コミットは数ファイルで 0.3 秒前後）。シェルスクリプトの検査は
    `scripts/hooks/shellcheck-staged.sh` で並列に流す（最大 `getconf _NPROCESSORS_ONLN` 個のバッチに分け、
    バッチごとに shellcheck を 1 回だけ呼ぶ。起動の回数はファイル数に比例しない。
    lefthook は glob が 2 つあると同じファイルを 2 回渡すため、重複は除く）。検査時間はほぼ行数に比例するので、
    最大だった `test-hooks.sh`（2818 行・単独で約 1.9 秒）は共有部と 7 つの part（各約 300〜440 行）に分けた（入口は変えない）。
    この実装の `--all-files` の summary は約 1.0〜1.35 秒（2026-11-21 に 10 コアの 1 台で測定。バッチ単位の実装は 5 回で 1.01〜1.11 秒だが、直後の load average が 5 前後で他の負荷が残っていた。
    それ以前の、ほかの負荷が無いときの 12 回は 1.1〜1.35 秒。ビルドと並走すると 2 秒前後）で、予算の 1 秒未満には届いていない
    （残りは shellcheck 自体の CPU 時間で、秘密情報の検査と editorconfig-checker の並走も効く）。予算の値は変えていない。残りは #6056（TASK-6）で追う
- `secret-scan.sh` 冒頭の正規表現自己テストは ms 単位の fail-closed ガードであり、
  上記「フックスクリプト自体のテスト」には当たらない。削除・CI への移動はしない

## 書き方の規約

- `pre-commit` の `parallel: true` を維持する（直列化しない）
- `{staged_files}` と `glob` / `exclude` で対象を絞り、外部取得物の `.agents/skills/**`・`.claude/skills/**` を除外する
- 実体が長くなる場合は `scripts/hooks/` へ切り出し、`lefthook.yml` は呼び出しのみにする
- 条件付き実行が必要な場合は lefthook の `skip` / `only` を使ってよい
  （`.claude/skills/lefthook/references/examples/skip.md`）

## フックスクリプトの書き方

秘密情報検査のように「検出できなかった = 合格」になる検査は、検査そのものの失敗を合格と取り違えると
黙って素通りする（fail-open）。以下は過去に実際に見つかった不具合クラスであり、`scripts/hooks/` を
書く・変える際は必ず守る。各項目には `scripts/hooks/test-hooks-N.sh`（`test-hooks.sh` が順に起動する part）に回帰ケースがある。

- (a) 検査エラーを「一致なし」と同一視しない（fail-closed）
  - `grep` の終了状態は 0 = 一致・1 = 不一致・2 以上 = エラー。`if grep -q ...` / `grep ... || true` は
    エラーを不一致として扱うため、終了状態を受けて 2 以上は NG（拒否）にする
  - bash の `[[ =~ ]]` も正規表現のコンパイル失敗で 2 を返す。`secret-scan.sh` は終了状態を区別し、
    冒頭の自己テストで「一致すべき例が一致する」ことも確認してから走査する
  - 検査を完了できない場合の終了コードは検出時（1）と分ける（`secret-scan.sh` は 2）
- (b) ASCII のパターンで照合する検査は `LC_ALL=C` で実行する
  - UTF-8 ロケールのまま非 UTF-8 のバイト列（Latin-1 等）を含む行を照合すると、macOS の awk / grep は
    異常終了・バイナリ扱いし、bash の `[[ =~ ]]` は（macOS・glibc とも）同じ行のトークンに一致しなくなる
  - bash はスクリプト内で `export LC_ALL=C` すると以降の照合に効く。`grep` は `-a` も付ける
- (c) `git diff` の `+++ b/<path>` は `diff --git` から最初の `@@` までの間でだけ解釈する
  - hunk 内の追加行の本文が `++ b/...`・`++...` で始まると diff 上は `+++...` になる。行頭だけで
    ヘッダと判定すると、その行が走査から漏れ、以降の行のファイル名も取り違える
  - ヘッダ区間以外の `+` 行はすべて走査する。空白を含むパスの末尾タブ・`+++ "b/..."` の引用符も扱う
- (d) `git diff` の出力形式を利用者の git 設定に依存させない
  - `--no-color --no-ext-diff` は必須（`color.diff=always` / `color.ui=always` で行頭に色コードが付き
    `+` 行を検出できない。`diff.external` は本文を差し替える）。内容検査では `--no-textconv` も付ける
  - リネーム・コピー検出で移動先の行が出ない・除外パスからの移動が素通りする場合は `--no-renames`
  - ファイル名を使う場合は `--src-prefix=a/ --dst-prefix=b/`（`diff.noprefix`・`diff.mnemonicPrefix` 対策）、
    パス一覧は `-z`（非 ASCII のパスが `core.quotePath` で引用符付き 8 進表記になり末尾一致しなくなる）
- (e) パイプの左辺・`$(...)` に置いた git の失敗を隠さない
  - `git diff ... | while ...` や `done <<< "$(git diff ...)"` は git が失敗しても `set -e` /
    `pipefail` で止まらず「対象なし」として合格し得る。`git ... > "$tmp/file" || exit 2` のように
    一時ファイルへ書き出して終了状態を確認してから読む
  - `cd "$(git rev-parse --show-toplevel)"` も失敗が `cd ""` に化ける。先に変数へ代入して確認する
- (f) 検査の除外の名前は、走査対象の一覧から足さない（#6000）
  - 除外の名前は検出器の側（`secret-scan.sh` の repo の `skills-lock.json`）から読む。走査対象の同名ファイル（対象の index の一覧）は
    除外名の追加に使わない。走査対象から名前を足すと、PR が同じ変更で名前を足して自分を除外にできる
  - 走査対象の index の一覧は、検証と除外の縮小にだけ使う。検証は通常ファイル・大きさ・構文・対応形式で、違反は検査不能（2）。
    除外にするのは検出器の一覧と対象の index の一覧の共通部分だけで、対象の index で登録が消えた名前は除外も取り消す
    （対象の index に一覧が無ければ除外は 0 件。intent-to-add の一覧は未登録として扱う）。別の checkout（CI）も同じ（`./github-actions.md`）
  - 検出器と走査対象が同じ checkout（ローカルの pre-commit・`make secret-scan`）では、作業ツリーの一覧も走査対象の側にある。
    作業ツリー・HEAD・index の 3 つすべてに登録された名前だけを使い（共通部分。登録の削除をステージしたあと作業ツリーだけへ戻しても除外は残らない）、未コミットの編集で除外を足せないようにする。
    読み元の解決は `CDPATH` を外し、`BASH_SOURCE` から親パスをシェルの展開で取る（dirname のコマンド置換は末尾の改行を落とす）。
    一覧の解析は JSON 全体の厳格な解析（bash のみ）で、不正は検査不能（2）。構文解析の前に入力全体が正しい UTF-8（最短形・
    サロゲート不可・U+10FFFF 以下）であることをバイト単位で確かめる。NUL を含む一覧・普通のファイルでない一覧も 2。
    数値は桁数を切らず全体を解析する。入れ子の深さの上限は 32
  - 一覧の大きさに上限を置く（暫定 64 KiB。bash の解析は遅く、巨大な一覧でフックが長時間止まるため。値の確定は人が行う）。
    超えたら固定の診断文で 2。読み取りは上限 + 1 バイトまでの複写で行い、HEAD 側は blob の大きさを先に確かめる
  - HEAD は 1 回だけ確定して固定の ID で読む。結果を返す前（集計の直前）に HEAD を取り直し、固定した ID と違えば 2
    （未作成ブランチのまま始まり、走査中にブランチが作られた場合も含む）。古い基準の差分で 0 を返さない
  - index は走査の最初に 1 回だけ複写して固定し、登録の確認・パス一覧・本文の走査はその複写から行う。結果を返す前に元の index を
    もう 1 回だけ複写し、その複写から取った一覧（`ls-files -s` と intent-to-add の印）が固定時と違えば 2。複写の失敗も 2。
    元の index が無いときは空として扱う。固定時にあった index が終了時に無ければ、登録が 0 件でも 2（固定時の有無を記録して判定する。一覧の差だけで判定しない）。
    index がシンボリックリンクなら、開始時も終了時も辿らず 2（`rev-parse --git-path index` を `--path-format=absolute` なしで得た、解く前のパスを
    `-L` で見る。実体に解いて固定すると、走査中のリンクの付け替えに気づけない。通常の運用で index はリンクにならない）。
    通常ファイルでない（FIFO・デバイス等）ときも複写せず（cp が止まらないよう）2。確認と `cp` の間に差し替えられる窓は残り、
    窓を狭めるだけで止まり得る（0 は返さない。監視付きの複写にはしない）。元の index のパスは git 自身の解決
    （`rev-parse --git-path index`）で受ける（相対パスの `GIT_INDEX_FILE` を git は作業ツリーのルート基準で読む）。
    終了時に、複写の後で同じ条件（呼び出し元の `GIT_INDEX_FILE`）のまま git の解決をやり直し、開始時の絶対パスと違えば 2
    （親ディレクトリのシンボリックリンクの付け替えで、実体の複写と git が読む index がずれる経路を塞ぐ。確認の後の付け替えは窓として残る）
  - 外部コマンド（git・readlink・mktemp・rm・cp・head・wc・sleep・tr・cmp）は、使う前にシェルの組み込み（`command -v`）で確かめ、
    欠けていれば固定の診断文で 2（欠けた `sleep` が待ち時間なしの打ち切りを生む等、失敗の吸収を避ける）。
    診断は `--summary` でも出るよう元の標準エラー（fd 4）へ出す。使うコマンドを足したら一覧にも足す（試験が踏む経路の漏れを検出する。踏まない経路は検出できない）
  - HEAD の一覧が壊れている・読めないときも 2 で止め、固定の診断文だけを出す（名前・パスは出さない）。
    この場合に除外なしで続行する等の扱いは人が決める（現状は止める）
  - 除外はパスの形（`*.lock`・`.claude/skills/**`・`.agents/skills/**`）で決めず、検証した名前の literal pathspec にする。
    一覧が読めない・形が想定と違うときは検査不能（2）で止める
  - 残る限界（TASK-6・#6000 で追跡）: 除外は登録名のディレクトリ単位のため、その下の既存ファイルへの追記・新規ファイル・
    リネームによる移動は本文の走査から外れる。登録名の下の変更は人のレビューで確かめる。内容（blob・`computedHash`）に結び付けた除外は未実装
- 検出時はファイル名（とパターン種別）のみを出力し、値そのものは出力しない
- macOS 標準の bash 3.2 で動く書き方にする（lefthook は PATH 上の `bash` で起動する。`mapfile`・
  連想配列・`${var,,}`・空配列の `"${arr[@]}"`（`set -u` で未定義扱い）は使わない）
- プロセス起動回数を diff 行数・ファイル数に比例させない（1 行ごとに `grep` 等を起動しない）
- 例示値（トークン等）はスクリプト・テスト内で実行時に組み立て、そのファイル自体のコミットが
  `secret-scan.sh` に検知されないようにする

## 検査を追加するときの判断手順

1. その検査は `ci.yml` で既に実施されているか
    - 実施済みで、staged 限定にしても 1 秒以上かかる → フックに入れない
2. staged ファイルに限定できるか
    - できない → CI へ置く
3. CI へ置く場合は `ci.yml` にジョブを追加し、集約ジョブ `ci-complete` の `needs` にも必ず追加する
    - ツールの導入・バージョン固定は既存の `check` ジョブの方式（バージョン + SHA256 検証）に合わせる
4. `scripts/hooks/` を変更した場合は `scripts/hooks/test-hooks-N.sh` の該当する part（共有部は `test-hooks-lib.sh`）に検出・許可（OK / NG）ケースを追加し、
    `make test-hooks` を通す（Linux・macOS の検証は CI の `test-hooks` ジョブが行う）
5. 編集後に以下を実行し、summary の合計時間をユーザーへ報告する（1 秒以上なら構成を見直す）

```bash
time lefthook run pre-commit --all-files
```

## バイパス禁止

遅いからといって `--no-verify` や `LEFTHOOK=0` で回避しない。遅いフックはフック側を直す
（検査を CI へ移す・staged 限定にする・対象 glob を絞る）。
