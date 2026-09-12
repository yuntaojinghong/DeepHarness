//! 模型提供方抽象层。
//!
//! `ModelProvider` 是自研 Agent 与大模型之间的唯一边界：规划器、
//! 反思器只依赖这个 trait，方便替换厂商与离线测试（`FakeProvider`）。
//!
//! HTTP 传输同样抽象为 `HttpTransport`：
//! - 生产实现 `WinHttpTransport`：Windows 原生 WinHTTP，使用系统
//!   TLS（SChannel），不引入 openssl / rustls 等编译负担；
//! - 测试实现 `FakeTransport`：脚本化响应，无需网络。

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// 一条对话消息。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: "system".to_string(), content: content.into() }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".to_string(), content: content.into() }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: "assistant".to_string(), content: content.into() }
    }
}

/// 模型提供方。
pub trait ModelProvider: Send + Sync {
    /// 发送一次对话补全请求，返回 assistant 文本。
    ///
    /// `temperature` 允许调用方按用途调节（规划偏低、反思居中）。
    fn chat(&self, messages: &[ChatMessage], temperature: f64) -> AppResult<String>;

    /// 提供方名称（日志与可观测性用）。
    fn name(&self) -> &str;
}

// ---------------------------------------------------------------------------
// HTTP 传输抽象
// ---------------------------------------------------------------------------

/// 一次 HTTP 响应。
#[derive(Debug, Clone, PartialEq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// HTTP 传输抽象（仅 POST JSON 的最小面）。
pub trait HttpTransport: Send + Sync {
    /// 向 `url` 发送 JSON POST，附带 `headers`（name: value）。
    fn post_json(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &str,
        timeout_ms: u32,
    ) -> AppResult<HttpResponse>;
}

// ---------------------------------------------------------------------------
// Windows WinHTTP 实现
// ---------------------------------------------------------------------------

/// WinHTTP 传输（系统 SChannel TLS）。
pub struct WinHttpTransport;

#[cfg(windows)]
mod winhttp {
    #![allow(non_snake_case, clippy::missing_safety_doc)]

    use std::ffi::c_void;
    use std::ptr;

    use windows_sys::Win32::Foundation::GetLastError;
    use crate::error::{AppError, AppResult};
    use windows_sys::Win32::Networking::WinHttp::{
        WinHttpAddRequestHeaders, WinHttpCloseHandle, WinHttpConnect,
        WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable,
        WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
        WinHttpSendRequest, WinHttpSetTimeouts,
        WINHTTP_ADDREQ_FLAG_ADD, WINHTTP_ADDREQ_FLAG_REPLACE,
        WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER,
        WINHTTP_QUERY_STATUS_CODE,
    };

    // windows-sys 未导出的宏常量按 MSDN 值内联
    const WINHTTP_NO_ADDITIONAL_HEADERS: *const u16 = std::ptr::null();
    const WINHTTP_NO_REFERER: *const u16 = std::ptr::null();
    const WINHTTP_NO_REQUEST_DATA: *const std::ffi::c_void = std::ptr::null();
    const WINHTTP_HEADER_NAME_BY_INDEX: u32 = 0xFFFF_FFFF;

    pub type Handle = *mut c_void;

    pub fn last_error(what: &str) -> AppError {
        let code = unsafe { GetLastError() };
        AppError::Other(format!("{what} 失败 (WinHTTP 错误码 {code})"))
    }

    pub fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// 解析 URL 为 (https?, host, port, path)。
    pub fn split_url(url: &str) -> AppResult<(bool, String, u16, String)> {
        let (scheme, rest) = url
            .split_once("://")
            .ok_or_else(|| AppError::Other(format!("URL 缺少协议: {url}")))?;
        let secure = match scheme {
            "https" => true,
            "http" => false,
            other => {
                return Err(AppError::Other(format!("不支持的协议: {other}")));
            }
        };
        let (host_port, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        let (host, port) = match host_port.rsplit_once(':') {
            Some((h, p)) => (
                h.to_string(),
                p.parse::<u16>().map_err(|_| {
                    AppError::Other(format!("非法端口: {host_port}"))
                })?,
            ),
            None => (host_port.to_string(), if secure { 443 } else { 80 }),
        };
        if host.is_empty() {
            return Err(AppError::Other(format!("URL 缺少主机名: {url}")));
        }
        Ok((secure, host, port, path.to_string()))
    }

    /// 执行一次完整的 POST 请求并读取全部响应体。
    pub fn post(
        url: &str,
        headers: &[(String, String)],
        body: &[u8],
        timeout_ms: u32,
    ) -> AppResult<(u16, String)> {
        let (secure, host, port, path) = split_url(url)?;

        unsafe {
            // 1) 会话
            let ua = to_wide("DeepHarness/1.0");
            let session = WinHttpOpen(
                ua.as_ptr(),
                0, // WINHTTP_ACCESS_TYPE_DEFAULT_PROXY
                std::ptr::null(),
                std::ptr::null(),
                0,
            );
            if session.is_null() {
                return Err(last_error("WinHttpOpen"));
            }
            // 确保任何提前返回都释放句柄
            let _guard = HandleGuard(session);

            // 2) 连接
            let host_w = to_wide(&host);
            let connect = WinHttpConnect(session, host_w.as_ptr(), port as _, 0);
            if connect.is_null() {
                return Err(last_error("WinHttpConnect"));
            }
            let _guard_conn = HandleGuard(connect);

            // 3) 请求
            let path_w = to_wide(&path);
            let verb_w = to_wide("POST");
            let flags = if secure { WINHTTP_FLAG_SECURE } else { 0 };
            let request = WinHttpOpenRequest(
                connect,
                verb_w.as_ptr(),
                path_w.as_ptr(),
                std::ptr::null(), // HTTP/1.1 默认
                WINHTTP_NO_REFERER,
                std::ptr::null(), // 默认 Accept 类型
                flags,
            );
            if request.is_null() {
                return Err(last_error("WinHttpOpenRequest"));
            }
            let _guard_req = HandleGuard(request);

            // 4) 超时：解析 / 连接 / 发送 / 接收
            if WinHttpSetTimeouts(request, timeout_ms as i32, timeout_ms as i32, timeout_ms as i32, timeout_ms as i32) == 0 {
                return Err(last_error("WinHttpSetTimeouts"));
            }

            // 5) 头部
            if !headers.is_empty() {
                let header_line = headers
                    .iter()
                    .map(|(k, v)| format!("{k}: {v}\r\n"))
                    .collect::<String>();
                let hdr_w = to_wide(&header_line);
                let ok = WinHttpAddRequestHeaders(
                    request,
                    hdr_w.as_ptr(),
                    (hdr_w.len() as u32) - 1,
                    WINHTTP_ADDREQ_FLAG_ADD | WINHTTP_ADDREQ_FLAG_REPLACE,
                );
                if ok == 0 {
                    return Err(last_error("WinHttpAddRequestHeaders"));
                }
            }

            // 6) 发送
            let sent = WinHttpSendRequest(
                request,
                WINHTTP_NO_ADDITIONAL_HEADERS,
                0,
                if body.is_empty() {
                    WINHTTP_NO_REQUEST_DATA
                } else {
                    body.as_ptr() as *const c_void
                },
                body.len() as u32,
                body.len() as u32,
                0usize,
            );
            if sent == 0 {
                return Err(last_error("WinHttpSendRequest"));
            }

            // 7) 接收响应
            if WinHttpReceiveResponse(request, ptr::null_mut()) == 0 {
                return Err(last_error("WinHttpReceiveResponse"));
            }

            // 8) 状态码
            let mut status: u32 = 0;
            let mut size = std::mem::size_of::<u32>() as u32;
            let ok = WinHttpQueryHeaders(
                request,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                WINHTTP_HEADER_NAME_BY_INDEX as usize as *const u16,
                &mut status as *mut u32 as *mut c_void,
                &mut size,
                ptr::null_mut(),
            );
            if ok == 0 || status == 0 {
                return Err(last_error("WinHttpQueryHeaders(STATUS_CODE)"));
            }

            // 9) 分块读取响应体
            let mut body_out: Vec<u8> = Vec::new();
            loop {
                let mut available: u32 = 0;
                if WinHttpQueryDataAvailable(request, &mut available) == 0 {
                    return Err(last_error("WinHttpQueryDataAvailable"));
                }
                if available == 0 {
                    break;
                }
                let mut chunk = vec![0u8; available as usize];
                let mut read: u32 = 0;
                if WinHttpReadData(
                    request,
                    chunk.as_mut_ptr() as *mut c_void,
                    available,
                    &mut read,
                ) == 0
                {
                    return Err(last_error("WinHttpReadData"));
                }
                if read == 0 {
                    break;
                }
                chunk.truncate(read as usize);
                body_out.extend_from_slice(&chunk);
            }

            Ok((status as u16, String::from_utf8_lossy(&body_out).into_owned()))
        }
    }

    /// RAII 句柄守卫。
    struct HandleGuard(Handle);
    impl Drop for HandleGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { WinHttpCloseHandle(self.0) };
            }
        }
    }
}

#[cfg(windows)]
impl HttpTransport for WinHttpTransport {
    fn post_json(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &str,
        timeout_ms: u32,
    ) -> AppResult<HttpResponse> {
        let (status, text) = winhttp::post(url, headers, body.as_bytes(), timeout_ms)?;
        Ok(HttpResponse { status, body: text })
    }
}

// ---------------------------------------------------------------------------
// DeepSeek 实现
// ---------------------------------------------------------------------------

/// DeepSeek Chat Completions 提供方。
pub struct DeepSeekProvider {
    base_url: String,
    api_key: String,
    model: String,
    transport: Box<dyn HttpTransport>,
    timeout_ms: u32,
}

#[derive(Debug, Deserialize)]
struct ChatApiChoice {
    message: Option<ChatApiMessage>,
}

#[derive(Debug, Deserialize)]
struct ChatApiMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatApiResponse {
    choices: Vec<ChatApiChoice>,
}

impl DeepSeekProvider {
    /// 构造提供方。`base_url` 形如 `https://api.deepseek.com`。
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            model,
            transport: Box::new(WinHttpTransport),
            timeout_ms: 120_000,
        }
    }

    /// 注入自定义传输（测试用）。
    pub fn with_transport(
        base_url: String,
        api_key: String,
        model: String,
        transport: Box<dyn HttpTransport>,
    ) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            model,
            transport,
            timeout_ms: 120_000,
        }
    }
}

impl ModelProvider for DeepSeekProvider {
    fn chat(&self, messages: &[ChatMessage], temperature: f64) -> AppResult<String> {
        if messages.is_empty() {
            return Err(AppError::Other("对话消息不能为空".to_string()));
        }
        let url = format!("{}/chat/completions", self.base_url);
        let body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "temperature": temperature,
            "stream": false,
        });
        let body_str = serde_json::to_string(&body)
            .map_err(|e| AppError::Other(format!("请求体序列化失败: {e}")))?;
        let headers = vec![
            ("Content-Type".to_string(), "application/json".to_string()),
            ("Accept".to_string(), "application/json".to_string()),
            ("Authorization".to_string(), format!("Bearer {}", self.api_key)),
        ];

        let resp = self
            .transport
            .post_json(&url, &headers, &body_str, self.timeout_ms)?;

        if resp.status != 200 {
            let detail: String = resp.body.chars().take(400).collect();
            return Err(AppError::Other(format!(
                "模型服务返回 HTTP {}: {detail}",
                resp.status
            )));
        }

        let parsed: ChatApiResponse = serde_json::from_str(&resp.body).map_err(|e| {
            AppError::Other(format!("模型响应解析失败: {e}"))
        })?;
        let content = parsed
            .choices
            .first()
            .and_then(|c| c.message.as_ref())
            .and_then(|m| m.content.clone())
            .ok_or_else(|| AppError::Other("模型响应缺少 choices[0].message.content".to_string()))?;
        Ok(content)
    }

    fn name(&self) -> &str {
        "deepseek"
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// 脚本化假传输。`seen` 是对外共享的请求记录（测试断言用）。
    struct FakeTransport {
        responses: Mutex<Vec<AppResult<HttpResponse>>>,
        seen: Arc<Mutex<Vec<String>>>,
    }

    impl FakeTransport {
        fn new(responses: Vec<AppResult<HttpResponse>>) -> (Self, Arc<Mutex<Vec<String>>>) {
            let seen = Arc::new(Mutex::new(Vec::new()));
            (
                Self { responses: Mutex::new(responses), seen: Arc::clone(&seen) },
                seen,
            )
        }
    }

    impl HttpTransport for FakeTransport {
        fn post_json(
            &self,
            url: &str,
            _headers: &[(String, String)],
            body: &str,
            _timeout_ms: u32,
        ) -> AppResult<HttpResponse> {
            self.seen.lock().unwrap().push(format!("{url}|{body}"));
            let mut q = self.responses.lock().unwrap();
            if q.is_empty() {
                Err(AppError::Other("脚本耗尽".to_string()))
            } else {
                q.remove(0)
            }
        }
    }

    fn ok_body(content: &str) -> AppResult<HttpResponse> {
        Ok(HttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{"message": {"role": "assistant", "content": content}}]
            })
            .to_string(),
        })
    }

    fn provider(t: FakeTransport) -> DeepSeekProvider {
        DeepSeekProvider::with_transport(
            "https://api.example.test".to_string(),
            "sk-test".to_string(),
            "deepseek-chat".to_string(),
            Box::new(t),
        )
    }

    /// 提供方 + 请求记录句柄。
    fn provider_with_log(
        responses: Vec<AppResult<HttpResponse>>,
    ) -> (DeepSeekProvider, Arc<Mutex<Vec<String>>>) {
        let (t, seen) = FakeTransport::new(responses);
        let p = provider(t);
        (p, seen)
    }

    #[test]
    fn chat_returns_first_choice_content() {
        let (p, _seen) = provider_with_log(vec![ok_body("你好，世界")]);
        let out = p.chat(&[ChatMessage::user("hi")], 0.2).expect("应成功");
        assert_eq!(out, "你好，世界");
        assert_eq!(p.name(), "deepseek");
    }

    #[test]
    fn chat_sends_expected_request_shape() {
        let (p, seen) = provider_with_log(vec![ok_body("ok")]);
        // base_url 尾斜杠应被修剪
        p.chat(&[ChatMessage::system("s"), ChatMessage::user("u")], 0.5)
            .unwrap();
        let log = seen.lock().unwrap().clone();
        assert_eq!(log.len(), 1);
        let line = &log[0];
        assert!(
            line.starts_with("https://api.example.test/chat/completions|"),
            "URL 应指向 /chat/completions: {line}"
        );
        let body: serde_json::Value =
            serde_json::from_str(line.split_once('|').unwrap().1).unwrap();
        assert_eq!(body["model"], "deepseek-chat");
        assert_eq!(body["messages"].as_array().unwrap().len(), 2);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["role"], "user");
    }

    #[test]
    fn chat_rejects_empty_messages() {
        let (p, _seen) = provider_with_log(vec![]);
        assert!(p.chat(&[], 0.1).is_err());
    }

    #[test]
    fn chat_surfaces_http_error_with_body_snippet() {
        let err_body = String::from(
            "{\"error\":{\"message\":\"invalid api key\"}}",
        );
        let (p, _seen) = provider_with_log(vec![Ok(HttpResponse {
            status: 401,
            body: err_body,
        })]);
        let err = p.chat(&[ChatMessage::user("hi")], 0.1).unwrap_err();
        assert!(err.to_string().contains("401"), "实际错误: {err}");
        assert!(err.to_string().contains("invalid api key"));
    }

    #[test]
    fn chat_rejects_malformed_json_body() {
        let (p, _seen) = provider_with_log(vec![Ok(HttpResponse {
            status: 200,
            body: "not json".to_string(),
        })]);
        assert!(p.chat(&[ChatMessage::user("hi")], 0.1).is_err());
    }

    #[test]
    fn chat_rejects_empty_choices() {
        let (p, _seen) = provider_with_log(vec![Ok(HttpResponse {
            status: 200,
            body: String::from("{\"choices\": []}"),
        })]);
        assert!(p.chat(&[ChatMessage::user("hi")], 0.1).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn url_splitting_covers_all_shapes() {
        use super::winhttp::split_url;
        let (secure, host, port, path) =
            split_url("https://api.deepseek.com/chat/completions").unwrap();
        assert!(secure);
        assert_eq!(host, "api.deepseek.com");
        assert_eq!(port, 443);
        assert_eq!(path, "/chat/completions");

        let (secure, host, port, path) = split_url("http://127.0.0.1:8080").unwrap();
        assert!(!secure);
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 8080);
        assert_eq!(path, "/");

        assert!(split_url("ftp://x").is_err());
        assert!(split_url("https://").is_err());
    }
}
