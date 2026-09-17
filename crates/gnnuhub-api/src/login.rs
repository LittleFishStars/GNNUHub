//! 统一身份认证登录流程
//!
//! 对应 Python 版 `login.py`。与原实现的差异：
//!
//! | 问题 | Python 版 | 本实现 |
//! |------|-----------|--------|
//! | 验证码错误重试 | 递归调用，无上限，可能栈溢出 | 有界循环，`max_login_retries` 控制 |
//! | uid 使用 | `Captcha` 类用了请求 id | 使用响应返回的 uid |
//! | SSO 跳转 | `while True` 无上限 | 有界循环 + 目标域校验 |
//! | 错误信息 | 混合返回元组 | 统一的 [`Error`] 枚举 |

use std::collections::HashMap;

use reqwest::header::LOCATION;
use serde::Deserialize;

use gnnuhub_core::{Error, Result, CAS_BASE_URL, JWGL_BASE_URL};
use gnnuhub_ocr::OcrEngine;

use crate::captcha::{Captcha, CaptchaResponse};
use crate::client::Client;

/// 票据接口路径
const TICKETS_PATH: &str = "/lyuapServer/v1/tickets";

/// 教务系统 SSO 登录入口
const SSO_LOGIN_PATH: &str = "/sso/lyiotlogin";

/// 全局登录成功后的票据
///
/// 实测响应有两种形态：
///
/// 成功（ticket 直接放在顶层）：
/// ```json
/// { "tgt": "TGT-...", "ticket": "ST-..." }
/// ```
///
/// 失败（包在 meta 里）：
/// ```json
/// { "meta": { "success": true, "statusCode": 200, "message": "ok" },
///   "data": { "code": "CODEFALSE" } }
/// ```
///
/// **注意**：失败响应中 `meta.success` 可能是 `true`，而
/// `statusCode` 在部分响应里是**整数**而非字符串。因此判断登录结果
/// 必须依据 `data.code`，不能依赖 `meta.success`。
#[derive(Debug, Clone, Deserialize)]
struct TicketResponse {
    data: Option<TicketData>,
    meta: Option<TicketMeta>,
}

/// 成功时的 data 字段
#[derive(Debug, Clone, Deserialize)]
struct TicketData {
    /// 业务状态码，成功时为空或 "SUCCESS"，验证码错误时为 "CODEFALSE"
    #[serde(default)]
    code: String,
}

/// 响应中的 meta 字段
#[derive(Debug, Clone, Deserialize)]
struct TicketMeta {
    /// 该字段不可靠，部分失败响应中仍为 `true`，仅用于日志
    #[serde(default)]
    #[allow(dead_code)]
    success: bool,
    /// 状态码。实测可能是**整数**（如 200）也可能是字符串，
    /// 因此用 `serde_json::Value` 兼容两种形态。
    #[serde(rename = "statusCode", default)]
    status_code: serde_json::Value,
    #[serde(default)]
    message: String,
}

impl TicketMeta {
    /// 把 statusCode 归一化成字符串
    fn status_code_str(&self) -> String {
        match &self.status_code {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Null => String::new(),
            other => other.to_string(),
        }
    }
}

/// 一次登录尝试的结果
#[derive(Debug, Clone)]
pub enum LoginOutcome {
    /// 认证成功，携带 TGT 与 ticket
    Success {
        /// 全局票据
        tgt: String,
        /// 服务票据
        ticket: String,
    },
    /// 验证码错误，可以换一张重试
    CaptchaIncorrect,
    /// 学号或密码错误，重试无意义
    BadCredentials(String),
}

/// 向票据接口提交认证请求
///
/// 对应 Python 版 `try_login`。响应结构有两种形态：
/// 成功时是 `{"tgt": "...", "ticket": "..."}`，
/// 失败时是 `{"meta": {"success": false, ...}}`。
pub async fn try_login(
    client: &Client,
    student_id: &str,
    encrypted_password: &str,
    captcha_code: &str,
    captcha_uid: &str,
    service: &str,
) -> Result<LoginOutcome> {
    let url = format!("{CAS_BASE_URL}{TICKETS_PATH}");

    let mut form: HashMap<&str, &str> = HashMap::new();
    form.insert("username", student_id);
    form.insert("password", encrypted_password);
    form.insert("service", service);
    form.insert("code", captcha_code);
    form.insert("id", captcha_uid);
    form.insert("loginType", "");
    form.insert("otpcode", "");

    let headers = client.cas_headers();
    let response = client
        .throttled(|http| http.post(&url).headers(headers.clone()).form(&form))
        .await?;

    let status = response.status();
    if !status.is_success() {
        return Err(Error::UnexpectedStatus {
            status: status.as_u16(),
            url,
        });
    }

    let body = response.text().await?;
    tracing::debug!("票据接口原始响应: {}", truncate(&body, 500));

    parse_ticket_response(&body)
}

/// 解析票据接口的响应体
///
/// 抽成独立函数以便单元测试覆盖各种响应形态。
///
/// # 判定逻辑
///
/// 1. 顶层同时存在 `tgt` 与 `ticket` → 成功
/// 2. 存在 `meta`：取 `statusCode` 与 `message` 判定失败原因
/// 3. 存在 `data.code`：`CODEFALSE` 表示验证码错误
pub fn parse_ticket_response(body: &str) -> Result<LoginOutcome> {
    // 先尝试「成功」形态：{"tgt": "...", "ticket": "..."}
    if let Ok(map) = serde_json::from_str::<HashMap<String, serde_json::Value>>(body)
        && let (Some(tgt), Some(ticket)) = (map.get("tgt"), map.get("ticket"))
        && let (Some(tgt), Some(ticket)) = (tgt.as_str(), ticket.as_str())
    {
        return Ok(LoginOutcome::Success {
            tgt: tgt.to_string(),
            ticket: ticket.to_string(),
        });
    }

    let parsed: TicketResponse = serde_json::from_str(body).map_err(Error::Json)?;

    // 优先看 data.code —— 这是最可靠的成功判据
    if let Some(data) = &parsed.data {
        let code = data.code.trim();
        if code.is_empty() || code.eq_ignore_ascii_case("SUCCESS") {
            return Err(Error::TicketMissing(
                "响应声明成功但未携带 ticket".to_string(),
            ));
        }
        if code.eq_ignore_ascii_case("CODEFALSE") {
            return Ok(LoginOutcome::CaptchaIncorrect);
        }
        return Err(Error::TicketMissing(format!("未识别的业务码: {code}")));
    }

    // 没有 data 时依据 meta 判定
    if let Some(meta) = parsed.meta {
        let status_code = meta.status_code_str();
        let message = if meta.message.is_empty() {
            status_code.clone()
        } else {
            meta.message.clone()
        };

        if status_code.eq_ignore_ascii_case("CODEFALSE")
            || message.contains("验证码")
            || message.to_lowercase().contains("code")
        {
            return Ok(LoginOutcome::CaptchaIncorrect);
        }

        if status_code.eq_ignore_ascii_case("USERNAMEORPASSWORDERROR")
            || message.contains("密码")
            || message.contains("用户名")
        {
            return Ok(LoginOutcome::BadCredentials(message));
        }

        return Err(Error::TicketMissing(message));
    }

    Err(Error::TicketMissing(truncate(body, 200).to_string()))
}

/// 用 ticket 换取教务系统的会话 Cookie
///
/// 对应 Python 版 `login.get_cookies` + `Student.__init__`。
/// 实测链路（⚠️ 与直觉不同，请勿「顺手」多加跳）：
///
/// ```text
/// ① GET /sso/lyiotlogin?ticket=ST-xxx
///      → 302 /sso/lyiotlogin               种下 /sso 作用域的 JSESSIONID
/// ② GET /sso/lyiotlogin           （带上 ① 的 Cookie + CASTGC）
///      → 302 /ticketlogin?uid=...&verify=...
/// ③ GET /ticketlogin?...
///      → 302 /xtgl/index_initMenu.html      种下 / 作用域的 JSESSIONID
/// ```
///
/// # 为什么必须停在 ③
///
/// ③ 的 `Set-Cookie` 才是真正可用的会话。此时 `/sso` 与 `/` 下**各有一个
/// 同名 `JSESSIONID`**，必须**丢弃 `/sso` 那份**（见 [`is_sso_scoped`]），
/// 否则后续带出去的是 `/sso` 的旧值，会话不成立。
///
/// 更强求一步去 GET `/xtgl/login_slogin.html` 会被服务端直接关闭连接
/// （`peer closed connection without sending TLS close_notify`）——
/// 该校对「已持有会话却重放登录页」的行为有防护，这是实测结论，
/// 不是网络抖动。因此本函数**在收到非 3xx 后立即停止**。
///
/// # 错误
///
/// - ticket 未被接受（跳回认证平台）时返回 [`Error::SsoRedirect`]
/// - 跳转超出上限时返回 [`Error::LoginRetriesExhausted`]
/// - 最终未取到根作用域的 `JSESSIONID` 时返回 [`Error::Unauthenticated`]
pub async fn exchange_ticket_for_session(
    client: &Client,
    ticket: &str,
    castgc: &str,
    max_hops: u32,
) -> Result<HashMap<String, String>> {
    // CASTGC 属于认证平台，显式声明它的归属，避免被当成教务系统的 Cookie
    let mut cookies: HashMap<String, String> = HashMap::new();

    // /sso 作用域下的 JSESSIONID——只是链路中的中转产物，最后必须丢掉
    let mut sso_jsessionid: Option<String> = None;

    // 第一跳必须携带 ticket 才能完成兑换
    let mut url = format!("{JWGL_BASE_URL}{SSO_LOGIN_PATH}?ticket={ticket}");
    let mut hops = 0;
    // SSO 链路实测 3 跳即可完成
    let hop_limit = max_hops.max(5);

    loop {
        hops += 1;
        if hops > hop_limit {
            return Err(Error::LoginRetriesExhausted {
                attempts: hop_limit,
            });
        }

        let response = match send_with_retry(client, &url, &cookies, &castgc_send(castgc), hops).await
        {
            Ok(r) => r,
            Err(e) => return Err(e),
        };

        let status = response.status();
        collect_set_cookies(&response, &mut cookies, &mut sso_jsessionid);
        tracing::debug!("SSO 第 {hops} 跳 {url} -> {status}");

        // 非重定向即到达目标页：会话已由本跳的 Set-Cookie 建立
        //
        // 这里必须停止，不能为了「确认会话有效」再请求一次登录页：
        // 服务端会把这种重放判定为异常并直接断开 TLS 连接。
        if !status.is_redirection() {
            break;
        }

        let Some(location) = response
            .headers()
            .get(LOCATION)
            .and_then(|v| v.to_str().ok())
        else {
            // 3xx 但没有 Location，无法继续，视为链路终止
            tracing::warn!("SSO 第 {hops} 跳返回 {status} 但缺少 Location");
            break;
        };

        let next = if location.starts_with("http") {
            location.to_string()
        } else {
            format!("{JWGL_BASE_URL}{location}")
        };

        let parsed = url::Url::parse(&next)
            .map_err(|e| Error::SsoRedirect(format!("非法的跳转地址 {next}: {e}")))?;
        let host = parsed.host_str().unwrap_or_default();

        if host == gnnuhub_core::CAS_HOST {
            return Err(Error::SsoRedirect(
                "ticket 未被教务系统接受，跳回了认证平台".to_string(),
            ));
        }
        if host != gnnuhub_core::JWGL_HOST {
            return Err(Error::SsoRedirect(format!(
                "跳转目标域不在教务系统内: {next}"
            )));
        }

        url = next;
    }

    // 丢掉 /sso 作用域的中转 Cookie
    if let Some(stale) = sso_jsessionid
        && cookies.get("JSESSIONID") == Some(&stale)
    {
        tracing::debug!("丢弃 /sso 作用域的中转 JSESSIONID");
        cookies.remove("JSESSIONID");
    }

    // 根作用域下必须存在 JSESSIONID，否则后续请求会被弹回登录页
    if !cookies.contains_key("JSESSIONID") {
        return Err(Error::Unauthenticated);
    }

    tracing::info!(
        "SSO 交换完成，共 {hops} 跳，获得 {} 个 Cookie",
        cookies.len()
    );
    Ok(cookies)
}

/// 打包 CASTGC 供请求头使用
fn castgc_send(castgc: &str) -> Option<String> {
    if castgc.is_empty() {
        None
    } else {
        Some(format!("CASTGC={castgc}"))
    }
}

/// 发起一次 SSO 跳转请求，对**传输层瞬时故障**做有限重试
///
/// 注意：SSO 每一跳都可能种下新 Cookie，重试时沿用同一份 Cookie 集合，
/// 不会破坏会话状态。
///
/// # 关于重试
///
/// 仅对传输层错误重试。**HTTP 状态码层面的失败不会重试**：
/// 服务端对「重放登录页」这类异常请求会主动关闭 TLS 连接，
/// 盲目重试只会加深风控，交由调用方按状态码判断更安全。
async fn send_with_retry(
    client: &Client,
    url: &str,
    cookies: &HashMap<String, String>,
    castgc_header: &Option<String>,
    hop: u32,
) -> Result<reqwest::Response> {
    /// 单跳最多尝试次数
    const MAX_ATTEMPTS: u32 = 3;
    /// 重试前的等待时间
    ///
    /// 取值需明显大于请求间隔：服务端的风控窗口是秒级的，
    /// 立刻重试会撞在同一个窗口上，反而加深封禁。
    const RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(1200);

    let mut cookie_header = build_cookie_header(cookies);
    if let Some(castgc) = castgc_header {
        if cookie_header.is_empty() {
            cookie_header.clone_from(castgc);
        } else {
            cookie_header.push_str("; ");
            cookie_header.push_str(castgc);
        }
    }

    let mut last_err = None;
    for attempt in 1..=MAX_ATTEMPTS {
        let result = client
            .throttled(|http| http.get(url).header(reqwest::header::COOKIE, &cookie_header))
            .await;

        match result {
            Ok(response) => return Ok(response),
            Err(e) => {
                tracing::warn!("SSO 第 {hop} 跳第 {attempt} 次请求失败: {e}");
                // 打出完整错误链，便于定位是 DNS / TLS / 代理哪一层出问题
                let mut source: Option<&(dyn std::error::Error + 'static)> =
                    std::error::Error::source(&e);
                while let Some(s) = source {
                    tracing::warn!("  起因: {s}");
                    source = std::error::Error::source(s);
                }
                last_err = Some(e);
                if attempt < MAX_ATTEMPTS {
                    tokio::time::sleep(RETRY_DELAY).await;
                }
            }
        }
    }

    Err(last_err.expect("循环至少执行一次，必然记录过错误"))
}

/// 收集响应中的所有 Set-Cookie，并按作用域归位
///
/// 同名 Cookie 可能带不同 `Path` 在链路上各出现一次——典型的是
/// `JSESSIONID` 在 `/sso` 与 `/` 下各一份。这里：
///
/// - **根作用域**（无 `Path` 或 `Path=/`）的值写入 `out`，作为有效会话
/// - **`/sso` 作用域**的值记入 `sso_scoped`，供调用方最后丢弃
///
/// 之所以不简单地「后出现的为准」：`/sso` 下的那份在链路上可能最后出现，
/// 若直接覆盖会让后续请求带上错误的会话标识。
fn collect_set_cookies(
    response: &reqwest::Response,
    out: &mut HashMap<String, String>,
    sso_scoped: &mut Option<String>,
) {
    for value in response.headers().get_all(reqwest::header::SET_COOKIE) {
        let Ok(text) = value.to_str() else { continue };
        let Some((name, val)) = parse_set_cookie(text) else {
            continue;
        };
        if is_sso_scoped(text) {
            tracing::trace!("记录 /sso 作用域 Cookie: {name}");
            *sso_scoped = Some(val);
        } else {
            out.insert(name, val);
        }
    }
}

/// 判断 Set-Cookie 是否只对 `/sso` 路径生效
///
/// 只解析 `Path` 属性；缺少该属性时按浏览器默认行为视为当前目录，
/// 对本次登录流程而言即是根作用域。
fn is_sso_scoped(set_cookie: &str) -> bool {
    set_cookie
        .split(';')
        .skip(1)
        .filter_map(|attr| attr.split_once('='))
        .any(|(k, v)| k.trim().eq_ignore_ascii_case("path") && v.trim().starts_with("/sso"))
}

/// 把 Cookie 映射拼成请求头
pub fn build_cookie_header(cookies: &HashMap<String, String>) -> String {
    cookies
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("; ")
}

/// 从 Set-Cookie 头中提取名称与值
///
/// 只取第一段 `name=value`，忽略 Path / Domain 等属性。
/// 同时剔除值为 `DELETED` 的过期 Cookie。
pub fn parse_set_cookie(header: &str) -> Option<(String, String)> {
    let first = header.split(';').next()?;
    let (name, value) = first.split_once('=')?;
    let name = name.trim().to_string();
    let value = value.trim().to_string();

    // 教务系统用 DELETED 标记失效
    if name.is_empty() || value.eq_ignore_ascii_case("DELETED") {
        return None;
    }
    Some((name, value))
}

/// 截断长字符串用于日志输出
fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    // 保证不会切断 UTF-8 边界
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// 申请一张新验证码
pub async fn fetch_captcha(client: &Client) -> Result<Captcha> {
    let request_id = Captcha::random_request_id();
    let url = Captcha::request_url(&request_id);

    let headers = client.cas_headers();
    let response = client
        .throttled(|http| http.get(&url).headers(headers.clone()))
        .await?;

    if !response.status().is_success() {
        return Err(Error::UnexpectedStatus {
            status: response.status().as_u16(),
            url,
        });
    }

    let parsed: CaptchaResponse = response.json().await?;
    Captcha::from_response(parsed)
}

/// 执行完整的带重试登录流程
///
/// 与 Python 版 `login()` 的差异：重试次数由配置决定，不再无限递归。
pub async fn login_with_retry(
    client: &Client,
    student_id: &str,
    password: &str,
    service: &str,
    ocr: &dyn OcrEngine,
    interactive: Option<&gnnuhub_ocr::InteractiveFn>,
) -> Result<LoginOutcome> {
    let max_attempts = client.config().max_login_retries;
    let encrypted_password = gnnuhub_crypto::encode_password(password);

    for attempt in 1..=max_attempts {
        let captcha = fetch_captcha(client).await?;
        tracing::debug!(
            "第 {attempt} 次尝试登录，验证码 uid = {}",
            captcha.uid
        );

        let code = ocr.recognize(&captcha.image, interactive)?;

        match try_login(
            client,
            student_id,
            &encrypted_password,
            &code,
            &captcha.uid,
            service,
        )
        .await?
        {
            LoginOutcome::Success { tgt, ticket } => {
                tracing::info!("第 {attempt} 次尝试登录成功");
                return Ok(LoginOutcome::Success { tgt, ticket });
            }
            LoginOutcome::CaptchaIncorrect => {
                tracing::debug!("第 {attempt} 次验证码错误，准备重试");
                continue;
            }
            LoginOutcome::BadCredentials(msg) => {
                // 密码错误，重试无意义，立即返回
                tracing::warn!("凭据错误: {msg}");
                return Ok(LoginOutcome::BadCredentials(msg));
            }
        }
    }

    Err(Error::LoginRetriesExhausted {
        attempts: max_attempts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 成功响应能被解析
    #[test]
    fn parses_success_response() {
        let body = r#"{"tgt":"TGT-abc","ticket":"ST-xyz"}"#;
        let outcome = parse_ticket_response(body).unwrap();
        match outcome {
            LoginOutcome::Success { tgt, ticket } => {
                assert_eq!(tgt, "TGT-abc");
                assert_eq!(ticket, "ST-xyz");
            }
            other => panic!("期望 Success，实际 {other:?}"),
        }
    }

    /// 验证码错误响应能被识别（实测形态：statusCode 为整数）
    #[test]
    fn parses_captcha_error_with_integer_status() {
        let body = r#"{"meta":{"success":true,"statusCode":200,"message":"ok"},
                       "data":{"code":"CODEFALSE"}}"#;
        let outcome = parse_ticket_response(body).unwrap();
        assert!(
            matches!(outcome, LoginOutcome::CaptchaIncorrect),
            "应识别为验证码错误，实际 {outcome:?}"
        );
    }

    /// meta.success 为 true 但 data.code 表示失败时，应判定为失败
    ///
    /// 这是实测中发现的坑：不能依赖 meta.success 判断结果。
    #[test]
    fn ignores_misleading_meta_success() {
        let body = r#"{"meta":{"success":true,"statusCode":"200","message":"ok"},
                       "data":{"code":"CODEFALSE"}}"#;
        let outcome = parse_ticket_response(body).unwrap();
        assert!(matches!(outcome, LoginOutcome::CaptchaIncorrect));
    }

    /// statusCode 为字符串时也能正确解析
    #[test]
    fn parses_captcha_error_with_string_status() {
        let body = r#"{"meta":{"success":false,"statusCode":"CODEFALSE","message":"验证码错误"}}"#;
        let outcome = parse_ticket_response(body).unwrap();
        assert!(matches!(outcome, LoginOutcome::CaptchaIncorrect));
    }

    /// 密码错误响应能被识别
    #[test]
    fn parses_bad_credentials() {
        let body = r#"{"meta":{"success":false,"statusCode":"USERNAMEORPASSWORDERROR","message":"用户名或密码错误"}}"#;
        let outcome = parse_ticket_response(body).unwrap();
        assert!(matches!(outcome, LoginOutcome::BadCredentials(_)));
    }

    /// data.code 表示成功但缺少 ticket 时应报错
    #[test]
    fn rejects_success_without_ticket() {
        let body = r#"{"data":{"code":"SUCCESS"}}"#;
        assert!(parse_ticket_response(body).is_err());
    }

    /// 完全无法识别的响应应报错
    #[test]
    fn rejects_unknown_response() {
        assert!(parse_ticket_response("not json at all").is_err());
    }

    /// Set-Cookie 基本解析
    #[test]
    fn parses_set_cookie() {
        let (name, value) = parse_set_cookie("JSESSIONID=ABC123; Path=/; HttpOnly").unwrap();
        assert_eq!(name, "JSESSIONID");
        assert_eq!(value, "ABC123");
    }

    /// DELETED 的 Cookie 应被丢弃
    #[test]
    fn drops_deleted_cookie() {
        assert!(parse_set_cookie("JSESSIONID=DELETED; Path=/sso").is_none());
    }

    /// 缺少等号的 Cookie 应被忽略
    #[test]
    fn ignores_malformed_cookie() {
        assert!(parse_set_cookie("justtext").is_none());
    }

    /// Cookie 头拼接
    #[test]
    fn builds_cookie_header() {
        let mut cookies = HashMap::new();
        cookies.insert("A".to_string(), "1".to_string());
        let header = build_cookie_header(&cookies);
        assert_eq!(header, "A=1");
    }

    /// 截断函数不应切断 UTF-8
    #[test]
    fn truncate_respects_utf8_boundary() {
        let s = "中文测试";
        let out = truncate(s, 5);
        assert!(s.starts_with(out));
    }

    /// `/sso` 作用域的 Cookie 应被识别出来
    #[test]
    fn detects_sso_scoped_cookie() {
        assert!(is_sso_scoped("JSESSIONID=ABC; Path=/sso; HttpOnly"));
        assert!(is_sso_scoped("JSESSIONID=ABC; path=/sso/lyiotlogin"));
        assert!(is_sso_scoped("JSESSIONID=ABC; Path=/sso5"));
    }

    /// 根作用域的 Cookie 不应被误判为 /sso
    #[test]
    fn root_cookie_is_not_sso_scoped() {
        assert!(!is_sso_scoped("JSESSIONID=ABC; Path=/; HttpOnly"));
        assert!(!is_sso_scoped("JSESSIONID=ABC"));
        assert!(!is_sso_scoped("SF_cookie_17=10086038; Path=/"));
    }

    /// CASTGC 应被拼进 Cookie 头，供 SSO 交换时携带
    #[test]
    fn castgc_is_appended_to_cookie_header() {
        let mut cookies = HashMap::new();
        cookies.insert("JSESSIONID".to_string(), "S1".to_string());
        let castgc = Some("CASTGC=TGT-1".to_string());

        // 复现 send_with_retry 中的拼接逻辑
        let mut header = build_cookie_header(&cookies);
        if let Some(c) = &castgc {
            if header.is_empty() {
                header.clone_from(c);
            } else {
                header.push_str("; ");
                header.push_str(c);
            }
        }

        assert!(header.contains("JSESSIONID=S1"));
        assert!(header.contains("CASTGC=TGT-1"));
    }
}
