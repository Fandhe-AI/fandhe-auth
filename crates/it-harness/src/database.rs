//! ハーネスの DB 構成要素（`dev/postgres` の compose を試験ごとに起動・停止する。TASK-10 (2/6)・TASK-5）。
//!
//! 役割: 結合試験が試験ごとに独立した PostgreSQL を持てるようにし、接続情報を [`EndpointHandle`] で
//! 試験と Host（最小の組込み先。TASK-716 の完成後に受け取る側を置く）へ渡す。呼び出し元は
//! [`crate::harness::Harness`]（`Component` として起動・停止する）と結合試験だけ。
//!
//! 方針:
//! - 起動・停止・確かめは `docker compose` と、コンテナ内の `psql` を子プロセスで呼ぶ。DB ドライバーと
//!   乱数の crate は足さない（`docs/harness/integration.md`。選定の範囲外）。
//! - パスワードは `openssl rand` で試験プロセスの中で生成して [`SecretString`] で持ち、子プロセスの
//!   環境変数にだけ渡す。argv・誤り・`Debug`・ファイル・ログに載せない（SECR-6・SECR-11・SECR-23・SECR-30）。
//!   生成を自前で書かない（DPOL-47）。`docker compose config`・`docker inspect` は展開した値を出すので呼ばない。
//! - 設定は引数の型で受け、環境変数・ファイルは読まない。検証を外す設定は作らない（DPOL-30〜32）。
//! - 確かめは fail-closed: ループバックだけの公開・ホストからの到達・TLS 必須の接続（`sslmode=require`）の
//!   どれかが満たされなければ起動の失敗にする。平文の拒否は compose 側の構成（AUDP-9）に任せ、ここで変えない。
//! - SQL は固定文字列だけで値を連結しない（AUDB-43）。
//!
//! 範囲外: DB を使う試験を CI に載せること（TASK-6）。この構成要素の合格は実物の Hub の DB の合格に数えない（ACPT-7）。

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fandhe_auth_secret::SecretString;

use crate::component::{Component, ComponentKind};
use crate::harness::{HarnessError, HarnessErrorKind};

const PASSWORD_ENV: &str = "FANDHE_AUTH_DEV_PG_PASSWORD";
const PORT_ENV: &str = "FANDHE_AUTH_DEV_PG_PORT";
const ENDPOINT_HOST: &str = "127.0.0.1";

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// 試験ごとの DB の設定。既定は `dev/postgres/compose.yaml`・空きポート・60 秒の待ち。
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    compose_file: PathBuf,
    port: Option<u16>,
    wait_timeout: Duration,
}

impl DatabaseConfig {
    /// 既定の設定。待ち時間 60 秒は開発の設定で、要件の値ではない（人が決める）。
    pub fn new() -> Self {
        Self {
            compose_file: PathBuf::from(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../dev/postgres/compose.yaml"
            )),
            port: None,
            wait_timeout: Duration::from_secs(60),
        }
    }

    /// compose ファイルを差し替える。
    #[must_use]
    pub fn with_compose_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.compose_file = path.into();
        self
    }

    /// ループバックの公開ポートを固定する。既定は空きポート。
    #[must_use]
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    /// 起動の待ち時間を変える。
    #[must_use]
    pub fn with_wait_timeout(mut self, timeout: Duration) -> Self {
        self.wait_timeout = timeout;
        self
    }

    /// 設定された compose ファイル。
    pub fn compose_file(&self) -> &Path {
        &self.compose_file
    }

    /// 固定したポート（未指定なら `None`）。
    pub fn port(&self) -> Option<u16> {
        self.port
    }

    /// 起動の待ち時間。
    pub fn wait_timeout(&self) -> Duration {
        self.wait_timeout
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// 試験と Host へ渡す DB の接続情報。URL の形（userinfo 付き）の接続文字列は作らない。
/// `Debug` はパスワードを [`SecretString`] の固定文字列で出す（SECR-11）。
#[derive(Debug, Clone)]
pub struct DatabaseEndpoint {
    /// ホスト（ループバック固定）。
    pub host: String,
    /// ホスト側の公開ポート。
    pub port: u16,
    /// ロール名。
    pub user: String,
    /// DB 名。
    pub dbname: String,
    /// TLS の要求（`require`）。
    pub sslmode: String,
    /// パスワード。
    pub password: SecretString,
}

/// 起動後の接続情報を受け渡す共有のハンドル。起動前と停止後は `None`。
///
/// 使い方: 試験が [`Database::endpoint_handle`] を先に取り、Host へ構築時に渡す。起動順は Database が
/// 先なので Host の `start` の時点で値がある。
#[derive(Debug, Clone, Default)]
pub struct EndpointHandle(Arc<Mutex<Option<DatabaseEndpoint>>>);

impl EndpointHandle {
    /// 現在の接続情報。
    pub fn get(&self) -> Option<DatabaseEndpoint> {
        self.0.lock().ok().and_then(|g| g.clone())
    }

    fn set(&self, value: Option<DatabaseEndpoint>) {
        if let Ok(mut g) = self.0.lock() {
            *g = value;
        }
    }
}

/// compose の呼び出しを組み立てる。パスワードは環境変数にだけ載せ、argv に載せない（SECR-6）。
/// 試験（Docker なし）から argv を確かめるために公開している。
#[doc(hidden)]
pub fn compose_command(
    compose_file: &Path,
    project: &str,
    port: u16,
    password: &SecretString,
    args: &[&str],
) -> Command {
    let mut cmd = Command::new("docker");
    cmd.arg("compose")
        .arg("-f")
        .arg(compose_file)
        .arg("-p")
        .arg(project)
        .args(args)
        .env(PASSWORD_ENV, password.expose())
        .env(PORT_ENV, port.to_string())
        .stdin(Stdio::null());
    cmd
}

/// 子プロセスの出力は誤りにも記録にも載せない。成功だけを返す。
fn succeeds(mut cmd: Command) -> bool {
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

/// 標準出力を返す（標準エラーは捨てる）。
fn stdout_of(mut cmd: Command) -> Option<String> {
    cmd.stderr(Stdio::null());
    let out = cmd.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn project_name() -> String {
    let seq = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut h = RandomState::new().build_hasher();
    h.write_u64(seq);
    // 並列の試験・worktree と衝突させない名前。英小文字・数字・ハイフンだけ。
    format!(
        "fa-it-{}-{}-{:08x}",
        std::process::id(),
        seq,
        h.finish() as u32
    )
}

fn free_port() -> Option<u16> {
    let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).ok()?;
    Some(l.local_addr().ok()?.port())
}

fn generate_password() -> Option<SecretString> {
    let mut cmd = Command::new("openssl");
    cmd.args(["rand", "-hex", "16"]);
    let out = stdout_of(cmd)?;
    (out.len() == 32 && out.bytes().all(|b| b.is_ascii_hexdigit())).then(|| SecretString::new(out))
}

/// DB の構成要素（種類は `Database`）。
#[derive(Debug)]
pub struct Database {
    config: DatabaseConfig,
    project: String,
    handle: EndpointHandle,
    // 起動を試みてから down するまでの (port, password)。
    running: Option<(u16, SecretString)>,
}

impl Database {
    /// 構成要素を作る。起動はしない。project 名は構成要素ごとに別になる。
    pub fn new(config: DatabaseConfig) -> Self {
        Self {
            config,
            project: project_name(),
            handle: EndpointHandle::default(),
            running: None,
        }
    }

    /// 接続情報のハンドル（clone で共有できる）。
    pub fn endpoint_handle(&self) -> EndpointHandle {
        self.handle.clone()
    }

    /// compose の project 名（試験ごとに固有。取り残しの確認に使う）。
    pub fn project(&self) -> &str {
        &self.project
    }

    fn cmd(&self, port: u16, password: &SecretString, args: &[&str]) -> Command {
        compose_command(
            &self.config.compose_file,
            &self.project,
            port,
            password,
            args,
        )
    }

    fn down(&mut self) -> bool {
        let Some((port, password)) = self.running.take() else {
            return true;
        };
        self.handle.set(None);
        succeeds(self.cmd(port, &password, &["down", "--volumes", "--remove-orphans"]))
    }

    /// 起動して確かめる。失敗の詳細（出力）は返さない。
    fn bring_up(&mut self) -> Option<()> {
        let port = match self.config.port {
            Some(p) => p,
            None => free_port()?,
        };
        let password = generate_password()?;
        // 途中で失敗しても down できるよう、起動の前に控える。
        self.running = Some((port, password.clone()));
        let timeout = self.config.wait_timeout.as_secs().max(1).to_string();
        if !succeeds(self.cmd(
            port,
            &password,
            &["up", "-d", "--wait", "--wait-timeout", &timeout],
        )) {
            return None;
        }
        // 公開はループバックだけ。
        let mapped = stdout_of(self.cmd(port, &password, &["port", "postgres", "5432"]))?;
        if mapped != format!("{ENDPOINT_HOST}:{port}") {
            return None;
        }
        // ホストから到達できる。
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        TcpStream::connect_timeout(&addr, Duration::from_secs(10)).ok()?;
        // TLS 必須の接続で入れる（pg_stat_ssl が真）。argv にはキー名だけを載せ、値は環境変数で渡す。
        let mut psql = self.cmd(
            port,
            &password,
            &[
                "exec",
                "-T",
                "-e",
                "PGPASSWORD",
                "postgres",
                "psql",
                "host=127.0.0.1 port=5432 user=postgres dbname=postgres sslmode=require connect_timeout=10",
                "-v",
                "ON_ERROR_STOP=1",
                "-qAt",
                "-c",
                "select ssl from pg_stat_ssl where pid = pg_backend_pid()",
            ],
        );
        psql.env("PGPASSWORD", password.expose());
        if stdout_of(psql)? != "t" {
            return None;
        }
        self.handle.set(Some(DatabaseEndpoint {
            host: ENDPOINT_HOST.to_string(),
            port,
            user: "postgres".to_string(),
            dbname: "postgres".to_string(),
            sslmode: "require".to_string(),
            password,
        }));
        Some(())
    }
}

impl Component for Database {
    fn kind(&self) -> ComponentKind {
        ComponentKind::Database
    }

    fn start(&mut self) -> Result<(), HarnessError> {
        let fail = || HarnessError::new(HarnessErrorKind::StartFailed, ComponentKind::Database);
        if self.running.is_some() {
            return Err(fail());
        }
        if self.bring_up().is_none() {
            // 失敗した要素は起動済みに数えられない。後始末は自分で持つ。
            let _ = self.down();
            return Err(fail());
        }
        Ok(())
    }

    fn stop(&mut self) -> Result<(), HarnessError> {
        if self.down() {
            Ok(())
        } else {
            Err(HarnessError::new(
                HarnessErrorKind::StopFailed,
                ComponentKind::Database,
            ))
        }
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        let _ = self.down();
    }
}
