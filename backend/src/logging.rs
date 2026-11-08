//! 結構化 JSON 日誌的訂閱者設定；正式程序與日誌機敏資料測試共用，確保測試的是實際輸出的格式。

use tracing_subscriber::fmt::{
    SubscriberBuilder,
    format::{Format, Json, JsonFields},
};

/// JSON 格式的 `fmt` 訂閱者；呼叫端再接上 filter／writer 並 `init()`。
pub fn json_subscriber() -> SubscriberBuilder<JsonFields, Format<Json>> {
    tracing_subscriber::fmt().json()
}
