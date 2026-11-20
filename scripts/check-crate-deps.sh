#!/usr/bin/env bash
# workspace 内の crate 間の依存の向きを許可表と照らして検査する（TASK-3 (5/6)）。
# 本体（core）から Plugin・組込み先へ矢印を引かない境界を機械的に守る（LIB-2・LIB-4・PLG-26・LIB-16）。
# normal・build・dev のすべて、全 target を見る（試験の都合でも逆向きの依存を足さない。LIB-2・LIB-4）。
# 表に無い member・表に無い fandhe-auth-* の依存は失敗にする（fail-closed）。検査を外す引数・環境変数は作らない（DPOL-30〜32）。
# 呼び出し元: scripts/lint.sh・`make crate-deps`。
#
# 表の値は spec の〔提案〕（LIB は検討中）から引いた暫定。表を変える（crate・辺を足す）ときは利用者に確かめる。
# TODO(TASK-714): 外部の禁止 crate（非同期ランタイム・DB ドライバー・HTTP の枠組み・SDK 等）の検査を足す
# TODO(TASK-663): RP 向け crate・共有型・画面の crate の DB 非依存（DPOL-7）の検査を足す
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

reject_args "$(basename "$0")" "$@"
require_cmd cargo

cd "${ROOT_DIR}"

# allowed_deps <crate>: 依存してよい workspace の crate を空白区切りで返す。表に無ければ失敗する。
# 根拠: secret は小さな葉（DPOL-41）、core は secret のみ（DPOL-1・LIB-2）、db-tests は契約の操作を呼ぶ
# （LIB-11・LIB-12・DPOL-37）、client は秘密値の型だけ（LIB-16・DPOL-7）、plugin-oidc は本体の公開 API へ
# 一方向（PLG-26・PLG-28・LIB-19。client には依存しない）、hub-stub は試験専用で secret のみ（秘密値の型。SECR-31）
# （TASK-7。本体・Plugin から hub-stub への辺は無い。LIB-2・LIB-4 の向き）、
# console-stub も試験専用で workspace 内の依存なし（TASK-8。同じ向き。依存されるのは it-harness からだけ）。
# it-harness も試験専用で secret・hub-stub・console-stub（DB のパスワードを秘密値の型で持つ。SECR-30。Hub・console の
# stub を構成要素に組み込む。TASK-10 (3/6)・(4/6)。どの crate からも依存されない。この行は人の確認待ち。
# minimal-host への辺は後続の段で足し、足すときに確かめる）。
# e2e-harness も試験専用で workspace 内の依存なし（TASK-11 (1/6)。どの crate からも依存されない。この行は人の確認待ち。
# it-harness・minimal-host への辺は後続の段で足し、足すときに確かめる）。
# load-harness も試験専用で、通常（normal・build）の依存は workspace 内になし。試験（dev）だけ it-harness・hub-stub に
# 依存する（dev_only_deps の表。dev の辺に限って許可し、normal・build に現れたら拒否する）
# （TASK-12 (5/10)。it-harness の上で構成要素へ負荷をかける。どの crate からも依存されない。この行は人の確認待ち）。
# backend は差し替え点の枠だけで workspace 内の依存なし（TASK-9。辺を足すのは中身の TASK で、足すときに確かめる）。
allowed_deps() {
  case "$1" in
    fandhe-auth-secret) echo "" ;;
    fandhe-auth-core) echo "fandhe-auth-secret" ;;
    fandhe-auth-db-tests) echo "fandhe-auth-core fandhe-auth-secret" ;;
    fandhe-auth-client) echo "fandhe-auth-secret" ;;
    fandhe-auth-plugin-oidc) echo "fandhe-auth-core fandhe-auth-secret" ;;
    fandhe-auth-hub-stub) echo "fandhe-auth-secret" ;;
    fandhe-auth-console-stub) echo "" ;;
    fandhe-auth-it-harness) echo "fandhe-auth-secret fandhe-auth-hub-stub fandhe-auth-console-stub" ;;
    fandhe-auth-e2e-harness) echo "" ;;
    fandhe-auth-load-harness) echo "" ;;
    fandhe-auth-backend) echo "" ;;
    *) return 1 ;;
  esac
}

# dev_only_deps <crate>: dev-dependencies の辺に限って追加で許可する workspace の crate（空白区切り）。
# normal・build の辺で現れたら allowed_deps だけで判定し、ここに書いた crate でも拒否する（試験用の許可を本体側へ広げない）。
dev_only_deps() {
  case "$1" in
    fandhe-auth-load-harness) echo "fandhe-auth-it-harness fandhe-auth-hub-stub" ;;
    *) echo "" ;;
  esac
}

TREE_EDGES="normal,build,dev"
# normal・build だけの辺（dev を含まない）。dev の許可と区別するために使う。
PROD_EDGES="normal,build"
# --all-features は依存の列挙だけに使う（ビルド・clippy には使わない。DPOL-31）。optional 依存を feature の
# 後ろに置いても逆向きの依存を見逃さない（LIB-16・DPOL-7）。
TREE_ARGS=(--locked --all-features --prefix none --format '{p}' -e "${TREE_EDGES}" --target all)
PROD_ARGS=(--locked --all-features --prefix none --format '{p}' -e "${PROD_EDGES}" --target all)

members_out="$(cargo tree "${TREE_ARGS[@]}" --workspace --depth 0)"
members=()
while IFS= read -r line; do
  [ -n "${line}" ] && members+=("${line%% *}")
done <<<"${members_out}"

if [ "${#members[@]}" -eq 0 ]; then
  log "error: no workspace members found"
  exit 1
fi

violations=0
for member in "${members[@]}"; do
  if ! allowed="$(allowed_deps "${member}")"; then
    log "error: unknown workspace member: ${member} (add it to the allow table after confirming with the owner)"
    violations=$((violations + 1))
    continue
  fi
  dev_allowed="$(dev_only_deps "${member}")"
  deps_out="$(cargo tree "${TREE_ARGS[@]}" -p "${member}" --depth 1)"
  # normal・build の辺に現れる依存（dev の許可の対象外）
  prod_out="$(cargo tree "${PROD_ARGS[@]}" -p "${member}" --depth 1)"
  prod_deps=" "
  while IFS= read -r pline; do
    [ -n "${pline}" ] && prod_deps="${prod_deps}${pline%% *} "
  done <<<"${prod_out}"
  while IFS= read -r line; do
    [ -n "${line}" ] || continue
    dep="${line%% *}"
    [ "${dep}" = "${member}" ] && continue
    case "${dep}" in
      fandhe-auth-*) ;;
      *) continue ;;
    esac
    ok=0
    for a in ${allowed}; do
      [ "${a}" = "${dep}" ] && ok=1
    done
    if [ "${ok}" -eq 0 ]; then
      # dev だけで現れる依存（normal・build の辺に無い）に限り、dev_only_deps の許可を使う
      case "${prod_deps}" in
        *" ${dep} "*) ;;
        *)
          for a in ${dev_allowed}; do
            [ "${a}" = "${dep}" ] && ok=1
          done
          ;;
      esac
    fi
    if [ "${ok}" -eq 0 ]; then
      log "error: forbidden dependency: ${member} -> ${dep}"
      violations=$((violations + 1))
    fi
  done <<<"${deps_out}"
done

if [ "${violations}" -ne 0 ]; then
  log "crate dependency direction check failed (${violations} violation(s))"
  exit 1
fi
log "crate dependency direction ok (${#members[@]} members)"
