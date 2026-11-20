//! `metrics` の出す口の試し（SECR-26・SECR-27）。key とラベルは固定の英語の文字列で、秘密値を載せない。
//! recorder は選ばない（`D-2026-11-20-5`）。

/// counter を 1 回出す。key `trial_probe_total`、ラベル `kind="probe"`。
pub fn emit_probe_counter() {
    metrics::counter!("trial_probe_total", "kind" => "probe").increment(1);
}
