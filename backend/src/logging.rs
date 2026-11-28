//! 結構化 JSON 日誌的訂閱者設定；正式程序與日誌機敏資料測試共用，確保測試的是實際輸出的格式。

use tracing_subscriber::{
    EnvFilter,
    fmt::{
        SubscriberBuilder,
        format::{FmtSpan, Format, Json, JsonFields},
    },
};

use crate::config::ConfigError;

/// 輸出格式（`LOG_FORMAT`）：正式環境維持 `json`（S-09.3）；本機除錯可選 `compact` 或 `pretty`。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogFormat {
    Json,
    Compact,
    Pretty,
}

impl LogFormat {
    pub fn parse(raw: Option<&str>) -> Result<Self, ConfigError> {
        let Some(value) = raw.map(str::trim).filter(|v| !v.is_empty()) else {
            return Ok(Self::Json);
        };
        match value.to_ascii_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "compact" => Ok(Self::Compact),
            "pretty" => Ok(Self::Pretty),
            _ => Err(ConfigError(
                "LOG_FORMAT must be one of json, compact, or pretty".into(),
            )),
        }
    }
}

/// 解析 `LOG_SPAN_EVENTS`（預設 `none`）：除錯效能瓶頸時可設 `close`／`active`／`full` 輸出 span 耗時事件。
pub fn parse_span_events(raw: Option<&str>) -> Result<FmtSpan, ConfigError> {
    let Some(value) = raw.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(FmtSpan::NONE);
    };
    match value.to_ascii_lowercase().as_str() {
        "none" => Ok(FmtSpan::NONE),
        "new" => Ok(FmtSpan::NEW),
        "close" => Ok(FmtSpan::CLOSE),
        "active" => Ok(FmtSpan::ACTIVE),
        "full" => Ok(FmtSpan::FULL),
        _ => Err(ConfigError(
            "LOG_SPAN_EVENTS must be one of none, new, close, active, or full".into(),
        )),
    }
}

/// JSON 格式的 `fmt` 訂閱者；呼叫端再接上 filter／writer 並 `init()`。
pub fn json_subscriber() -> SubscriberBuilder<JsonFields, Format<Json>> {
    #[cfg(test)]
    ensure_test_interest_subscriber();
    tracing_subscriber::fmt().json()
}

/// 僅在單元測試（`cargo test --lib`）安裝一個無輸出的全域訂閱者，對所有 callsite 回傳
/// `Interest::sometimes()` 與 `TRACE` 上限。
///
/// 若沒有全域預設訂閱者，平行執行的無訂閱者測試可能先以 `NoSubscriber` 註冊 callsite 並快取
/// `Interest::never()`，與使用 `tracing::subscriber::set_default` 的測試競態導致事件被靜默丟棄。
#[cfg(test)]
fn ensure_test_interest_subscriber() {
    use std::sync::Once;
    use tracing::{
        Event, Id, Metadata, Subscriber,
        level_filters::LevelFilter,
        span::{Attributes, Record},
        subscriber::Interest,
    };

    struct TestInterestSubscriber;

    impl Subscriber for TestInterestSubscriber {
        fn register_callsite(&self, _metadata: &'static Metadata<'static>) -> Interest {
            Interest::sometimes()
        }

        fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
            false
        }

        fn max_level_hint(&self) -> Option<LevelFilter> {
            Some(LevelFilter::TRACE)
        }

        fn new_span(&self, _span: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }

        fn record(&self, _span: &Id, _values: &Record<'_>) {}
        fn record_follows_from(&self, _span: &Id, _follows: &Id) {}
        fn event(&self, _event: &Event<'_>) {}
        fn enter(&self, _span: &Id) {}
        fn exit(&self, _span: &Id) {}
    }

    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        let _ = tracing::subscriber::set_global_default(TestInterestSubscriber);
    });
}

/// 依環境變數（`RUST_LOG`、`LOG_FORMAT`、`LOG_SPAN_EVENTS`）初始化全域日誌訂閱者。
/// 不論選擇哪種格式，`genai` 目標一律由 `env_filter` 關閉（S-02.2）。
pub fn init_from_env() -> Result<(), ConfigError> {
    let format = LogFormat::parse(std::env::var("LOG_FORMAT").ok().as_deref())?;
    let span_events = parse_span_events(std::env::var("LOG_SPAN_EVENTS").ok().as_deref())?;
    let filter = env_filter(std::env::var("RUST_LOG").ok().as_deref());
    match format {
        LogFormat::Json => json_subscriber()
            .with_span_events(span_events)
            .with_env_filter(filter)
            .init(),
        LogFormat::Compact => tracing_subscriber::fmt()
            .compact()
            .with_span_events(span_events)
            .with_env_filter(filter)
            .init(),
        LogFormat::Pretty => tracing_subscriber::fmt()
            .pretty()
            .with_span_events(span_events)
            .with_env_filter(filter)
            .init(),
    }
    Ok(())
}

/// 安裝 panic hook：在程序或背景工作發生 panic 時寫出一筆結構化 `tracing::error!`。
/// 只記原始碼位置（檔案、行號、欄號）與執行緒名稱，**不記 panic 訊息內容**，避免意外把敏感字串寫入日誌（S-02.2）。
/// 以 `Once` 保護，重複呼叫不會疊加多層 hook。
pub fn install_panic_hook() {
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    INSTALLED.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            log_panic_info(info);
            previous(info);
        }));
    });
}

fn log_panic_info(info: &std::panic::PanicHookInfo<'_>) {
    let thread = std::thread::current();
    let thread_name = thread.name().unwrap_or("<unnamed>");
    if let Some(location) = info.location() {
        tracing::error!(
            thread = thread_name,
            file = location.file(),
            line = location.line(),
            column = location.column(),
            "panic occurred"
        );
    } else {
        tracing::error!(thread = thread_name, "panic occurred");
    }
}

/// 由 `RUST_LOG` 風格的設定建立 filter；沒有設定或格式錯誤時用 `info`。
///
/// genai 在自己的錯誤路徑會把錯誤（可能帶請求內容與回應本文，也就是學生對話原文）寫進日誌，
/// 所以不論設定怎麼寫，`genai` 開頭的目標一律關閉（S-02.2、F-40.2）。
pub fn env_filter(spec: Option<&str>) -> EnvFilter {
    let mut directives: Vec<&str> = spec
        .unwrap_or("info")
        .split(',')
        .map(str::trim)
        .filter(|d| !d.is_empty() && !targets_genai(d))
        .collect();
    directives.push("genai=off");
    EnvFilter::try_new(directives.join(",")).unwrap_or_else(|_| EnvFilter::new("info,genai=off"))
}

fn targets_genai(directive: &str) -> bool {
    directive
        .split('=')
        .next()
        .is_some_and(|target| target.trim_start().starts_with("genai"))
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        sync::{Arc, Mutex},
    };

    use tracing_subscriber::fmt::MakeWriter;

    use super::*;

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Sink {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Captured {
        type Writer = Sink;
        fn make_writer(&'a self) -> Sink {
            Sink(self.0.clone())
        }
    }

    /// genai 在自己的錯誤路徑會 `tracing::error!("{err}")`，而那個錯誤可能帶請求內容與回應本文
    /// （學生對話原文）。就算 `RUST_LOG` 明確要求 genai 的日誌，也不能輸出。
    #[test]
    fn genai_events_are_dropped_even_when_rust_log_asks_for_them() {
        // （設定，app 事件是否應該出現）：只開 genai 的設定本來就不會有 app 事件
        for (spec, app_visible) in [
            (None, true),
            (Some("trace"), true),
            (Some("debug,genai=debug"), true),
            (Some("trace,not valid[[="), true),
            (Some("genai=trace"), false),
            (Some("genai::adapter::gemini=trace"), false),
        ] {
            let captured = Captured::default();
            let subscriber = json_subscriber()
                .with_env_filter(env_filter(spec))
                .with_writer(captured.clone())
                .finish();
            tracing::subscriber::with_default(subscriber, || {
                tracing::error!(target: "genai::adapter::gemini", "GENAI-LEAK-SENTINEL");
                tracing::error!(target: "socrates_chat_backend::reply", "APP-EVENT-SENTINEL");
            });
            let out = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
            assert_eq!(
                out.contains("APP-EVENT-SENTINEL"),
                app_visible,
                "app events: {spec:?}"
            );
            assert!(
                !out.contains("GENAI-LEAK-SENTINEL"),
                "genai logs leak: {spec:?}"
            );
        }
    }

    #[test]
    fn log_format_and_span_events_parse_valid_and_reject_invalid_values() {
        assert_eq!(LogFormat::parse(None).unwrap(), LogFormat::Json);
        assert_eq!(LogFormat::parse(Some("")).unwrap(), LogFormat::Json);
        assert_eq!(LogFormat::parse(Some(" JSON ")).unwrap(), LogFormat::Json);
        assert_eq!(
            LogFormat::parse(Some("compact")).unwrap(),
            LogFormat::Compact
        );
        assert_eq!(LogFormat::parse(Some("PRETTY")).unwrap(), LogFormat::Pretty);
        assert!(LogFormat::parse(Some("xml")).is_err());

        for valid in [
            None,
            Some(""),
            Some("none"),
            Some("new"),
            Some("CLOSE"),
            Some("active"),
            Some("full"),
        ] {
            assert!(parse_span_events(valid).is_ok(), "{valid:?}");
        }
        assert!(parse_span_events(Some("invalid")).is_err());
    }

    #[test]
    fn span_close_events_emit_busy_and_idle_timing_for_bottleneck_analysis() {
        let captured = Captured::default();
        let subscriber = json_subscriber()
            .with_span_events(parse_span_events(Some("close")).unwrap())
            .with_env_filter(env_filter(Some("info")))
            .with_writer(captured.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("component", component = "ai", operation = "chat_reply");
            let _enter = span.enter();
        });
        let out = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
        let entry: serde_json::Value =
            serde_json::from_str(out.lines().next().expect("span close log")).unwrap();
        assert_eq!(entry["fields"]["message"], "close");
        assert!(entry["fields"]["time.busy"].is_string());
        assert!(entry["fields"]["time.idle"].is_string());
        assert_eq!(entry["span"]["component"], "ai");
        assert_eq!(entry["span"]["operation"], "chat_reply");
    }
}
