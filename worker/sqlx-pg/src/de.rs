//! 把 PostgreSQL 查回的列轉成 Rust 型別（以 serde 的 `Deserialize` 當作 `FromRow`）。
//!
//! 每個欄位依它的 PostgreSQL 型別先轉成 `Cell`，再交給 serde：struct 依欄位名稱對應，
//! tuple 與純量依欄位順序對應。時間轉成 RFC 3339 字串、UUID 轉成字串，由 chrono／uuid 的
//! `Deserialize` 解析；jsonb 交給 serde_json。

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, NaiveDateTime, SecondsFormat, Utc};
use serde::de::{
    self, DeserializeOwned, DeserializeSeed, Deserializer, IntoDeserializer, MapAccess, SeqAccess,
    Visitor,
};
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::Error;

#[derive(Debug, Clone)]
pub enum Cell {
    Null,
    Bool(bool),
    Int(i64),
    Real(f64),
    Text(String),
    Bytes(Vec<u8>),
    Json(serde_json::Value),
    Array(Vec<Cell>),
}

fn opt<T>(v: Option<T>, f: impl FnOnce(T) -> Cell) -> Cell {
    v.map_or(Cell::Null, f)
}

fn list<T>(v: Option<Vec<Option<T>>>, f: impl Fn(T) -> Cell) -> Cell {
    v.map_or(Cell::Null, |items| {
        Cell::Array(items.into_iter().map(|x| opt(x, &f)).collect())
    })
}

fn time(t: DateTime<Utc>) -> Cell {
    Cell::Text(t.to_rfc3339_opts(SecondsFormat::AutoSi, true))
}

impl Cell {
    /// 依欄位的 PostgreSQL 型別取值。
    pub(crate) fn from_row(row: &Row, i: usize) -> Result<Cell, Error> {
        let ty = row.columns()[i].type_().clone();
        let bad = |e: tokio_postgres::Error| {
            Error::Decode(format!("column {} ({ty}): {e}", row.columns()[i].name()))
        };
        Ok(match ty {
            Type::BOOL => opt(row.try_get(i).map_err(bad)?, Cell::Bool),
            Type::INT2 => opt(row.try_get::<_, Option<i16>>(i).map_err(bad)?, |v| {
                Cell::Int(v.into())
            }),
            Type::INT4 => opt(row.try_get::<_, Option<i32>>(i).map_err(bad)?, |v| {
                Cell::Int(v.into())
            }),
            Type::INT8 => opt(row.try_get(i).map_err(bad)?, Cell::Int),
            Type::OID => opt(row.try_get::<_, Option<u32>>(i).map_err(bad)?, |v| {
                Cell::Int(v.into())
            }),
            Type::FLOAT4 => opt(row.try_get::<_, Option<f32>>(i).map_err(bad)?, |v| {
                Cell::Real(v.into())
            }),
            Type::FLOAT8 => opt(row.try_get(i).map_err(bad)?, Cell::Real),
            Type::UUID => opt(row.try_get::<_, Option<Uuid>>(i).map_err(bad)?, |v| {
                Cell::Text(v.hyphenated().to_string())
            }),
            Type::TIMESTAMPTZ => opt(row.try_get(i).map_err(bad)?, time),
            Type::TIMESTAMP => opt(
                row.try_get::<_, Option<NaiveDateTime>>(i).map_err(bad)?,
                |v| time(v.and_utc()),
            ),
            Type::DATE => opt(row.try_get::<_, Option<NaiveDate>>(i).map_err(bad)?, |v| {
                Cell::Text(v.format("%Y-%m-%d").to_string())
            }),
            Type::JSON | Type::JSONB => opt(row.try_get(i).map_err(bad)?, Cell::Json),
            Type::BYTEA => opt(row.try_get(i).map_err(bad)?, Cell::Bytes),
            Type::TEXT_ARRAY | Type::VARCHAR_ARRAY | Type::NAME_ARRAY => {
                list(row.try_get(i).map_err(bad)?, Cell::Text)
            }
            Type::UUID_ARRAY => list(
                row.try_get::<_, Option<Vec<Option<Uuid>>>>(i)
                    .map_err(bad)?,
                |v| Cell::Text(v.hyphenated().to_string()),
            ),
            Type::INT4_ARRAY => list(
                row.try_get::<_, Option<Vec<Option<i32>>>>(i).map_err(bad)?,
                |v| Cell::Int(v.into()),
            ),
            Type::INT8_ARRAY => list(row.try_get(i).map_err(bad)?, Cell::Int),
            Type::BOOL_ARRAY => list(row.try_get(i).map_err(bad)?, Cell::Bool),
            Type::JSONB_ARRAY | Type::JSON_ARRAY => list(row.try_get(i).map_err(bad)?, Cell::Json),
            Type::TIMESTAMPTZ_ARRAY => list(row.try_get(i).map_err(bad)?, time),
            // TEXT、VARCHAR、BPCHAR、NAME，以及其他可讀成文字的型別
            _ => opt(
                row.try_get::<_, Option<String>>(i).map_err(bad)?,
                Cell::Text,
            ),
        })
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

fn json_err(e: serde_json::Error) -> DeError {
    DeError(e.to_string())
}

impl CellDe {
    /// 文字內容是 JSON 時（陣列、物件），交給 serde_json。
    fn json_text<'de, V: Visitor<'de>>(
        text: &str,
        f: impl FnOnce(serde_json::Value, V) -> Result<V::Value, serde_json::Error>,
        v: V,
    ) -> Result<V::Value, DeError> {
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| DeError(format!("invalid JSON column: {e}")))?;
        f(value, v).map_err(json_err)
    }
}

impl<'de> Deserializer<'de> for CellDe {
    type Error = DeError;

    fn deserialize_any<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        match self.0 {
            Cell::Null => v.visit_unit(),
            Cell::Bool(b) => v.visit_bool(b),
            Cell::Int(n) => v.visit_i64(n),
            Cell::Real(n) => v.visit_f64(n),
            Cell::Text(s) => v.visit_string(s),
            Cell::Bytes(b) => v.visit_byte_buf(b),
            Cell::Json(j) => j.deserialize_any(v).map_err(json_err),
            Cell::Array(items) => v.visit_seq(CellSeq(items.into_iter())),
        }
    }

    fn deserialize_bool<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        match self.0 {
            Cell::Bool(b) => v.visit_bool(b),
            Cell::Int(n) => v.visit_bool(n != 0),
            Cell::Json(j) => j.deserialize_bool(v).map_err(json_err),
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
            Cell::Json(j) => j.deserialize_f64(v).map_err(json_err),
            other => Err(DeError(format!("expected number, got {other:?}"))),
        }
    }

    fn deserialize_f32<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        self.deserialize_f64(v)
    }

    fn deserialize_i64<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        match self.0 {
            Cell::Int(n) => v.visit_i64(n),
            Cell::Real(n) if n.fract() == 0.0 => v.visit_i64(n as i64),
            Cell::Json(j) => j.deserialize_i64(v).map_err(json_err),
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
            Cell::Json(serde_json::Value::String(s)) => v.visit_string(s),
            other => Err(DeError(format!("expected text, got {other:?}"))),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        match self.0 {
            Cell::Null | Cell::Json(serde_json::Value::Null) => v.visit_none(),
            other => v.visit_some(CellDe(other)),
        }
    }

    fn deserialize_seq<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        match self.0 {
            Cell::Array(items) => v.visit_seq(CellSeq(items.into_iter())),
            Cell::Bytes(b) => v.visit_seq(CellSeq(
                b.into_iter()
                    .map(|x| Cell::Int(x.into()))
                    .collect::<Vec<_>>()
                    .into_iter(),
            )),
            Cell::Json(j) => j.deserialize_seq(v).map_err(json_err),
            Cell::Text(s) => Self::json_text(&s, |value, v| value.deserialize_seq(v), v),
            Cell::Null => v.visit_seq(CellSeq(Vec::new().into_iter())),
            other => Err(DeError(format!("expected array, got {other:?}"))),
        }
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        self.deserialize_byte_buf(v)
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        match self.0 {
            Cell::Bytes(b) => v.visit_byte_buf(b),
            other => CellDe(other).deserialize_seq(v),
        }
    }

    fn deserialize_map<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        match self.0 {
            Cell::Json(j) => j.deserialize_map(v).map_err(json_err),
            Cell::Text(s) => Self::json_text(&s, |value, v| value.deserialize_map(v), v),
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
            Cell::Json(j) => j.deserialize_struct(name, fields, v).map_err(json_err),
            Cell::Text(s) => {
                Self::json_text(&s, |value, v| value.deserialize_struct(name, fields, v), v)
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
        name: &'static str,
        variants: &'static [&'static str],
        v: V,
    ) -> Result<V::Value, DeError> {
        match self.0 {
            Cell::Text(s) => v.visit_enum(s.into_deserializer()),
            Cell::Json(j) => j.deserialize_enum(name, variants, v).map_err(json_err),
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
        i128 u128 char unit_struct tuple_struct identifier
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    fn cols(names: &[&str]) -> Arc<[String]> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[derive(Deserialize, Debug)]
    struct Item {
        id: Uuid,
        name: Option<String>,
        enabled: bool,
        tags: Vec<String>,
        at: DateTime<Utc>,
        meta: crate::types::Json<serde_json::Value>,
    }

    #[test]
    fn struct_columns_map_by_name_and_types_round_trip() {
        let id = Uuid::from_u128(0x1234_5678_9abc_def0_1234_5678_9abc_def0);
        let at = DateTime::parse_from_rfc3339("2026-11-11T01:02:03.456Z")
            .unwrap()
            .with_timezone(&Utc);
        let item: Item = from_row(
            &cols(&["enabled", "id", "name", "tags", "at", "meta", "extra"]),
            vec![
                Cell::Bool(true),
                Cell::Text(id.to_string()),
                Cell::Null,
                Cell::Array(vec![Cell::Text("a".into()), Cell::Text("b".into())]),
                time(at),
                Cell::Json(serde_json::json!({"k": [1, 2]})),
                Cell::Int(7),
            ],
        )
        .unwrap();
        assert_eq!(item.id, id);
        assert_eq!(item.name, None);
        assert!(item.enabled);
        assert_eq!(item.tags, ["a", "b"]);
        assert_eq!(item.at, at);
        assert_eq!(item.meta.0, serde_json::json!({"k": [1, 2]}));
    }

    #[test]
    fn tuples_and_scalars_map_by_position() {
        let (n, s, f): (i64, String, f64) = from_row(
            &cols(&["count", "?column?", "avg"]),
            vec![Cell::Int(3), Cell::Text("x".into()), Cell::Real(1.5)],
        )
        .unwrap();
        assert_eq!((n, s.as_str(), f), (3, "x", 1.5));
        let v: Option<i32> = from_scalar(vec![Cell::Null]).unwrap();
        assert_eq!(v, None);
        let b: Vec<u8> = from_scalar(vec![Cell::Bytes(vec![1, 2, 3])]).unwrap();
        assert_eq!(b, [1, 2, 3]);
    }

    #[test]
    fn decode_errors_do_not_panic() {
        let r: Result<i64, _> = from_scalar(vec![Cell::Text("nope".into())]);
        assert!(r.is_err());
    }
}
