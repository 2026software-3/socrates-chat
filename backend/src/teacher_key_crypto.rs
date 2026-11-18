//! F-40.1-DB (#591): 教師 API key 的加密保存（S-03.1）。
//!
//! **待確認**（需求方尚未決定演算法與輪替，這裡是提案）：
//! - AES-256-GCM（AEAD），每次加密產生新的 96 位元 nonce，密文格式為 `nonce || ciphertext+tag`。
//! - 加密金鑰由環境變數 `TEACHER_KEY_ENCRYPTION_KEYS` 提供，格式 `id:base64(32 位元組)`，多把以逗號分隔；
//!   **第一把用來加密新 key，其餘只用來解密舊資料**。輪替＝在最前面加一把新金鑰；舊 key 在下次更換時才改用新金鑰
//!   （批次重新加密留給後續）。
//! - 重複偵測用的指紋是 key 的 SHA-256：API key 熵很高，不需要加鹽，也不依賴加密金鑰，所以輪替不影響重複偵測。
//!
//! 金鑰、明文與密文不得出現在日誌、錯誤或 `Debug` 輸出中。

// aes-gcm 0.10 的 `from_slice` 已標為棄用（建議升級 generic-array 1.x），但該版本尚未被 aes-gcm 採用。
#![allow(deprecated)]

use std::fmt;

use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};

const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

/// 加密後的結果：用哪把加密金鑰，與要存進資料庫的位元組。
pub struct Sealed {
    pub key_id: String,
    pub ciphertext: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CipherConfigError(&'static str);

impl fmt::Display for CipherConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid TEACHER_KEY_ENCRYPTION_KEYS: {}", self.0)
    }
}

impl std::error::Error for CipherConfigError {}

/// 解密失敗（金鑰 id 不存在、被竄改、金鑰不符）。不帶任何內容。
#[derive(Debug, PartialEq, Eq)]
pub struct DecryptError;

pub struct KeyCipher {
    /// 第一把是目前用來加密的。
    keys: Vec<(String, Aes256Gcm)>,
}

impl fmt::Debug for KeyCipher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "KeyCipher({} keys)", self.keys.len())
    }
}

impl KeyCipher {
    /// 解析 `id:base64,id:base64`；id 不可重複，每把必須剛好 32 位元組。錯誤訊息不含設定內容。
    pub fn parse(raw: &str) -> Result<Self, CipherConfigError> {
        let mut keys: Vec<(String, Aes256Gcm)> = Vec::new();
        for item in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let (id, b64) = item
                .split_once(':')
                .ok_or(CipherConfigError("each entry must be id:base64"))?;
            let id = id.trim();
            if id.is_empty() {
                return Err(CipherConfigError("empty key id"));
            }
            if keys.iter().any(|(k, _)| k == id) {
                return Err(CipherConfigError("duplicate key id"));
            }
            let bytes = STANDARD
                .decode(b64.trim())
                .map_err(|_| CipherConfigError("key is not valid base64"))?;
            if bytes.len() != KEY_LEN {
                return Err(CipherConfigError("key must be 32 bytes"));
            }
            keys.push((
                id.to_owned(),
                Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&bytes)),
            ));
        }
        if keys.is_empty() {
            return Err(CipherConfigError("no keys"));
        }
        Ok(Self { keys })
    }

    pub fn encrypt(&self, plaintext: &str) -> Sealed {
        let (key_id, cipher) = &self.keys[0];
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let mut out = nonce.to_vec();
        // AES-GCM 加密只在明文超過 2^36 位元組時失敗；API key 遠小於此。
        out.extend(
            cipher
                .encrypt(&nonce, plaintext.as_bytes())
                .expect("AES-GCM encryption of a short value cannot fail"),
        );
        Sealed {
            key_id: key_id.clone(),
            ciphertext: out,
        }
    }

    pub fn decrypt(&self, key_id: &str, data: &[u8]) -> Result<String, DecryptError> {
        let (_, cipher) = self
            .keys
            .iter()
            .find(|(id, _)| id == key_id)
            .ok_or(DecryptError)?;
        if data.len() <= NONCE_LEN {
            return Err(DecryptError);
        }
        let (nonce, body) = data.split_at(NONCE_LEN);
        let plain = cipher
            .decrypt(Nonce::from_slice(nonce), body)
            .map_err(|_| DecryptError)?;
        String::from_utf8(plain).map_err(|_| DecryptError)
    }
}

/// 重複偵測用的指紋（32 位元組）。
pub fn fingerprint(key: &str) -> Vec<u8> {
    Sha256::digest(key.as_bytes()).to_vec()
}

/// 畫面上的遮蔽識別：夠長的 key 只露出最後四個字元，太短的不露出任何字元。
pub fn mask(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() >= 16 {
        format!("…{}", chars[chars.len() - 4..].iter().collect::<String>())
    } else {
        "…".to_owned()
    }
}
