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
admin() { dc exec -T postgres psql -U postgres -v ON_ERROR_STOP=1 -qAt "$@"; }  # 値を含まない SQL 専用

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

# 一時のオブジェクト。名前は 16 進だけで組む。名前もパスワードも SQL の文字列に連結せず、環境変数で
# コンテナへ渡して psql の \getenv で変数に取り、:'var'（リテラル）・:"var"（識別子）・format の %I・%L で束縛する（AUDB-43）。
# 値は docker・psql の argv にも載せない（-e NAME は名前だけを渡す）。
sfx="$(openssl rand -hex 4)"
FA_DB="fa_devchk_${sfx}"
FA_APP="fa_devchk_${sfx}_app"
FA_OTHER="fa_devchk_${sfx}_other"
FA_APP_PW="$(openssl rand -hex 16)"
FA_OTHER_PW="$(openssl rand -hex 16)"
export FA_DB FA_APP FA_OTHER FA_APP_PW FA_OTHER_PW

# 各 SQL の先頭に付ける。環境変数を psql 変数へ取り込む（psql 15 以降）。
pre='\getenv fa_db FA_DB
\getenv fa_app FA_APP
\getenv fa_other FA_OTHER
\getenv fa_app_pw FA_APP_PW
\getenv fa_other_pw FA_OTHER_PW'
# 管理用（unix socket・trust）で、標準入力の SQL に pre を前置して流す。
admin_env() {
  { printf '%s\n' "${pre}"; cat; } |
    dc exec -T -e FA_DB -e FA_APP -e FA_OTHER -e FA_APP_PW -e FA_OTHER_PW postgres \
      psql -U postgres -v ON_ERROR_STOP=1 -qAt "$@"
}

cleanup() {
  admin_env <<'SQL' >/dev/null 2>&1 || true
SELECT format('DROP DATABASE IF EXISTS %I WITH (FORCE)', :'fa_db') \gexec
SELECT format('DROP ROLE IF EXISTS %I', :'fa_app') \gexec
SELECT format('DROP ROLE IF EXISTS %I', :'fa_other') \gexec
SQL
}
trap cleanup EXIT

admin_env >/dev/null <<'SQL'
SELECT format('CREATE ROLE %I LOGIN PASSWORD %L', :'fa_app', :'fa_app_pw') \gexec
SELECT format('CREATE ROLE %I LOGIN PASSWORD %L', :'fa_other', :'fa_other_pw') \gexec
SELECT format('CREATE DATABASE %I', :'fa_db') \gexec
SQL

# コンテナ内の TCP 接続。パスワードはホストの環境変数に置き、docker へは名前だけ渡す（-e PGPASSWORD。値は argv に出ない）。
tcp_psql() { # <role> <pwvar> <sslmode> ...psql args
  local role="$1" pwvar="$2" mode="$3"
  shift 3
  PGPASSWORD="${!pwvar}" dc exec -T -e PGPASSWORD postgres \
    psql "host=127.0.0.1 port=5432 user=${role} dbname=${FA_DB} sslmode=${mode}" -v ON_ERROR_STOP=1 -qAt "$@"
}

# 認証の方式: TLS の成功だけでは scram の証拠にならない（hostssl が trust でも通る）ので、実効ルールを見る。
# 実効ルールは pg_hba_file_rules。host 系は scram-sha-256 か reject だけで、両方が存在すること。
expect "pg_hba: no host rule other than scram-sha-256/reject" \
  "$(echo "select count(*) from pg_hba_file_rules where error is null and type like 'host%' and auth_method not in ('scram-sha-256','reject')" | admin_env)" "0"
expect "pg_hba: hostssl is scram-sha-256" \
  "$(echo "select count(*) > 0 from pg_hba_file_rules where error is null and type = 'hostssl' and auth_method = 'scram-sha-256'" | admin_env)" "t"
expect "pg_hba: hostnossl is reject" \
  "$(echo "select count(*) > 0 from pg_hba_file_rules where error is null and type = 'hostnossl' and auth_method = 'reject'" | admin_env)" "t"

# 平文の拒否（pg_hba の拒否であること）・誤ったパスワードの拒否・正しい資格情報での TLS ログインを別々に判定する。
plain="$(tcp_psql "${FA_APP}" FA_APP_PW disable -c 'select 1' 2>&1 || true)"
case "${plain}" in
  *pg_hba.conf*) pass "plaintext tcp rejected by pg_hba (AUDP-9)" ;;
  *) fail "plaintext-tcp-rejected" ;;
esac
wrong="$(tcp_psql "${FA_APP}" FA_OTHER_PW require -c 'select 1' 2>&1 || true)"
case "${wrong}" in
  *"password authentication failed"*) pass "wrong password rejected over tls" ;;
  *) fail "wrong-password-rejected" ;;
esac
expect "tls login with correct password (sslmode=require)" \
  "$(tcp_psql "${FA_APP}" FA_APP_PW require -c 'select ssl from pg_stat_ssl where pid = pg_backend_pid()' 2>/dev/null || true)" "t"
expect "temp role is not superuser" \
  "$(echo "select rolsuper from pg_roles where rolname = :'fa_app'" | admin_env)" "f"

# AUDB-80 の前提だけ: REVOKE/GRANT が効く環境か（本体は TASK-797）。
admin_env -d "${FA_DB}" >/dev/null <<'SQL'
CREATE FUNCTION public.fa_probe() RETURNS int LANGUAGE sql AS 'select 1';
REVOKE EXECUTE ON FUNCTION public.fa_probe() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION public.fa_probe() TO :"fa_app";
GRANT USAGE ON SCHEMA public TO :"fa_app", :"fa_other";
SQL
expect "granted role can execute" \
  "$(tcp_psql "${FA_APP}" FA_APP_PW require -c 'select public.fa_probe()' 2>/dev/null || true)" "1"
denied="$(tcp_psql "${FA_OTHER}" FA_OTHER_PW require -c '\set VERBOSITY verbose' -c 'select public.fa_probe()' 2>&1 || true)"
case "${denied}" in
  *42501*) pass "ungranted role denied with 42501" ;;
  *) fail "revoke-effective" ;;
esac

# AUDB-82・ADB-11 の前提だけ: 64 本の同時接続が収まる（40P01 の吸収の試験は TASK-799）。
# 各接続は 1 文を流した後、バリア（go ファイル）が現れるまで標準入力で保持する。管理接続から対象ロールの
# 接続数（pg_stat_activity）が 64 に達したことを確かめてから解放し、解放後の文も全接続で成功することを数える。
conc="$(PGPASSWORD="${FA_APP_PW}" dc exec -T -e PGPASSWORD -e FA_APP -e FA_DB postgres bash -s <<'SCRIPT' 2>/dev/null || true
set -u
dir="$(mktemp -d)"
trap 'rm -rf "${dir}"' EXIT
for i in $(seq 1 64); do
  (
    {
      echo 'select 1;'
      while [ ! -e "${dir}/go" ]; do sleep 0.1; done
      echo 'select 2;'
    } | psql "host=127.0.0.1 port=5432 user=${FA_APP} dbname=${FA_DB} sslmode=require" -v ON_ERROR_STOP=1 -qAt >/dev/null 2>&1 &&
      : > "${dir}/ok.${i}"
  ) &
done
peak=0
end=$((SECONDS + 30))
while [ "${SECONDS}" -lt "${end}" ]; do
  n="$(echo "select count(*) from pg_stat_activity where backend_type = 'client backend' and usename = :'app' and datname = :'db'" |
    psql -U postgres -v app="${FA_APP}" -v db="${FA_DB}" -qAt 2>/dev/null || echo 0)"
  [ "${n:-0}" -gt "${peak}" ] && peak="${n}"
  [ "${peak}" -ge 64 ] && break
  sleep 0.2
done
: > "${dir}/go"
wait
ok="$(find "${dir}" -name 'ok.*' | wc -l)"
echo "peak=${peak} ok=${ok}"
SCRIPT
)"
conc="$(printf '%s' "${conc}" | tr -d '[:space:]')"
if [ "${conc}" = "peak=64ok=64" ]; then pass "64 concurrent tls connections held at the barrier (ADB-11)"; else fail "concurrent-64"; fi

if [ -n "${failed}" ]; then
  echo "summary: FAILED:${failed}" >&2
  exit 1
fi
log "summary: all checks ok (not a spec confirmation; ACPT-7)"
