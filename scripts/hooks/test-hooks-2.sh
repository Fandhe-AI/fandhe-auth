#!/usr/bin/env bash
# scripts/hooks/ のフックスクリプト自体の回帰テストの part 2（TASK-6）。
# 入口は test-hooks.sh（`make test-hooks`・CI の test-hooks ジョブ）。単体でも実行できる。
# 準備・helper・例示値は test-hooks-lib.sh が持つ。part の間で状態は引き継がない。
set -euo pipefail
# shellcheck source=./test-hooks-lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/test-hooks-lib.sh"

check_bad_lock "skills-lock.json with a missing comma between members is rejected" '{
  "version": 2
  "skills": {
    "vendored-a": {
      "source": "x"
    }
  }
}'
check_bad_lock "skills-lock.json with a missing comma between skills is rejected" '{
  "version": 2,
  "skills": {
    "vendored-a": {
      "source": "x"
    }
    "vendored-b": {
      "source": "y"
    }
  }
}'
check_bad_lock "skills-lock.json with a trailing comma is rejected" '{
  "version": 2,
  "skills": {
    "vendored-a": {
      "source": "x"
    },
  }
}'
check_bad_lock "skills-lock.json with a broken skill body is rejected" '{
  "version": 2,
  "skills": {
    "vendored-a": {
      "source": "x",
      "hash":
    }
  }
}'
check_bad_lock "skills-lock.json with an unquoted skill key body is rejected" '{
  "version": 2,
  "skills": {
    "vendored-a": {
      source: "x"
    }
  }
}'
check_bad_lock "skills-lock.json with skills as an array is rejected" '{
  "version": 2,
  "skills": ["vendored-a"]
}'
check_bad_lock "skills-lock.json with a string skill value is rejected" '{
  "version": 2,
  "skills": {
    "vendored-a": "x"
  }
}'
check_bad_lock "skills-lock.json with a string version is rejected" '{
  "version": "2",
  "skills": {
    "vendored-a": {
      "source": "x"
    }
  }
}'
check_bad_lock "skills-lock.json with trailing garbage is rejected" '{
  "version": 2,
  "skills": {
    "vendored-a": {
      "source": "x"
    }
  }
} x'
check_bad_lock "skills-lock.json with a second top-level value is rejected" '{
  "version": 2,
  "skills": {
    "vendored-a": {
      "source": "x"
    }
  }
}
{}'
check_bad_lock "skills-lock.json with an escaped skill name is rejected" '{
  "version": 2,
  "skills": {
    "vendored\/a": {
      "source": "x"
    }
  }
}'
check_bad_lock "skills-lock.json with an invalid string escape is rejected" '{
  "version": 2,
  "skills": {
    "vendored-a": {
      "source": "a\qb"
    }
  }
}'
check_bad_lock "skills-lock.json with an unterminated string is rejected" '{
  "version": 2,
  "skills": {
    "vendored-a": {
      "source": "x
    }
  }
}'
check_bad_lock "skills-lock.json with a duplicate version is rejected" '{
  "version": 2,
  "version": 2,
  "skills": {
    "vendored-a": {
      "source": "x"
    }
  }
}'
check_bad_lock "skills-lock.json that is empty is rejected" ''

# 一覧の source 欄だけを差し替えた最小の一覧
lock_with_source() {
  printf '{\n  "version": 2,\n  "skills": {\n    "vendored-a": {\n      "source": "%s"\n    }\n  }\n}' "$1"
}
bs=$'\x5c'
check_bad_lock "skills-lock.json with an invalid UTF-8 byte is rejected" "$(lock_with_source "a"$'\xff'"b")"
check_bad_lock "skills-lock.json with an overlong UTF-8 sequence is rejected" "$(lock_with_source "a"$'\xc0\x80'"b")"
check_bad_lock "skills-lock.json with a UTF-8 surrogate is rejected" "$(lock_with_source "a"$'\xed\xa0\x80'"b")"
check_bad_lock "skills-lock.json with a truncated UTF-8 sequence is rejected" "$(lock_with_source "a"$'\xe3\x81'"b")"
# 不正なバイトの両側が、正しい列の除去で接して新しい列にならない（c2 + 正しい 3 バイト列 + 80）
check_bad_lock "skills-lock.json with a stray UTF-8 lead around a valid sequence is rejected" "$(lock_with_source "a"$'\xc2\xe2\x82\xac\x80'"b")"
check_bad_lock "skills-lock.json with a raw U+0001 in a string is rejected" "$(lock_with_source "a"$'\x01'"b")"
check_bad_lock "skills-lock.json with a raw U+001F in a string is rejected" "$(lock_with_source "a"$'\x1f'"b")"
check_bad_lock "skills-lock.json with a raw tab in a string is rejected" "$(lock_with_source "a"$'\t'"b")"
check_bad_lock "skills-lock.json with a raw newline in a string is rejected" "$(lock_with_source "a"$'\n'"b")"
check_bad_lock "skills-lock.json without a version is rejected" '{
  "skills": {
    "vendored-a": {
      "source": "x"
    }
  }
}'
check_bad_lock "skills-lock.json with a duplicate skills key is rejected" '{
  "version": 2,
  "skills": {
    "vendored-a": {
      "source": "x"
    }
  },
  "skills": {
    "vendored-b": {
      "source": "y"
    }
  }
}'
check_bad_lock "skills-lock.json with a unicode-escaped skill name is rejected" "{
  \"version\": 2,
  \"skills\": {
    \"vendored${bs}u0061\": {
      \"source\": \"x\"
    }
  }
}"
deep_open="$(printf '[%.0s' {1..40})"
deep_close="$(printf ']%.0s' {1..40})"
check_bad_lock "skills-lock.json nested deeper than the limit is rejected" "{
  \"version\": 2,
  \"skills\": {
    \"vendored-a\": {
      \"deep\": ${deep_open}${deep_close}
    }
  }
}"
# NUL を含む一覧（読み取りが NUL で止まり、後ろが未検証になる経路）
bad_n=$((bad_n + 1))
make_detector "bad${bad_n}" "${lock_ok}"
printf '{\n  "version": 2,\n  "skills": {\n    "vendored-a": {\n      "source": "x"\n    }\n  }\n}\n\0trailing' > "${det_bin%/scripts/hooks/secret-scan.sh}/skills-lock.json"
run_bad_lock "skills-lock.json containing a NUL byte is rejected"
# 一覧が普通のファイルでない（行き先の無いシンボリックリンク・ディレクトリ・FIFO）
bad_n=$((bad_n + 1))
make_detector "bad${bad_n}" "${lock_ok}"
rm -f "${det_bin%/scripts/hooks/secret-scan.sh}/skills-lock.json"
ln -s "no-such-target" "${det_bin%/scripts/hooks/secret-scan.sh}/skills-lock.json"
run_bad_lock "skills-lock.json as a dangling symlink is rejected"
bad_n=$((bad_n + 1))
make_detector "bad${bad_n}" "${lock_ok}"
rm -f "${det_bin%/scripts/hooks/secret-scan.sh}/skills-lock.json"
mkdir "${det_bin%/scripts/hooks/secret-scan.sh}/skills-lock.json"
run_bad_lock "skills-lock.json as a directory is rejected"
bad_n=$((bad_n + 1))
make_detector "bad${bad_n}" "${lock_ok}"
rm -f "${det_bin%/scripts/hooks/secret-scan.sh}/skills-lock.json"
mkfifo "${det_bin%/scripts/hooks/secret-scan.sh}/skills-lock.json"
run_bad_lock "skills-lock.json as a FIFO is rejected"

# 構造の記号を含む文字列値・1 行に詰めた JSON は正しく読める（行単位の深さの数え方では誤判定した形）
check_good_lock() {
  local desc="$1" content="$2"
  make_detector "good${bad_n}" "${content}"
  bad_n=$((bad_n + 1))
  new_repo; stage_lock; stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
  SCAN_BIN="${det_bin}" check_scan "${desc}" 0
}
check_good_lock "skills-lock.json with braces inside strings is accepted" '{
  "version": 2,
  "skills": {
    "vendored-a": {
      "source": "a{b}c } {",
      "note": "q\"{\\"
    }
  }
}'
check_good_lock "skills-lock.json packed on one line is accepted" '{"version":2,"skills":{"vendored-a":{"source":"x","n":[1,-2.5e3,true,null,{"k":[]}]}}}'

check_good_lock "skills-lock.json with valid multibyte UTF-8 is accepted" "$(lock_with_source "$(printf 'caf\xc3\xa9 \xe6\x97\xa5\xe6\x9c\xac \xf0\x9f\x98\x80')")"
check_good_lock "skills-lock.json with a raw U+0020 in a string is accepted" "$(lock_with_source "a b")"
check_good_lock "skills-lock.json with a raw U+007F in a string is accepted" "$(lock_with_source "a"$'\x7f'"b")"
check_good_lock "skills-lock.json with a 70-digit number is accepted" "{
  \"version\": 2,
  \"skills\": {
    \"vendored-a\": {
      \"big\": $(printf '1%.0s' {1..70}),
      \"neg\": -$(printf '2%.0s' {1..70}).5e10
    }
  }
}"
check_good_lock "skills-lock.json nested within the depth limit is accepted" "{
  \"version\": 2,
  \"skills\": {
    \"vendored-a\": {
      \"deep\": $(printf '[%.0s' {1..20})$(printf ']%.0s' {1..20})
    }
  }
}"

# 検出器のあるディレクトリ名が改行で終わっていても、除外の一覧を読める（dirname のコマンド置換は末尾の改行を落とす）
nl_root="${work_dir}/det-nl"
nl_dir="${nl_root}/s/h"$'\n'
mkdir -p "${nl_dir}"
cp "${SECRET_SCAN}" "${nl_dir}/secret-scan.sh"
printf '%s\n' "${lock_ok}" > "${nl_root}/skills-lock.json"
new_repo; stage_lock; stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${nl_dir}/secret-scan.sh" check_scan "detector in a directory whose name ends with a newline reads its list" 0

# scripts がシンボリックリンクでも、検出器の実体の場所から一覧を読む（論理的な `..` で別のルートを選ばない。#6000）
lnk_real="${work_dir}/det-real"
lnk_other="${work_dir}/det-other"
mkdir -p "${lnk_real}/scripts/hooks" "${lnk_other}"
cp "${SECRET_SCAN}" "${lnk_real}/scripts/hooks/secret-scan.sh"
printf '%s\n' "${lock_ok}" > "${lnk_real}/skills-lock.json"
ln -s "${lnk_real}/scripts" "${lnk_other}/scripts"
new_repo; stage_lock; stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${lnk_other}/scripts/hooks/secret-scan.sh" check_scan "detector reached through a symlinked scripts directory reads the real list" 0

# ファイル名だけで起動した相対リンクの先が / を含んでも、リンクの親ディレクトリを基準に解決する（#6000）
new_repo; stage_lock; stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
ln -s "../det-real/scripts/hooks/secret-scan.sh" sslink
SCAN_BIN="sslink" check_scan "a bare-name symlink with a relative target containing a slash resolves from the link's directory" 0

# 同じ checkout（検出器と走査対象が同じ repo）では、除外に使うのは HEAD・作業ツリー・index の 3 つすべてに
# 登録された名前（共通部分）だけ（#6000）。未コミットの作業ツリー・index の編集で除外を足せない。
same_lock_two='{
  "version": 2,
  "skills": {
    "vendored-a": {
      "source": "x"
    },
    "other": {
      "source": "y"
    }
  }
}'

same_checkout_repo "${lock_ok}"; stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a committed name is excluded" 0

same_checkout_repo "${lock_ok}"
printf '%s\n' "${same_lock_two}" > skills-lock.json
stage .agents/skills/other/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a name added in the working tree only is scanned" 1 .agents/skills/other/SKILL.md

same_checkout_repo "${lock_ok}"
printf '%s\n' "${same_lock_two}" > skills-lock.json
git add skills-lock.json
stage .agents/skills/other/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a name staged but not committed is scanned" 1 .agents/skills/other/SKILL.md

same_checkout_repo "${lock_ok}"
printf '%s\n' "${same_lock_two}" > skills-lock.json
stage .agents/skills/other/SKILL.md "x = ${token}"
SCAN_ARG=--tracked SCAN_BIN="${same_bin}" check_scan "same checkout (--tracked): a name added in the working tree only is scanned" 1 .agents/skills/other/SKILL.md

same_checkout_repo "${lock_ok}"
printf '%s\n' '{"version":2,"skills":{"vendored-b":{"source":"x"}}}' > skills-lock.json
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a committed name removed in the working tree is scanned" 1 .agents/skills/vendored-a/SKILL.md

same_checkout_repo "${lock_ok}"
printf '%s\n' '{ "version": 2, "skills": {' > skills-lock.json
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a broken working-tree list is rejected" 2

# 登録の削除をステージしたあと、作業ツリーだけへ登録を戻しても除外は残らない（index の一覧でも名前を確かめる。#6000）
same_checkout_repo "${lock_ok}"
printf '%s\n' '{"version":2,"skills":{"vendored-b":{"source":"x"}}}' > skills-lock.json
git add skills-lock.json
printf '%s\n' "${lock_ok}" > skills-lock.json
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a name unregistered in the index is scanned even if restored in the working tree" 1 .agents/skills/vendored-a/SKILL.md

same_checkout_repo "${lock_ok}"
git rm -q --cached skills-lock.json
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a list deleted from the index excludes nothing" 1 .agents/skills/vendored-a/SKILL.md

same_checkout_repo "${lock_ok}"
printf '%s\n' '{ "version": 2, "skills": {' > skills-lock.json
git add skills-lock.json
printf '%s\n' "${lock_ok}" > skills-lock.json
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a broken staged list is rejected" 2

same_checkout_repo
printf '%s\n' "${lock_ok}" > skills-lock.json
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a list absent from HEAD excludes nothing" 1 .agents/skills/vendored-a/SKILL.md

# 作業ツリーに一覧が無くても、同じ checkout では HEAD の一覧を検証する（壊れていれば 2。#6011）
# 拒否の原因を HEAD に限るため、index には有効な一覧を登録してから作業ツリーの一覧を消す（index の検証が先に拒否しない形）
same_checkout_repo '{ "version": 2, "skills": {'
printf '%s\n' "${lock_ok}" > skills-lock.json
git add skills-lock.json
rm -f skills-lock.json
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a broken committed list is rejected even without a working-tree list" 2
expect_out "same checkout: a broken committed list without a working-tree list reports a fixed message" "secret-scan: skills-lock.json is unreadable or has an unexpected shape; aborting"

# 作業ツリーに一覧が無く HEAD の一覧が有効なら、何も除外しない
same_checkout_repo "${lock_ok}"
rm -f skills-lock.json
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a valid committed list without a working-tree list excludes nothing" 1 .agents/skills/vendored-a/SKILL.md

# 作業ツリーの一覧がシンボリックリンクなら（リンク先が有効でも）拒否する
same_checkout_repo "${lock_ok}"
cp skills-lock.json real-lock.json
rm -f skills-lock.json
ln -s real-lock.json skills-lock.json
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a working-tree list that is a symlink is rejected" 2
expect_out "same checkout: a symlinked working-tree list reports a fixed message" "secret-scan: skills-lock.json is unreadable or has an unexpected shape; aborting"

# HEAD の一覧がシンボリックリンクなら（作業ツリーが通常ファイルでも）拒否する
new_repo
mkdir -p scripts/hooks
cp "${SECRET_SCAN}" scripts/hooks/secret-scan.sh
# リンク先の文字列そのものを有効な一覧にして、blob の中身の構文検査だけでは拒否されない形にする
ln -s '{"version":2,"skills":{"vendored-a":{}}}' skills-lock.json
git add -A
git commit -q -m "chore: add detector with a symlinked list"
rm -f skills-lock.json
printf '%s\n' "${lock_ok}" > skills-lock.json
# index も通常ファイルの有効な一覧にして、拒否の原因を HEAD の種類に限る
git add skills-lock.json
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a committed list that is a symlink is rejected" 2
expect_out "same checkout: a symlinked committed list reports a fixed message" "secret-scan: skills-lock.json is unreadable or has an unexpected shape; aborting"

# index の一覧がシンボリックリンク（mode 120000）なら、作業ツリーと HEAD が通常ファイルの有効な一覧でも拒否する。
# リンク先の文字列そのものを有効な一覧にして、blob の中身の構文検査だけでは拒否されない形にする（拒否の原因を index の種類に限る）
same_checkout_repo "${lock_ok}"
rm -f skills-lock.json
ln -s '{"version":2,"skills":{"vendored-a":{}}}' skills-lock.json
git add skills-lock.json
rm -f skills-lock.json
printf '%s\n' "${lock_ok}" > skills-lock.json
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
if [ "$(git ls-files -s -- skills-lock.json | cut -d' ' -f1)" = 120000 ]; then
  pass "secret-scan: same checkout: the staged list is a symlink entry in the test setup"
else
  fail "secret-scan: same checkout: the staged list is a symlink entry in the test setup" "index: $(git ls-files -s -- skills-lock.json)"
fi
SCAN_BIN="${same_bin}" check_scan "same checkout: a staged list that is a symlink is rejected" 2
expect_out "same checkout: a symlinked staged list reports a fixed message" "secret-scan: skills-lock.json is unreadable or has an unexpected shape; aborting"

# 検出器の一覧がシンボリックリンク（リンク先は有効）なら拒否する
bad_n=$((bad_n + 1))
make_detector "bad${bad_n}" "${lock_ok}"
mv "${det_bin%/scripts/hooks/secret-scan.sh}/skills-lock.json" "${det_bin%/scripts/hooks/secret-scan.sh}/real-lock.json"
ln -s real-lock.json "${det_bin%/scripts/hooks/secret-scan.sh}/skills-lock.json"
run_bad_lock "skills-lock.json as a symlink to a valid file is rejected"

# HEAD の参照: 未作成ブランチは除外なしで走査する。参照があるのに解決できない場合は検査不能（2）
same_checkout_repo "${lock_ok}"
git symbolic-ref HEAD refs/heads/unborn
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: an unborn branch excludes nothing" 1 .agents/skills/vendored-a/SKILL.md


finish "part 2"
