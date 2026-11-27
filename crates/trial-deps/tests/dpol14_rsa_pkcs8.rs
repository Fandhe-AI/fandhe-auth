//! DPOL-14: RS256 の署名鍵を PKCS#8 で持つとき、`jsonwebtoken`（`aws_lc_rs` backend）に
//! DER で渡すと署名に失敗し、PEM で渡すと成功することの試験（TASK-667 (3/4)・#319）。
//!
//! 〔仮想〕の確認で、spec の「確定」ではない（ACPT-7）。「実装が PEM の形を使う」ことの確認は
//! TASK-667 (4/4)・#320 の範囲で、ここでは扱わない。
//! 鍵は実行時に生成し、鍵・DER・PEM・トークンを出力に出さない（SECR-23）。

use aws_lc_rs::encoding::{AsDer, Pkcs8V1Der};
use aws_lc_rs::rsa::KeySize;
use aws_lc_rs::signature::{KeyPair, RsaKeyPair};
use fandhe_auth_secret::SecretString;
use jsonwebtoken::errors::ErrorKind;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, decode_header, encode};
use std::collections::BTreeMap;

/// 実行時に生成した鍵と、その PKCS#8 の秘密鍵 PEM を返す。
/// 秘密鍵は `SecretString` だけで保持する（SECR-10・SECR-30）。DER は必要な瞬間に PEM から
/// 取り出す（`pkcs8_der`）ため、生の `Vec<u8>` を入れ物に持たせない。
/// 大きさは 2048 で足りる（PoC の値は要件の値ではない）。
fn pkcs8_rsa_key() -> (RsaKeyPair, SecretString) {
    let kp = RsaKeyPair::generate(KeySize::Rsa2048).expect("key generation must succeed");
    let der = AsDer::<Pkcs8V1Der>::as_der(&kp).expect("pkcs8 export must succeed");
    // 見出しは pem crate にタグを渡して作る（鍵の実体をソースに置かない）。
    let pem = pem::encode(&pem::Pem::new("PRIVATE KEY", der.as_ref().to_vec()));
    (kp, SecretString::new(pem))
}

/// 署名・読み取りの直前にだけ値を取り出す（SECR-16）。この試験で `expose()` を呼ぶのは、この関数と
/// `from_rsa_pem` への受け渡し・`decode_header`・署名の分割の各呼び出しだけ。
fn pkcs8_der(secret_pem: &SecretString) -> Vec<u8> {
    pem::parse(secret_pem.expose())
        .expect("generated pem must parse")
        .into_contents()
}

fn claims() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([("sub", "dpol-14")])
}

/// 陽性対照。入力が PKCS#8 で、PKCS#1 の読み手が拒否することを示し、
/// 下の失敗の理由を「形式の取り違え」と特定できるようにする。
#[test]
fn dpol14_control_generated_key_is_pkcs8_not_pkcs1() {
    let (_kp, secret_pem) = pkcs8_rsa_key();
    let der = pkcs8_der(&secret_pem);
    assert!(
        RsaKeyPair::from_pkcs8(&der).is_ok(),
        "pkcs8 reader must accept the key"
    );
    assert!(
        RsaKeyPair::from_der(&der).is_err(),
        "pkcs1 reader must reject the pkcs8 key"
    );
}

/// `from_rsa_der` は PKCS#1 を期待するため、PKCS#8 の DER では `encode` が署名に失敗する。
/// 失敗は `from_rsa_der` ではなく `encode` の時点で起きる。文言ではなく種別で照合する。
#[test]
fn dpol14_from_rsa_der_with_pkcs8_der_fails_to_sign() {
    let (_kp, secret_pem) = pkcs8_rsa_key();
    let der = pkcs8_der(&secret_pem);
    let key = EncodingKey::from_rsa_der(&der);
    let result = encode(&Header::new(Algorithm::RS256), &claims(), &key);
    // SECR-23: `expect_err` は成功値（署名済みトークン）を panic に出すため使わない。
    let err = match result {
        Err(e) => e,
        Ok(_) => panic!("pkcs8 der was accepted for signing"),
    };
    assert!(
        matches!(err.kind(), ErrorKind::Signing(_)),
        "failure must be a signing error"
    );
}

/// `from_rsa_pem` は PKCS#8 の PEM で署名に成功し、対応する公開鍵で検証できる。
#[test]
fn dpol14_from_rsa_pem_with_pkcs8_pem_signs_rs256() {
    let (kp, secret_pem) = pkcs8_rsa_key();
    let key = EncodingKey::from_rsa_pem(secret_pem.expose().as_bytes());
    assert!(key.is_ok(), "pkcs8 pem must be accepted");
    let key = key.unwrap_or_else(|_| unreachable!());

    // 署名済みトークンも秘密値として包む（SECR-30）。
    let token = encode(&Header::new(Algorithm::RS256), &claims(), &key);
    assert!(token.is_ok(), "pkcs8 pem must sign");
    let token = SecretString::new(token.unwrap_or_else(|_| unreachable!()));

    let header = decode_header(token.expose()).expect("header must parse");
    assert_eq!(header.alg, Algorithm::RS256);

    // 時刻に依る Validation の経路は使わず、署名だけを検証する。
    let spki = kp.public_key().as_der().expect("spki export must succeed");
    let pub_pem = pem::encode(&pem::Pem::new("PUBLIC KEY", spki.as_ref().to_vec()));
    let dk = DecodingKey::from_rsa_pem(pub_pem.as_bytes()).expect("public pem must parse");
    let (msg, sig) = token
        .expose()
        .rsplit_once('.')
        .expect("jws has three parts");
    let ok = jsonwebtoken::crypto::verify(sig, msg.as_bytes(), &dk, Algorithm::RS256)
        .expect("verify must run");
    assert!(ok, "signature must verify");
}
