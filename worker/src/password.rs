//! 內建登入的密碼處理（S-08.2 第 1 節）：雜湊、驗證與臨時密碼。
//!
//! Workers 版改用 WebCrypto 的 PBKDF2-SHA256（原生實作，不吃 Wasm 的 CPU）取代 Argon2id：
//! 免費方案每個請求只有 10 ms CPU，Argon2id 跑不完。PostgreSQL 版留下的 Argon2 雜湊無法驗證（視為不符），
//! 這些帳號需由管理者重設密碼。
//!
//! 輪數：使用者自訂的密碼用 [`USER_ITERATIONS`]（Workers 允許的上限）；臨時密碼本身有約 60 位元的隨機性，
//! 用 [`TEMPORARY_ITERATIONS`]，大量匯入名單時才不會超過 CPU 限制。輪數記在雜湊字串裡，驗證時照字串的值。
//!
//! 密碼、臨時密碼與雜湊不得寫進日誌、API 回應（除了建立當下回給管理者的臨時密碼）或稽核日誌。

use base64::Engine;
use base64::engine::general_purpose::STANDARD_NO_PAD as B64;
use js_sys::{Array, Object, Reflect, Uint8Array};
use rand::Rng;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use worker::send::SendFuture;

/// 密碼長度下限（字元數）；不限制字元種類。
pub const MIN_LENGTH: usize = 8;
/// 密碼長度上限，避免用超長輸入消耗雜湊運算。
pub const MAX_LENGTH: usize = 128;

/// 使用者自訂密碼的 PBKDF2 輪數；Workers 的 WebCrypto 不接受超過 100,000。
pub const USER_ITERATIONS: u32 = 100_000;
/// 臨時密碼（隨機 12 字元）的輪數。
pub const TEMPORARY_ITERATIONS: u32 = 1_000;

/// 臨時密碼字元集：32 個字元，去掉容易混淆的 `0/O`、`1/I/L`（256 可被 32 整除，取樣沒有偏差）。
const TEMP_ALPHABET: &[u8; 32] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789a";
const TEMP_LENGTH: usize = 12;

const PREFIX: &str = "$pbkdf2-sha256$i=";

#[derive(Debug)]
pub struct HashError;

impl std::fmt::Display for HashError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("password hashing failed")
    }
}

impl std::error::Error for HashError {}

fn subtle() -> Result<web_sys::SubtleCrypto, HashError> {
    let crypto = Reflect::get(&js_sys::global(), &"crypto".into()).map_err(|_| HashError)?;
    let crypto: web_sys::Crypto = crypto.dyn_into().map_err(|_| HashError)?;
    Ok(crypto.subtle())
}

async fn derive(password: &str, salt: &[u8], iterations: u32) -> Result<Vec<u8>, HashError> {
    let subtle = subtle()?;
    let key_data = Uint8Array::from(password.as_bytes());
    let usages = Array::of1(&JsValue::from_str("deriveBits"));
    let key = JsFuture::from(
        subtle
            .import_key_with_str("raw", &key_data, "PBKDF2", false, &usages)
            .map_err(|_| HashError)?,
    )
    .await
    .map_err(|_| HashError)?;
    let key: web_sys::CryptoKey = key.dyn_into().map_err(|_| HashError)?;
    let params = Object::new();
    let set = |k: &str, v: &JsValue| Reflect::set(&params, &k.into(), v).map_err(|_| HashError);
    set("name", &"PBKDF2".into())?;
    set("hash", &"SHA-256".into())?;
    set("salt", &Uint8Array::from(salt))?;
    set("iterations", &JsValue::from(iterations))?;
    let bits = JsFuture::from(
        subtle
            .derive_bits_with_object(&params, &key, 256)
            .map_err(|_| HashError)?,
    )
    .await
    .map_err(|_| HashError)?;
    Ok(Uint8Array::new(&bits).to_vec())
}

async fn hash_with(password: String, iterations: u32) -> Result<String, HashError> {
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|_| HashError)?;
    let dk = derive(&password, &salt, iterations).await?;
    Ok(format!(
        "{PREFIX}{iterations}${}${}",
        B64.encode(salt),
        B64.encode(dk)
    ))
}

/// 雜湊使用者自訂的密碼，回傳 PHC 風格字串。
pub fn hash(password: &str) -> impl Future<Output = Result<String, HashError>> + Send {
    SendFuture::new(hash_with(password.to_string(), USER_ITERATIONS))
}

/// 雜湊系統產生的臨時密碼（輪數較少，見模組說明）。
pub fn hash_temporary(password: &str) -> impl Future<Output = Result<String, HashError>> + Send {
    SendFuture::new(hash_with(password.to_string(), TEMPORARY_ITERATIONS))
}

fn parse(hash: &str) -> Option<(u32, Vec<u8>, Vec<u8>)> {
    let rest = hash.strip_prefix(PREFIX)?;
    let mut parts = rest.split('$');
    let iterations: u32 = parts.next()?.parse().ok()?;
    if iterations == 0 || iterations > USER_ITERATIONS {
        return None;
    }
    let salt = B64.decode(parts.next()?).ok()?;
    let dk = B64.decode(parts.next()?).ok()?;
    parts.next().is_none().then_some((iterations, salt, dk))
}

/// 驗證密碼；雜湊格式不合（包括 PostgreSQL 版的 Argon2 雜湊）視為不符。
pub fn verify(password: &str, hash: &str) -> impl Future<Output = bool> + Send {
    let password = password.to_string();
    let parsed = parse(hash);
    SendFuture::new(async move {
        let Some((iterations, salt, expected)) = parsed else {
            return false;
        };
        match derive(&password, &salt, iterations).await {
            // 固定時間比較
            Ok(actual) => {
                actual.len() == expected.len()
                    && actual
                        .iter()
                        .zip(&expected)
                        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                        == 0
            }
            Err(_) => false,
        }
    })
}

/// 產生隨機臨時密碼（12 字元，約 60 位元）。
pub fn generate_temporary() -> String {
    let mut bytes = [0u8; TEMP_LENGTH];
    rand::rng().fill_bytes(&mut bytes);
    bytes
        .iter()
        .map(|b| TEMP_ALPHABET[(*b % 32) as usize] as char)
        .collect()
}

/// 新密碼是否符合長度規則。
pub fn is_acceptable(password: &str) -> bool {
    let n = password.chars().count();
    (MIN_LENGTH..=MAX_LENGTH).contains(&n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_well_formed_pbkdf2_strings() {
        assert!(parse("$pbkdf2-sha256$i=1000$AAAA$AAAA").is_some());
        assert!(parse("$argon2id$v=19$m=19456,t=2,p=1$abc$def").is_none());
        assert!(parse("$pbkdf2-sha256$i=0$AAAA$AAAA").is_none());
        assert!(parse("$pbkdf2-sha256$i=200000$AAAA$AAAA").is_none());
        assert!(parse("$pbkdf2-sha256$i=1000$AAAA$AAAA$extra").is_none());
    }
}
