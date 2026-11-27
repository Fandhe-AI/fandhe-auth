//! TASK-12 9/10: 小さな試し実行。it-harness の上で Hub の stub に対して段階付きの負荷を最後まで流し、
//! 記録のファイルが出ること・どの値も要件の値として記録されないことを確かめる。
//!
//! Docker・ネットワークなし（主試験は `make test` で必ず流れる。本文を出す専用の試験だけ `#[ignore]` で、`make load-trial` が選ぶ）。値は試験の中だけの仮の値
//! （A-2026-10-28-1・A-2026-10-28-14）。時間の値は判定しない。stub の合格を実物の合格に数えない
//! （A-2026-10-28-9・ACPT-7）。出たファイルは追跡ツリーの外（`CARGO_TARGET_TMPDIR`）に置く。

use std::path::Path;
use std::time::Duration;

use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair};

use fandhe_auth_hub_stub::assertion::AssertionSigningKey;
use fandhe_auth_hub_stub::config::{HubStubConfig, StubValue};
use fandhe_auth_it_harness::{
    ComponentKind, Harness, HarnessConfig, HubStubComponent, VersionInfo,
};
use fandhe_auth_load_harness::{
    ApprovedValues, CallResult, ComparisonValue, ComponentConditions, ConditionComponent,
    IntervalName, LoadConfig, LoadRecord, LoadRecordError, LoopModel, MeasuredSource,
    MeasurementConditions, MetricKey, RefId, RunRecordFile, RunSession, StageConfig, StageError,
    StageLimits, StagePlan, TargetName, TargetSpec, ValueClass, ValueName, ValueUnit,
};
use fandhe_auth_secret::SecretString;

/// 照合済みの本文を挟む固定の行。`scripts/load-trial.sh` の同名の文字列と一致させる。
const PRINT_BEGIN: &str = "-----BEGIN LOAD RECORD BODY-----";
const PRINT_END: &str = "-----END LOAD RECORD BODY-----";
const T0: u64 = 1_000;
const HOST_MARK: &str = "load-canary-host.example.test";
const CLIENT_MARK: &str = "load-canary-client.test";

fn hub_config() -> HubStubConfig {
    HubStubConfig {
        client_id: StubValue::test(CLIENT_MARK.to_owned()),
        public_host: StubValue::test(HOST_MARK.to_owned()),
        jwks_uri: StubValue::test(format!("https://{HOST_MARK}/jwks")),
        logout_receiver_uri: StubValue::test(format!("https://{HOST_MARK}/logout")),
        jwks_max_age: StubValue::test(Duration::from_secs(1)),
        jwks_fetch_min_interval: StubValue::test(Duration::from_secs(2)),
        jwks_cache_max: StubValue::test(Duration::from_secs(3)),
        jwks_stale_max: StubValue::test(Duration::from_secs(6)),
        assertion_max_lifetime: StubValue::test(Duration::from_secs(300)),
        feed_fetch_min_interval: StubValue::test(Duration::from_secs(5)),
        heartbeat_loss_threshold: StubValue::test(Duration::from_secs(10)),
    }
}

fn to_condition(kind: ComponentKind) -> ConditionComponent {
    match kind {
        ComponentKind::Database => ConditionComponent::Database,
        ComponentKind::Host => ConditionComponent::Host,
        ComponentKind::HubStub => ConditionComponent::HubStub,
        ComponentKind::ConsoleStub => ConditionComponent::ConsoleStub,
    }
}

const META: [&str; 6] = [
    "format:",
    "run_record_file:",
    "run_started_at:",
    "recorded_at:",
    "note:",
    "judgment:",
];

/// 主試験の本体。記録を書いて検査し、停止と後片付けまで済ませてから、記録の本文と照合の登録簿を返す。
/// 主試験と、本文を出す専用の試験（`print_checked_load_record_body`）が同じ試行を使い回す。
fn run_small_trial() -> (String, Vec<SecretString>) {
    // 照合の登録簿は、stub へ実際に渡す設定そのもの（同じインスタンス）から、渡す前に作る。
    let cfg = hub_config();
    // 署名鍵は試験の側で作り、秘密値（PKCS#8 の全文と `d`）を登録簿に足す（#6050・SECR-23・AAUD-30）。
    let (key, key_secrets) = signing_key_with_secrets("kid-load-trial.test");
    let registry = trial_registry(&cfg, &key_secrets);
    let hub = HubStubComponent::new(cfg);
    let handle = hub.handle();
    let running = Harness::start(HarnessConfig::new().with(Box::new(hub))).unwrap();
    handle.with(|s| s.jwks.publish(&key)).unwrap();

    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("load-trial");
    let it_record = running.run_record(VersionInfo::detect().unwrap()).unwrap();
    let it_path = it_record.write_new(&dir).unwrap();
    // 書き出した it-run の記録も、消す前に読み戻して登録簿に掛ける（#6050・SECR-23）。
    assert_no_leak(
        "it-run record",
        &std::fs::read(&it_path).unwrap(),
        &registry,
    );
    let it_name = it_path.file_name().unwrap().to_str().unwrap().to_owned();
    let run_file = RunRecordFile::new(&it_name).unwrap();
    let started = it_record.started_at().unix_seconds();

    let started_kinds: Vec<(ConditionComponent, bool)> = running
        .composition_record()
        .entries
        .iter()
        .map(|e| (to_condition(e.kind), e.stub))
        .collect();
    let conditions = MeasurementConditions::detect(
        ComponentConditions::new(&started_kinds).unwrap(),
        LoopModel::Closed,
    )
    .unwrap();

    let spec = TargetSpec::new(
        TargetName::new("hub_stub_jwks").unwrap(),
        IntervalName::new("stub_jwks").unwrap(),
    );
    let plan = StagePlan::new(
        vec![
            StageConfig::new(LoadConfig::new(1, 10, 1).unwrap(), 1).unwrap(),
            StageConfig::new(LoadConfig::new(2, 20, 2).unwrap(), 2).unwrap(),
        ],
        MetricKey::P99Ns,
        StageLimits::new(8, 8, 1_000, 10_000).unwrap(),
    )
    .unwrap();
    let call = || match handle.with(|s| s.jwks.serve(T0)) {
        Some(r) if r.status == 200 => CallResult::Ok,
        Some(_) => CallResult::Rejected,
        None => CallResult::Unavailable,
    };
    // 段階は実行の文脈（実行の記録・測定の条件）の中で回し、呼ぶ構成要素は呼ぶ前に申告する。
    // 閉包が実際に Hub の stub を呼ぶことは、この試験が stub 側の呼び出しの件数で確かめる（下の assert）。
    let evidence = RunSession::new(run_file, started, conditions)
        .unwrap()
        .run(&[ConditionComponent::HubStub], &spec, &plan, call)
        .unwrap();
    // 経路を実際に呼んだ証拠: 暖機を含む全試行数（11 x 1 + 22 x 2）。
    assert_eq!(handle.with(|s| s.jwks.records().len()), Some(55));

    // 性能の目標は持ち越し（AUDP-29）。要件の値として記録せず、未決を明示する。
    let target = ComparisonValue::new(
        ValueName::new("p99_target").unwrap(),
        ValueClass::Undecided,
        None,
        ValueUnit::Ns,
        vec![RefId::new("AUDP-29").unwrap()],
        &ApprovedValues::default(),
    )
    .unwrap();
    // 記録を組み立てる時点の実測時刻。逆行していれば LoadRecord が ClockOrder で拒否する（補正しない）。
    let recorded_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let record = LoadRecord::new(&evidence, recorded_at, vec![target]).unwrap();
    let path = record.write_new(&dir).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    // 失敗診断に記録の本文を出さないため、本文を含む比較は固定の文言にする（SECR-23）。
    assert!(
        text == record.to_string(),
        "written record differs from Display"
    );

    let name = path.file_name().unwrap().to_str().unwrap();
    let pid = std::process::id();
    assert!(name.starts_with(&format!("load-run-{started}-{pid}-")) && name.ends_with(".txt"));
    assert!(it_path.parent() == path.parent() && it_path.exists());

    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines[0] == "format: fandhe-auth-load-record/1",
        "format line differs"
    );
    assert!(
        lines[1] == format!("run_record_file: {it_name}"),
        "run_record_file line differs"
    );
    assert!(
        lines[2] == format!("run_started_at: {}", it_record_started_line(&it_record)),
        "run_started_at line differs"
    );
    let count = |p: &str| lines.iter().filter(|l| l.starts_with(p)).count();
    assert_eq!(count("stage:"), 2);
    assert_eq!(count("run:"), 3);
    assert_eq!(count("metric:"), 3);
    assert_eq!(count("worst_run:"), 2);
    for l in lines.iter().filter(|l| l.starts_with("run:")) {
        assert!(l.contains("failure=0 timeout=0"));
    }
    assert!(
        lines
            .iter()
            .any(|l| l.contains("trials_planned=10 warmup=1"))
    );
    assert!(lines.iter().any(|l| l.contains("executed=20 success=20")));
    assert!(lines.iter().any(|l| l.contains("excluded=2")));

    // どの値も要件の値として扱わない。
    for c in ["class=requirement", "class=initial", "class=poc_parameter"] {
        assert!(!text.contains(c));
    }
    let values: Vec<&&str> = lines.iter().filter(|l| l.starts_with("value:")).collect();
    assert_eq!(values.len(), 1);
    assert!(values[0].contains("class=undecided value=none"));
    for l in &lines {
        if META.iter().any(|m| l.starts_with(m)) {
            continue;
        }
        assert!(
            l.contains(" class=") && l.contains(" refs="),
            "a record line lacks class= or refs="
        );
    }
    assert!(*lines.last().unwrap() == "judgment:", "last line differs");
    assert!(text.ends_with('\n'));

    // 設定の目印・出力先のパスが出ない。
    let dir_str = dir.to_str().unwrap();
    let dbg = format!("{record:?}");
    for out in [&text, &dbg] {
        assert!(!out.contains(HOST_MARK) && !out.contains(CLIENT_MARK));
        assert!(!out.contains(dir_str));
    }

    // 上書きしない: 同じ記録をもう一度書くと別名の新しいファイルになる。
    let path2 = record.write_new(&dir).unwrap();
    assert!(path != path2, "write_new reused the same file name");
    assert!(
        std::fs::read_to_string(&path).unwrap() == text,
        "re-written record differs"
    );
    assert_no_leak(
        "re-written load record",
        &std::fs::read(&path2).unwrap(),
        &registry,
    );

    running.stop().unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_file(&path2).unwrap();
    std::fs::remove_file(&it_path).unwrap();

    (text, registry)
}

#[test]
fn small_trial_run_writes_record_without_requirement_values() {
    let (text, registry) = run_small_trial();
    // 出す本文そのものを秘密値の各表現の照合（`printable_body`）に通せることを、本文を出さずに確かめる（SECR-23）。
    // 本文を出すかどうかは環境変数・設定に依らない。出す入口は下の `#[ignore]` の試験だけ。
    assert!(
        printable_body(&text, &registry).is_ok(),
        "the load record body failed the registered-value check"
    );
}

/// 照合を通した記録の本文を標準出力へ出す専用の試験。`#[ignore]` のため `make test` では走らず、
/// `make load-trial`（`scripts/load-trial.sh`）が `--ignored --exact` で明示的に選んだときだけ出す。
/// 環境変数では切り替えない（DPOL-32。親の環境から本文の出力が有効にならない）。
#[test]
#[ignore = "prints the checked record body; run via make load-trial"]
fn print_checked_load_record_body() {
    let (text, registry) = run_small_trial();
    // SECR-23・AAUD-30: 標準出力へ渡す本文そのものを、登録簿（設定の 4 欄に入れた値の全文と、署名鍵の秘密値 2 件〔PKCS#8 の全文・`d`〕）の
    // 生・base64・base64url・16 進の各形と JWT の形で走査する。一致したら本文を出さず失敗させる。
    let body = printable_body(&text, &registry).unwrap();
    // `scripts/load-trial.sh` が照合済みの本文を見分けるための固定の開始行・終了行（ちょうど 1 区画だけ出す）。
    println!("{PRINT_BEGIN}\n{body}{PRINT_END}");
}

/// 主試験の照合の登録簿: 設定の 4 欄と、署名鍵の秘密値（PKCS#8 の全文・`d`）。
/// 登録簿の要素は `SecretString`（16 進）のまま持ち、生のバイト列へ戻すのは照合の関数の中だけ（`match_counts`。呼び出しごとに作って捨てる）。
fn trial_registry(cfg: &HubStubConfig, key_secrets: &[SecretString]) -> Vec<SecretString> {
    let mut registry = stdout_registry(cfg);
    registry.extend(key_secrets.iter().cloned());
    registry
}

/// 秘密のバイト列は共有の秘密値の型 `SecretString`（16 進の文字列）で持ち、試験内に型を新設しない（SECR-15・SECR-31）。
/// 登録簿（設定の欄の値・乱数の値・署名鍵の秘密値）は全要素を `SecretString` で持ち、試験の間じゅう生の `Vec<u8>` で保持しない（SECR-10・SECR-29・SECR-30・SECR-33）。
/// 生のバイト列へ戻すのは `secret_raw` だけで、照合（`match_counts`）と、登録簿の中身を確かめる試験の中で、`expose` を明示的に呼んで一時的に作る（SECR-16）。
/// 戻した値は関数の中に留め、ログ・誤り・記録に渡さない。
fn secret_hex(bytes: &[u8]) -> SecretString {
    SecretString::new(hex(bytes, false))
}

/// `secret_hex` の逆。戻り値は呼び出した関数の中の一時の値として使い、保持せず、ログ・誤り・記録に渡さない。
fn secret_raw(s: &SecretString) -> Vec<u8> {
    let h = s.expose().as_bytes();
    assert!(h.len().is_multiple_of(2), "malformed secret encoding");
    h.chunks(2)
        .map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap())
        .collect()
}

/// DER の 1 要素（タグ・内容・残り）を読む。長さは 1 バイト形か `0x81`・`0x82` の形だけ（ring の出力で足りる）。
fn der_tlv(b: &[u8]) -> (u8, &[u8], &[u8]) {
    let tag = b[0];
    let (len, hdr) = match b[1] {
        n if n < 0x80 => (n as usize, 2),
        0x81 => (b[2] as usize, 3),
        0x82 => (((b[2] as usize) << 8) | b[3] as usize, 4),
        _ => panic!("unsupported DER length form"),
    };
    (tag, &b[hdr..hdr + len], &b[hdr + len..])
}

/// 試験の側で署名鍵を作り、その秘密値を照合用に返す（hub-stub の API は変えず、鍵を外へ出す口を足さない）。
/// 返す 2 件は、PKCS#8 の全文と、秘密のスカラー `d`（32 バイト）。`d` の 16 進は 64 バイトで記録の 1 語の上限に収まり、
/// 形の網では止まらないため、全文とは別に照合する。暗号の処理は `ring` に任せ、ここは DER の構造をたどって位置を取る（DPOL-47）。
/// PKCS#8 { INTEGER, SEQUENCE(alg), OCTET STRING { ECPrivateKey { INTEGER 1, OCTET STRING d, [1] { BIT STRING 公開鍵 } } } }。
/// 取り出した位置が正しいことは、公開鍵の部分と `public_key()` の一致で確かめる。失敗の文言は値を出さない（SECR-23）。
fn signing_key_with_secrets(kid: &str) -> (AssertionSigningKey, Vec<SecretString>) {
    let rng = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
    let pkcs8 = pkcs8.as_ref().to_vec();
    let key = AssertionSigningKey::from_pkcs8(kid, &pkcs8).unwrap();
    let (t, outer, rest) = der_tlv(&pkcs8);
    assert!(t == 0x30 && rest.is_empty(), "unexpected PKCS#8 outer form");
    let (t, _version, outer) = der_tlv(outer);
    assert_eq!(t, 0x02, "unexpected PKCS#8 version form");
    let (t, _alg, outer) = der_tlv(outer);
    assert_eq!(t, 0x30, "unexpected PKCS#8 algorithm form");
    let (t, inner, _) = der_tlv(outer);
    assert_eq!(t, 0x04, "unexpected PKCS#8 private key form");
    let (t, ec, _) = der_tlv(inner);
    assert_eq!(t, 0x30, "unexpected ECPrivateKey form");
    let (t, ver, ec) = der_tlv(ec);
    assert!(t == 0x02 && ver == [1], "unexpected ECPrivateKey version");
    let (t, d, ec) = der_tlv(ec);
    assert!(t == 0x04 && d.len() == 32, "unexpected scalar form");
    // 公開鍵は文脈タグ [1]（0xA1）の中の BIT STRING（先頭 1 バイトは未使用ビット数 0、続く 65 バイトが非圧縮の点）。
    let (t, ctx, _) = der_tlv(ec);
    assert_eq!(t, 0xA1, "unexpected public key context form");
    let (t, bits, _) = der_tlv(ctx);
    assert!(
        t == 0x03 && bits.len() == 66 && bits[0] == 0,
        "unexpected public key form"
    );
    assert!(
        bits[1..] == *key.public_key(),
        "PKCS#8 does not belong to the generated key"
    );
    (key, vec![secret_hex(&pkcs8), secret_hex(d)])
}

/// 書き出したファイルの全文を登録簿の各形と JWT の形で走査し、あれば失敗させる。文言は種類と件数だけ（値・パス・本文を出さない）。
fn assert_no_leak(what: &str, bytes: &[u8], registry: &[SecretString]) {
    let n = count_matches(registry, bytes);
    assert_eq!(n, 0, "registered values found in the {what}: {n} match(es)");
    let j = count_jwt_like(bytes);
    assert_eq!(j, 0, "JWT-like words found in the {what}: {j}");
}

/// 標準出力へ出す本文の走査用の登録簿: 主試験が stub の設定の 4 欄へ入れた値の全文。
/// 渡す設定の文字列の欄 4 つ（`client_id`・`public_host`・`jwks_uri`・`logout_receiver_uri`）をすべて入れる。
fn stdout_registry(cfg: &HubStubConfig) -> Vec<SecretString> {
    vec![
        secret_hex(cfg.client_id.get().as_bytes()),
        secret_hex(cfg.public_host.get().as_bytes()),
        secret_hex(cfg.jwks_uri.get().as_bytes()),
        secret_hex(cfg.logout_receiver_uri.get().as_bytes()),
    ]
}

/// 本文に登録簿の値（生・base64・base64url・16 進）か JWT の形が 1 件でもあれば `Err`（本文は返さず、件数だけ）。
fn printable_body<'a>(text: &'a str, registry: &[SecretString]) -> Result<&'a str, String> {
    let n = count_matches(registry, text.as_bytes());
    let j = count_jwt_like(text.as_bytes());
    if n == 0 && j == 0 {
        Ok(text)
    } else {
        Err(format!(
            "refusing to print the load record: {n} registered-value match(es), {j} JWT-like word(s)"
        ))
    }
}

/// it-harness の記録の `started_at` の RFC 3339 表記（`UtcTimestamp` の `Display`）。
fn it_record_started_line(r: &fandhe_auth_it_harness::RunRecord) -> String {
    r.started_at().to_string()
}

// ---- 照合の試験 ----
//
// AAUD-30・AAUD-31 の方法（登録簿の各値を生・base64・base64url・16 進の形で出力に掛け、一致の件数を数える）を
// ハーネスの出力に当てはめたもので、AAUD-30・AAUD-31 を満たしたものではない（実環境の走査は Hub〔`AOPS-73`〕）。
// 対象は「stub の設定の 4 欄から負荷の記録へ値が流れないこと」の回帰の検出と、it-run の記録（設定の入る口
// `with_hub_stub_record` を通して書いたもの）に登録簿の値が現れないことの確認まで。it-run の記録に入るのは
// 欄名（`&'static str`）と印（`ValueMark`）だけで、設定の値が届く経路は無い。そのため it-run の記録側の
// 陽性対照は置かない（設定から値が届く経路が無い）。照合そのものの検出力は負荷の記録側の陽性対照で見る。
// 署名鍵（`AssertionSigningKey`）の秘密値（PKCS#8 の全文と `d`）は、試験の側で鍵を作って登録簿に入れる
// （`signing_key_with_secrets`。#6050）。書き出した 3 種のファイル（it-run の記録・負荷の記録・再書き出し）はすべて
// 読み戻して照合する。残りは、実環境の走査（Hub `AOPS-73`）と、形の一覧から独立した固定ベクトル（#6039）。
// TODO(TASK-500): 実環境の走査と、形の一覧から独立した固定ベクトルでの照合（AAUD-30・AAUD-31。#1391・#6039）。
// 出力の面の秘密値の扱いは TASK-518（SECR-23。#1407）。

/// 実行時に乱数から作る 16 バイトの値（固定の値をソースに置かない）。`RandomState` は実行ごとに種が変わる。
fn random_value() -> Vec<u8> {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher as _, Hasher as _};
    let state = RandomState::new();
    let mut out = Vec::with_capacity(16);
    for i in 0..2u64 {
        let mut h = state.build_hasher();
        h.write_u64(i);
        out.extend_from_slice(&h.finish().to_be_bytes());
    }
    out
}

fn hex(v: &[u8], upper: bool) -> String {
    v.iter()
        .map(|b| {
            if upper {
                format!("{b:02X}")
            } else {
                format!("{b:02x}")
            }
        })
        .collect()
}

fn base64_with(v: &[u8], alphabet: &[u8; 64], pad: bool) -> String {
    let mut out = String::new();
    for c in v.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        let take = c.len() + 1;
        for i in 0..4 {
            if i < take {
                out.push(char::from(alphabet[((n >> (18 - 6 * i)) & 63) as usize]));
            } else if pad {
                out.push('=');
            }
        }
    }
    out
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const B64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// 値ごとの照合の形: 生・base64（パディングあり・なし）・base64url（なし）・16 進（小文字・大文字）。
/// 生のバイト列は UTF-8 とは限らないためバイト列で探す。
fn forms(v: &[u8]) -> Vec<Vec<u8>> {
    vec![
        v.to_vec(),
        base64_with(v, B64, true).into_bytes(),
        base64_with(v, B64, false).into_bytes(),
        base64_with(v, B64URL, false).into_bytes(),
        hex(v, false).into_bytes(),
        hex(v, true).into_bytes(),
    ]
}

/// 符号化の既知の入力と出力の対（RFC 4648 の 10 節の例・16 進・`+`/`/` を含む入力）。
/// 符号化が誤っていると、照合が何も見つけないまま「0 件」になるため、対で確かめる。
#[test]
fn encoders_match_rfc4648_vectors() {
    let b64 = [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ];
    for (i, o) in b64 {
        assert_eq!(base64_with(i.as_bytes(), B64, true), o);
        assert_eq!(
            base64_with(i.as_bytes(), B64, false),
            o.trim_end_matches('=')
        );
        assert_eq!(
            base64_with(i.as_bytes(), B64URL, false),
            o.trim_end_matches('=')
        );
    }
    // 62・63 番目の文字: 標準は `+` `/`、URL 安全は `-` `_`。
    assert_eq!(base64_with(&[0xfb, 0xff], B64, true), "+/8=");
    assert_eq!(base64_with(&[0xfb, 0xff], B64URL, false), "-_8");
    assert_eq!(hex(b"foobar", false), "666f6f626172");
    assert_eq!(hex(&[0xab, 0xcd], true), "ABCD");
    assert_eq!(hex(&[0xab, 0xcd], false), "abcd");
}

/// 登録簿の全値・全形が `haystack` に現れる件数だけを返す（値は返さない）。
fn count_matches(registry: &[SecretString], haystack: &[u8]) -> usize {
    match_counts(registry, haystack).iter().flatten().sum()
}

/// 登録簿の値ごと・形ごとの一致の件数（`[値の番号][形の番号]`。形の順は `forms`）。`count_matches` はこの合計。
/// 陽性対照は、混ぜた形そのものの件数を見る（合計だけだと、ある形が別の形の部分列として見つかり、
/// その形の照合が外れても通る。例: パディングありの base64 はパディングなしの形を含む）。
fn match_counts(registry: &[SecretString], haystack: &[u8]) -> Vec<Vec<usize>> {
    registry
        .iter()
        .map(|s| {
            // 一時の生のバイト列。この呼び出しの中だけで持つ。
            let v = secret_raw(s);
            forms(&v)
                .iter()
                .map(|f| {
                    haystack
                        .windows(f.len())
                        .filter(|w| *w == f.as_slice())
                        .count()
                })
                .collect()
        })
        .collect()
}

/// JWT の形（`eyJ<英数字・-・_>.eyJ<…>.` の部分列）の件数。語の頭に限らず、本文のどこにあっても数える
/// （`x.`・`name_` などが前に付いた形も数える）。
fn count_jwt_like(haystack: &[u8]) -> usize {
    let seg = |from: usize| {
        let mut i = from;
        while i < haystack.len()
            && (haystack[i].is_ascii_alphanumeric() || matches!(haystack[i], b'-' | b'_'))
        {
            i += 1;
        }
        i
    };
    (0..haystack.len())
        .filter(|&p| haystack[p..].starts_with(b"eyJ"))
        .filter(|&p| {
            let e1 = seg(p + 3);
            haystack[e1..].starts_with(b".eyJ") && haystack.get(seg(e1 + 4)) == Some(&b'.')
        })
        .count()
}

/// 書き出した 2 つのファイルの全文。
struct Written {
    load: Vec<u8>,
    it_run: Vec<u8>,
}

/// 負荷の記録を作って書く（呼び出し元の入力は `cfg`。`hub_config` と同じ組み立て）。測る区間の名前は固定語彙の `stub_jwks`。負荷の記録と it-run の記録の全文を読み、消してから返す。
fn write_record_for(cfg: HubStubConfig, key: &AssertionSigningKey) -> Written {
    let hub = HubStubComponent::new(cfg);
    let handle = hub.handle();
    let running = Harness::start(HarnessConfig::new().with(Box::new(hub))).unwrap();
    handle.with(|s| s.jwks.publish(key)).unwrap();
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("load-trial-canary");
    // 設定が it-run の記録に入る口は `with_hub_stub_record` だけ（欄名と印のみを取り込む）。その口を通して書く。
    let it_record = running
        .run_record(VersionInfo::detect().unwrap())
        .unwrap()
        .with_hub_stub_record(&handle.provisional_record().unwrap())
        .unwrap();
    let it_path = it_record.write_new(&dir).unwrap();
    let it_name = it_path.file_name().unwrap().to_str().unwrap().to_owned();
    let started_kinds: Vec<(ConditionComponent, bool)> = running
        .composition_record()
        .entries
        .iter()
        .map(|e| (to_condition(e.kind), e.stub))
        .collect();
    let conditions = MeasurementConditions::detect(
        ComponentConditions::new(&started_kinds).unwrap(),
        LoopModel::Closed,
    )
    .unwrap();
    let spec = TargetSpec::new(
        TargetName::new("hub_stub_jwks").unwrap(),
        IntervalName::new("stub_jwks").unwrap(),
    );
    let plan = StagePlan::new(
        vec![StageConfig::new(LoadConfig::new(1, 5, 1).unwrap(), 1).unwrap()],
        MetricKey::P99Ns,
        StageLimits::new(8, 8, 1_000, 10_000).unwrap(),
    )
    .unwrap();
    let call = || match handle.with(|s| s.jwks.serve(T0)) {
        Some(r) if r.status == 200 => CallResult::Ok,
        _ => CallResult::Unavailable,
    };
    let evidence = RunSession::new(
        RunRecordFile::new(&it_name).unwrap(),
        it_record.started_at().unix_seconds(),
        conditions,
    )
    .unwrap()
    .run(&[ConditionComponent::HubStub], &spec, &plan, call)
    .unwrap();
    let recorded_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let record = LoadRecord::new(&evidence, recorded_at, Vec::new()).unwrap();
    let path = record.write_new(&dir).unwrap();
    running.stop().unwrap();
    let out = Written {
        load: std::fs::read(&path).unwrap(),
        it_run: std::fs::read(&it_path).unwrap(),
    };
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_file(&it_path).unwrap();
    out
}

/// 登録簿と、その値を呼び出し元の入力（接続の設定の 4 欄）へ入れた構成。
/// 登録簿は、乱数の 4 値（設定には 16 進で入る）と、4 欄に入れた文字列そのものの全文 4 件、署名鍵の秘密値 2 件の計 10 件。
/// 署名鍵は呼び出し元が `write_record_for` へ渡す（登録簿の位置 8・9 が PKCS#8 の全文と `d`）。
fn canary_registry_and_config() -> (Vec<SecretString>, HubStubConfig, AssertionSigningKey) {
    let (key, key_secrets) = signing_key_with_secrets("kid-load-trial.test");
    let raw: Vec<Vec<u8>> = (0..4).map(|_| random_value()).collect();
    let client_id = hex(&raw[0], false);
    let public_host = format!("{}.example.test", hex(&raw[1], false));
    let jwks_uri = format!("https://{}/jwks", hex(&raw[2], false));
    let logout_receiver_uri = format!("https://{}/logout", hex(&raw[3], false));
    let mut registry: Vec<SecretString> = raw.iter().map(|v| secret_hex(v)).collect();
    registry.extend([
        secret_hex(client_id.as_bytes()),
        secret_hex(public_host.as_bytes()),
        secret_hex(jwks_uri.as_bytes()),
        secret_hex(logout_receiver_uri.as_bytes()),
    ]);
    registry.extend(key_secrets.iter().cloned());
    assert_eq!(registry.len(), 10);
    let mut cfg = hub_config();
    cfg.client_id = StubValue::test(client_id);
    cfg.public_host = StubValue::test(public_host);
    cfg.jwks_uri = StubValue::test(jwks_uri);
    cfg.logout_receiver_uri = StubValue::test(logout_receiver_uri);
    (registry, cfg, key)
}

/// AAUD-30・SECR-23（方法を当てはめたもの）: stub の設定の 4 欄に入れた登録簿の値（乱数の値と欄の全文）が、
/// 負荷の記録と、設定の口（`with_hub_stub_record`）を通した it-run の記録の全文に、生・base64・base64url・16 進のどの形でも現れない（一致の件数が 0）。
/// JWT の形の語も 0 件。
#[test]
fn written_records_have_no_registered_secret_in_any_form() {
    let (registry, cfg, key) = canary_registry_and_config();
    let w = write_record_for(cfg, &key);
    assert!(!w.load.is_empty() && !w.it_run.is_empty());
    // 設定の口（`with_hub_stub_record`）を通った行が it-run の記録に実際に書き出されている（欄名と印だけ）。
    let it_text = String::from_utf8(w.it_run.clone()).unwrap();
    for field in [
        "client_id",
        "public_host",
        "jwks_uri",
        "logout_receiver_uri",
    ] {
        assert!(
            it_text
                .lines()
                .any(|l| l.starts_with(&format!("hub_stub_field: {field} "))),
            "hub_stub_field line for {field} missing from the it-run record"
        );
    }
    for (what, bytes) in [("load record", &w.load), ("it-run record", &w.it_run)] {
        assert_no_leak(what, bytes, &registry);
    }
}

/// 陽性対照 1（照合の検出力）: 登録簿の全値・全形（生・`+` を含む base64 も含む）を出力に混ぜると、
/// 照合は形ごとに検出する（混ぜた値の、混ぜた形そのものの件数が 1 以上。合計の件数では見ない）。JWT の形の語も数える。生のバイトと `+` を含む base64 は形の網の外にあり、
/// `write_new` を通らない。ここは照合そのものの検出力を見る（`write_new` に届く前に、登録簿の 16 進の文字列が語彙外の名前・出典・区間名として拒否されることを次の試験で見る）。
#[test]
fn matcher_detects_every_form_mixed_into_output() {
    let (registry, cfg, key) = canary_registry_and_config();
    let w = write_record_for(cfg, &key);
    assert_eq!(count_matches(&registry, &w.load), 0);
    // 署名鍵の 2 件（PKCS#8 の全文・`d`）も登録簿に入っていて、下の全値・全形の検出に回る。
    assert_eq!(registry.len(), 10);
    assert_eq!(secret_raw(&registry[8]).len(), 138);
    assert_eq!(secret_raw(&registry[9]).len(), 32);
    for (vi, s) in registry.iter().enumerate() {
        let v = secret_raw(s);
        for (fi, f) in forms(&v).iter().enumerate() {
            let mut mixed = w.load.clone();
            mixed.extend_from_slice(b"note: ");
            mixed.extend_from_slice(f);
            mixed.push(b'\n');
            let n = match_counts(&registry, &mixed)[vi][fi];
            assert!(n >= 1, "form {fi} of value {vi} was not detected");
        }
    }
    let mut jwt = w.load.clone();
    jwt.extend_from_slice(
        format!(
            "note: eyJ{}.eyJ{}.{}\n",
            "a".repeat(20),
            "b".repeat(20),
            "c".repeat(20)
        )
        .as_bytes(),
    );
    assert_eq!(count_jwt_like(&jwt), 1);
    // 前に文字が付いた形（`x.`・`name_`・語の途中）も数える。
    let body = format!(
        "eyJ{}.eyJ{}.{}",
        "a".repeat(20),
        "b".repeat(20),
        "c".repeat(20)
    );
    for prefix in ["x.", "name_", "Zm9v", "k="] {
        let mut m = w.load.clone();
        m.extend_from_slice(format!("note: {prefix}{body}\n").as_bytes());
        assert_eq!(count_jwt_like(&m), 1, "prefix {prefix}");
    }
    assert_eq!(count_jwt_like(b"eyJabc.def.ghi"), 0);
}

/// 登録簿から取った 16 進の文字列（固定語彙の外。文字種の規則には合う）は、値の名前・出典・区間名として記録に出せない
/// （SECR-23・#6038。語彙は crate の固定の定数で、呼び出し元は足せない）。設定由来の値を一律に拒否するわけではなく、
/// 固定語彙に一致する文字列（`p99_target` 等）は由来を問わず通る。この試験は、語彙外の 16 進の文字列を名前・出典に渡すと拒否され、
/// 区間名にした対象は 1 回も呼ばれない（実行の前の拒否と呼び出し 0 件。記録のファイルの未作成までは確かめない）。
/// 以前はここで「語彙にも入れた 16 進の値が書き出される」ことを陽性対照にしていたが、その経路を塞いだため
/// 拒否の試験に置き換えた。照合の検出力は陽性対照 1（`matcher_detects_every_form_mixed_into_output`）で見る。
#[test]
fn registry_hex_strings_outside_vocabulary_are_rejected_as_names() {
    let (registry, _cfg, _key) = canary_registry_and_config();
    let candidates: Vec<String> = registry
        .iter()
        .take(3)
        .map(|s| s.expose().to_owned())
        .collect();
    for c in &candidates {
        assert_eq!(ValueName::new(c), Err(LoadRecordError::InvalidName));
        assert_eq!(RefId::new(c), Err(LoadRecordError::InvalidRef));
    }
    // 区間名に使うと、実行の前に拒否される（対象を呼ばない）。
    let conditions = MeasurementConditions::detect(
        ComponentConditions::new(&[(ConditionComponent::HubStub, true)]).unwrap(),
        LoopModel::Closed,
    )
    .unwrap();
    let calls = std::sync::atomic::AtomicU64::new(0);
    for c in &candidates {
        let spec = TargetSpec::new(
            TargetName::new("hub_stub_jwks").unwrap(),
            IntervalName::new(c).unwrap(),
        );
        let plan = StagePlan::new(
            vec![StageConfig::new(LoadConfig::new(1, 5, 1).unwrap(), 1).unwrap()],
            MetricKey::P99Ns,
            StageLimits::new(8, 8, 1_000, 10_000).unwrap(),
        )
        .unwrap();
        let session = RunSession::new(
            RunRecordFile::new("it-run-0-1-1.txt").unwrap(),
            0,
            conditions.clone(),
        )
        .unwrap();
        let r = session.run(&[ConditionComponent::HubStub], &spec, &plan, || {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            CallResult::Ok
        });
        assert_eq!(
            r.err(),
            Some(LoadRecordError::Stage(StageError::InvalidPlan))
        );
    }
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

/// 署名鍵の秘密値が、主試験と canary の両方の登録簿に入っている（外すと落ちる。#6050・SECR-23）。
#[test]
fn registries_include_signing_key_secret() {
    let (key, secrets) = signing_key_with_secrets("kid-load-trial.test");
    assert_eq!(secrets.len(), 2);
    let (pkcs8, d) = (secret_raw(&secrets[0]), secret_raw(&secrets[1]));
    assert!(pkcs8.len() > d.len() && d.len() == 32);
    let trial = trial_registry(&hub_config(), &secrets);
    assert_eq!(trial.len(), 6);
    let tail: Vec<Vec<u8>> = trial[4..].iter().map(secret_raw).collect();
    assert_eq!(tail, vec![pkcs8.clone(), d.clone()]);
    let (canary, _cfg, _key) = canary_registry_and_config();
    assert!(
        canary.len() == 10
            && secret_raw(&canary[8]).len() == 138
            && secret_raw(&canary[9]).len() == 32
    );
    // 登録簿の `d` は鍵そのものを示す: 公開鍵と対の PKCS#8 の中にだけ現れる。
    assert!(pkcs8.windows(32).any(|w| w == d.as_slice()));
    assert_eq!(key.public_key().len(), 65);
}

/// 標準出力用の登録簿はログアウトの受信 URI を含み、その符号化した形（base64・base64url・16 進）を含む本文を拒否する。
/// 登録簿から外れていると、平文の目印（`HOST_MARK`）の無い符号化の表現が照合から漏れる（SECR-23）。
#[test]
fn stdout_registry_covers_logout_receiver_uri_in_encoded_forms() {
    let cfg = hub_config();
    let registry = stdout_registry(&cfg);
    assert_eq!(registry.len(), 4);
    let uri = cfg.logout_receiver_uri.get().clone().into_bytes();
    assert!(registry.iter().any(|s| secret_raw(s) == uri));
    for f in forms(&uri) {
        let text = format!("note: {}\n", String::from_utf8(f).unwrap());
        assert!(printable_body(&text, &registry).is_err());
        assert!(printable_body(&text, &[]).is_ok());
    }
}

/// 出力の走査（`printable_body`）は、登録簿の値の各形・JWT の形が混ざった本文を出さず、通常の本文は通す。
#[test]
fn printable_body_refuses_registered_forms_and_jwt() {
    let value = random_value();
    let registry = vec![secret_hex(&value)];
    assert!(printable_body("format: x\njudgment:\n", &registry).is_ok());
    // 生のバイト列は UTF-8 とは限らないため、文字列にできる形（先頭以外）で確かめる。
    for f in forms(&value).into_iter().skip(1) {
        let text = format!("note: {}\n", String::from_utf8(f).unwrap());
        assert!(printable_body(&text, &registry).is_err());
    }
    let jwt = format!(
        "note: eyJ{}.eyJ{}.{}\n",
        "a".repeat(20),
        "b".repeat(20),
        "c".repeat(20)
    );
    let err = printable_body(&jwt, &registry).unwrap_err();
    assert!(!err.contains("eyJ"));
}

/// #6049: 公開の口から測定値の行を任意の整数で作れない。導く口も名前ごとの区分の規則（AUDP-29）に従う。
#[test]
fn measured_row_cannot_be_made_from_arbitrary_values() {
    let not_allowed = LoadRecordError::InvalidValue(
        fandhe_auth_load_harness::InvalidValueReason::ClassNotAllowed,
    );
    let name = || ValueName::new("p99_target").unwrap();
    assert_eq!(
        ComparisonValue::new(
            name(),
            ValueClass::Measured,
            Some(1),
            ValueUnit::Ns,
            vec![],
            &ApprovedValues::default(),
        )
        .err(),
        Some(not_allowed)
    );
    let conditions = MeasurementConditions::detect(
        ComponentConditions::new(&[(ConditionComponent::HubStub, true)]).unwrap(),
        LoopModel::Closed,
    )
    .unwrap();
    let spec = TargetSpec::new(
        TargetName::new("hub_stub_jwks").unwrap(),
        IntervalName::new("client_response").unwrap(),
    );
    let plan = StagePlan::new(
        vec![StageConfig::new(LoadConfig::new(1, 5, 1).unwrap(), 1).unwrap()],
        MetricKey::P99Ns,
        StageLimits::new(8, 8, 1_000, 10_000).unwrap(),
    )
    .unwrap();
    let evidence = RunSession::new(
        RunRecordFile::new("it-run-0-1-1.txt").unwrap(),
        0,
        conditions,
    )
    .unwrap()
    .run(&[ConditionComponent::HubStub], &spec, &plan, || {
        CallResult::Ok
    })
    .unwrap();
    assert_eq!(
        ComparisonValue::measured(
            name(),
            &evidence,
            MeasuredSource::new(1, 1, MetricKey::P99Ns)
        )
        .err(),
        Some(not_allowed)
    );
}
