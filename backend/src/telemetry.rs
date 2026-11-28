//! 分散式追蹤上下文（W3C Trace Context）與元件子 Span 工具。
//!
//! 每個進入的 HTTP 請求會解析或建立 `TraceContext`（`trace_id`、`span_id`、`parent_span_id`），
//! 以 `traceparent` 標頭回應並向下游內部服務傳遞；背景工作與主要元件（AI、資料庫探測、周邊服務探測、
//! 外部書目、客訴寄信）則建立子 span，形成 `Trace -> Span -> Event` 的樹狀結構。
//! 不記錄任何原始 URL、訊息原文或敏感個資（S-02.2）。

use std::future::Future;

use tracing::Instrument;
use uuid::Uuid;

pub const TRACEPARENT_HEADER: &str = "traceparent";
pub const REQUEST_ID_HEADER: &str = "x-request-id";

const ZERO_TRACE_ID: &str = "00000000000000000000000000000000";
const ZERO_SPAN_ID: &str = "0000000000000000";
const DEFAULT_TRACE_FLAGS: u8 = 0x01;

/// W3C Trace Context（版本 `00`）的執行期表示。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceContext {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub trace_flags: u8,
}

impl TraceContext {
    /// 建立新的根追蹤上下文（沒有上游 `traceparent` 或格式無效時使用）。
    pub fn new_root() -> Self {
        Self {
            trace_id: generate_trace_id(),
            span_id: generate_span_id(),
            parent_span_id: None,
            trace_flags: DEFAULT_TRACE_FLAGS,
        }
    }

    /// 依 W3C Trace Context 規格解析上游 `traceparent`；缺少或格式無效時產生新的根追蹤。
    pub fn from_traceparent(raw: Option<&str>) -> Self {
        let Some(parsed) = raw.and_then(parse_traceparent) else {
            return Self::new_root();
        };
        Self {
            trace_id: parsed.trace_id,
            span_id: generate_span_id(),
            parent_span_id: Some(parsed.parent_id),
            trace_flags: parsed.trace_flags,
        }
    }

    /// 產生同一條 Trace 下的子 span 上下文。
    pub fn child_span(&self) -> Self {
        Self {
            trace_id: self.trace_id.clone(),
            span_id: generate_span_id(),
            parent_span_id: Some(self.span_id.clone()),
            trace_flags: self.trace_flags,
        }
    }

    /// 序列化為標準 W3C `traceparent` 標頭值（`00-<trace_id>-<span_id>-<flags>`）。
    pub fn traceparent(&self) -> String {
        format!(
            "00-{}-{}-{:02x}",
            self.trace_id, self.span_id, self.trace_flags
        )
    }
}

struct ParsedTraceparent {
    trace_id: String,
    parent_id: String,
    trace_flags: u8,
}

fn is_lower_hex(s: &str, expected_len: usize) -> bool {
    s.len() == expected_len
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn parse_traceparent(raw: &str) -> Option<ParsedTraceparent> {
    let trimmed = raw.trim();
    let mut parts = trimmed.split('-');
    let version = parts.next()?;
    let trace_id = parts.next()?;
    let parent_id = parts.next()?;
    let flags = parts.next()?;

    if !is_lower_hex(version, 2) || version == "ff" {
        return None;
    }
    // W3C Trace Context：version 00 必須剛好 4 段；未來版本（01..fe）允許尾端多出擴充段
    if version == "00" && parts.next().is_some() {
        return None;
    }
    if !is_lower_hex(trace_id, 32) || trace_id == ZERO_TRACE_ID {
        return None;
    }
    if !is_lower_hex(parent_id, 16) || parent_id == ZERO_SPAN_ID {
        return None;
    }
    if !is_lower_hex(flags, 2) {
        return None;
    }
    let trace_flags = u8::from_str_radix(flags, 16).ok()?;
    Some(ParsedTraceparent {
        trace_id: trace_id.to_owned(),
        parent_id: parent_id.to_owned(),
        trace_flags,
    })
}

fn generate_trace_id() -> String {
    loop {
        let value = Uuid::new_v4().as_u128();
        if value != 0 {
            return format!("{value:032x}");
        }
    }
}

fn generate_span_id() -> String {
    loop {
        let value = (Uuid::new_v4().as_u128() >> 64) as u64;
        if value != 0 {
            return format!("{value:016x}");
        }
    }
}

tokio::task_local! {
    static TRACE_CONTEXT: TraceContext;
}

/// 在目前 task 上綁定 `TraceContext`。
pub fn scope_trace<F: Future>(ctx: TraceContext, future: F) -> impl Future<Output = F::Output> {
    TRACE_CONTEXT.scope(ctx, future)
}

/// 取得目前的 `TraceContext`（不在請求或元件追蹤範圍內時為 `None`）。
pub fn current_trace() -> Option<TraceContext> {
    TRACE_CONTEXT.try_with(Clone::clone).ok()
}

/// 取得目前的 W3C `traceparent` 標頭值（供跨服務傳遞）。
pub fn current_traceparent() -> Option<String> {
    current_trace().map(|ctx| ctx.traceparent())
}

/// 讓 `tokio::spawn` 出去的背景工作同時繼承 `TraceContext`、`REQUEST_ID` 與目前 `tracing::Span`。
pub fn inherit_trace<F: Future>(future: F) -> impl Future<Output = F::Output> {
    // 先放到堆上：否則下面各個 `match` 分支會各自內嵌一份 `F`，在未最佳化的建置裡讓 future 大小倍增而撐爆堆疊。
    let future = Box::pin(future);
    let trace = current_trace();
    let request_id = crate::error::REQUEST_ID.try_with(Clone::clone).ok();
    let span = tracing::Span::current();
    async move {
        let with_req = async move {
            match request_id {
                Some(id) => crate::error::REQUEST_ID.scope(id, future).await,
                None => future.await,
            }
        };
        match trace {
            Some(ctx) => TRACE_CONTEXT.scope(ctx, with_req).await,
            None => with_req.await,
        }
    }
    .instrument(span)
}

/// 在元件子 span 內執行非同步作業，建立 `Trace -> Span -> Event` 樹狀結構。
///
/// 子 span 會帶上同一條 `trace_id`、`parent_span_id` 與新的 `span_id`，並保留外層請求的
/// `request_id` 與 `route`（若存在），確保內部日誌事件與既有監控欄位完全相容。
pub fn in_component_span<F: Future>(
    component: &'static str,
    operation: &'static str,
    future: F,
) -> impl Future<Output = F::Output> {
    in_component_span_with_target(component, operation, None::<&'static str>, future)
}

/// 同 [`in_component_span`]，另帶受控的目標識別名稱（例如監控服務名稱，**絕不可**傳入個資或原始 URL）。
pub fn in_component_span_with_target<'a, F: Future>(
    component: &'static str,
    operation: &'static str,
    target: Option<&'a str>,
    future: F,
) -> impl Future<Output = F::Output> + use<'a, F> {
    // 先放到堆上，避免巢狀包裝在未最佳化的建置裡把 future 大小層層疊加而撐爆堆疊。
    let future = Box::pin(future);
    async move {
        let child = current_trace()
            .map(|parent| parent.child_span())
            .unwrap_or_else(TraceContext::new_root);
        let request_id = crate::error::REQUEST_ID.try_with(Clone::clone).ok();
        let route = crate::diagnostics::current_route();
        let span = tracing::info_span!(
            "component",
            component,
            operation,
            target,
            trace_id = %child.trace_id,
            span_id = %child.span_id,
            parent_span_id = child.parent_span_id.as_deref(),
            request_id = request_id.as_deref(),
            route = route.as_deref(),
        );
        scope_trace(child, future).instrument(span).await
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        sync::{Arc, Mutex},
    };

    use serde_json::Value;
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

    #[test]
    fn parses_valid_w3c_traceparent_and_creates_child_span() {
        let upstream_trace = "4bf92f3577b34da6a3ce929d0e0e4736";
        let upstream_parent = "00f067aa0ba902b7";
        let header = format!("00-{upstream_trace}-{upstream_parent}-01");
        let ctx = TraceContext::from_traceparent(Some(&header));

        assert_eq!(ctx.trace_id, upstream_trace);
        assert_eq!(ctx.parent_span_id.as_deref(), Some(upstream_parent));
        assert_eq!(ctx.trace_flags, 0x01);
        assert_eq!(ctx.span_id.len(), 16);
        assert_ne!(ctx.span_id, upstream_parent);
        assert_ne!(ctx.span_id, ZERO_SPAN_ID);
        assert_eq!(
            ctx.traceparent(),
            format!("00-{upstream_trace}-{}-01", ctx.span_id)
        );
    }

    #[test]
    fn supports_future_traceparent_versions_with_extra_fields() {
        let upstream_trace = "4bf92f3577b34da6a3ce929d0e0e4736";
        let upstream_parent = "00f067aa0ba902b7";
        let header = format!("01-{upstream_trace}-{upstream_parent}-00-extra-field");
        let ctx = TraceContext::from_traceparent(Some(&header));

        assert_eq!(ctx.trace_id, upstream_trace);
        assert_eq!(ctx.parent_span_id.as_deref(), Some(upstream_parent));
        assert_eq!(ctx.trace_flags, 0x00);
    }

    #[test]
    fn rejects_malformed_or_zero_traceparent_headers() {
        let invalid = [
            None,
            Some(""),
            Some("not-a-traceparent"),
            // version ff is reserved/invalid per W3C spec
            Some("ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"),
            // version 00 does not allow extra segments
            Some("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-extra"),
            // uppercase hex is invalid per W3C spec
            Some("00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01"),
            // all-zero trace-id
            Some("00-00000000000000000000000000000000-00f067aa0ba902b7-01"),
            // all-zero parent-id
            Some("00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01"),
            // wrong lengths
            Some("00-4bf92f35-00f067aa0ba902b7-01"),
            Some("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa-01"),
            Some("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-1"),
        ];
        for raw in invalid {
            let ctx = TraceContext::from_traceparent(raw);
            assert!(
                ctx.parent_span_id.is_none(),
                "expected fallback root for {raw:?}"
            );
            assert!(is_lower_hex(&ctx.trace_id, 32));
            assert_ne!(ctx.trace_id, ZERO_TRACE_ID);
            assert!(is_lower_hex(&ctx.span_id, 16));
            assert_ne!(ctx.span_id, ZERO_SPAN_ID);
        }
    }

    #[tokio::test]
    async fn component_spans_form_nested_tree_and_preserve_request_fields() {
        let captured = Captured::default();
        let subscriber = crate::logging::json_subscriber()
            .with_env_filter(crate::logging::env_filter(Some("info")))
            .with_writer(captured.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let root_ctx = TraceContext::new_root();
        let req_uuid = Uuid::new_v4();
        let req_id = req_uuid.to_string();
        let route = "/api/conversations/{id}/messages";
        let root_span = tracing::info_span!(
            "http_request",
            request_id = %req_id,
            trace_id = %root_ctx.trace_id,
            span_id = %root_ctx.span_id,
            method = "POST",
            route = route
        );

        let root_trace_id = root_ctx.trace_id.clone();
        let root_span_id = root_ctx.span_id.clone();

        crate::error::REQUEST_ID
            .scope(
                req_id.clone(),
                crate::diagnostics::scope_request(
                    req_uuid,
                    route,
                    scope_trace(
                        root_ctx,
                        async {
                            in_component_span("background_job", "reply", async {
                                let mid = current_trace().expect("mid trace");
                                assert_eq!(mid.trace_id, root_trace_id);
                                assert_eq!(
                                    mid.parent_span_id.as_deref(),
                                    Some(root_span_id.as_str())
                                );
                                in_component_span("ai", "reply_generation", async {
                                    let leaf = current_trace().expect("leaf trace");
                                    assert_eq!(leaf.trace_id, root_trace_id);
                                    assert_eq!(
                                        leaf.parent_span_id.as_deref(),
                                        Some(mid.span_id.as_str())
                                    );
                                    tracing::warn!("nested span event");
                                })
                                .await;
                            })
                            .await;
                        }
                        .instrument(root_span),
                    ),
                ),
            )
            .await;

        let out = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
        let entry: Value = serde_json::from_str(out.lines().next().expect("log line")).unwrap();
        assert_eq!(entry["fields"]["message"], "nested span event");
        assert_eq!(entry["span"]["component"], "ai");
        assert_eq!(entry["span"]["operation"], "reply_generation");
        assert_eq!(entry["span"]["trace_id"], root_trace_id);
        assert_eq!(entry["span"]["request_id"], req_id);
        assert_eq!(entry["span"]["route"], route);

        let spans = entry["spans"].as_array().expect("span tree");
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0]["name"], "http_request");
        assert_eq!(spans[1]["name"], "component");
        assert_eq!(spans[1]["component"], "background_job");
        assert_eq!(spans[2]["name"], "component");
        assert_eq!(spans[2]["component"], "ai");
    }

    /// 包裝後的 future 不能把內層 future 內嵌進自己：未最佳化的建置裡，內嵌會讓大型背景工作
    /// 在 `tokio::spawn` 之前先吃掉呼叫端的堆疊（曾讓 `POST /api/topics` 的測試 stack overflow）。
    #[test]
    fn wrappers_do_not_inline_the_wrapped_future() {
        let large = async move {
            let buffer = [0u8; 65536];
            tokio::task::yield_now().await;
            std::hint::black_box(buffer.len())
        };
        assert!(std::mem::size_of_val(&large) >= 65536);
        let wrapped = crate::diagnostics::inherit_request(in_component_span(
            "background_job",
            "large",
            large,
        ));
        assert!(
            std::mem::size_of_val(&wrapped) < 4096,
            "wrapped future size: {}",
            std::mem::size_of_val(&wrapped)
        );
    }

    #[tokio::test]
    async fn inherit_trace_propagates_context_across_spawn() {
        let root_ctx = TraceContext::new_root();
        let expected_trace = root_ctx.trace_id.clone();
        let req_id = Uuid::new_v4().to_string();
        let expected_req = req_id.clone();

        let (seen_trace, seen_req) = crate::error::REQUEST_ID
            .scope(
                req_id,
                scope_trace(root_ctx, async {
                    tokio::spawn(inherit_trace(async {
                        let t = current_trace().map(|c| c.trace_id);
                        let r = crate::error::REQUEST_ID.try_with(Clone::clone).ok();
                        (t, r)
                    }))
                    .await
                    .unwrap()
                }),
            )
            .await;

        assert_eq!(seen_trace.as_deref(), Some(expected_trace.as_str()));
        assert_eq!(seen_req.as_deref(), Some(expected_req.as_str()));
    }

    #[tokio::test]
    async fn component_span_with_target_and_inherit_request_preserve_route_across_spawn() {
        let captured = Captured::default();
        let subscriber = crate::logging::json_subscriber()
            .with_env_filter(crate::logging::env_filter(Some("info")))
            .with_writer(captured.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let root_ctx = TraceContext::new_root();
        let expected_trace = root_ctx.trace_id.clone();
        let req_uuid = Uuid::new_v4();
        let req_id = req_uuid.to_string();
        let route = "/api/admin/services/check";

        crate::error::REQUEST_ID
            .scope(
                req_id.clone(),
                crate::diagnostics::scope_request(
                    req_uuid,
                    route,
                    scope_trace(root_ctx, async {
                        tokio::spawn(crate::diagnostics::inherit_request(
                            in_component_span_with_target(
                                "service_probe",
                                "health_check",
                                Some("cascade-tts"),
                                async {
                                    tracing::info!("probe completed");
                                },
                            ),
                        ))
                        .await
                        .unwrap();
                    }),
                ),
            )
            .await;

        let out = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
        let entry: Value = serde_json::from_str(out.lines().next().expect("log line")).unwrap();
        assert_eq!(entry["span"]["component"], "service_probe");
        assert_eq!(entry["span"]["operation"], "health_check");
        assert_eq!(entry["span"]["target"], "cascade-tts");
        assert_eq!(entry["span"]["trace_id"], expected_trace);
        assert_eq!(entry["span"]["request_id"], req_id);
        assert_eq!(entry["span"]["route"], route);
    }
}
