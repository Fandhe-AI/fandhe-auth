#!/usr/bin/env bash
# ハーネス（crates/e2e-harness）の Firefox の仮想 authenticator の E2E（#[ignore]）を流す（TASK-11 (3/6)）。手で呼ぶ。
# 呼び出し元: `make e2e-firefox`。CI では流さない（runner のブラウザは前提にしない。TASK-6）。
# geckodriver と Firefox が無ければ失敗する（黙って飛ばさない。DPOL-30〜32）。結果は〔仮想〕で実機・Safari に数えない（ACPT-7）。
# 前提: 仮想 authenticator の WebDriver エンドポイントを使える Firefox と geckodriver の組み合わせ（両バイナリの存在だけでは足りない）。
# 動作の記録がある組み合わせ（spec の 03-poc/ の PoC の記録。〔仮想〕）: Firefox 156〜158 と geckodriver 0.37.1。
# ハーネスは geckodriver が起動する Firefox へ capabilities の `moz:firefoxOptions.prefs` で softtoken 有効・usbtoken 無効の
# 2 つの pref を渡し（crates/e2e-harness/src/firefox.rs）、この 2 つだけで登録まで通った記録（PoC-3 の値。要件の値ではない）。
# それより古い版（geckodriver 0.34.0 等）や、2 つの pref で足りない版は未確認で、この repo の `make e2e-firefox` でも未実行。
# 追加の pref を渡す口は無く、足りない版は登録操作の前に失敗する。その場合は上の組み合わせへ上げて試し、結果を人が記録する。
# 前提: unix（Linux・macOS 等。geckodriver をプロセスグループで起動・停止するため）。
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

reject_args "$(basename "$0")" "$@"
log "requires a Firefox + geckodriver pair with WebDriver virtual authenticator support (recorded as working: Firefox 156-158 + geckodriver 0.37.1); see the header of this script"
run_e2e_registration "e2e-firefox" geckodriver firefox_registration
