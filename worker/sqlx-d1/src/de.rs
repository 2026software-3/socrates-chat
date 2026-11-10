//! 把 D1 查回的列轉成 Rust 型別。
//!
//! SQLite 沒有布林、陣列與時間型別，所以轉換時比 serde_json 寬鬆：整數 0／1 可讀成 `bool`，
//! JSON 文字可讀成 `Vec`、map 或 struct，時間是 ISO 8601 字串（chrono 直接解析）。

use std::sync::Arc;

use serde::de::{
    self, DeserializeOwned, DeserializeSeed, Deserializer, IntoDeserializer, MapAccess, SeqAccess,
    Visitor,
};
use wasm_bindgen::JsValue;

use crate::Error;

#[derive(Debug, Clone)]
pub enum Cell {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
}

impl Cell {
    pub(crate) fn from_js(v: JsValue) -> Cell {
        if v.is_null() || v.is_undefined() {
            return Cell::Null;
        }
        if let Some(n) = v.as_f64() {
            if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 {
                return Cell::Int(n as i64);
            }
            return Cell::Real(n);
        }
        if let Some(s) = v.as_string() {
            return Cell::Text(s);
        }
        if let Some(b) = v.as_bool() {
            return Cell::Int(b as i64);
        }
        // BLOB 等其他型別：轉成 JSON 文字
        match serde_wasm_bindgen::from_value::<serde_json::Value>(v) {
            Ok(value) => Cell::Text(value.to_string()),
            Err(_) => Cell::Null,
        }
    }
}

#[derive(Debug)]
pub struct DeError(String);

impl std::fmt::Display for DeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for DeError {}
impl de::Error for DeError {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        DeError(msg.to_string())
    }
}

pub(crate) fn from_row<O: DeserializeOwned>(
    columns: &Arc<[String]>,
    row: Vec<Cell>,
) -> Result<O, Error> {
    O::deserialize(RowDe {
        columns: columns.clone(),
        cells: row,
    })
    .map_err(|e| Error::Decode(e.0))
}

pub(crate) fn from_scalar<O: DeserializeOwned>(row: Vec<Cell>) -> Result<O, Error> {
    let cell = row.into_iter().next().unwrap_or(Cell::Null);
    O::deserialize(CellDe(cell)).map_err(|e| Error::Decode(e.0))
}

struct RowDe {
    columns: Arc<[String]>,
    cells: Vec<Cell>,
}

impl<'de> Deserializer<'de> for RowDe {
    type Error = DeError;

    fn deserialize_any<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        self.deserialize_map(v)
    }

    fn deserialize_map<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        v.visit_map(RowMap {
            columns: self.columns,
            cells: self.cells.into_iter(),
            index: 0,
            pending: None,
        })
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        v: V,
    ) -> Result<V::Value, DeError> {
        self.deserialize_map(v)
    }

    fn deserialize_seq<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        v.visit_seq(CellSeq(self.cells.into_iter()))
    }

    fn deserialize_tuple<V: Visitor<'de>>(self, _len: usize, v: V) -> Result<V::Value, DeError> {
        self.deserialize_seq(v)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _len: usize,
        v: V,
    ) -> Result<V::Value, DeError> {
        self.deserialize_seq(v)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct enum identifier ignored_any
    }
}

struct RowMap {
    columns: Arc<[String]>,
    cells: std::vec::IntoIter<Cell>,
    index: usize,
    pending: Option<Cell>,
}

impl<'de> MapAccess<'de> for RowMap {
    type Error = DeError;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, DeError> {
        let Some(cell) = self.cells.next() else {
            return Ok(None);
        };
        let name = self.columns.get(self.index).cloned().unwrap_or_default();
        self.index += 1;
        self.pending = Some(cell);
        seed.deserialize(name.into_deserializer()).map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, DeError> {
        seed.deserialize(CellDe(self.pending.take().unwrap_or(Cell::Null)))
    }
}

struct CellSeq(std::vec::IntoIter<Cell>);

impl<'de> SeqAccess<'de> for CellSeq {
    type Error = DeError;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, DeError> {
        match self.0.next() {
            Some(cell) => seed.deserialize(CellDe(cell)).map(Some),
            None => Ok(None),
        }
    }
}

struct CellDe(Cell);

impl CellDe {
    /// 文字內容是 JSON 時（陣列、物件），交給 serde_json。
    fn json<'de, V: Visitor<'de>>(
        text: &str,
        f: impl FnOnce(serde_json::Value, V) -> Result<V::Value, serde_json::Error>,
        v: V,
    ) -> Result<V::Value, DeError> {
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| DeError(format!("invalid JSON column: {e}")))?;
        f(value, v).map_err(|e| DeError(e.to_string()))
    }
}

impl<'de> Deserializer<'de> for CellDe {
    type Error = DeError;

    fn deserialize_any<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        match self.0 {
            Cell::Null => v.visit_unit(),
            Cell::Int(n) => v.visit_i64(n),
            Cell::Real(n) => v.visit_f64(n),
            Cell::Text(s) => v.visit_string(s),
        }
    }

    fn deserialize_bool<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        match self.0 {
            Cell::Int(n) => v.visit_bool(n != 0),
            Cell::Real(n) => v.visit_bool(n != 0.0),
            Cell::Text(s) if s == "true" || s == "1" => v.visit_bool(true),
            Cell::Text(s) if s == "false" || s == "0" => v.visit_bool(false),
            other => Err(DeError(format!("expected boolean, got {other:?}"))),
        }
    }

    fn deserialize_f64<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        match self.0 {
            Cell::Int(n) => v.visit_f64(n as f64),
            Cell::Real(n) => v.visit_f64(n),
            Cell::Text(s) => s
                .parse::<f64>()
                .map_err(|_| DeError("expected number".into()))
                .and_then(|n| v.visit_f64(n)),
            Cell::Null => Err(DeError("expected number, got NULL".into())),
        }
    }

    fn deserialize_f32<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        self.deserialize_f64(v)
    }

    fn deserialize_i64<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        match self.0 {
            Cell::Int(n) => v.visit_i64(n),
            Cell::Real(n) if n.fract() == 0.0 => v.visit_i64(n as i64),
            Cell::Text(s) => s
                .parse::<i64>()
                .map_err(|_| DeError("expected integer".into()))
                .and_then(|n| v.visit_i64(n)),
            other => Err(DeError(format!("expected integer, got {other:?}"))),
        }
    }

    fn deserialize_i8<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        self.deserialize_i64(v)
    }
    fn deserialize_i16<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        self.deserialize_i64(v)
    }
    fn deserialize_i32<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        self.deserialize_i64(v)
    }
    fn deserialize_u8<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        self.deserialize_i64(v)
    }
    fn deserialize_u16<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        self.deserialize_i64(v)
    }
    fn deserialize_u32<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        self.deserialize_i64(v)
    }
    fn deserialize_u64<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        self.deserialize_i64(v)
    }

    fn deserialize_str<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        self.deserialize_string(v)
    }

    fn deserialize_string<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        match self.0 {
            Cell::Text(s) => v.visit_string(s),
            Cell::Int(n) => v.visit_string(n.to_string()),
            Cell::Real(n) => v.visit_string(n.to_string()),
            Cell::Null => Err(DeError("expected text, got NULL".into())),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        match self.0 {
            Cell::Null => v.visit_none(),
            other => v.visit_some(CellDe(other)),
        }
    }

    fn deserialize_seq<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        match self.0 {
            Cell::Text(s) => Self::json(&s, |value, v| value.deserialize_seq(v), v),
            Cell::Null => v.visit_seq(CellSeq(Vec::new().into_iter())),
            other => Err(DeError(format!("expected JSON array, got {other:?}"))),
        }
    }

    fn deserialize_map<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        match self.0 {
            Cell::Text(s) => Self::json(&s, |value, v| value.deserialize_map(v), v),
            other => Err(DeError(format!("expected JSON object, got {other:?}"))),
        }
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        v: V,
    ) -> Result<V::Value, DeError> {
        match self.0 {
            Cell::Text(s) => {
                Self::json(&s, |value, v| value.deserialize_struct(name, fields, v), v)
            }
            other => Err(DeError(format!("expected JSON object, got {other:?}"))),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        v: V,
    ) -> Result<V::Value, DeError> {
        v.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        v: V,
    ) -> Result<V::Value, DeError> {
        match self.0 {
            Cell::Text(s) => v.visit_enum(s.into_deserializer()),
            other => Err(DeError(format!("expected enum text, got {other:?}"))),
        }
    }

    fn deserialize_unit<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        v.visit_unit()
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        v.visit_unit()
    }

    fn deserialize_tuple<V: Visitor<'de>>(self, _len: usize, v: V) -> Result<V::Value, DeError> {
        self.deserialize_seq(v)
    }

    serde::forward_to_deserialize_any! {
        i128 u128 char bytes byte_buf unit_struct tuple_struct identifier
    }
}
