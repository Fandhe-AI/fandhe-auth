#!/usr/bin/env bash
# CI の DB ジョブの入口。DB を使う結合試験の枠の試験（crates/it-harness の db_job_frame）だけを流す（TASK-6 5/10。#52）。
# 呼び出し元: `make it-db-ci`・CI の it-db-ci ジョブ。全件の入口は scripts/it-db.sh（手で流す。CI 外）。
# 終了コード: 0 = 1 件以上が流れて全合格、1 = 失敗または一致 0 件、2 = 誤用（引数）。
# DB のパスワードは試験プロセスの中で生成し、子プロセスの環境変数にだけ渡す（SECR-6）。ここでは扱わず、
# set -x・docker compose config・docker inspect は使わない（展開した値が出る）。
# docker・openssl・cargo が無ければ失敗する（黙って飛ばさない。DPOL-30〜32）。
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

reject_args "$(basename "$0")" "$@"
require_cmd cargo
require_cmd docker
require_cmd openssl
cd "${ROOT_DIR}"

# 一致 0 件でも cargo test は 0 で終わるため、先に数えて 0 件なら失敗にする（AUDB-6 の向き）
listed="$(cargo test -p fandhe-auth-it-harness --test db_job_frame --locked -- --ignored --list)"
count="$(printf '%s\n' "${listed}" | grep -c ': test$' || true)"
if [ "${count}" -lt 1 ]; then
  echo "error: no DB frame test found" >&2
  exit 1
fi

cargo test -p fandhe-auth-it-harness --test db_job_frame --locked -- --ignored
