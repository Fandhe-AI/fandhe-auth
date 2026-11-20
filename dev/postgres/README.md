# 開発・検証用 PostgreSQL

契約テストを流す参照実装向けの、開発・検証用 PostgreSQL の起動定義（`TASK-5`、2026-11-04 の追随で範囲を絞った後の内容）。

手順の流れは「前提 → 起動 → 初期化（初回の起動で自動）→ 起動の確かめ → 接続 → 停止 → 初期化のやり直し」。

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
- TLS は起動のたびに自己署名の証明書をコンテナ内（tmpfs）に生成する（`initdb/tls.sh`）。鍵はホストにもリポジトリにも出さない。
  TCP の平文接続は拒否し（`AUDP-9` → Hub `ASTORE-55`）、認証は `scram-sha-256`。証明書の検証（`verify-full`）は対象外で、
  `sslmode=require` の範囲にとどめる
- `max_connections` は 300。`AUDB-82`・`ADB-11` の 64 並列と試験ごとのプールを収める開発の設定で、PoC の起動の条件に由来する。
  要件の値ではなく、人が決める
- `--env-file` を付けない場合、compose は変数補間用の `.env` を自動で読む。読み込み元は project directory（`-f` 指定時は
  compose ファイルのあるディレクトリ）と実行ディレクトリで、compose の版により異なる。リポジトリルートから実行するとルートの
  `.env` も読まれうる。読み込み元を固定したいときは `--env-file` を明示する。`dev/postgres/.env` は置いてもコミットされないが、使わない

確かめ:

```bash
docker compose -f dev/postgres/compose.yaml exec postgres psql -U postgres -c 'select version()'
```

## 初期化

初回の起動（データが無い状態）で、次が自動で行われる。手で流す手順は無い。

- `initdb` が認証方式を `scram-sha-256` にする（`POSTGRES_INITDB_ARGS` の `--auth-host=scram-sha-256`）
- `initdb/tls.sh` が自己署名の証明書を作り、`ssl = on` にして `pg_hba.conf` を書き換える（TCP の平文は `reject`）

データは tmpfs にあるため、初期化をやり直すには `down` で消してから `up` し直す。パスワードは `up` のたびに生成して `export` する。

```bash
FANDHE_AUTH_DEV_PG_PASSWORD=unused docker compose -f dev/postgres/compose.yaml down
export FANDHE_AUTH_DEV_PG_PASSWORD="$(openssl rand -hex 16)"
docker compose -f dev/postgres/compose.yaml up -d --wait
```

ここで行わないこと: migration の適用・常設ロールの作成・DB の主体の分離。`TASK-5` の 2026-11-04 の追随で範囲から外れており、
Hub `ASTORE-5` 以降の担当である（下の「ロール」）。

## 接続設定

資格情報はリポジトリに入れず、環境から注入する（`TASK-5`・`SECR-6`）。変数名は開発用の暫定で、人が決める。
雛形は `.env.example`（秘密でないポートだけ）。

| 項目 | 値 |
| --- | --- |
| host | `127.0.0.1` |
| port | `FANDHE_AUTH_DEV_PG_PORT`（既定 55432） |
| user | `postgres` |
| dbname | `postgres` |
| sslmode | `require` |
| password | `FANDHE_AUTH_DEV_PG_PASSWORD` から注入する。値は書かない |

- ルートの `.env`（`make setup` が作る）は、実行ディレクトリがルートのとき compose に自動で読まれうる。ポートを変えるときは
  `export` するか、`docker compose --env-file .env -f dev/postgres/compose.yaml up` のように `--env-file` で読み込み元を明示する。
  `--env-file` で読んだ値は compose にだけ渡り、呼び出し元のシェルには `export` されない。ホストからの接続（下の `psql` 例）も
  同じポートに揃えるため、起動と接続のどちらでも `FANDHE_AUTH_DEV_PG_PORT` を `export` する（`.env` を使うなら
  `set -a; . ./.env; set +a` で読み込む）。揃えないと `psql` は既定の 55432 に接続する
- ルートの `.env` にパスワードを書かない。起動時に生成して export する
- ホストからの接続例。パスワードは環境変数で渡し、引数・URL に書かない

```bash
PGPASSWORD="${FANDHE_AUTH_DEV_PG_PASSWORD}" psql "host=127.0.0.1 port=${FANDHE_AUTH_DEV_PG_PORT:-55432} user=postgres dbname=postgres sslmode=require" -c 'select 1'
```

## 停止

```bash
FANDHE_AUTH_DEV_PG_PASSWORD=unused docker compose -f dev/postgres/compose.yaml down
```

データは tmpfs にあり、停止で消える。compose.yaml は `FANDHE_AUTH_DEV_PG_PASSWORD` を必須の補間にしているため、
変数が未設定だと `down` も補間で失敗する。起動時の `export` が残っていなければ、上のように値を与える
（停止には使われない値で、実際のパスワードでなくてよい）。

## 起動の確かめ

契約テストを流す参照実装が前提にする条件が、起動中のインスタンスで揃っていることを確かめる。起動も停止もしない。

```bash
make pg-check   # 直接: scripts/dev-pg-check.sh
```

`FANDHE_AUTH_DEV_PG_PASSWORD` を起動時と同じ値で `export` しておく。ターゲット名 `pg-check` は暫定で、人が確定する。
起動・停止の make ターゲットは置かない（生成したパスワードを呼び出し元のシェルへ `export` できず、保存へ流れるため。`SECR-6`）。

- 確かめる項目: コンテナの healthy・ループバックだけの公開・PostgreSQL 17・`ssl on`・使える接続数が 64 以上・
  `pg_hba_file_rules` の実効ルール（host 系が `scram-sha-256` か `reject` だけで、`hostssl` は `scram-sha-256`・
  `hostnossl` は `reject`）・平文の TCP の拒否・誤ったパスワードの拒否・正しい資格情報での TLS ログイン・
  一時ロールが非 superuser・`REVOKE`/`GRANT` が効くこと（`AUDB-80` の前提だけ）・
  64 本の同時の TLS 接続（`AUDB-82` の前提だけ。全接続をバリアで保持し、`pg_stat_activity` で 64 到達を確かめてから解放する）
- 一時のロールと DB（`fa_devchk_` 接頭辞）を作り、終了時に必ず消す。名前とパスワードは SQL に連結せず、環境変数経由で psql 変数に束縛する（`AUDB-43`）。パスワードは表示せず、引数にも載せない
- 失敗が 1 つでもあれば非 0 で終わる。CI では流さない（runner に DB が無い。`TASK-6`）
- 並列で流すときは `COMPOSE_PROJECT_NAME` とポートを固有にする
- 成功は「前提が揃った」という証拠にとどまる。`AUDB-80`・`AUDB-82` は検討中のままで、確定は人が記録する（`ACPT-7`）

## ロール

常設のロールは作らない。参照実装の試験の道具が superuser で試験用のロールと DB をその場で作る。
DB の主体の分離・初期設定は Hub `ASTORE-5` 以降の担当で、ここでは扱わない。

## 注意

- `docker compose config`（`-q` なし）と `docker inspect` は展開したパスワードを表示する。出力を PR・Issue・ログに貼らない
- 接続文字列を URL の形（userinfo 付き）で文書・コードに書かない

## 持ち越し

- `AUDB-80` の確かめの本体（カタログの点検・100 回の試行・変異）: `TASK-797`。`AUDB-82` の `40P01` の吸収の試験: `TASK-799`
- CI への搭載: DB ジョブの枠（`it-db-ci`。`#52`）は harness 経由で compose を起動する。`pg-check` は CI 外のまま。他の Docker の試験の搭載は人が決める（`TASK-6`）
- TLS 証明書の検証（`verify-full`。`AUDP-10`・`AUDP-11`）・`ssl_min_protocol_version` の引き上げ: 人が決める
