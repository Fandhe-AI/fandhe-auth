#!/usr/bin/env bash
# Claude Code の PostToolUse フック（.claude/settings.json の Edit|Write|MultiEdit から呼ばれる）。
#
# Claude が編集した .rs ファイル 1 つだけに rustfmt をかける。整形は補助であり、
# 整形漏れの最終判定は CI と `cargo fmt --check`（TASK-3 で make ターゲット化）が担う。
# そのため、どの失敗（jq・rustfmt が無い、構文エラーで整形できない等）でも
# 編集を止めないよう常に終了コード 0 で終わる（失敗は stderr に 1 行だけ出す）。
#
# 1 ファイルに限る: rustfmt にファイルを渡すと `mod x;` の子モジュール（`#[path]` 指定先を含む）まで
# 辿って書き換えるため、標準入力で整形し、変化があったときだけ同じファイルへ書き戻す。
#
# 対象外: リポジトリ外のパス（symlink を解決した実体で判定）、symlink そのもの、
# .agents/・.claude/skills/（外部取得のスキル文書）、docs/spec/（private submodule）、target/（ビルド成果物）。
#
# edition: ルートに rustfmt.toml / .rustfmt.toml があればその設定に従い（ルートで実行して読ませる）、
# 無い間（MSRV・edition は TASK-3/TASK-4 で未定）は 2024 として整形する。
set -uo pipefail

command -v jq >/dev/null 2>&1 || exit 0
command -v rustfmt >/dev/null 2>&1 || exit 0

file="$(jq -r '.tool_input.file_path // empty' 2>/dev/null)" || exit 0
[ -n "${file}" ] || exit 0
case "${file}" in
  *.rs) ;;
  *) exit 0 ;;
esac
[ -f "${file}" ] || exit 0
[ -L "${file}" ] && exit 0

# file と root を物理パス（symlink・`.`・`..` を解決した実体）にしてから比べる
root="$(cd "${CLAUDE_PROJECT_DIR:-$(pwd)}" 2>/dev/null && pwd -P)" || exit 0
dir="$(cd "$(dirname "${file}")" 2>/dev/null && pwd -P)" || exit 0
real="${dir}/$(basename "${file}")"
case "${real}" in
  "${root}"/*) rel="${real#"${root}"/}" ;;
  *) exit 0 ;;
esac
case "${rel}" in
  .agents/* | .claude/skills/* | docs/spec/* | target/* | */target/*) exit 0 ;;
esac

args=(--emit stdout)
if [ ! -f "${root}/rustfmt.toml" ] && [ ! -f "${root}/.rustfmt.toml" ]; then
  args+=(--edition 2024)
fi

safe_rel="$(printf '%s' "${rel}" | tr -d '[:cntrl:]')"
tmp="$(mktemp "${TMPDIR:-/tmp}/rustfmt-on-edit.XXXXXX")" || exit 0
trap 'rm -f "${tmp}"' EXIT

if (cd "${root}" && rustfmt "${args[@]}" <"${real}" >"${tmp}" 2>/dev/null); then
  # 中身だけを書き戻す（cat で上書きし、権限・inode を保つ）
  cmp -s "${tmp}" "${real}" || cat "${tmp}" >"${real}"
else
  echo "rustfmt-on-edit: ${safe_rel} を整形できませんでした（構文エラー等）。cargo fmt で確認してください" >&2
fi
exit 0
