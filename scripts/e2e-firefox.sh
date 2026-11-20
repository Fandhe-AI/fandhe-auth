#!/usr/bin/env bash
# ハーネス（crates/e2e-harness）の Firefox の仮想 authenticator の E2E（#[ignore]）を流す（TASK-11 (3/6)）。手で呼ぶ。
# 呼び出し元: `make e2e-firefox`。CI では流さない（runner のブラウザは前提にしない。TASK-6）。
# geckodriver と Firefox が無ければ失敗する（黙って飛ばさない。DPOL-30〜32）。結果は〔仮想〕で実機・Safari に数えない（ACPT-7）。
# 前提: 仮想 authenticator の WebDriver エンドポイントを使える Firefox と geckodriver の組み合わせ（両バイナリの存在だけでは足りない）。
# 使える版の組み合わせは、この repo ではまだ実測していない（実機の Firefox・geckodriver が無い環境で作ったため。未検証）。
# 既定でエンドポイントが無効な版では登録操作の前に失敗する。ハーネスは geckodriver が起動する Firefox へ capabilities の
# `moz:firefoxOptions.prefs` で softtoken 有効・usbtoken 無効の 2 つの pref を渡す（crates/e2e-harness/src/firefox.rs）。
# これ以外の設定の渡し方は無い。pref の切り替えだけで足りない版は、より新しい Firefox・geckodriver の組み合わせで試す。
# 人が実測して記録する: 候補の組み合わせで `make e2e-firefox` を流し、登録の操作まで進むものを控える（PoC-3 の値。要件の値ではない）。
# 前提: unix（Linux・macOS 等。geckodriver をプロセスグループで起動・停止するため）。
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

reject_args "$(basename "$0")" "$@"
log "requires a Firefox + geckodriver pair whose WebDriver virtual authenticator endpoints are enabled; see the header of this script"
run_e2e_registration "e2e-firefox" geckodriver firefox_registration
