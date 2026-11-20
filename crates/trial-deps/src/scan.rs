//! 構文木の走査の最小の試し（SECR-21。本番の走査と CI への組み込みは TASK-514）。
//!
//! 秘密値らしい欄名で、型が生の `String` の構造体の欄を拾う。マクロ呼び出しの中の型は
//! 構文木に展開されないため見えない。この見落としの観測は試験が行い、判定は TASK-514 で行う。
//! `syn` は dev 依存で、試験ビルドでだけ本 module を有効にする（K7。`lib.rs` の `cfg(test)`）。

use syn::{Fields, Item, Type};

/// 秘密値らしい欄名の部分文字列（小文字で比較）。試し用の最小の集合で、本番の一覧ではない。
const SECRET_LIKE: [&str; 3] = ["password", "secret", "token"];

/// 拾った欄。
#[derive(Debug, PartialEq, Eq)]
pub struct Finding {
    /// 構造体名。
    pub struct_name: String,
    /// 欄名。
    pub field_name: String,
}

/// `source` を `syn::parse_file` で読み、秘密値らしい欄名で型が生の `String` の欄を返す。
/// モジュールの中は辿る。マクロ呼び出しの中は辿らない。
///
/// # Errors
/// 構文として読めないとき、`syn` の誤りを返す。
pub fn find_raw_string_secret_fields(source: &str) -> syn::Result<Vec<Finding>> {
    let file = syn::parse_file(source)?;
    let mut out = Vec::new();
    collect(&file.items, &mut out);
    Ok(out)
}

fn collect(items: &[Item], out: &mut Vec<Finding>) {
    for item in items {
        match item {
            Item::Struct(s) => {
                if let Fields::Named(named) = &s.fields {
                    for f in &named.named {
                        let Some(ident) = &f.ident else { continue };
                        let name = ident.to_string();
                        if is_secret_like(&name) && is_raw_string(&f.ty) {
                            out.push(Finding {
                                struct_name: s.ident.to_string(),
                                field_name: name,
                            });
                        }
                    }
                }
            }
            Item::Mod(m) => {
                if let Some((_, inner)) = &m.content {
                    collect(inner, out);
                }
            }
            _ => {}
        }
    }
}

fn is_secret_like(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    SECRET_LIKE.iter().any(|k| lower.contains(k))
}

/// 型が生の `String` かを、型の経路の末尾の識別子だけで判定する。
///
/// 型を解決しない、構文の上だけの判定である。このため次の 2 つの制約がある。
/// - 誤検出: 独自型の `custom::String` も末尾が `String` なので拾う。
/// - 見逃し: 標準の `String` を別名（`type S = String;`、`use std::string::String as S;`）
///   で参照する欄は拾わない。
///
/// この結果の解釈と扱いは TASK-514 で決める（SECR-21）。
fn is_raw_string(ty: &Type) -> bool {
    matches!(ty, Type::Path(p) if p.qself.is_none()
        && p.path.segments.last().is_some_and(|s| s.ident == "String"))
}

#[cfg(test)]
mod tests {
    //! SECR-21 の試し。本番の走査は TASK-514。見落としの扱いは TASK-514 で決める（main の読み）。
    //! ここは〔仮想〕の確認で、マクロ内の観測は合否の判定ではない。

    use super::*;

    fn found(src: &str) -> Vec<(String, String)> {
        find_raw_string_secret_fields(src)
            .expect("fixture parses")
            .into_iter()
            .map(|f| (f.struct_name, f.field_name))
            .collect()
    }

    fn pair(s: &str, f: &str) -> (String, String) {
        (s.to_owned(), f.to_owned())
    }

    /// SECR-21: 生の `String` の `password` 欄を拾う。
    #[test]
    fn secr21_raw_string_password_field_is_found() {
        let src = "pub struct Login { pub user: String, pub password: String }";
        assert_eq!(found(src), vec![pair("Login", "password")]);
    }

    /// SECR-21: `SecretString` の欄は拾わない。
    #[test]
    fn secr21_secret_string_field_is_not_found() {
        let src = "pub struct Login { pub password: SecretString, pub api_token: fandhe_auth_secret::SecretString }";
        assert_eq!(found(src), Vec::<(String, String)>::new());
    }

    /// SECR-21: インラインの `mod` の中も拾う。
    #[test]
    fn secr21_inline_mod_is_traversed() {
        let src = "mod outer { pub mod inner { pub struct S { pub refresh_token: String } } }";
        assert_eq!(found(src), vec![pair("S", "refresh_token")]);
    }

    /// edition 2024 の構文（`unsafe extern`・`r#gen` 識別子・`let` チェーン・`async` クロージャ）を読める。
    #[test]
    fn syn_reads_edition_2024_syntax() {
        let src = r#"
            unsafe extern "C" { pub safe fn abs(x: i32) -> i32; }
            pub struct Cfg { pub r#gen: u32, pub client_secret: String }
            pub fn f(x: Option<u32>) -> bool {
                if let Some(a) = x && a > 1 { return true; }
                let _c = async |v: u32| v + 1;
                false
            }
        "#;
        assert_eq!(found(src), vec![pair("Cfg", "client_secret")]);
    }

    /// 観測の固定: マクロ呼び出しの中に書いた同種の構造体は見えない（合否の判定ではない）。
    /// 見落としの扱いは TASK-514 で決める（main の読み）。
    #[test]
    fn observation_struct_inside_macro_call_is_not_seen() {
        let src = "make_types! { pub struct Hidden { pub password: String } }";
        assert_eq!(found(src), Vec::<(String, String)>::new());
    }

    /// 観察用（TASK-514 で扱いを決める）: 型を解決しない判定のため、独自型の `custom::String` は
    /// 拾われ（誤検出）、`String` の別名の欄は拾われない（見逃し）。合否の判定ではない（SECR-21）。
    #[test]
    fn observation_custom_string_is_found_and_alias_is_not() {
        let src = concat!(
            "type S = String;\n",
            "use std::string::String as Alias;\n",
            "pub struct A { pub password: custom::String }\n",
            "pub struct B { pub password: S }\n",
            "pub struct C { pub api_token: Alias }",
        );
        assert_eq!(found(src), vec![pair("A", "password")]);
    }
}
