# TASK-6 CI の実行時間の記録

`TASK-6`（9/10。Issue #56）の成果物。CI の 1 回あたりの実行時間をジョブごとに記録する形式と、取得の入口と、初回の実測を残す。
この記録は `TASK-66`（PR ごとと定期の時間の予算、`ACPT-33`・`ACPT-37`・`ACPT-38`・`ACPT-39`・`ACPT-44` の周期。担当は共同）の入力になる。

この文書は実行したコマンドと出力の事実だけを書く。予算の値・「1 回あたり」の読み方・周期は決めない。判定は人が行う（`ACPT-7`）。
spec の本文は写さない。

## 1. 目的と範囲

- 終わった実行を GitHub の REST（GET のみ）から後で読む。ワークフロー（`.github/workflows/`）・必須チェックは変えず、CI の中では流さない。
- 系統の対応:
  - PR ごと: `ci.yml`・`ai-review.yml` の `pull_request`
  - main への push: `ci.yml` の `push`（`main`）
  - 定期: `scheduled.yml`（今は `workflow_dispatch` だけの枠。`TASK-681`）
  - 参考: `update-external.yml` の `schedule`

## 2. 記録の形式

行の TSV（`scripts/ci-timing.sh rows`・`fetch` の出力）。許可した 10 列だけを出す。

| 列 | 内容 |
| --- | --- |
| `workflow`・`event` | 系統 |
| `run_id`・`run_attempt` | 実行の識別（attempt は jobs API の既定の最新） |
| `head_sha` | 先頭 12 文字 |
| `job_name` | タブ・改行は空白に置換 |
| `conclusion` | 未完了は `-` |
| `created_epoch`・`started_epoch`・`completed_epoch` | UTC の秒。欠けは `-` |

runner の名前・group・label・step・URL・ブランチ名は出さない。

集計（`scripts/ci-timing.sh summary`。Markdown。単位は整数の秒）:

- 対象: 実行は `success`・`failure` だけを取る（それ以外は件数を stderr に出して除く）。
  ジョブは `conclusion` が `success`・`failure` で、時刻が数値かつ created <= started <= completed のものだけを集計し、他は除外の件数に数える（時間は `-`）。
- ジョブごと（系統 × ジョブ名）: 件数（集計 / 除外）、実行 = `completed - started`、待ち = `started - created` の中央値・p90・最大。
- 実行ごと（系統）: 壁時計 = 集計したジョブの `max(completed) - min(created)`、ジョブの実行時間の合計。それぞれ中央値・p90・最大。両方を出し、どちらで「1 回あたり」と読むかは決めない。
- 分位は最近傍順位（昇順の ceil(p × n) 番目。中央値は p = 0.5）。出力は決定的。

## 3. 取得の手順

前提: `gh` の認証（読み取り）・`jq`・ネットワーク。

```bash
make ci-timing                                   # 5 系統をまとめて取る（scripts/ci-timing.sh report）
scripts/ci-timing.sh fetch --workflow ci.yml --event push --branch main --limit 30 | scripts/ci-timing.sh summary
```

0 件の取得・取得の失敗・形式の不一致は終了コード 2 で、空の記録を成功にしない。

## 4. 初回の記録

- 取得（UTC）: 2026-11-28T04:27:25Z
- `origin/main`: `f9aa8586eff49057d90b86272616edd437d07353`
- ツール: gh 2.103.3、jq 1.8.1
- 各系統の直近 30 件までの完了した実行（`--limit 30`）


### ci.yml / pull_request

#### Per job (seconds)

| workflow / event | job | counted / excluded | exec median | exec p90 | exec max | wait median | wait p90 | wait max |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| ci.yml / pull_request | PR title (Conventional Commits) | 7 / 0 | 5 | 9 | 9 | 2 | 102 | 102 |
| ci.yml / pull_request | ci-complete | 7 / 0 | 3 | 4 | 4 | 2 | 3 | 3 |
| ci.yml / pull_request | deps audit frame (tool install only; checks wired in TASK-680) | 7 / 0 | 4 | 6 | 6 | 2 | 169 | 169 |
| ci.yml / pull_request | hook script tests (macOS bash 3.2) | 7 / 0 | 458 | 590 | 590 | 3 | 570 | 570 |
| ci.yml / pull_request | hook script tests (macos-latest) | 7 / 0 | 462 | 536 | 536 | 3 | 761 | 761 |
| ci.yml / pull_request | hook script tests (ubuntu-latest) | 7 / 0 | 166 | 202 | 202 | 2 | 14 | 14 |
| ci.yml / pull_request | make build + make test | 7 / 0 | 38 | 47 | 47 | 2 | 155 | 155 |
| ci.yml / pull_request | make check (editorconfig-checker + shellcheck) | 7 / 0 | 18 | 19 | 19 | 2 | 152 | 152 |
| ci.yml / pull_request | make it-db-ci (DB integration frame) | 7 / 0 | 32 | 38 | 38 | 2 | 177 | 177 |
| ci.yml / pull_request | make lint (rustfmt + clippy + crate-deps) | 7 / 0 | 21 | 24 | 24 | 2 | 185 | 185 |
| ci.yml / pull_request | secret scan (tracked files) | 7 / 0 | 16 | 19 | 19 | 2 | 14 | 14 |

#### Per run (seconds; wall = max(completed) - min(created) over counted jobs, job sum = sum of exec)

| workflow / event | runs | wall median | wall p90 | wall max | job sum median | job sum p90 | job sum max |
| --- | --- | --- | --- | --- | --- | --- | --- |
| ci.yml / pull_request | 7 | 544 | 1229 | 1229 | 1200 | 1344 | 1344 |

### ci.yml / push (main)

#### Per job (seconds)

| workflow / event | job | counted / excluded | exec median | exec p90 | exec max | wait median | wait p90 | wait max |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| ci.yml / push | PR title (Conventional Commits) | 0 / 14 | - | - | - | - | - | - |
| ci.yml / push | ci-complete | 14 / 0 | 3 | 4 | 4 | 2 | 2 | 2 |
| ci.yml / push | deps audit frame (tool install only; checks wired in TASK-680) | 14 / 0 | 4 | 5 | 5 | 2 | 2 | 2 |
| ci.yml / push | hook script tests (macOS bash 3.2) | 6 / 0 | 441 | 472 | 472 | 3 | 1168 | 1168 |
| ci.yml / push | hook script tests (macos-latest) | 14 / 0 | 558 | 861 | 868 | 380 | 1350 | 1871 |
| ci.yml / push | hook script tests (ubuntu-latest) | 14 / 0 | 157 | 189 | 198 | 2 | 2 | 3 |
| ci.yml / push | make build + make test | 14 / 0 | 42 | 56 | 61 | 2 | 2 | 3 |
| ci.yml / push | make check (editorconfig-checker + shellcheck) | 14 / 0 | 17 | 19 | 21 | 2 | 3 | 3 |
| ci.yml / push | make it-db-ci (DB integration frame) | 14 / 0 | 37 | 41 | 65 | 2 | 2 | 2 |
| ci.yml / push | make lint (rustfmt + clippy + crate-deps) | 14 / 0 | 24 | 39 | 47 | 2 | 2 | 3 |
| ci.yml / push | secret scan (tracked files) | 14 / 0 | 15 | 17 | 17 | 2 | 2 | 41 |

#### Per run (seconds; wall = max(completed) - min(created) over counted jobs, job sum = sum of exec)

| workflow / event | runs | wall median | wall p90 | wall max | job sum median | job sum p90 | job sum max |
| --- | --- | --- | --- | --- | --- | --- | --- |
| ci.yml / push | 14 | 1009 | 1970 | 2434 | 1043 | 1332 | 1340 |

### ai-review.yml / pull_request

#### Per job (seconds)

| workflow / event | job | counted / excluded | exec median | exec p90 | exec max | wait median | wait p90 | wait max |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| ai-review.yml / pull_request | codex / post_feedback | 28 / 2 | 7 | 9 | 10 | 2 | 24 | 39 |
| ai-review.yml / pull_request | codex / preflight | 30 / 0 | 5 | 9 | 10 | 2 | 66 | 92 |
| ai-review.yml / pull_request | codex / review | 28 / 2 | 45 | 55 | 62 | 2 | 2 | 3 |

#### Per run (seconds; wall = max(completed) - min(created) over counted jobs, job sum = sum of exec)

| workflow / event | runs | wall median | wall p90 | wall max | job sum median | job sum p90 | job sum max |
| --- | --- | --- | --- | --- | --- | --- | --- |
| ai-review.yml / pull_request | 30 | 65 | 136 | 176 | 56 | 66 | 74 |

### scheduled.yml / workflow_dispatch

0 runs

### update-external.yml / schedule

#### Per job (seconds)

| workflow / event | job | counted / excluded | exec median | exec p90 | exec max | wait median | wait p90 | wait max |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| update-external.yml / schedule | update-external / Update agent skills | 9 / 0 | 146 | 178 | 178 | 2 | 2 | 2 |
| update-external.yml / schedule | update-external / Update submodule references | 9 / 0 | 10 | 18 | 18 | 1 | 2 | 2 |
| update-external.yml / schedule | update-external / Validate inputs | 9 / 0 | 4 | 6 | 6 | 2 | 3 | 3 |

#### Per run (seconds; wall = max(completed) - min(created) over counted jobs, job sum = sum of exec)

| workflow / event | runs | wall median | wall p90 | wall max | job sum median | job sum p90 | job sum max |
| --- | --- | --- | --- | --- | --- | --- | --- |
| update-external.yml / schedule | 9 | 152 | 185 | 185 | 161 | 190 | 190 |

## 5. 読み方の注意（事実だけ）

- macOS のジョブ（`hook script tests (macos-latest)`・`(macOS bash 3.2)`）は待ちが長いことがあり、壁時計の大半を占めうる。待ちと実行を分けて読む。
- キャッシュの hit と miss を分けていない（保存は main の push だけ。#55）。
- GitHub ホステッドの runner の時間は揺れる。課金の倍率（macOS 等）は扱わない。
- `cancelled` の実行・ジョブは時間の集計から除いている（PR の concurrency による取り消しを含む）。
- 標本の件数が系統とジョブで違う。PR の系統は成功・失敗の実行が少ない。
- 記録した時点のジョブの構成に限る。ジョブを足したら取り直す。
- `ci.yml` の push の `PR title` は push では走らないため、全件が除外になる。

## 6. 判定の欄

空欄。予算の値と周期は `TASK-66` で人が記入する。

| 項目 | 記入 |
| --- | --- |
| PR ごとの予算 |  |
| 定期の予算・周期 |  |
| 「1 回あたり」の読み方 |  |
