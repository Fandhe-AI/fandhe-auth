#!/usr/bin/env bash
# 開発・検証用 PostgreSQL（dev/postgres/compose.yaml。TASK-5）の起動の確かめ。手で呼ぶ。
# 契約テストを流す参照実装が前提にする条件（TLS 必須の接続・接続数・REVOKE/GRANT の効き・64 並列）が
# 起動中のインスタンスで揃っていることだけを確かめる。起動も停止もしない。CI では流さない（runner に DB は無い。TASK-6）。
# 作るのは一時のロールと DB だけで、trap で必ず消す。秘密値は表示せず、docker・psql の引数にも載せない。
# AUDB-80 のカタログ点検・100 回の試行・変異は TASK-797、AUDB-82 の 40P01 の吸収の試験は TASK-799 の担当で、
# ここでは「流せる前提」の確認にとどめる。両行は検討中のままで、本スクリプトの成功は確定の根拠にしない（ACPT-7）。
# 終了コード: 0 = 全項目 ok、1 = 失敗あり、2 = 誤用。
# compose の project 名は COMPOSE_PROJECT_NAME で固有にでき、並列の検証と衝突させない。
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

reject_args "$(basename "$0")" "$@"
require_cmd docker
require_cmd openssl
cd "${ROOT_DIR}"

# compose.yaml の補間と同じ変数。値は表示しない。
if [ -z "${FANDHE_AUTH_DEV_PG_PASSWORD:-}" ]; then
  echo "error: FANDHE_AUTH_DEV_PG_PASSWORD must be set (see dev/postgres/README.md)" >&2
  exit 1
fi
FANDHE_AUTH_DEV_PG_PORT="${FANDHE_AUTH_DEV_PG_PORT:-55432}"
export FANDHE_AUTH_DEV_PG_PORT FANDHE_AUTH_DEV_PG_PASSWORD

# docker compose config（-q 無し）は展開した値を出すので使わない。
dc() { docker compose -f dev/postgres/compose.yaml "$@"; }
# 管理用: コンテナ内の unix socket（pg_hba の local trust）。SQL は標準入力で渡す。
admin() { dc exec -T postgres psql -U postgres -v ON_ERROR_STOP=1 -qAt "$@"; }

failed=""
fail() { failed="${failed} $1"; echo "[$(basename "$0")] FAIL: $1" >&2; }
pass() { log "ok: $1"; }
# expect <name> <actual> <expected>
expect() { if [ "$2" = "$3" ]; then pass "$1"; else fail "$1"; fi; }

cid="$(dc ps -q postgres 2>/dev/null || true)"
if [ -z "${cid}" ]; then
  echo "error: postgres container is not running (see dev/postgres/README.md \"起動\")" >&2
  exit 1
fi
health="$(docker inspect --format '{{.State.Health.Status}}' "${cid}" 2>/dev/null || true)"
if [ "${health}" != "healthy" ]; then
  echo "error: postgres container is not healthy (status: ${health:-unknown})" >&2
  exit 1
fi
pass "container healthy"

# 公開はループバックのみ。
mapped="$(dc port postgres 5432 2>/dev/null || true)"
case "${mapped}" in
  127.0.0.1:*) pass "published on loopback only" ;;
  *) fail "published-on-loopback" ;;
esac
host_port="${mapped##*:}"
if [ -n "${host_port}" ] && (exec 3<>"/dev/tcp/127.0.0.1/${host_port}") 2>/dev/null; then
  pass "host tcp reachable"
else
  fail "host-tcp-reachable"
fi

# サーバーの設定。64 は AUDB-82・ADB-11 の試験の条件で、要件の値ではない。
ver="$(admin -c 'show server_version_num' | cut -c1-2)"
expect "postgres 17 (PoC-1)" "${ver}" "17"
expect "ssl on" "$(admin -c 'show ssl')" "on"
usable="$(admin -c "select current_setting('max_connections')::int - current_setting('superuser_reserved_connections')::int - current_setting('reserved_connections')::int")"
if [ "${usable:-0}" -ge 64 ]; then pass "usable connections >= 64"; else fail "usable-connections"; fi

# 一時のオブジェクト。名前は 16 進だけで組み、外部入力を SQL に連結しない。
sfx="$(openssl rand -hex 4)"
db="fa_devchk_${sfx}"
app="fa_devchk_${sfx}_app"
other="fa_devchk_${sfx}_other"
app_pw="$(openssl rand -hex 16)"
other_pw="$(openssl rand -hex 16)"

cleanup() {
  admin <<SQL >/dev/null 2>&1 || true
DROP DATABASE IF EXISTS ${db} WITH (FORCE);
DROP ROLE IF EXISTS ${app};
DROP ROLE IF EXISTS ${other};
SQL
}
trap cleanup EXIT

admin >/dev/null <<SQL
CREATE ROLE ${app} LOGIN PASSWORD '${app_pw}';
CREATE ROLE ${other} LOGIN PASSWORD '${other_pw}';
CREATE DATABASE ${db};
SQL

# コンテナ内の TCP 接続。パスワードはホストの環境変数に置き、docker へは名前だけ渡す（-e PGPASSWORD。値は argv に出ない）。
tcp_psql() { # <role> <pwvar> <sslmode> ...psql args
  local role="$1" pwvar="$2" mode="$3"
  shift 3
  PGPASSWORD="${!pwvar}" dc exec -T -e PGPASSWORD postgres \
    psql "host=127.0.0.1 port=5432 user=${role} dbname=${db} sslmode=${mode}" -v ON_ERROR_STOP=1 -qAt "$@"
}

if tcp_psql "${app}" app_pw disable -c 'select 1' >/dev/null 2>&1; then
  fail "plaintext-tcp-rejected"
else
  pass "plaintext tcp rejected (AUDP-9)"
fi
expect "tls login with scram (sslmode=require)" \
  "$(tcp_psql "${app}" app_pw require -c 'select ssl from pg_stat_ssl where pid = pg_backend_pid()' 2>/dev/null || true)" "t"
expect "temp role is not superuser" \
  "$(admin -c "select rolsuper from pg_roles where rolname = '${app}'")" "f"

# AUDB-80 の前提だけ: REVOKE/GRANT が効く環境か（本体は TASK-797）。
admin -d "${db}" >/dev/null <<SQL
CREATE FUNCTION public.fa_probe() RETURNS int LANGUAGE sql AS 'select 1';
REVOKE EXECUTE ON FUNCTION public.fa_probe() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION public.fa_probe() TO ${app};
GRANT USAGE ON SCHEMA public TO ${app}, ${other};
SQL
expect "granted role can execute" \
  "$(tcp_psql "${app}" app_pw require -c 'select public.fa_probe()' 2>/dev/null || true)" "1"
denied="$(tcp_psql "${other}" other_pw require -c '\set VERBOSITY verbose' -c 'select public.fa_probe()' 2>&1 || true)"
case "${denied}" in
  *42501*) pass "ungranted role denied with 42501" ;;
  *) fail "revoke-effective" ;;
esac

# AUDB-82 の前提だけ: 64 本の同時接続が収まる（40P01 の吸収の試験は TASK-799）。
ok_count="$(PGPASSWORD="${app_pw}" dc exec -T -e PGPASSWORD postgres bash -s <<SCRIPT 2>/dev/null || true
for i in \$(seq 1 64); do
  ( psql "host=127.0.0.1 port=5432 user=${app} dbname=${db} sslmode=require" -qAt -c 'select pg_sleep(1)' >/dev/null 2>&1 && echo ok ) &
done | wc -l
wait
SCRIPT
)"
ok_count="$(printf '%s' "${ok_count}" | tr -d '[:space:]')"
if [ "${ok_count}" = "64" ]; then pass "64 concurrent tls connections (ADB-11)"; else fail "concurrent-64"; fi

if [ -n "${failed}" ]; then
  echo "summary: FAILED:${failed}" >&2
  exit 1
fi
log "summary: all checks ok (not a spec confirmation; ACPT-7)"
