#!/usr/bin/env bash
# 開発・検証用 PostgreSQL の初期化（TASK-5）。公式 image の初期化（/docker-entrypoint-initdb.d）から
# コンテナ内で 1 度だけ呼ばれる。参照実装の接続は TLS 必須（Require。AUDP-9 → Hub ASTORE-55）なので、
# 起動のたびに自己署名の証明書をコンテナ内（tmpfs の PGDATA）に作り、TCP を TLS 必須にする。
# コンテナ内で動くので scripts/lib.sh は読まない。鍵はホストにもリポジトリにも出さない。
# 証明書の検証（VerifyFull。AUDP-10・AUDP-11）は対象外で、Require の範囲にとどめる。
set -euo pipefail

# 有効日数は開発の設定で、要件の値ではない（tmpfs なので起動のたびに作り直される）。
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes \
  -subj "/CN=localhost" -days 30 \
  -keyout "${PGDATA}/server.key" -out "${PGDATA}/server.crt" >/dev/null 2>&1
chmod 600 "${PGDATA}/server.key"

# ssl_cert_file・ssl_key_file は既定（PGDATA 相対の server.crt・server.key）を使う。
# ssl_min_protocol_version は既定のまま。引き上げるかは人が決める。
# 初期化中の一時サーバーは TLS 無しで動き、この設定は本体の起動から効く。
echo "ssl = on" >> "${PGDATA}/postgresql.conf"

# 公式 entrypoint は pg_hba.conf を書いた後でこのスクリプトを流すので、上書きが残る。
# local の trust はコンテナ内の unix socket だけ（docker compose exec での管理用）。
cat > "${PGDATA}/pg_hba.conf" <<'HBA'
local all all trust
hostssl all all all scram-sha-256
hostnossl all all all reject
HBA
