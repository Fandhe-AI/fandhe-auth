//! ビルド時のコンパイラの版を記録用に埋め込む（TASK-10 の 5/6）。
//!
//! 実行時の PATH 上の `rustc` ではなく、このバイナリを作った `rustc` の版を残すため。
//! 失敗時は空にして、記録の生成を検査で拒否させる（fail-closed）。

use std::process::Command;

fn main() {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let ver = Command::new(rustc)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .unwrap_or_default();
    println!("cargo:rustc-env=FANDHE_IT_BUILD_RUSTC={ver}");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=RUSTC");
}
