//! 結構化 JSON 日誌的訂閱者設定；正式程序與日誌機敏資料測試共用，確保測試的是實際輸出的格式。

use tracing_subscriber::{
    EnvFilter,
    fmt::{
        SubscriberBuilder,
        format::{Format, Json, JsonFields},
    },
};

/// JSON 格式的 `fmt` 訂閱者；呼叫端再接上 filter／writer 並 `init()`。
pub fn json_subscriber() -> SubscriberBuilder<JsonFields, Format<Json>> {
    tracing_subscriber::fmt().json()
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
}
