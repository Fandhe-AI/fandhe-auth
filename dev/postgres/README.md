# 開発・検証用 PostgreSQL

契約テストを流す参照実装向けの、開発・検証用 PostgreSQL の起動定義（`TASK-5`、2026-11-04 の追随で範囲を絞った後の内容）。

## 位置づけ

- 開発・検証用だけである。**本番では Docker を必須にしない**（`TASK-5`）
- 本番の DB の所有・接続・migration は Hub の担当（Hub `ASTORE-5`・`ASTORE-46`・`ASTORE-47`）。ここでは扱わない
- 構成の根拠は `PoC-1`（PostgreSQL 17）。版と digest の更新は人が決める

## 前提

Docker と Docker Compose v2 以降。

## 起動

パスワードは実行時に生成して環境にだけ置く。値を表示せず、ファイル・コマンドライン引数に書かない。

```bash
export FANDHE_AUTH_DEV_PG_PASSWORD="$(openssl rand -hex 16)"
docker compose -f dev/postgres/compose.yaml up -d --wait
```

- 公開は `127.0.0.1` のみ。ポートは `FANDHE_AUTH_DEV_PG_PORT`（既定 55432）で変える
- パスワードが未設定なら起動に失敗する
- `--env-file` を付けない場合、compose は変数補間用の `.env` を自動で読む。読み込み元は project directory（`-f` 指定時は
  compose ファイルのあるディレクトリ）と実行ディレクトリで、compose の版により異なる。リポジトリルートから実行するとルートの
  `.env` も読まれうる。読み込み元を固定したいときは `--env-file` を明示する。`dev/postgres/.env` は置いてもコミットされないが、使わない

確かめ:

```bash
docker compose -f dev/postgres/compose.yaml exec postgres psql -U postgres -c 'select version()'
```

## 接続設定

資格情報はリポジトリに入れず、環境から注入する（`TASK-5`・`SECR-6`）。変数名は開発用の暫定で、人が決める。
雛形は `.env.example`（秘密でないポートだけ）。

| 項目 | 値 |
| --- | --- |
| host | `127.0.0.1` |
| port | `FANDHE_AUTH_DEV_PG_PORT`（既定 55432） |
| user | `postgres` |
| dbname | `postgres` |
| password | `FANDHE_AUTH_DEV_PG_PASSWORD` から注入する。値は書かない |

- ルートの `.env`（`make setup` が作る）は、実行ディレクトリがルートのとき compose に自動で読まれうる。ポートを変えるときは
  `export` するか、`docker compose --env-file .env -f dev/postgres/compose.yaml up` のように `--env-file` で読み込み元を明示する
- ルートの `.env` にパスワードを書かない。起動時に生成して export する
- ホストからの接続例。パスワードは環境変数で渡し、引数・URL に書かない

```bash
PGPASSWORD="${FANDHE_AUTH_DEV_PG_PASSWORD}" psql "host=127.0.0.1 port=${FANDHE_AUTH_DEV_PG_PORT:-55432} user=postgres dbname=postgres" -c 'select 1'
```

## 停止

```bash
docker compose -f dev/postgres/compose.yaml down
```

データは tmpfs にあり、停止で消える。

## 注意

- `docker compose config`（`-q` なし）と `docker inspect` は展開したパスワードを表示する。出力を PR・Issue・ログに貼らない
- 接続文字列を URL の形（userinfo 付き）で文書・コードに書かない

## 持ち越し

- TLS・ロール・`max_connections`: #39
- 手順のまとめとルート README への反映: #40
