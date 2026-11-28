//! DPOL-14 / TASK-667 (4/4)・#320: RS256 の PKCS#8 署名鍵を `jsonwebtoken` に渡すとき、
//! 実装が `from_rsa_pem` の形を使い、`from_rsa_der` を使わないことの走査の試験。
//!
//! 3/4（`dpol14_rsa_pkcs8.rs`）は「DER では失敗し PEM なら成功する」ことを示す。ここはその帰結を
//! リポジトリ側で守る。2026-11 時点で本番メンバーに RS256 の鍵の読み込みは無い（実装は `TASK-347`
//! の RS256 の署名部品〔#439〕と読む。書き手の読みで、人が確定する）。このため
//! 「本番が PEM の形を使う」ことの陽性の確認はできず、0 件は未検証である（ACPT-7）。
//! 次の 2 つだけを置く。
//!
//! - ガード: `crates/` に DER の形が入ったら止める（陽性対照・陰性対照・fail-closed つき）。
//! - 引き継ぎの印: 本番に `from_rsa_pem` が現れたら、人の確認を促して失敗する観測。
//!   期待を緩めて通さない（AUDB-6）。観測を更新するのは人の確認の後だけ。
//!
//! 〔仮想〕の確認で、spec の「確定」ではない。文字列の走査で、人のレビュー（TASK-83）の代わりにならない。
//! 走査の限界:
//! - マクロが生成する識別子は見えない。
//! - 行頭が `//` の行だけを飛ばす。`/* */` の中の行頭が `//` でない行は読むため、誤検出の側に倒す。
//!   一方、行頭が `//` の行は `/* */` の中にあっても飛ばすので、ブロックコメント内のその形の実コードは見落とす。
//! - `.rs` 以外のファイル（文書・テンプレート・生成物等）は見ない。
//! - 引き継ぎの印は `from_rsa_pem` という名前だけを見る。`jsonwebtoken` を経ない RS256 の鍵の
//!   読み込み（`aws-lc-rs` の直接の呼び出しなど）は拾わない。
//! - 除くディレクトリは `.git` と `crates/<x>/target` の直下だけ（ビルド成果物）。
//!
//! `DecodingKey::from_rsa_der` も拾う（fail-closed。公開鍵の用途で要る場面が出たら人が見直す）。
//! この試験は `tests/` にあり、lib の「ファイルを直接呼ばない」定めの外にある。
//! 出力は相対パス・行番号・件数だけ（SECR-23）。

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// 本番メンバー。`scripts/check-crypto-primitives.sh` の `PROD_DIRS` と同じ分類に保つ
/// （`dpol14_classification_matches_scripts` が照合する）。
const PROD_DIRS: [&str; 6] = [
    "core",
    "secret",
    "db-tests",
    "client",
    "plugin-oidc",
    "backend",
];
/// 試験専用メンバー。同上（`TEST_ONLY_DIRS`）。
const TEST_ONLY_DIRS: [&str; 6] = [
    "hub-stub",
    "console-stub",
    "it-harness",
    "e2e-harness",
    "load-harness",
    "trial-deps",
];
/// 走査から除く 2 ファイル（完全一致の repo ルートからの相対パス）。
/// 陽性対照の 3/4 の試験と、この試験自身（DER の形の名前を文字列で持つため）。
const EXCLUDED: [&str; 2] = [
    "crates/trial-deps/tests/dpol14_rsa_pkcs8.rs",
    "crates/trial-deps/tests/dpol14_pem_form.rs",
];
const DER_IDENT: &str = "from_rsa_der";
const PEM_IDENT: &str = "from_rsa_pem";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root must exist")
        .to_path_buf()
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// 識別子の境界で一致した行の 1 始まりの行番号を返す。行頭が `//` の行は飛ばす。
/// 行末のコメントは剥がさない（文字列中の `//` が本物の呼び出しを隠さないように誤検出の側へ倒す）。
fn find_ident(src: &str, ident: &str) -> Vec<usize> {
    let mut hits = Vec::new();
    for (i, line) in src.lines().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        let mut from = 0;
        while let Some(pos) = line[from..].find(ident) {
            let start = from + pos;
            let end = start + ident.len();
            let before_ok = line[..start]
                .chars()
                .next_back()
                .is_none_or(|c| !is_ident_char(c));
            let after_ok = line[end..].chars().next().is_none_or(|c| !is_ident_char(c));
            if before_ok && after_ok {
                hits.push(i + 1);
                break;
            }
            from = end;
        }
    }
    hits
}

/// 走査から除くディレクトリか（repo ルートからの相対パス）。
/// `.git` と、crate 直下のビルド成果物 `crates/<x>/target` だけを除く。
/// `src/target/` のような同名のモジュールは除かない。
fn skip_dir(rel_path: &str) -> bool {
    let parts: Vec<&str> = rel_path.split('/').collect();
    parts.last() == Some(&".git")
        || (parts.len() == 3 && parts[0] == "crates" && parts[2] == "target")
}

fn collect_rs(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).expect("directory must be readable");
    for e in entries {
        let p = e.expect("entry must be readable").path();
        if p.is_dir() {
            if skip_dir(&rel(root, &p)) {
                continue;
            }
            collect_rs(root, &p, out);
        } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .expect("path must be under root")
        .to_string_lossy()
        .replace('\\', "/")
}

/// `crates/` の `.rs` を（相対パス, 内容）で決定的な順に返す。
fn all_sources() -> Vec<(String, String)> {
    let root = repo_root();
    let mut files = Vec::new();
    collect_rs(&root, &root.join("crates"), &mut files);
    let mut v: Vec<(String, String)> = files
        .iter()
        .map(|p| {
            let s = fs::read_to_string(p).expect("source must be readable utf-8");
            (rel(&root, p), s)
        })
        .collect();
    v.sort();
    v
}

/// 除外の対照。ビルド成果物だけを除き、`src/target/` のようなモジュールは走査に残す。
#[test]
fn dpol14_skip_dir_only_excludes_crate_level_target() {
    assert!(skip_dir("crates/core/target"));
    assert!(skip_dir("crates/core/.git"));
    assert!(!skip_dir("crates/core/src/target"));
    assert!(!skip_dir("crates/core/tests/target"));
    assert!(!skip_dir("crates/core/target/debug/x"));
    assert!(!skip_dir("crates/target"));
    assert!(!skip_dir("crates/core/src"));
}

/// 走査の対照（作業ツリー）。`crates/<x>/src/target/` の `.rs` は拾い、`crates/<x>/target/` は拾わない。
/// 試験用の作業領域（`CARGO_TARGET_TMPDIR`）に鍵を含まない空のファイルだけを作る。
#[test]
fn dpol14_walk_keeps_nested_target_named_directories() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("dpol14_walk");
    let _ = fs::remove_dir_all(&root);
    let kept = root.join("crates/x/src/target");
    let skipped = root.join("crates/x/target/debug");
    fs::create_dir_all(&kept).expect("create kept dir");
    fs::create_dir_all(&skipped).expect("create skipped dir");
    fs::write(kept.join("a.rs"), "").expect("write a.rs");
    fs::write(skipped.join("b.rs"), "").expect("write b.rs");
    let mut files = Vec::new();
    collect_rs(&root, &root.join("crates"), &mut files);
    let rels: Vec<String> = files.iter().map(|p| rel(&root, p)).collect();
    assert_eq!(rels, vec!["crates/x/src/target/a.rs".to_string()]);
    let _ = fs::remove_dir_all(&root);
}

/// 陽性対照（実ファイル）。3/4 の試験は DER の形を使うので、走査器が当てられることを示す。
#[test]
fn dpol14_scanner_detects_der_form_in_control_file() {
    let src = fs::read_to_string(repo_root().join(EXCLUDED[0])).expect("control must be readable");
    let hits = find_ident(&src, DER_IDENT);
    assert!(!hits.is_empty(), "scanner must detect the control file");
    let lines: Vec<&str> = src.lines().collect();
    assert!(
        hits.iter()
            .any(|&n| lines[n - 1].contains("EncodingKey::from_rsa_der")),
        "a hit must be the EncodingKey call"
    );
}

/// 陽性対照（合成）。別名・改行・DecodingKey・行末コメントつきの形を拾う。
#[test]
fn dpol14_scanner_detects_synthetic_variants() {
    let cases = [
        "use jsonwebtoken::EncodingKey as K;\nlet k = K::from_rsa_der(&d);\n",
        "let k = EncodingKey::\n    from_rsa_der(&d);\n",
        "let k = DecodingKey::from_rsa_der(&d);\n",
        "let k = EncodingKey::from_rsa_der(&d); // note\n",
    ];
    for c in cases {
        assert!(
            !find_ident(c, DER_IDENT).is_empty(),
            "variant must be detected"
        );
    }
}

/// 陰性対照。PEM の形・コメント・長い識別子は当てない。
#[test]
fn dpol14_scanner_ignores_pem_form_comments_and_longer_identifiers() {
    let cases = [
        "let k = EncodingKey::from_rsa_pem(&p);\n",
        "// from_rsa_der\n",
        "/// from_rsa_der\n",
        "//! from_rsa_der\n",
        "fn my_from_rsa_der_helper() {}\n",
        "fn from_rsa_der_x() {}\n",
    ];
    for c in cases {
        assert!(find_ident(c, DER_IDENT).is_empty(), "must not be detected");
    }
}

/// ガード。`crates/` のどこにも DER の形が無い（DPOL-14）。fail-closed:
/// 本番メンバーの存在・走査数・除外の実在を確かめ、空振りで通さない。
#[test]
fn dpol14_no_der_form_in_crates() {
    let root = repo_root();
    for ex in EXCLUDED {
        assert!(root.join(ex).is_file(), "excluded file must exist: {ex}");
    }
    let sources = all_sources();
    assert!(!sources.is_empty(), "no sources scanned");
    for d in PROD_DIRS {
        let prefix = format!("crates/{d}/");
        assert!(
            sources.iter().any(|(p, _)| p.starts_with(&prefix)),
            "production member must have sources: {d}"
        );
    }
    let mut violations = Vec::new();
    for (path, src) in &sources {
        if EXCLUDED.contains(&path.as_str()) {
            continue;
        }
        for n in find_ident(src, DER_IDENT) {
            violations.push(format!("{path}:{n}"));
        }
    }
    assert!(
        violations.is_empty(),
        "DPOL-14: use the PEM form (from_rsa_pem) for PKCS#8 keys; found DER form at: {}",
        violations.join(", ")
    );
}

/// 分類が scripts と一致し、`crates/` の直下が全て分類済みであること。
#[test]
fn dpol14_classification_matches_scripts() {
    let root = repo_root();
    let sh = fs::read_to_string(root.join("scripts/check-crypto-primitives.sh"))
        .expect("script must be readable");
    let parse = |name: &str| -> BTreeSet<String> {
        let prefix = format!("{name}=(");
        let line = sh
            .lines()
            .find(|l| l.starts_with(&prefix))
            .unwrap_or_else(|| panic!("{name} line not found"));
        line[prefix.len()..]
            .trim_end_matches(')')
            .split_whitespace()
            .map(String::from)
            .collect()
    };
    let prod: BTreeSet<String> = PROD_DIRS.iter().map(|s| s.to_string()).collect();
    let test_only: BTreeSet<String> = TEST_ONLY_DIRS.iter().map(|s| s.to_string()).collect();
    assert_eq!(prod, parse("PROD_DIRS"), "PROD_DIRS differs from script");
    assert_eq!(
        test_only,
        parse("TEST_ONLY_DIRS"),
        "TEST_ONLY_DIRS differs from script"
    );
    for e in fs::read_dir(root.join("crates")).expect("crates must be readable") {
        let p = e.expect("entry must be readable").path();
        if p.is_dir() {
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            assert!(
                prod.contains(&name) || test_only.contains(&name),
                "crate directory not classified: {name}"
            );
        }
    }
}

/// 観測の固定（引き継ぎの印）。合格の証拠ではない: 0 件は「未検証」で、本番に RS256 の鍵の
/// 読み込みがまだ無いことを示すだけ。現れたら人が DPOL-14 の確認（PKCS#8 の PEM を
/// `from_rsa_pem` に渡す形か）を行い、この観測を更新する（引き取りは `TASK-347` と読む）。
/// 期待を緩めて通さない（AUDB-6）。
#[test]
fn dpol14_observation_no_production_rs256_key_loading_yet() {
    let mut found = Vec::new();
    let mut scanned_prod = 0usize;
    for (path, src) in all_sources() {
        let in_prod = PROD_DIRS
            .iter()
            .any(|d| path.starts_with(&format!("crates/{d}/")));
        if in_prod {
            scanned_prod += 1;
            for n in find_ident(&src, PEM_IDENT) {
                found.push(format!("{path}:{n}"));
            }
        }
    }
    // 空振りで通さない: 本番のソースを 1 件以上走査している
    assert!(scanned_prod > 0, "no production sources scanned");
    assert!(
        found.is_empty(),
        "RS256 key loading appeared in production code ({}). A human must verify DPOL-14 / TASK-667 (4/4) (PKCS#8 PEM passed to from_rsa_pem) and then update this observation. See TASK-347.",
        found.join(", ")
    );
}
