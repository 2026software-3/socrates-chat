//! 對外 HTTP 呼叫（AI、語音、Google OAuth）：API 形狀仿照後端用到的 reqwest 子集，底層是 Workers 的 `fetch`。
//!
//! 所有 future 與串流都包成 `Send`（Workers 是單執行緒，見 `worker::send`），才能放進 axum handler 與 `async_trait`。

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use futures_util::{Stream, StreamExt};
use serde::{Serialize, de::DeserializeOwned};
use worker::send::{SendFuture, SendWrapper};

#[derive(Debug, Clone)]
pub struct Error {
    timeout: bool,
}

impl Error {
    fn failed() -> Self {
        Error { timeout: false }
    }

    pub fn is_timeout(&self) -> bool {
        self.timeout
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 不帶網址或回應內容（可能含金鑰或個資）
        f.write_str(if self.timeout {
            "HTTP request timed out"
        } else {
            "HTTP request failed"
        })
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Default)]
pub struct Client {
    timeout: Option<Duration>,
    manual_redirect: bool,
}

impl Client {
    pub fn new() -> Self {
        Self::default()
    }

    /// 這個用戶端的每個請求都套用的逾時。
    pub fn with_timeout(mut self, d: Duration) -> Self {
        self.timeout = Some(d);
        self
    }

    /// 不跟隨重新導向（OAuth 端點）。
    pub fn without_redirects(mut self) -> Self {
        self.manual_redirect = true;
        self
    }

    pub fn post(&self, url: impl Into<String>) -> RequestBuilder {
        self.request(Method::POST, url)
    }

    pub fn get(&self, url: impl Into<String>) -> RequestBuilder {
        self.request(Method::GET, url)
    }

    pub fn request(&self, method: Method, url: impl Into<String>) -> RequestBuilder {
        RequestBuilder {
            method,
            url: url.into(),
            headers: Vec::new(),
            body: Body::Empty,
            timeout: self.timeout,
            manual_redirect: self.manual_redirect,
        }
    }
}

enum Body {
    Empty,
    Text(String),
    Multipart(multipart::Form),
}

pub struct RequestBuilder {
    method: Method,
    url: String,
    headers: Vec<(String, String)>,
    body: Body,
    timeout: Option<Duration>,
    manual_redirect: bool,
}

impl RequestBuilder {
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn bearer_auth(self, token: impl std::fmt::Display) -> Self {
        self.header("authorization", format!("Bearer {token}"))
    }

    pub fn basic_auth(self, user: &str, password: &str) -> Self {
        use base64::Engine;
        let raw = format!("{user}:{password}");
        self.header(
            "authorization",
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(raw)
            ),
        )
    }

    pub fn json<T: Serialize + ?Sized>(mut self, body: &T) -> Self {
        if let Ok(text) = serde_json::to_string(body) {
            self.body = Body::Text(text);
            self.headers
                .push(("content-type".into(), "application/json".into()));
        }
        self
    }

    pub fn form<T: Serialize + ?Sized>(mut self, body: &T) -> Self {
        if let Ok(text) = serde_urlencoded::to_string(body) {
            self.body = Body::Text(text);
            self.headers.push((
                "content-type".into(),
                "application/x-www-form-urlencoded".into(),
            ));
        }
        self
    }

    pub fn multipart(mut self, form: multipart::Form) -> Self {
        self.body = Body::Multipart(form);
        self
    }

    pub fn timeout(mut self, d: Duration) -> Self {
        self.timeout = Some(d);
        self
    }

    fn build(self) -> Result<worker::Request, Error> {
        let headers = worker::Headers::new();
        for (name, value) in &self.headers {
            headers.set(name, value).map_err(|_| Error::failed())?;
        }
        let mut init = worker::RequestInit::new();
        init.with_method(match self.method {
            Method::POST => worker::Method::Post,
            Method::PUT => worker::Method::Put,
            Method::DELETE => worker::Method::Delete,
            Method::PATCH => worker::Method::Patch,
            _ => worker::Method::Get,
        });
        init.with_headers(headers);
        if self.manual_redirect {
            init.with_redirect(worker::RequestRedirect::Manual);
        }
        match self.body {
            Body::Empty => {}
            Body::Text(text) => {
                init.with_body(Some(text.into()));
            }
            Body::Multipart(form) => {
                let data = web_sys::FormData::new().map_err(|_| Error::failed())?;
                for (name, part) in form.parts {
                    match part {
                        multipart::PartData::Text(value) => data
                            .append_with_str(&name, &value)
                            .map_err(|_| Error::failed())?,
                        multipart::PartData::Bytes {
                            bytes,
                            file_name,
                            mime,
                        } => {
                            let array =
                                js_sys::Array::of1(&js_sys::Uint8Array::from(bytes.as_slice()));
                            let options = web_sys::BlobPropertyBag::new();
                            if let Some(mime) = &mime {
                                options.set_type(mime);
                            }
                            let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(
                                &array, &options,
                            )
                            .map_err(|_| Error::failed())?;
                            data.append_with_blob_and_filename(&name, &blob, &file_name)
                                .map_err(|_| Error::failed())?
                        }
                    }
                }
                init.with_body(Some(data.into()));
            }
        }
        worker::Request::new_with_init(&self.url, &init).map_err(|_| Error::failed())
    }

    pub fn send(self) -> impl Future<Output = Result<Response, Error>> + Send {
        let timeout = self.timeout;
        let built = self.build().map(SendWrapper::new);
        SendFuture::new(async move {
            let request = built?.0;
            let fetch = worker::Fetch::Request(request);
            let response = match timeout {
                Some(d) => crate::rt::timeout(d, fetch.send())
                    .await
                    .map_err(|_| Error { timeout: true })?,
                None => fetch.send().await,
            }
            .map_err(|_| Error::failed())?;
            Ok(Response::from_worker(response))
        })
    }
}

pub struct Response {
    status: StatusCode,
    headers: HeaderMap,
    inner: SendWrapper<worker::Response>,
}

impl Response {
    fn from_worker(inner: worker::Response) -> Self {
        let status = StatusCode::from_u16(inner.status_code()).unwrap_or(StatusCode::BAD_GATEWAY);
        let mut headers = HeaderMap::new();
        for (name, value) in inner.headers().entries() {
            if let (Ok(n), Ok(v)) = (
                HeaderName::from_bytes(name.as_bytes()),
                HeaderValue::from_str(&value),
            ) {
                headers.append(n, v);
            }
        }
        Self {
            status,
            headers,
            inner: SendWrapper::new(inner),
        }
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    pub fn text(mut self) -> impl Future<Output = Result<String, Error>> + Send {
        SendFuture::new(async move { self.inner.text().await.map_err(|_| Error::failed()) })
    }

    pub fn bytes(mut self) -> impl Future<Output = Result<bytes::Bytes, Error>> + Send {
        SendFuture::new(async move {
            self.inner
                .bytes()
                .await
                .map(bytes::Bytes::from)
                .map_err(|_| Error::failed())
        })
    }

    pub fn json<T: DeserializeOwned>(self) -> impl Future<Output = Result<T, Error>> + Send {
        let text = self.text();
        async move { serde_json::from_str(&text.await?).map_err(|_| Error::failed()) }
    }

    /// 逐段讀取回應本文（AI 串流）。
    pub fn bytes_stream(
        mut self,
    ) -> impl Stream<Item = Result<bytes::Bytes, Error>> + Send + Unpin {
        let stream = self.inner.stream().map_err(|_| Error::failed());
        let inner: Pin<Box<dyn Stream<Item = Result<bytes::Bytes, Error>>>> = match stream {
            Ok(s) => Box::pin(s.map(|r| r.map(bytes::Bytes::from).map_err(|_| Error::failed()))),
            Err(e) => Box::pin(futures_util::stream::once(async move { Err(e) })),
        };
        SendStream(inner)
    }
}

impl Response {
    /// 由 JS 持續讀取回應本文、累積在 JS 端的緩衝區；Rust 每次取走目前累積的全部位元組（AI 串流）。
    ///
    /// 用 `bytes_stream` 時每個網路片段都是一次 JS→wasm 的 Promise／喚醒／複製，AI 串流一則回覆上百個片段，
    /// 是 Workers 免費方案 CPU 用量的大宗。JS 自己讀不跨 wasm，便宜得多；Rust 可以隔一段時間才來取一次。
    pub fn into_body_buffer(self) -> Result<BodyBuffer, Error> {
        match self.inner.body() {
            worker::ResponseBody::Stream(stream) => Ok(BodyBuffer(SendWrapper::new(
                JsBodyBuffer::new(stream.as_ref()),
            ))),
            _ => Err(Error::failed()),
        }
    }
}

#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
export class BodyBuffer {
  constructor(stream) {
    this.reader = stream.getReader();
    this.chunks = [];
    this.state = 0; // 0 讀取中、1 正常結束、2 失敗
    this.waiter = null;
    this.pump();
  }
  async pump() {
    try {
      for (;;) {
        const { value, done } = await this.reader.read();
        if (done) { this.state = 1; break; }
        if (value && value.byteLength) { this.chunks.push(value); this.wake(); }
      }
    } catch (_) {
      this.state = 2;
    }
    this.wake();
  }
  wake() {
    const w = this.waiter;
    if (w) { this.waiter = null; w(); }
  }
  ready() {
    if (this.chunks.length || this.state) return Promise.resolve();
    return new Promise((r) => { this.waiter = r; });
  }
  take() {
    const c = this.chunks;
    if (!c.length) return undefined;
    this.chunks = [];
    if (c.length === 1) return c[0];
    let n = 0;
    for (const x of c) n += x.byteLength;
    const out = new Uint8Array(n);
    let at = 0;
    for (const x of c) { out.set(x, at); at += x.byteLength; }
    return out;
  }
  get status() { return this.state; }
  cancel() {
    if (!this.state) { this.state = 2; this.reader.cancel().catch(() => {}); }
  }
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = BodyBuffer)]
    type JsBodyBuffer;
    #[wasm_bindgen(constructor, js_class = BodyBuffer)]
    fn new(stream: &wasm_bindgen::JsValue) -> JsBodyBuffer;
    #[wasm_bindgen(method, js_class = BodyBuffer)]
    fn ready(this: &JsBodyBuffer) -> js_sys::Promise;
    #[wasm_bindgen(method, js_class = BodyBuffer)]
    fn take(this: &JsBodyBuffer) -> Option<js_sys::Uint8Array>;
    #[wasm_bindgen(method, getter, js_class = BodyBuffer)]
    fn status(this: &JsBodyBuffer) -> u32;
    #[wasm_bindgen(method, js_class = BodyBuffer)]
    fn cancel(this: &JsBodyBuffer);
}

/// `BodyBuffer::next` 的結果。
pub enum BodyChunk {
    /// 上次取走之後累積的全部位元組（至少一個位元組）
    Bytes(Vec<u8>),
    /// 本文已讀完
    End,
    /// 讀取失敗（連線中斷等）
    Failed,
}

/// 見 `Response::into_body_buffer`。丟棄時取消讀取。
pub struct BodyBuffer(SendWrapper<JsBodyBuffer>);

impl BodyBuffer {
    /// 等到有資料（或本文結束、失敗）為止，取走目前累積的全部位元組。
    /// 失敗前已收到的資料會先交出，下一次才回報失敗。
    pub fn next(&self) -> impl Future<Output = BodyChunk> + Send + '_ {
        let ready = wasm_bindgen_futures::JsFuture::from(self.0.ready());
        let js = &self.0;
        SendFuture::new(async move {
            let _ = ready.await;
            match js.take() {
                Some(bytes) => BodyChunk::Bytes(bytes.to_vec()),
                None if js.status() == 1 => BodyChunk::End,
                None => BodyChunk::Failed,
            }
        })
    }
}

impl Drop for BodyBuffer {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

struct SendStream<S>(S);

// Workers 是單執行緒；與 `worker::send::SendWrapper` 相同的理由
unsafe impl<S> Send for SendStream<S> {}

impl<S: Stream + Unpin> Stream for SendStream<S> {
    type Item = S::Item;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<S::Item>> {
        Pin::new(&mut self.0).poll_next(cx)
    }
}

pub mod multipart {
    pub struct Form {
        pub(super) parts: Vec<(String, PartData)>,
    }

    pub(super) enum PartData {
        Text(String),
        Bytes {
            bytes: Vec<u8>,
            file_name: String,
            mime: Option<String>,
        },
    }

    impl Default for Form {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Form {
        pub fn new() -> Self {
            Self { parts: Vec::new() }
        }

        pub fn text(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
            self.parts.push((name.into(), PartData::Text(value.into())));
            self
        }

        pub fn part(mut self, name: impl Into<String>, part: Part) -> Self {
            self.parts.push((name.into(), part.0));
            self
        }
    }

    pub struct Part(PartData);

    impl Part {
        pub fn bytes(bytes: impl Into<Vec<u8>>) -> Self {
            Part(PartData::Bytes {
                bytes: bytes.into(),
                file_name: "file".into(),
                mime: None,
            })
        }

        pub fn file_name(mut self, name: impl Into<String>) -> Self {
            if let PartData::Bytes { file_name, .. } = &mut self.0 {
                *file_name = name.into();
            }
            self
        }

        pub fn mime_str(mut self, value: &str) -> Result<Self, super::Error> {
            if let PartData::Bytes { mime, .. } = &mut self.0 {
                *mime = Some(value.to_string());
            }
            Ok(self)
        }
    }
}
