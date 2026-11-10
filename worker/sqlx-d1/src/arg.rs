//! 綁定參數的編碼。

use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use uuid::Uuid;
use wasm_bindgen::JsValue;

#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
}

impl Arg {
    pub(crate) fn to_js(&self) -> JsValue {
        match self {
            Arg::Null => JsValue::NULL,
            Arg::Int(n) => JsValue::from_f64(*n as f64),
            Arg::Real(n) => JsValue::from_f64(*n),
            Arg::Text(s) => JsValue::from_str(s),
        }
    }
}

/// 可以綁定成查詢參數的值。
pub trait Encode {
    fn encode(&self) -> Arg;
}

impl<T: Encode + ?Sized> Encode for &T {
    fn encode(&self) -> Arg {
        (**self).encode()
    }
}

impl<T: Encode> Encode for Option<T> {
    fn encode(&self) -> Arg {
        match self {
            Some(v) => v.encode(),
            None => Arg::Null,
        }
    }
}

impl<T: Encode + ?Sized> Encode for Box<T> {
    fn encode(&self) -> Arg {
        (**self).encode()
    }
}

macro_rules! ints {
    ($($t:ty),*) => {$(
        impl Encode for $t {
            fn encode(&self) -> Arg {
                Arg::Int(*self as i64)
            }
        }
    )*};
}
ints!(i8, i16, i32, i64, u8, u16, u32, u64, usize, isize);

impl Encode for f64 {
    fn encode(&self) -> Arg {
        Arg::Real(*self)
    }
}

impl Encode for f32 {
    fn encode(&self) -> Arg {
        Arg::Real(*self as f64)
    }
}

impl Encode for bool {
    fn encode(&self) -> Arg {
        Arg::Int(*self as i64)
    }
}

impl Encode for str {
    fn encode(&self) -> Arg {
        Arg::Text(self.to_string())
    }
}

impl Encode for String {
    fn encode(&self) -> Arg {
        Arg::Text(self.clone())
    }
}

impl Encode for Uuid {
    fn encode(&self) -> Arg {
        Arg::Text(self.hyphenated().to_string())
    }
}

/// 時間一律存成 `YYYY-MM-DDTHH:MM:SS.mmmZ`，與 SQL 裡的 `now()` 改寫結果相同。
pub fn format_time(t: &DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Millis, true)
}

impl Encode for DateTime<Utc> {
    fn encode(&self) -> Arg {
        Arg::Text(format_time(self))
    }
}

impl Encode for NaiveDate {
    fn encode(&self) -> Arg {
        Arg::Text(self.format("%Y-%m-%d").to_string())
    }
}

/// 位元組（例如 session token 的雜湊）存成小寫十六進位字串。
impl Encode for [u8] {
    fn encode(&self) -> Arg {
        Arg::Text(self.iter().map(|b| format!("{b:02x}")).collect())
    }
}

impl Encode for Vec<u8> {
    fn encode(&self) -> Arg {
        self.as_slice().encode()
    }
}

impl<const N: usize> Encode for [u8; N] {
    fn encode(&self) -> Arg {
        self.as_slice().encode()
    }
}

/// 陣列參數存成 JSON 文字，SQL 端用 `json_each(?N)` 展開（取代 PostgreSQL 的 `= ANY($N)`）。
macro_rules! json_lists {
    ($($t:ty),*) => {$(
        impl Encode for [$t] {
            fn encode(&self) -> Arg {
                Arg::Text(serde_json::to_string(self).unwrap_or_else(|_| "[]".into()))
            }
        }
        impl Encode for Vec<$t> {
            fn encode(&self) -> Arg {
                self.as_slice().encode()
            }
        }
    )*};
}
json_lists!(String, &str, Uuid, i32, i64);

impl Encode for serde_json::Value {
    fn encode(&self) -> Arg {
        Arg::Text(self.to_string())
    }
}

impl Encode for Arg {
    fn encode(&self) -> Arg {
        self.clone()
    }
}
