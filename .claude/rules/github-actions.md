---
paths:
  - ".github/workflows/**"
description: >
  .github/workflows/ を編集する際の方針。runner 選択（public = GitHub ホステッド、ai-review の
  codex 専用 self-hosted runner は例外）、必須チェックと集約ジョブ ci-complete（先行配置の期間と、組織の ruleset が固定して要求する間の `trust-root-secret-scan.yml` の例外を含む）、発火条件・concurrency・
  paths フィルタの書き方、ジョブ追加時のチェックリスト（外部ツールの版と SHA256 の規則と、Rust の toolchain の
  例外を含む）、キャッシュ方針（容量 10 GB・保存は main の push のみ・キー設計・効果の計測）、所要時間の計測方法と
  棚卸し結果を規定する。
---

# GitHub Actions 編集ルール

## 目的

CI は全 PR で走るため、ワークフロー数・ジョブ数・所要時間はそのまま待ち時間と課金分数になる。
**既存ワークフローへの集約・不要な発火の抑止・並列化**を優先し、ワークフロー・ジョブを安易に増やさない。
CI 独自のロジックは持たず、ローカルと同じ `make` / `scripts/` を呼ぶ（`.claude/rules/makefile.md`）。

## 現状のワークフロー

| ファイル | ジョブ（check 名） | トリガー | runs-on |
| --- | --- | --- | --- |
| `ci.yml` | `make check (...)` / `make build + make test` / `make lint (...)` / `secret scan (tracked files)` / `make it-db-ci (DB integration frame)` / `deps audit frame (tool install only; ...)` / `hook script tests (ubuntu-latest\|macos-latest)` / `PR title (...)` / `ci-complete` | `pull_request`（`opened` / `synchronize` / `reopened` / `edited`。`edited` でも全ジョブを走らせ、`if:` で skip しない）・`push`（`main` のみ） | `ubuntu-latest`（`test-hooks` の matrix のみ `macos-latest`） |
| `ai-review.yml` | `codex / preflight`・`codex / review`・`codex / post_feedback`（Fandhe-AI/actions の reusable workflow） | `pull_request`（base が `main`。`opened` / `synchronize` / `reopened`） | preflight・post_feedback は `ubuntu-latest`、review は self-hosted の `codex` ラベル |
| `trust-root-secret-scan.yml` | `trusted secret scan (tracked files)` | `pull_request`（`branches` を一致しにくい名前に絞っており、通常は発火しない。組織の ruleset の「Require workflows to pass」から呼ばれたときに動く。発火の前提は「必須チェックと集約ジョブ」の節の例外を参照） | `ubuntu-latest` |
| `scheduled.yml` | `scheduled frame (no checks wired; TASK-681)` | `workflow_dispatch` のみ（`schedule` は持たない。足すときに例外を見直す（TASK-681）。`pull_request`・`push` は持たない） | `ubuntu-latest` |
| `update-external.yml` | `update-external / *`（Fandhe-AI/actions の reusable workflow） | `schedule`（毎日 00:00 UTC）・`workflow_dispatch` | `ubuntu-latest`（`runner-json` で指定） |

`ci.yml` の Rust のジョブ（`build-test`・`lint`・`it-db-ci`）は、toolchain を MSRV の版で入れて流す（利用者の指示、2026-11-20）。
入れるのは `scripts/install-msrv.sh` で、版は `scripts/msrv.sh` が読む（ルートの `[workspace.package].rust-version` を TOML の構造どおりに読み、全 workspace メンバーの `rust-version` と一致するときだけ採る。食い違いは終了コード 2）。
MSRV の値そのものは `.claude/rules/coding-rust.md` の「unsafe・MSRV・edition・整形」を正とし、ここには写さない。
新しい stable での検査は無く、足すかは未定。入れ方の例外は「ジョブ・ワークフローを追加するときのチェックリスト」の 9 を参照する。

## PR ごとの実行と定期の実行の振り分け（TASK-6 7/10。#54）

- PR の系統は `ci.yml`（`pull_request`・main への `push`）。`ci.yml` 内のジョブでマージを止める検査はここに置き、`ci-complete` で束ねる。
  `ci-complete` の集約は `ci.yml` 内のジョブに限る。`codex / *` の必須ステータスチェックと `trust-root-secret-scan.yml` は別に要求される（「必須チェックと集約ジョブ」の節を参照）。
  依存の変更ごとの監査（DPOL-34）も PR の系統の `deps-audit` が持つ（TASK-680）。
- 定期の系統は `scheduled.yml`（今は `workflow_dispatch` のみ。`schedule` は持たない。足すときは下の例外を見直す）。コードが変わらなくても結果が変わりうる検査を置き、マージは止めない。PR の系統の代わりにしない。
  今は検査を結線しない枠で、緑でも DPOL-35 を満たしたことにならない。結線・advisory DB の取得日の記録・警報は TASK-681（#1046）、警報の経路は Hub の `AOPS-3`。
- 定期のジョブも `make`・`scripts/` の同じ入口を呼び、CI 側に検査のロジックを書かない。
- 道具の版と SHA256 を宣言するのは `ci.yml` の `env` の 1 か所だけ。定期の系統で共有する方法（`workflow_call` か宣言の移設）は未定（TASK-681 で決める）。
- 例外: PR の検査を担わない定期ジョブに限り、`scheduled.yml` を `ci-complete` の `needs` に入れず、required にも登録しない。
  この例外は利用者が承認した（2026-11-26。承認は人が行う。ACPT-7）。承認の範囲は、`scheduled.yml` が `workflow_dispatch` だけで動く間に限る。
  例外の根拠は 2 点に分ける。
  (1) 集約できない理由: `needs` は同一ワークフロー内のジョブしか参照できず、別ワークフローの `scheduled.yml` のジョブを `ci.yml` の `ci-complete` に直接束ねられない（GitHub Actions の仕様）。
  (2) required に登録しない理由: `scheduled.yml` は PR で起動しないため、required に登録するとチェックの結果が報告されないこと自体がマージを妨げる。
  `schedule` などの発火を足すとき（TASK-681）は、承認の範囲の外なので、この例外を見直す。見直して承認が記録されるまで `schedule` は有効にしない。
  チェックリスト 2 は追加ジョブを `ci-complete` に含めるよう求めるが、`scheduled.yml` はこの承認の範囲で例外とする。PR の検査を担うジョブを足すときは `ci.yml` に置く。
  - 承認の記録
    - 根拠: 定期ジョブは PR を検査しないため、`ci-complete` に入れても PR の判定に使われず、失敗が無関係な PR を止める
    - 2026-11-27 追記: 例外の根拠を 2 点に分けた記述は、上の (1)(2) を参照する（承認時の根拠とは別に足した説明。codex の指摘による補足。承認の範囲は変えない）
    - 適用範囲: PR の検査を担わない `scheduled.yml` のジョブに限り、かつ `scheduled.yml` が `workflow_dispatch` だけで動く間に限る。PR の検査を担うジョブには広げない
    - 承認者: 利用者
    - 承認日: 2026-11-26
    - 見直し: `schedule` などの発火を足すとき（TASK-681）
  - `ci-complete` で束ねない代わりの失敗の検知経路: 今は実装に在る経路は無く、GitHub の既定の通知だけ（下の「失敗の通知」の項。失敗時の通知・Issue 起票の結線は `scheduled.yml` に無い）。運用の警報への結線は TASK-681（Hub の `AOPS-3`）で、結線までは定期ジョブの失敗を人が Actions の画面で確かめる。この経路で足りるかは、見直しのときに人が判断する
- `workflow_dispatch` は、ワークフローのファイルがデフォルトブランチ（main）にあるときだけ起動できる（GitHub の公式の手順書。PR ブランチにしか `scheduled.yml` が無いあいだは手動で起動できない）。起動できる状態では main 以外の ref も指定して流せる。check 名は必須のものと違うのでマージの判定には使われない（読み。〔実機〕では未確認）。
- 失敗の通知は GitHub の既定だけで、運用の警報には結線していない。
- schedule は default ブランチ（main）の定義で動く。活動の無い public の repo では GitHub が止めることがある。
- cron の時刻と頻度は暫定で、人が決める（TASK-681）。頻度の上限は下の「schedule は日次以下」の項の規則のままで、`scheduled.yml` に足すときの候補（日次）だけが暫定。

## 依存の検査の拡張点（`deps-audit`。TASK-6 6/10）

`ci.yml` の `deps-audit` は、cargo-deny・cargo-audit を版固定 + SHA256 検証で導入して版を照合するだけの枠で、検査は流さない。
緑でも DPOL-34 を満たしたことにならない（ジョブ名の「frame」はそのため）。結線は TASK-680（前提は TASK-668・TASK-679）で、定期の系統の枠は `scheduled.yml`（#54）で、定期の監査の結線は TASK-681（#1046）。
TASK-680 が結線するときの契約は次のとおり。値は spec の ID で引き、本文は写さない。

1. ジョブ ID `deps-audit` を変えず、`ci-complete` の `needs` から外さない。`if:` による skip・`continue-on-error`・`paths` フィルタを付けない（DPOL-34）。
2. 処理は TASK-679 の監査の入口（`scripts/` と make ターゲット。名前は未定）を呼ぶ。`ci.yml` に検査のロジックを書かない。
3. DPOL-33 を満たす。advisory DB は実行の前に 1 回だけ取得し、以後の実行に取得なしの指定（cargo deny は `--disable-fetch`、cargo audit は `--no-fetch`）を付ける。
    使った DB の commit と取得日を記録し、`Cargo.lock` を書き換えない。具体の手順は TASK-679 で決める（未定。cargo-audit の取得の方法、記録の様式、
    `--locked` を持たない cargo-audit 0.22.2 での lock の不変の確かめ方を含む）。`./dependency-policy.md` の手元の手順はこの契約の実装ではない。
4. 対象はルートの `Cargo.lock` と、別 workspace の `crates/trial-deps/fuzz/Cargo.lock` の両方で、合否に両方を含める（`crates/trial-deps/README.md` の「依存の監査」。DPOL-33・DPOL-34）。
5. 設定は TASK-668 の厳格版 `deny.toml` だけを使う。例外（`ignore`・`skip`・許可の追加）を足すかは人が決め、足すときは理由・期限・範囲を同じ場所に書く（DPOL-20・DPOL-35）。
6. 4 検査と cargo-audit の 5 つがすべて成功したときだけ緑にする（DPOL-34）。結線までの緑は道具の導入の成功だけを表し、DPOL-34 の合格ではない。
    結線したらジョブ名から「frame」を外す（必須チェックは `ci-complete` だけなので影響しない）。
7. 同じ入口を定期の実行（`scheduled.yml`・TASK-681）からも呼べる形に保つ（DPOL-35）。
8. 道具の版は `env` の版と SHA256 を同時に更新する。現在の版は暫定で、人が確定する。
9. 公開ログには版・件数・advisory ID 程度だけを出し、秘密値は出さない（SECR-23）。

## runner 方針

本リポジトリは **public** であり、組織方針（Fandhe-AI/actions `docs/runner-policy.md`）に従い
**GitHub ホステッド（`ubuntu-latest`）を使う**。private の派生リポジトリでは `self-hosted`（または絞り込んだラベル）を使う。

- public リポジトリで self-hosted runner を使うと、fork からの PR 経由で untrusted なコードが runner 上で
  実行され得る。新規ジョブで `runs-on: self-hosted` を書かない
- reusable workflow の runner 入力は既定値が可視性で自動切替しないものがある。public から呼ぶ場合は
  `ubuntu-latest` を明示する（`ai-review.yml` の `post-feedback-runner`・`update-external.yml` の `runner-json`）

### 例外 1: `test-hooks` の `macos-latest`

開発者の多くは macOS でフックを実行するため、BSD 系コマンドの挙動と macOS 標準の bash 3.2 を検証する目的で
`macos-latest` を使う（GitHub ホステッドのため runner 方針には反しない。分単価が高いので他ジョブには広げない）。

### 例外 2: `ai-review.yml` の review ジョブ（public で self-hosted）

`codex / review` は provider と同名のラベル `codex` を持つ self-hosted の codex 専用 runner で動く
（2026-10-11 の実行で、ラベル `codex` を持つ self-hosted runner で動くことを確認）。
組織方針 `docs/runner-policy.md` §3 が public リポジトリでも許容する例外と定めている。理由は、認証が runner 上の
ログイン済みディレクトリ（home-dir 方式）で、GitHub ホステッドでは資格情報を保持できないため。

fork PR で実行されない根拠（Fandhe-AI/actions の `latest` タグ = `187cd8b` を 2026-10-11 に確認した範囲）:

- reusable workflow `.github/workflows/ai-review.yml` の `preflight` ジョブは
  `if: github.event.pull_request.head.repo.full_name == github.repository` で fork PR では起動しない
- `review` は `needs: preflight` かつ `if: needs.preflight.outputs.run == 'true'` のため、preflight が起動しなければ実行されない
- `docs/runner-policy.md` §3 は例外の前提として「fork からの PR では実行しない」「ジョブユーザーに passwordless sudo を
  与えない」「信頼できないリポジトリへ runner を共有しない」を挙げ、review ジョブには sudo を検証する fail-closed の
  ステップがある

この保証は本リポジトリではなく上流（可変タグ `latest`）にある。上流の変更で前提が崩れていないかは、
`ai-review.yml` を変更するときに上記の条件を再確認する。`ai-review.yml` の trigger に `pull_request_target` を
使わない（fork PR に書き込み権限・secrets を渡すことになる）。

## 必須チェックと集約ジョブ

- **必須チェックの 2026-11-22 の読み取りの記録**（2026-11-22 に main が `gh api repos/Fandhe-AI/fandhe-auth/rulesets` と `.../rules/branches/main` の読み取りで確認した。
  AI による確認で、人の判定ではない。ruleset の ID は書かない。今の状態は同じ読み取りで確かめ直す。
  ただし bypass が空かは、一覧と `rules/branches/main` には出ない。個別の ruleset の取得（組織の owner の権限が要る）で確かめる）
  - 2026-11-22 の読み取りでは、組織の ruleset（enforcement は active）が、この repo の `.github/workflows/trust-root-secret-scan.yml` を commit SHA `651305f`
    （#6062 のマージ commit）で固定して要求していた（`rules/branches/main` に workflow の規則として出た）。以下の「例外」の節を参照
  - 同じ読み取りでは、この repo 単位の ruleset に、required status checks（`ci-complete`・`codex / preflight`・`codex / review`・`codex / post_feedback` を含む）が登録されていた
  - 以前の記述（2026-10-11 時点で必須チェックは未設定、`main` の branch protection も無い）は、2026-11-22 の読み取りとは異なる。
    その記述は別の repo を読んだ記録で、この repo の記録ではない
- README の想定では、ruleset の required status checks に `ci-complete` と `codex / preflight`・`codex / review`・
  `codex / post_feedback` を登録する（導入は `setup-repo-guards` スキル）。上の読み取りの記録の 2 つ目の項目は、この想定に当たる登録である。preflight が失敗すると後続は skipped になり、
  skipped は required check を満たすため、preflight も必ず登録する
- `ci.yml` のジョブを required に個別登録せず、集約ジョブ `ci-complete` だけを登録する
  - `ci-complete` は `if: always()` で全ジョブの結果を検査する fail-closed のジョブ。success 以外は失敗とし、
    skipped は明示的に許容したもの（`pr-title` の push イベント時）に限る
- **例外: `trust-root-secret-scan.yml` は `ci-complete` の `needs` に入れない**（集約の対象の外）。組織の ruleset が固定した commit SHA で要求する workflow は
  `ci.yml` とは別の workflow で、ruleset が直接要求するため、`needs` で束ねられない。置き場所をこの repo にしたのは `D-2026-11-22-4`、仕組みは `D-2026-11-21-19`。
  この例外は利用者が承認した（2026-11-22。承認は人が行う。ACPT-7）。spec への記録は #5985 で追跡する。承認の範囲は、次の (a)・(b) の 2 つの期間に限る
  - (a) 先行配置の期間（`trust-root-secret-scan.yml` を `main` に入れたマージから、組織の ruleset の固定先を `trust-root-secret-scan.yml` に切り替えるまで）
    - この workflow は必須の検査として働かない。ファイルはあるが、ruleset が固定していない
    - (a) の間に同じ名前の check が PR に出ても、必須の検査の合格と読まない
    - 普通の PR では発火しない前提である（前提は未試験。下の発火の経路の記述を参照）
    - (a) の間の守りは、組織の ruleset が別の repo の workflow を固定して要求していれば、その workflow だけである。
      ruleset の状態はこの規約では言わない（下の (a)・(b) のどちらでもない間の記述と同じ立場）
    - `ci.yml` の secret scan は PR が書き換えられるので、守りに数えない（「既知の限界」。`D-2026-11-21-18`）
    - この期間は `ci-complete` の `needs` にも入れず、必須の検査としても扱わない
    - (a) の見直しの期限は置かない。固定先を変えるときに見直す（`D-2026-11-22-8` の (3)(l)。期間 (a) は終わった。次の項）
    - 2026-11-22 に (a) の期間は終わった。組織の ruleset の固定先を、別の repo の workflow から、この repo の `trust-root-secret-scan.yml` @ `651305f`
      （#6062 のマージ commit）へ切り替えた（利用者の指示で、main が API で変更し、読み返して照合した。承認は利用者。上の (a) の記述は記録時点の文として残す）
    - 2026-11-22 の切り替えの時点では (b) の条件を満たしていた（main が組織の ruleset を読み返して照合した。AI による確認で、人の判定ではない）。
      以後の状態はこの規約では言わない
    - 〔実機〕の試験を省いて切り替えたので、切り替えの時点では、同じ名前の check を必須の判定に数えないか〔この節〕・取り消された実行の判定・`types`・base の切り替え〔「発火条件の方針」〕・fork〔「秘密情報の検査ジョブ」〕などが未試験だった。
      切り替えの後に試験用の repo で〔実機〕の試験を流し、2026-11-22 に人（利用者）が合否を判定した。試験の構成は spec の `D-2026-11-22-7` の (4)、確定の範囲は同 (6)。合格は、試験した範囲に限る（`D-2026-11-22-8` の (3)(a)）
      - 合格: 同じ名前の check が並ぶ場合と、base を切り替える場合のうち、`D-2026-11-22-8` の (3)(a) に挙げた範囲。観察は、どれも必須の workflow でマージが止まったこと。
        同じ名前の check が並ぶ場合の合格は、PR 側の同じ名前の success が数えられなかった範囲に限る。固定版の結論が success 以外の場面の合格は、failure・cancelled のときの観察に限り、それ以外の結論は含まない。取り消しの経路の違いは未試験
      - 別のブランチを base にして開いた PR を、head を変えずに `main` に切り替える場合に、切り替える前から同じ head の SHA に別の PR で付いた固定版の success があるときは、その success を固定版の有効な結果に数える（同 (3)(b)。観察は `D-2026-11-22-7` の (4)(e)(ii)）。
        数えても言えないことは `D-2026-11-23-1` の (4)(c)（main の要旨。〔実機〕未試験の main の読みに基づく）。この条件の外は数えるとは決めていない。数えるのは、いまの ruleset の固定先と同じ版で付いた success に限り、固定先を変える前の版で付いた success は利用者が判定するまで数えない（`D-2026-11-23-1` の (3)(a)）
      - 試験が成り立たなかった構成は、範囲外として記録した（同 (3)(c)）。やり直すかは決めていない（判定は利用者）
      - 未試験のまま: fork・取り消された実行の判定のうち上の観察の外・`types`・イベントの種類。Checks API・commit status・merge queue は試験の範囲外の経路（`D-2026-11-22-6` の (5)(e-iv)。範囲外のまま進めるかは未決〔`D-2026-11-22-8` の (5)(g)〕）。
        そのほか (3)(a) に挙げた範囲の外は未試験（`D-2026-11-22-7` の (6)(c)）
      - (b) の間の補いは、合格した範囲と、未試験の点が成り立つ前提に依る。未試験の点が成り立つかは分からない。未試験の点は #5985 で追跡
    - 上の合格の判定は〔実機〕の観察（試験用の repo）についての人の判定で、04 の行の「確定」への変更ではなく、`ACPT-34` の確定の範囲も広げない（`D-2026-11-22-8` の (6)）
  - (b) 次の条件をすべて満たす間
    - 組織の ruleset（Require workflows to pass）が、この repo のこのファイルを commit SHA で固定して要求している
    - その ruleset が active で、bypass が空である
    - その ruleset の対象の条件に、この repo と既定ブランチ（`main`）が入っている
    - 2026-11-22 の切り替えの時点で、main が ruleset を読み返して照合し、上の 3 条件を満たすことを確認した（enforcement は active、bypass は空、対象はこの repo の既定のブランチ。
      AI による確認で、人の判定ではない）。これは切り替えた時点の確認で、この repo の CI からは確かめられない。以後の状態は保証せず、崩れたときは下の終了の条件に従う
  - 終了の条件: 次のいずれかで、この例外は終わる。終わったら、扱いを利用者に確かめる
    - 固定先の切り替えをやめて、`trust-root-secret-scan.yml` を消すとき
    - (b) の後に、ruleset の固定を外すとき（固定先を別の所へ移すときを含む。同じファイルで固定の SHA を上げる操作は含まない。その操作は spec に記録する手順に従う）
    - 切り替えた後に (b) の条件を満たさないとき（最初から満たされない場合を含む。例: Evaluate への変更・bypass の追加・対象から外れること・Disabled）。
      組織の owner が気づいたら利用者に知らせる。この repo からは検出できない
      - 緊急のときも、固定の SHA を上げる手順で対応する。enforcement を Evaluate に下げるのは利用者の明示の指示があるときだけ（`D-2026-11-22-8` の (3)(k)、`D-2026-11-22-9` の (3)(c)。操作の条件は spec が正本で、ここに写さない）。Evaluate の間は (b) の条件を満たさない。enforcement を Evaluate に下げる操作も active に戻す操作も組織の owner が行い、main は差分の用意と読み取りの照合までにする（`D-2026-11-23-3` の (3)(h)）。Evaluate の間に main へ入った変更は、active に戻す前に人が確かめる（同 (3)(i)）。active に戻す操作のほかの条件と記録の形は未決（同 (5)。#5985 で追跡）
  - (a)・(b) のどちらでもない間（条件が崩れたとき）は、この workflow は必須の検査として働かない。ruleset の固定先が別の repo やファイルを指している間も同じ。
    ruleset が今このファイルを固定しているとは、この規約では言わない（固定先の確認は組織の owner の操作で、マージ時点で別の repo を指していることがある）
  - 適用範囲は `trust-root-secret-scan.yml` だけ。ほかの workflow に広げるのは、利用者の承認を待つ（承認は人が行う。ACPT-7）
  - 普通の `pull_request` で発火しないのは前提つき（以下は未試験の読み）。`on.pull_request.branches` の値は有効なブランチ名で、
    本ファイルが普通に発火する経路には、少なくとも次がある（例）
    - `branches` の値に当たる名前のブランチを base にした PR。抑えるには、その名前のブランチを作らない・base にしないことが前提になる
    - PR 側が本ファイルの `on:` を変える場合。`pull_request` は PR 側の版の workflow で評価されると読めるため、`on:` を変えた版は、変更後の `on:` の条件を満たすイベントで発火しうる
  - 普通の発火の実行は、同じ job 名 `trusted secret scan (tracked files)` で、PR の版の検出器を使うと読める。
    本ファイルの `on:` を書き換える経路は、試験が成り立たず範囲外とした（`D-2026-11-22-8` の (3)(c)）ので、この経路の実行が数えられないかは未試験。同じ名前の check が PR 上に並ぶので、人が取り違えうる
    `D-2026-11-22-6` の (5)(e-i) の 3 通り目の経路は、版 C にあたる。PR の base を `main` に切り替えた後に、この経路の PR の版の success が数えられないことは V1 として合格の範囲に入る（V1 の定義は `D-2026-11-22-6` の (5)(e-ii)。観察は `D-2026-11-22-7` の (4)(e)(ii)、合格は `D-2026-11-22-8` の (3)(a)）。PR の base を `main` に切り替える前に固定版と並んだときの順序は、試験が成り立たず範囲外とし、未試験（`D-2026-11-22-7` の (4)(e)(i)、`D-2026-11-22-8` の (3)(c)）。合格の範囲は `D-2026-11-22-8` の (3)(a) に限る
  - 同じ名前の check は、本ファイルの発火とは別の経路でも出せる。並ぶことは試験で観察した（`D-2026-11-22-7` の (4)(e)。経路の数え方は上の例とは別）
  - ruleset から呼ばれた実行かどうかを、PR が書けない情報で見分けられるかは未確認（未試験）。ログの中身（`workflow sha:` の行など、workflow 自身が出す行）は根拠にしない。
    ruleset の判定が同じ名前の check を数えないことは、`D-2026-11-22-8` の (3)(a) に挙げた範囲は合格。範囲外と未試験の点は上の 2026-11-22 の記述のとおり（#5985 で追跡）
- ジョブ名（`name:`）・ジョブ ID・reusable workflow を呼ぶジョブ ID（`codex`）を変えると check 名が変わる。
  required に登録済みの名前を変える場合は ruleset 側も同時に更新する（更新漏れは「永久に pending」でマージ不能になる）

## 発火条件の方針

- `push` は default ブランチ（`main`）のみ。PR ブランチは `pull_request` で検査し、二重実行しない
- concurrency は ref 単位で、PR では古い実行を取り消す。`main` への push は取り消さない
  （連続マージで前のコミットの CI 結果が失われないようにする）

    ```yaml
    concurrency:
      group: ci-${{ github.workflow }}-${{ github.ref }}
      cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}
    ```

    - reusable workflow のトップレベル concurrency は効かないため、呼び出し側（wrapper）で設定する
    - schedule・自動 PR 作成のように中断すると不整合が残るものは `cancel-in-progress: false`（`update-external.yml`）
- **例外: `trust-root-secret-scan.yml` には concurrency を付けない**（利用者の選択、2026-11-22）
  - 理由: concurrency の group は repo 全体で共有され、PR 側の workflow の指定が信頼する検査の実行に及びうる。
    group を通じて信頼する検査の実行を取り消させないため、この workflow には付けない。
    待機中に取り消された実行を ruleset がどう判定するかは未試験。
    concurrency が無ければ、group を通じた取り消しは無くなる。
    group を通じない取り消しの経路は残る。取り消された実行の判定は未試験（観察したのは、固定版の結論が failure・cancelled のときマージが止まったことだけ。`D-2026-11-22-8` の (3)(a)。取り消しの経路の違いは未試験）
  - 失うもの: 同じ PR に続けて push したとき、古い head の実行（数十秒）も最後まで走る。古い実行は head の SHA が違うので、最新の head の判定には使われない（未試験の読み）
  - 適用範囲は `trust-root-secret-scan.yml` だけ。concurrency を付ける・この例外を広げるのは、利用者の承認を待つ（承認は人が行う。ACPT-7）。
    付けるには、取り消しの判定を〔実機〕で試験する必要がある
- **例外: `trust-root-secret-scan.yml` は `pull_request.types` を指定しない**（既定の `opened`・`synchronize`・`reopened` のまま。利用者の選択、2026-11-22）。`ci.yml` の `edited` を足した指定とは揃えない
  - 理由: 中身を試験した版から変えないため。head の走査は head の tree 全体にかかる（外れるのは、登録済みの skill のディレクトリ〔固定した版の一覧と head の一覧の共通部分。#6000〕と、submodule の中〔検出器の範囲外。`scripts/hooks/secret-scan.sh` の冒頭〕）ので、
    PR が足した中身は、その範囲を除き base に関係なく走査される
  - 組織の ruleset の対象は既定のブランチだけと読む。これは上の (b) の条件（対象の条件に既定のブランチ〔`main`〕が入っている）を引く形で、この repo からは確かめられない
  - base を切り替えても、マージ結果の走査が古くなるだけと読めるのは、今の PR で、切り替える前にその head でのマージ結果を ruleset の実行が走査していたときに限る（main の読み。〔実機〕未試験）。その head の実行が別の PR のものだけのときは、古くなるだけと断定しない（`D-2026-11-23-1` の (4)(c)。main の要旨で、〔実機〕未試験の main の読みに基づく）。
    別の PR で付いた固定版の success を数える条件と、数えても言えないことは、上の〔実機〕の試験の合格の項（「必須チェックと集約ジョブ」の節。`D-2026-11-22-8` の (3)(b)）による。ここで条件を書き直さない
    別のブランチを base にして開いた PR を `main` に切り替えた場合は、固定版の結果がその head に無いことがありうる（例: `D-2026-11-22-6` の (5)(e-ii) の V0・V1）。この場合の結果は 2 通りが考えられた
    - 必須の検査が待ちになり、マージが止まる（fail-closed）
    - 検査が無い扱いになり、マージが止まらない（fail-open）
    - `D-2026-11-22-8` の (3)(a) に挙げた範囲では、切り替えた後にマージが止まった（fail-closed。合格）。fail-open は観察していない
  - 未試験の点（#5985 で追跡）: ruleset から呼ばれた実行で `types` が効くか。base の切り替えで起きるイベントの種類（記録していない）。固定版を長く待った場合と次の push の後。
    切り替えの時点で未試験だった点のうち、`D-2026-11-22-8` の (3)(a) に挙げた範囲は 2026-11-22 に人が合格と判定した。範囲外の点と今も未試験の点は未試験のまま（`D-2026-11-22-7` の (6)(c)・`D-2026-11-22-8` の (3)(a)。詳細は「必須チェックと集約ジョブ」の節の 2026-11-22 の追記と「既知の限界」）
  - 指定を足す・この例外を広げるのは、利用者の承認を待つ（承認は人が行う。ACPT-7）
- `paths` / `paths-ignore` は、required check を含むワークフロー（`ci.yml`・`ai-review.yml`、および組織の ruleset から必須として呼ばれうる `trust-root-secret-scan.yml`）に
  **ワークフロー単位で付けない**。発火しないと check が報告されず pending のままマージ不能になる
  - ドキュメントのみの変更で重いジョブを省きたい場合は、ジョブ内の変更検知（`git diff` ベース等）で
    ジョブを skip し、`ci-complete` で「その条件での skipped」だけを許容する
  - 現状の `ci.yml` は全体で数十秒のため導入していない（費用対効果が見合う重いジョブを足した時に検討する）
- draft PR の skip は現状入れていない。入れる場合は `if: github.event.pull_request.draft == false` とし、
  trigger の `types` に `ready_for_review` を加える（加えないと ready にしても CI が走らない）
- schedule は日次以下の頻度にする（`update-external.yml` は 00:00 UTC の 1 回。`scheduled.yml` に足すときの候補は日次で、暫定。決めるのは TASK-681）

## ジョブ・ワークフローを追加するときのチェックリスト

1. 新規ワークフローを作らず、まず既存の `ci.yml` へのジョブ追加で済むか検討する（同じトリガーなら `ci.yml` に集約）
    - 例外: `scheduled.yml`。トリガーが `schedule`・`workflow_dispatch` の定期の系統は別にする（「PR ごとの実行と定期の実行の振り分け」。例外は利用者が承認済み。2026-11-26。範囲は `workflow_dispatch` だけで動く間。`schedule` を足すときは見直す〔TASK-681〕）
    - 例外: `trust-root-secret-scan.yml`。(a) 先行配置の期間と、(b) 組織の ruleset が workflow のファイルを固定して要求する間は、`ci.yml` に入れられない（要求されうる）（「必須チェックと集約ジョブ」の例外）
2. 追加したジョブを `ci-complete` の `needs` に必ず加える（漏れると未検査のままマージ可能になる）。条件付きで skip
    するジョブは、許容する条件を `ci-complete` の jq 式に明示する
    - 例外: `scheduled.yml` は `needs` に入れない（PR を検査しない。利用者が承認済み。2026-11-26。範囲は `workflow_dispatch` だけで動く間）
    - 例外: `trust-root-secret-scan.yml` は `needs` に入れない。(a)・(b) の期間の間だけ（「必須チェックと集約ジョブ」の節の例外）
3. 処理は `make <target>` / `scripts/*.sh` を呼ぶ。CI 側に複数行のロジックを書かない
    （ツール導入のように CI 固有のステップは例外。secret-scan ジョブの走査対象の取得も例外で、理由は「秘密情報の検査ジョブ」の節を参照）
    - 例外: `trust-root-secret-scan.yml` の検出器と走査対象の取得の step は workflow に置く。取得の前に検出器（`scripts/`）が無いため（「秘密情報の検査ジョブ」の節の `scripts/` に置かない理由と同じ趣旨）。取得後の走査は `scripts/secret-scan-ci.sh` を呼ぶ
4. lefthook と同じ検査を同じ範囲で重複させない。役割分担は `.claude/rules/git-hooks.md`（重い・網羅的な検査は CI が正）
5. 不要な `needs` で直列化しない。独立したジョブは並列に走らせ、クリティカルパス（最長ジョブ）を短くする
    - 逆に、セットアップ（checkout・ツール導入・キャッシュ復元）が実処理より長いほど細かく分けない
6. キャッシュは効果を測ってから入れる。保存は `main` への push に限り、キーはルートの lockfile のハッシュで作る
    （「キャッシュ方針」節）。self-hosted で `CARGO_HOME` を共有する場合の rust-cache の注意は
    Fandhe-AI/actions `docs/runner-policy.md` §5
7. `actions/checkout` は既定の `fetch-depth: 1` のまま使い、履歴が必要な場合だけ深くする。submodule は必要時のみ取得する
8. 各ジョブに `timeout-minutes` と最小の `permissions` を付け、checkout は `persist-credentials: false`
9. ピン止め方針に合わせる
    - 公式・サードパーティ action は commit SHA で固定し、行末にタグをコメントする（`@<sha> # v7.0.1`）
    - 自組織の reusable workflow（`Fandhe-AI/actions`）は可変タグ `@latest`
    - CLI ツールはバージョン + SHA256 を `ci.yml` の `env` に宣言し、検証してから導入する（既存の `check` ジョブの方式）
    - **例外: Rust の toolchain だけ**は、`env` の版 + SHA256 の宣言を置かず、`scripts/install-msrv.sh` が rustup で入れる。
      利用者が 2026-11-20 に認めた（理由: rustup の公式の配布を使う・配布物の検証は rustup が行う・版をパッチまで固定する）。
      次の条件をすべて満たすときだけ例外にできる
      - 版をパッチ版まで固定する（`rust-version` が `X.Y` なら `X.Y.0` に補う。`X.Y.Z` はそのまま）。補い方は `install-msrv.sh` が持つ
      - 導入は `scripts/install-msrv.sh` を経る。ワークフローに toolchain の導入・切り替え（`rustup toolchain install`・`rustup default`・`rustup override`・`rustup update` 等）を直接書かない（手順 3 の「処理は `scripts/`」と同じ）。今の toolchain の表示（`rustup show active-toolchain`）は許す
      - 後続のステップにも同じ版を使わせる。`install-msrv.sh` が `$GITHUB_ENV` に `RUSTUP_TOOLCHAIN=<版>` を書き、導入の直後に使われる `rustc` の版が MSRV と一致することを確かめる（違えば終了コード 2）
      - `latest`・`stable`・`beta`・`nightly` など、動く名前の追従にしない
      - rustup の公式の配布元を使う。`RUSTUP_DIST_SERVER`・`RUSTUP_UPDATE_ROOT` を CI（ワークフロー・`scripts/`）で設定しない。`install-msrv.sh` は設定されていれば（空の値も含む）、rustup を呼ぶ前に終了コード 2 で止める。配布元を変えるには利用者の承認が要る
      - 他の外部ツール（`check` ジョブの検査ツール等）には広げない。それらは従来どおり版 + SHA256 の宣言と検証を要る
      - 例外の範囲を広げる・条件を緩めるのは、利用者の承認を待つ（承認は人が行う。ACPT-7）
      - #47（TASK-6）で追跡していた残りの課題（ルートの宣言との照合・後続のステップでの toolchain の明示・配布元の条件の機械的な強制）は解消した。
        ルートの照合は `scripts/msrv.sh`、後者 2 つは `scripts/install-msrv.sh` が持ち、`scripts/hooks/test-hooks-7.sh` で試験する。
        試験は `cargo`・`jq`・`tomllib` を持つ `python3`（3.11 以上）を要し、CI（`CI`・`GITHUB_ACTIONS` が設定）では前提が足りなければ skip せず失敗にする
10. 信頼できない入力（PR タイトル等）は `${{ }}` で `run` に直接展開せず `env` 経由で渡す（`pr-title` ジョブの方式）
11. 時間予算: `ci.yml` は PR の wall-clock で 2 分以内を目安とする。超える場合は並列化・キャッシュ・
    変更検知による skip を検討し、PR 本文に変更前後の実測を書く
12. 変更後は `actionlint` で検査し、PR 上で実際に CI を走らせて所要時間を確認する

## キャッシュ方針

GitHub Actions のキャッシュ（`actions/cache`・`actions/setup-node` の `cache`・`Swatinem/rust-cache` 等）を
追加・変更するときの方針。効かないキャッシュは復元時間と容量を浪費し、他のキャッシュを追い出すため、
「入れれば速くなる」とは考えない。

### 現状（2026-10-11 時点）

- 本リポジトリは GitHub Actions のキャッシュを使っていない（キャッシュ 0 件・0 バイト）。
  `ci.yml` のツールは `env` に宣言した版を SHA256 で検証して毎回導入する方式で、所要時間は数十秒
  （Rust のジョブの toolchain だけは例外で、rustup で毎回導入する。チェックリストの 9）
- 言語固有のジョブを足してキャッシュを入れる場合は、本節に従う

### 前提: 容量と課金

- リポジトリあたりの上限は 10 GB（Org 設定も 10 GB）。超えると最終アクセスの古い順に追い出され、
  7 日間アクセスの無いキャッシュは削除される
- Org の Actions 予算は $0 で、超過時は停止する。そのため 10 GB を超える上限は使えない
  （上限を上げると、10 GB を超えた時点でキャッシュが読み取り専用になる）。上限・予算の変更はユーザーが判断する

### 入れる前に効果を測る

- self-hosted runner ではツールチェーン・pnpm store・`CARGO_TARGET_DIR` 等がジョブをまたいで残ることがあり、
  GitHub のキャッシュが無意味（復元時間だけかかる）な場合がある。本リポジトリは GitHub ホステッドだが、
  private の派生リポジトリで self-hosted を使う場合は特に確かめる
- 組織内の実例
  - mirror-ui: pnpm キャッシュの復元に 1 ジョブ平均 75 秒かけていたが、install は `downloaded 0` で効果が無かったため撤去した
  - team-hub: `Swatinem/rust-cache` が `./target` を保存対象にしていたが、runner の `CARGO_TARGET_DIR` が別の場所で
    registry しか保存されておらず、さらに後処理が共有 `CARGO_HOME` の crate を消していたため撤去した
- キャッシュを足す・残す前に、キャッシュ有無のジョブログで効果を確かめる。見るのはパッケージの `downloaded` 件数、
  依存の再ビルド数（`Compiling` の行数）、復元にかかった秒数

### 保存は main への push だけ

PR から保存したキャッシュは `refs/pull/<N>/merge` スコープに入り、その PR からしか読めないのに容量を共有し、
`main` のキャッシュを追い出す。保存は `main` への push に限る。

- `actions/cache` は restore（全イベント）と save（`main` の push のみ）に分ける

    ```yaml
    - name: Restore cache
      id: cache
      uses: actions/cache/restore@<sha> # vX.Y.Z
      with:
        path: <path>
        key: <key>
        restore-keys: <prefix>
    # ...ビルド・テスト...
    - name: Save cache
      if: github.event_name == 'push' && github.ref == 'refs/heads/main' && steps.cache.outputs.cache-hit != 'true'
      uses: actions/cache/save@<sha> # vX.Y.Z
      with:
        path: <path>  # restore と同じ
        key: ${{ steps.cache.outputs.cache-primary-key }}
    ```

- `Swatinem/rust-cache` は `save-if: ${{ github.event_name == 'push' && github.ref == 'refs/heads/main' }}` を付ける
  （`github.ref` だけだと `main` 上の `schedule`・`workflow_dispatch` でも保存する。下の `cache-save` も同じ）
- Fandhe-AI/actions の `rust-base-ci.yml` を `cache: true` で呼ぶ場合は `cache-save: ${{ github.event_name == 'push' && github.ref == 'refs/heads/main' }}`
  を渡す（`cache` の既定は false、`cache-save` の既定は true のため、渡さないと PR でも保存する）
- `actions/setup-node` の `cache: pnpm` のように restore と save が一体の action は、PR でも保存する点に注意する

### キーの設計

- キーには中身のハッシュ（lockfile 等）を入れる。`github.sha`・`github.run_id` など毎回変わる値は入れない
  （入れる場合は restore-keys の前方一致で直近を拾う設計にし、保存は `main` に限る）
- `hashFiles('**/Cargo.lock')`・`hashFiles('**/pnpm-lock.yaml')` のような再帰 glob は使わず、ビルド対象に本当に入る
  ルートの lockfile を指定する。再帰 glob では `.agents/skills/*/samples/**` などスキルのサンプルの lockfile が混入し、
  依存が変わらないのにキーが変わって全キャッシュを作り直す（fandhe-browser・fandhe-frontend・fandhe-db・
  fandhe-container・template-frontend-react_router で発生）
- ビルド成果物（Rust の `target/` 等）をキャッシュするなら、キーにツールチェーンの版を入れる（`rustc -vV` の
  release・commit-hash、`dtolnay/rust-toolchain` の `cachekey` 出力等）。入れないと runner イメージの更新後も古い成果物に
  exact hit し続け、exact hit では再保存されないため依存を毎回全再ビルドする（fandhe-db で依存 135 crate を毎回再ビルドしていた）
- restore-keys で前方一致の部分復元を許し、キーが変わっても cold にならないようにする
- キーが捕捉しない変更で保存内容が変わるときは、キーの版セグメント（`-v2-` 等）を上げる。restore-keys の版も同時に上げ、
  版セグメントが無ければ追加する。対象はルートの `Cargo.toml` の `[profile]`、`RUSTFLAGS`・`CARGO_PROFILE_*` 等の env、
  `.cargo/config.toml`、ビルドコマンド・フラグなど（Rust の具体例は `.claude/rules/rust-build-profile.md`）。上げないと
  ツールチェーンの版と同じく古い成果物に exact hit し続ける。キーが `Cargo.toml` を含んで変わる場合も、restore-keys で
  旧世代を部分復元すると新旧の依存成果物が混ざった大きな初回保存になるため、版を上げて旧世代を拾わないようにする
- `Swatinem/rust-cache` はキーに `CARGO`・`RUST` 等で始まる env（action を実行した時点の値）・`.cargo/config.toml`・
  `rust-toolchain(.toml)`・**workspace メンバーの** `Cargo.toml` を含める（上流 `src/config.ts` は `cargo metadata --no-deps`
  の packages の manifest だけをハッシュする）。ルートが仮想 manifest（`[package]` なし）だとルートの `Cargo.toml` は
  ハッシュされず、ルートの `[profile]` の変更はキーに入らない。これや、後続のステップで設定する env・ビルドコマンドの
  フラグを変えたときは、`prefix-key` の版（既定 `v0-rust`）を上げる（fandhe-edge-model #530）
- Fandhe-AI/actions の `rust-base-ci.yml` を `cache: true` で呼ぶ場合、clippy・test のキーは
  `<cache-key-prefix>-<OS>-{clippy,test}-v4-<rustc の版>-<Cargo.lock のハッシュ>`（`cache-key-prefix` の既定は `rust-base-ci`）で、
  版セグメントは呼び出し側から変えられない（`cache-key-prefix` を変えると deny ジョブを含む全キャッシュが cold になる）。
  キーに入らない変更をしたら、**マージ後に** `main` スコープの `<prefix>-<OS>-{clippy,test}-v4-*` を削除する。
  マージ前に消すと、その間の `main` への push が旧設定の内容を同じキーで保存し直す

    ```bash
    # clippy・test それぞれ、OS（runner.os: Linux 等）ごとに一覧して id で消す
    gh cache list -L 100 --ref refs/heads/main --key <prefix>-Linux-clippy-v4- --json id,key
    gh cache delete <id>
    ```

### サイズを抑える

- 1 世代（キャッシュ種別ごとの最新 1 件）の合計が 10 GB に余裕をもって収まるようにする。
  キャッシュ種別を増やすときは「1 件のサイズ × 種別（ジョブ）× OS」を見積もる
- Rust なら保存前に workspace メンバーの成果物を prune する、`CARGO_INCREMENTAL=0`、CI 限定で
  `CARGO_PROFILE_DEV_DEBUG=line-tables-only` 等で縮める

### 追い出されても hit する設計

- 上限に達しても、毎回読まれる `main` の最新キャッシュは最後まで残る（最終アクセスの古い順に追い出されるため）。
  キーの切り替えで古い世代が追い出されても、新しい世代が保存済みならミスにならない
- 保存を `main` に限り、キーが不要に変わらない状態を保てていれば上限を上げる必要はない

### 計測コマンド

```bash
# 使用量（件数・バイト数）
gh api repos/{owner}/{repo}/actions/cache/usage
# 一覧（key・サイズ・ref・最終アクセス）
gh cache list -L 100
# 不要なキャッシュの削除（共有状態のため、key と ref を確認してから消す）
gh cache delete <id>
```

ジョブログでは次の行を見る。

- `Cache restored from key: <key>`: primary key と一致すれば exact hit、それ以外は restore-keys による部分復元
- `Cache not found for input keys: ...`: ミス
- `Cache saved with key: <key>`: 保存された（PR で出ていれば保存条件の誤り）

### キャッシュを追加・変更する PR のチェックリスト

PR 本文に次を確認したことを書く。

1. 効果を測った（キャッシュ有無の `downloaded` 件数・再ビルド数・復元秒数）
2. 保存が `main` への push に限られている
3. キーが中身のハッシュ（ルートの lockfile のみ）と、成果物を含むならツールチェーンの版でできており、restore-keys がある
4. 1 世代の合計サイズを見積もり、10 GB に余裕をもって収まる

## 所要時間の計測方法

```bash
# ワークフロー単位の実行時間（作成〜更新）。直近 30 件
gh run list --workflow ci.yml --limit 30 --json databaseId,conclusion,createdAt,updatedAt,event
# ジョブ単位の開始・終了
gh run view <run-id> --json jobs --jq '.jobs[] | [.name, .conclusion, .startedAt, .completedAt] | @tsv'
```

### 棚卸し結果（2026-10-11）

| ワークフロー | 件数 | 中央値 | 最大 | 主なジョブ（直近 1 件） |
| --- | --- | --- | --- | --- |
| `ci.yml` | 14 | 14 秒 | 69 秒 | check 約 7 秒・test-hooks ubuntu 約 6 秒 / macOS 約 20 秒・ci-complete 約 2 秒 |
| `ai-review.yml` | 7 | 50 秒 | 69 秒 | preflight 約 4 秒・review 約 37 秒・post_feedback 約 7 秒 |
| `update-external.yml` | 5 | 100 秒 | 106 秒 | skills 更新 約 96 秒・submodule 約 8 秒 |

この時点で push は `main` のみ・concurrency 設定済み・ジョブは並列で、ワークフローの統合や削除の余地は無かった。
変更点は `main` への push で `cancel-in-progress` を無効化したことのみ。

## 秘密情報の検査ジョブ（`secret scan (tracked files)`）

- 検出器は、PR では保護された `main` の先端（`refs/heads/main`。PR の base ブランチではない）を `trusted/` へ取得した版だけを使い、
  PR の head（`target/`）は走査対象のデータとして渡す（`scripts/secret-scan-ci.sh <target>`）。push（main）では `github.sha` を両方に取得する。
  PR 側の検出器を呼ばない。stacked PR のように base が `main` 以外でも、未承認の base ブランチの検出器を使わない
- `trusted/`・`target/` とも `actions/checkout` で取らず、ジョブの `run:` で取得する。checkout が出す `HEAD is now at <コミット件名>` や
  展開するファイルのパスが、公開ログへ PR 由来の未加工の文字列を出すため（SECR-23。push では `github.sha` の件名も同様）。
  git の出力は捨て、失敗は固定の文言で exit 2。ref は `refs/heads/main` か 40 桁の小文字 16 進数だけを通し、値は `env` 経由で渡す。
  取得したコミットの sha だけを `trusted detector: <sha>` の書式でログに出す
  - `trusted/`: `git init`・`git fetch --depth=1`・`git checkout -q --detach FETCH_HEAD`（検出器を実行するので作業ツリーを展開する）
  - `target/`: `git init`・`git fetch --depth=1`・`git update-ref HEAD`（作業ツリーを展開せずオブジェクトだけを取る）
  - 取得処理は `scripts/` に置かない（取得の前に `trusted/` が無く、PR の CI では main の版に新しいスクリプトが無いため）
- 本文の走査の除外（`.agents/skills/<名前>/`）の名前は、`trusted/` の `skills-lock.json` から検出器が読む。走査対象（`target/`）の
  `skills-lock.json` から名前を足さない（PR が自分で名前を足して検査を免れないため。#6000）。PR 側の一覧は、通常ファイルか・大きさ・構文・対応形式を検出器が検証し、違反は検査不能（2）にする（マージ前に壊れた一覧を見つけるため）。
  検証を通った PR 側（対象の index）の一覧は除外を縮めるためにだけ使い、除外にするのは `trusted/` の一覧と対象の index の一覧の共通部分だけ（PR が登録を削除した名前はマージ前に走査される。対象に一覧が無ければ除外は 0 件）。
  index は走査の最初に複写して固定し、登録の確認・パス一覧・本文の走査はその複写から行う。走査中に元の index の内容が変われば検査不能（2）。`*.lock`・`.claude/skills/` は除外しない。
  除外はディレクトリ単位で、登録名の下の既存ファイルへの追記・新規ファイル・リネームによる移動は走査されない（残る限界。TASK-6・#6000 で追跡。
  登録名の下の変更は人がレビューで確かめる）。
  新しい skill を足す PR は、main の一覧にまだ名前が無いため中身が全部走査される。誤検出が出たときの扱い（除外の範囲・手順）は人が決める。
  lock だけを先に main へ入れて名前を事前登録する運用は、後続の PR が足す本文を初回から走査対象外にするため案内しない
- 取得は匿名で `GITHUB_TOKEN` を使わないため、ジョブの権限は `permissions: {}` にする
- 公開ログへ出すのは規則 ID・件数の集計行だけ（`--summary` の契約）。検査不能（2）のときは、共有の許可リスト（`scripts/lib.sh`）と行全体が完全一致する固定の診断文も出す（#6036）。再走査の案内の 1 行は、検出器が 1（検出）または 2 以上・その他（検査不能）で終わり、出力の集計行のフィルタまで進んだときだけ出る。対象 HEAD の解決の失敗・一時 index の構築の失敗・検出器が 0 でも集計行が 1 行でない不整合・陰性対照の失敗では、入口は非ゼロで終わっても案内は出ない（案内の有無で原因を判断しない。原因は出力の固定の診断文で見る）。案内の行に出る値は、走査対象の HEAD と、検出器の HEAD（別の checkout のとき。比べられなければ `unknown`）の 2 つの sha だけで、パス・秘密値・git の診断は含まない。案内は、詳細付きの再走査を非公開のローカル端末で行い、出力を公開の場へ出さないことも示す（SECR-23）。案内の行は、集計行のフィルタが成功した後にだけ出る（上記）。案内の処理は検出の結果（終了コード 1・2）を変えない。案内は元の実行の形で分かれる。検出器の実体のルートは、検出器と同じ規則（自身がリンクなら照合不能、親ディレクトリのリンクは物理パスで解決して 2 階層上）で求め、入口のルートと一致し git のルートそのものであるときだけ、(a)(b) の判定と照合に使う。一致しなければ (b) の作業ツリーの案内に倒す。(a) 走査対象が入口と同じ checkout（引数なし、または同じパスを明示したローカル実行）なら、検出器と一覧はその作業ツリーのもので、検出器は同じ checkout の規則（作業ツリー・HEAD・index の共通部分だけを除外。一覧は今の HEAD のものも読む）を選ぶため、別の場所へ移さず、その checkout の中で対象の引数なしに流すよう案内する。条件は、検出器と一覧が元の実行と同じ版のままで、HEAD も走査した sha のままであること。(b) 別の checkout（CI で PR の head を走査する形）なら、検出器と一覧の作業ツリーのファイルをコミットの blob と直接照合する。通常のファイルか、実行ビットがコミットの mode（100755・100644）と同じか、内容が同じかを見る（skip-worktree・assume-unchanged・ignore された未追跡も見落とさない）。読む量は blob の大きさ + 1 バイト、一覧は検出器と同じ 65,536 バイトまで、時間は検出器と同じ 5 秒で打ち切り、超えたら照合できないものとして扱う。コミットにも作業ツリーにも無いときに一致とみなすのは `skills-lock.json` だけで（検出器が除外なしで読む）、検出器の本体は無ければ不一致である。一致したときだけ検出器の HEAD の sha を案内し、一致しない・照合できないときは、sha では再現できないとして作業ツリーの版を使う案内にする（fail-closed）。`if:` で skip せず、`ci-complete` の `needs` に入れる
- 既知の限界: `ci.yml` のジョブの定義（この節の `secret scan (tracked files)`・`ci-complete`）は、今も PR が書き換えられる（`D-2026-11-21-18`。#5985 で追跡。人の担当）
  - これを補うのが、組織の ruleset が固定した SHA で要求する `trust-root-secret-scan.yml`（`D-2026-11-21-19`・`D-2026-11-22-4`）。
    補えるのは「必須チェックと集約ジョブ」の節の (b) の条件が満たされている間だけ。(a) の間と、(a)・(b) のどちらでもない間は補えない
  - 検出器と除外の一覧（`skills-lock.json`）は、ruleset が固定した commit から取る。新しい skill の文書は、固定の SHA を上げるまで走査される（安全側）
  - 固定先（参照先の repo・パス・SHA）の変え方（確かめる事項と手順）の正本は spec に記録する。現状（2026-11-22 時点）は spec の `D-2026-11-22-6` に記録した手順を、利用者が 2026-11-22 に採った（`D-2026-11-22-8` の (3)(d)。#5985 で追跡）。切り替えの記録は `D-2026-11-22-7`。初回の切り替え（参照先を別の repo からこの repo へ移す）も含む。ここにも `trust-root-secret-scan.yml` のヘッダにも写さない
  - spec に手順が記録され、その〔実機〕の試験に人が合格と判定した記録ができるまで、組織の owner は固定先（repo・パス・SHA）を変えない。初回の切り替えも含む（2026-11-22 の初回は、〔実機〕の試験の人の合格判定を利用者の承認で省いて行った。手順の記録の条件は満たさないまま行った経緯がある。下の項。ACPT-7）
  - 初回の切り替え（2026-11-22）は、切り替えの前の〔実機〕の試験を省くことを利用者が承認し、利用者の指示で main が API で行った（承認は人が行う。ACPT-7。利用者が承認したのは、差分と〔実機〕の試験の省略〔`D-2026-11-22-7` の (3)(a)〕）。
    例外として省いたのは〔実機〕の試験（人の合格判定）だけで、手順の記録など関門のほかの条件を免除したのではない。
    手順の記録の条件などは、切り替えの時点で満たしていなかった経緯として記録されている（`D-2026-11-22-7` の (5)(d)）。その後、手順は利用者が採り（`D-2026-11-22-8` の (3)(d)）、〔実機〕の試験は (3)(a) の範囲で合格と判定された。
    fork の PR は未試験のままで、試験の記録の全体が関門を満たすかは `D-2026-11-22-8` が判定していない（同 (4)(b)）。この関門が全体として満たされたとは、この規約では言わない。
    - 過去の切り替えのずれ（`D-2026-11-22-6` の (5)(f) の前提と (5)(g-2) の判定を満たさなかったこと）は、利用者が認めた（`D-2026-11-22-9` の (3)(b)。例外の承認で、ずれの記録は残る。`D-2026-11-22-6` の (5)(g-2) の判定が行われたことや関門が満たされたことを意味しない）
    - 固定した commit `651305f` の人の確認は、切り替えの後の事後の確認として 2026-11-22 に済んだ（`D-2026-11-22-9` の (3)(a)。確認の範囲は workflow の実行の部分と検出器の 5 ファイル。確認の方法の詳細の記録は無い。ほかの commit には及ばない）
    経緯・未決は spec の `D-2026-11-22-7`〜`D-2026-11-22-9` に記録してある（正本は spec。ここに写さない。#5985 で追跡）。
    以後の固定先の変更（固定の SHA を上げることを含む）は、この関門に従う
  - 今後、main（利用者と対話している Claude Code）が API で組織の設定を変える例外（`D-2026-11-22-8` の (3)(f)）を、この規約で当てるのは、`D-2026-11-22-6` の (5) の手順がある操作（この節の秘密情報の検査を必須にする組織の ruleset の固定先の変更）だけ（対象と操作を限るのは main の読み。原文は「組織の設定」と書く）。当てるのは、(3)(f) の条件（利用者の明示の指示・差分の提示と承認・控え・照合の 4 つ）をすべて満たすときに限る。固定先の変更は、上の関門（固定先を変えない条件の項）に従う。
    Evaluate への変更・Evaluate から active への復帰は組織の owner が行い、main は差分の用意と読み取りの照合までにする（`D-2026-11-23-3` の (3)(h)。Evaluate の間に main へ入った変更の確認は同 (3)(i)）。Disabled・ほかの組織の設定は `D-2026-11-21-18` の (4)(d) のままで、main は行わず利用者に上げる。
    戻しと照合のやり方の正本は `D-2026-11-22-6` の (5)(g)、`D-2026-11-22-8` の (3)(e)。読み返しの結果と判明の時点（人の合格判定の前後）ごとの分岐は `D-2026-11-23-7` の (4)(a) に整理した（既存の決定の写しで、食い違うときは各分岐の括弧の ID の元の決定が正。ここに写さない）。例外の範囲と owner・Disabled の扱いは上のとおりで、ここに残す条件は次のとおり（括弧に引いた (4) は main の読み。ただし、`D-2026-11-23-4` の (4)(a)(c) は `D-2026-11-23-5` の (3)(e) で利用者が確認した読みで、`D-2026-11-23-7` の (4)(a) は既存の決定の写しなので、決定か読みかは各分岐の括弧の ID に従う）。戻す前に必ず読み返して照合する。切り替えの PUT が失敗したときも同じ（`D-2026-11-22-8` の (3)(e)、`D-2026-11-23-3` の (3)(g)）。固定先を切り替えたあと（切り替えの PUT の失敗のあとを含む）、読み返した `enforcement` が控えと違うと分かった時点で、main は書き込まず、読み返した内容を添えて直ちに利用者に報告し、owner の判断を待つ（添える内容は控えの 6 項目に限り、ruleset の ID を含めず、相手は利用者だけ。`D-2026-11-23-4` の (3)・(4)(a)(c)、`D-2026-11-23-5` の (3)(d)(e)・(4)(d)）。読み返しが取れないときも、main は書き込まず止めて owner に上げる（`D-2026-11-23-5` の (3)(a)。main の報告先は利用者で、owner への伝達は利用者が行う。同 (4)(d)）。この 2 つの場合の後の書き込みは owner が行い、main が行うのは新しい控えを示して新しい承認を得たときだけで、元の承認は使わない（同 (3)(c)・(4)(c)）。ただし、人の合格判定の前にこの書き込みができるかは未決で（`D-2026-11-23-5` の (5)、`D-2026-11-23-7` の (5)(オ)）、確かめるまで main は書き込まない。切り替えと戻しの PUT の本体に `enforcement` を含めない。省いても値が保たれることを人が合格と判定するまで、次の切り替えをしない（`D-2026-11-23-5` の (3)(b)。判定は人が行う）。この確認は本番とは別の試験用の ruleset で行う（`D-2026-11-23-7` の (3)(b)）。切り替え後の読み返しで、人の合格判定の前に `enforcement` が一致しほかの項目だけが違うと分かったら、main は戻しの PUT を送らず、読み返した内容を利用者に報告して判断を待つ（`D-2026-11-23-6` の (3)。添える内容は上と同じ 6 項目で、同 (4)(b)）。この止めた戻しは、合格の後も自動で戻さず、今の状態と控えを示して改めて承認を得る（`D-2026-11-23-7` の (3)(a)）。利用者の判断の後と改めて承認を得た後の書き込みの主体と PUT の形、合格の前に元の承認で main が戻せるか、PUT の失敗のあとや (g-1)(g-2) で止めた戻しに `D-2026-11-23-7` の (3)(a) を当てるかは未決で（`D-2026-11-23-6` の (4)(c)・(5)、`D-2026-11-23-7` の (5)(カ)(キ)）、確かめるまで main は戻しの PUT を送らない（main の読み）。止めている間に、合格の後で別の不一致が分かったときの扱いは spec に定めが無い。控えへの PUT は止めた項目まで書き戻すので、main は書き込まず利用者に確かめる（main の読み）。戻せる先は承認のときに示した控えに限り、戻しも失敗したら止めて上げる（`D-2026-11-23-2` の (3)）。固定先の変更の承認・確認・戻しは `D-2026-11-23-3` の (3)(a)〜(g) に従う（ここに写さない。(3)(g) の直ちに戻す扱いの例外は `D-2026-11-23-7` の (4)(a) と、上の止めている間の扱い）。読み返した内容・ruleset の ID は public の repo・issue・PR に書かない（`D-2026-11-23-3` の (3)(e)・(4)(e)。読み返した内容は main の読み）。事後の報告を公開の issue に書くときは日付・spec の ID・合否だけ（同 (3)(e)）。未決は `D-2026-11-23-3`〜`D-2026-11-23-7` の各 (5) にあり、この規約では決めない（#5985 で追跡）。未決に当たる場面の決まるまでの扱いは各 (5) と `D-2026-11-23-7` の (4)(a) に従い、定めが無いときは main は組織の設定へ書き込まず、利用者に確かめる（main の読み）。人の合格判定の後に分かった、`enforcement` が一致しほかの項目だけが違う不一致を直ちに戻す扱い（止めた戻しが残っている間を除く。同 (4)(a)(iii)(v)）は、この未決に含めない
    サブエージェント・自動処理（CI のトークンなど）は含まない
  - fork からの PR での動きは未試験
