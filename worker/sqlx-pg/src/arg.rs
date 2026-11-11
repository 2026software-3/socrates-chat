//! 綁定參數的編碼：每個值帶著 PostgreSQL 型別，交給 `query_typed`／`execute_typed`（一次往返，不需要 prepare）。
//!
//! 型別對應與 SQLx 相同（`i32` → INT4、`Uuid` → UUID、`DateTime<Utc>` → TIMESTAMPTZ、`Vec<String>` → TEXT[]…），
//! 所以後端 SQL 的型別推斷與用 SQLx 時一樣。

use bytes_shim::BytesMut;
use chrono::{DateTime, NaiveDate, Utc};
use tokio_postgres::types::{IsNull, ToSql, Type, to_sql_checked};
use uuid::Uuid;

mod bytes_shim {
    pub use tokio_postgres::types::private::BytesMut;
}

#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    /// NULL，帶著原本值的型別（PostgreSQL 需要知道參數型別）
    Null(Type),
    Bool(bool),
    I16(i16),
    I32(i32),
    I64(i64),
    F64(f64),
    Text(String),
    Bytes(Vec<u8>),
    Uuid(Uuid),
    Time(DateTime<Utc>),
    Date(NaiveDate),
    Json(serde_json::Value),
    TextArray(Vec<String>),
    UuidArray(Vec<Uuid>),
    I32Array(Vec<i32>),
    I64Array(Vec<i64>),
}

/// 任何型別的 NULL。
#[derive(Debug)]
struct PgNull;

impl ToSql for PgNull {
    fn to_sql(
        &self,
        _ty: &Type,
        _out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn std::error::Error + Sync + Send>> {
        Ok(IsNull::Yes)
    }
    fn accepts(_ty: &Type) -> bool {
        true
    }
    to_sql_checked!();
}

impl Arg {
    pub(crate) fn param(&self) -> (&(dyn ToSql + Sync), Type) {
        match self {
            Arg::Null(t) => (&PgNull, t.clone()),
            Arg::Bool(v) => (v, Type::BOOL),
            Arg::I16(v) => (v, Type::INT2),
            Arg::I32(v) => (v, Type::INT4),
            Arg::I64(v) => (v, Type::INT8),
            Arg::F64(v) => (v, Type::FLOAT8),
            Arg::Text(v) => (v, Type::TEXT),
            Arg::Bytes(v) => (v, Type::BYTEA),
            Arg::Uuid(v) => (v, Type::UUID),
            Arg::Time(v) => (v, Type::TIMESTAMPTZ),
            Arg::Date(v) => (v, Type::DATE),
            Arg::Json(v) => (v, Type::JSONB),
            Arg::TextArray(v) => (v, Type::TEXT_ARRAY),
            Arg::UuidArray(v) => (v, Type::UUID_ARRAY),
            Arg::I32Array(v) => (v, Type::INT4_ARRAY),
            Arg::I64Array(v) => (v, Type::INT8_ARRAY),
        }
    }
}

/// 可以綁定成查詢參數的值。`null_type` 是值為 `None` 時要告訴 PostgreSQL 的型別。
pub trait Encode {
    fn encode(&self) -> Arg;
    fn null_type() -> Type;
}

impl<T: Encode + ?Sized> Encode for &T {
    fn encode(&self) -> Arg {
        (**self).encode()
    }
    fn null_type() -> Type {
        T::null_type()
    }
}

impl<T: Encode> Encode for Option<T> {
    fn encode(&self) -> Arg {
        match self {
            Some(v) => v.encode(),
            None => Arg::Null(T::null_type()),
        }
    }
    fn null_type() -> Type {
        T::null_type()
    }
}

impl<T: Encode + ?Sized> Encode for Box<T> {
    fn encode(&self) -> Arg {
        (**self).encode()
    }
    fn null_type() -> Type {
        T::null_type()
    }
}

macro_rules! scalar {
    ($t:ty, $variant:ident, $ty:ident, $conv:expr) => {
        impl Encode for $t {
            fn encode(&self) -> Arg {
                #[allow(clippy::redundant_closure_call)]
                Arg::$variant(($conv)(self))
            }
            fn null_type() -> Type {
                Type::$ty
            }
        }
    };
}

scalar!(bool, Bool, BOOL, |v: &bool| *v);
scalar!(i16, I16, INT2, |v: &i16| *v);
scalar!(i32, I32, INT4, |v: &i32| *v);
scalar!(i64, I64, INT8, |v: &i64| *v);
// 後端只綁有號整數；Workers 版的少數地方會綁無號值，一律當 INT8
scalar!(u32, I64, INT8, |v: &u32| i64::from(*v));
scalar!(usize, I64, INT8, |v: &usize| i64::try_from(*v)
    .unwrap_or(i64::MAX));
scalar!(f64, F64, FLOAT8, |v: &f64| *v);
scalar!(f32, F64, FLOAT8, |v: &f32| f64::from(*v));
scalar!(String, Text, TEXT, |v: &String| v.clone());
scalar!(Uuid, Uuid, UUID, |v: &Uuid| *v);
scalar!(DateTime<Utc>, Time, TIMESTAMPTZ, |v: &DateTime<Utc>| *v);
scalar!(NaiveDate, Date, DATE, |v: &NaiveDate| *v);
scalar!(serde_json::Value, Json, JSONB, |v: &serde_json::Value| v
    .clone());
scalar!(Vec<u8>, Bytes, BYTEA, |v: &Vec<u8>| v.clone());
scalar!(Vec<String>, TextArray, TEXT_ARRAY, |v: &Vec<String>| v
    .clone());
scalar!(Vec<Uuid>, UuidArray, UUID_ARRAY, |v: &Vec<Uuid>| v.clone());
scalar!(Vec<i32>, I32Array, INT4_ARRAY, |v: &Vec<i32>| v.clone());
scalar!(Vec<i64>, I64Array, INT8_ARRAY, |v: &Vec<i64>| v.clone());

impl Encode for str {
    fn encode(&self) -> Arg {
        Arg::Text(self.to_string())
    }
    fn null_type() -> Type {
        Type::TEXT
    }
}

impl Encode for [u8] {
    fn encode(&self) -> Arg {
        Arg::Bytes(self.to_vec())
    }
    fn null_type() -> Type {
        Type::BYTEA
    }
}

impl<const N: usize> Encode for [u8; N] {
    fn encode(&self) -> Arg {
        Arg::Bytes(self.to_vec())
    }
    fn null_type() -> Type {
        Type::BYTEA
    }
}

impl Encode for [String] {
    fn encode(&self) -> Arg {
        Arg::TextArray(self.to_vec())
    }
    fn null_type() -> Type {
        Type::TEXT_ARRAY
    }
}

impl Encode for [&str] {
    fn encode(&self) -> Arg {
        Arg::TextArray(self.iter().map(|s| s.to_string()).collect())
    }
    fn null_type() -> Type {
        Type::TEXT_ARRAY
    }
}

impl Encode for Vec<&str> {
    fn encode(&self) -> Arg {
        self.as_slice().encode()
    }
    fn null_type() -> Type {
        Type::TEXT_ARRAY
    }
}

impl Encode for [Uuid] {
    fn encode(&self) -> Arg {
        Arg::UuidArray(self.to_vec())
    }
    fn null_type() -> Type {
        Type::UUID_ARRAY
    }
}

impl Encode for [i32] {
    fn encode(&self) -> Arg {
        Arg::I32Array(self.to_vec())
    }
    fn null_type() -> Type {
        Type::INT4_ARRAY
    }
}

impl Encode for [i64] {
    fn encode(&self) -> Arg {
        Arg::I64Array(self.to_vec())
    }
    fn null_type() -> Type {
        Type::INT8_ARRAY
    }
}

impl Encode for Arg {
    fn encode(&self) -> Arg {
        self.clone()
    }
    fn null_type() -> Type {
        Type::TEXT
    }
}
