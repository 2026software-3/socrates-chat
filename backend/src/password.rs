//! 內建登入的密碼處理（S-08.2 第 1 節）：Argon2id 雜湊、驗證與臨時密碼。
//!
//! 密碼、臨時密碼與雜湊不得寫進日誌、API 回應（除了建立當下回給管理者的臨時密碼）或稽核日誌。

use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use rand::Rng;

/// 密碼長度下限（字元數）；不限制字元種類。
pub const MIN_LENGTH: usize = 8;
/// 密碼長度上限，避免用超長輸入消耗雜湊運算。
pub const MAX_LENGTH: usize = 128;

/// 臨時密碼字元集：32 個字元，去掉容易混淆的 `0/O`、`1/I/L`（256 可被 32 整除，取樣沒有偏差）。
const TEMP_ALPHABET: &[u8; 32] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789a";
const TEMP_LENGTH: usize = 12;

/// 以 Argon2id 雜湊密碼，回傳 PHC 字串。
pub fn hash(password: &str) -> Result<String, argon2::password_hash::Error> {
    Ok(Argon2::default()
        .hash_password(password.as_bytes())?
        .to_string())
}

/// 驗證密碼；雜湊格式不合也視為不符。
pub fn verify(password: &str, hash: &str) -> bool {
    Argon2::default()
        .verify_password(password.as_bytes(), hash)
        .is_ok()
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
