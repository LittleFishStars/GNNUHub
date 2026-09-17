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
#[derive(Debug, Clone, Deserialize)]
struct TicketResponse {
    data: Option<TicketData>,
    meta: Option<TicketMeta>,
}

/// 成功时的 data 字段
#[derive(Debug, Clone, Deserialize)]
struct TicketData {
    /// 票据
    ticket: String,
}

/// 失败时的 meta 字段
#[derive(Debug, Clone, Deserialize)]
struct TicketMeta {
    /// 认证是否成功。`statusCode` 与 `message` 已足以判定结果，
    /// 保留该字段以便日志与后续扩展。
    #[serde(default)]
    #[allow(dead_code)]
    success: bool,
    #[serde(rename = "statusCode", default)]
    status_code: String,
    #[serde(default)]
    message: String,
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

    let response = client
        .http()
        .post(&url)
        .headers(client.cas_headers())
        .form(&form)
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        return Err(Error::UnexpectedStatus {
            status: status.as_u16(),
            url,
        });
    }

    let body = response.text().await?;
    tracing::debug!("票据接口原始响应: {}", truncate(&body, 300));

    parse_ticket_response(&body)
}

/// 解析票据接口的响应体
///
/// 抽成独立函数以便单元测试覆盖各种响应形态。
pub fn parse_ticket_response(body: &str) -> Result<LoginOutcome> {
    // 先尝试「成功」形态：{"tgt": "...", "ticket": "..."}
    if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(body)
        && let (Some(tgt), Some(ticket)) = (map.get("tgt"), map.get("ticket"))
    {
        return Ok(LoginOutcome::Success {
            tgt: tgt.clone(),
            ticket: ticket.clone(),
        });
    }

    // 再尝试带 meta 的形态
    let parsed: TicketResponse = serde_json::from_str(body).map_err(Error::Json)?;

    if let Some(meta) = parsed.meta {
        let status_code = meta.status_code.as_str();
        let message = if meta.message.is_empty() {
            status_code.to_string()
        } else {
            meta.message.clone()
        };

        // 验证码错误：可以换一张重试
        if status_code.eq_ignore_ascii_case("CODEFALSE")
            || message.contains("验证码")
            || message.to_lowercase().contains("code")
        {
            return Ok(LoginOutcome::CaptchaIncorrect);
        }

        // 凭据错误：重试无意义
        if status_code.eq_ignore_ascii_case("USERNAMEORPASSWORDERROR")
            || message.contains("密码")
            || message.contains("用户名")
        {
            return Ok(LoginOutcome::BadCredentials(message));
        }

        return Err(Error::TicketMissing(message));
    }

    if let Some(data) = parsed.data {
        return Ok(LoginOutcome::Success {
            // 部分版本不返回 tgt，此时用 ticket 占位
            tgt: String::new(),
            ticket: data.ticket,
        });
    }

    Err(Error::TicketMissing(truncate(body, 200).to_string()))
}

/// 用 ticket 换取教务系统的会话 Cookie
///
/// 对应 Python 版 `get_cookies` 与 `Student.__init__` 中的 SSO 跳转。
/// 返回教务系统域下的 Cookie 键值对。
pub async fn exchange_ticket_for_session(
    client: &Client,
    ticket: &str,
    castgc: &str,
    max_hops: u32,
) -> Result<HashMap<String, String>> {
    let mut cookies: HashMap<String, String> = HashMap::new();
    if !castgc.is_empty() {
        cookies.insert("CASTGC".to_string(), castgc.to_string());
    }

    let mut url = format!("{JWGL_BASE_URL}{SSO_LOGIN_PATH}?ticket={ticket}");
    let mut hops = 0;

    loop {
        hops += 1;
        if hops > max_hops {
            return Err(Error::LoginRetriesExhausted {
                attempts: max_hops,
            });
        }

        let response = client
            .http()
            .get(&url)
            .header(reqwest::header::COOKIE, build_cookie_header(&cookies))
            .send()
            .await?;

        let status = response.status();

        // 收集本跳的 Set-Cookie
        for value in response.headers().get_all(reqwest::header::SET_COOKIE) {
            if let Ok(text) = value.to_str()
                && let Some((name, val)) = parse_set_cookie(text)
            {
                cookies.insert(name, val);
            }
        }

        // 非重定向意味着已经落在目标页面
        if !status.is_redirection() {
            break;
        }

        let location = response
            .headers()
            .get(LOCATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| Error::SsoRedirect("重定向缺少 Location 头".to_string()))?;

        let next = if location.starts_with("http") {
            location.to_string()
        } else {
            format!("{JWGL_BASE_URL}{location}")
        };

        // SSO 往返过程中可能跳回 CAS 或跳往其他域，只有落在教务系统
        // 且不再是 sso 路径时才算完成
        let parsed = url::Url::parse(&next)
            .map_err(|e| Error::SsoRedirect(format!("非法的跳转地址 {next}: {e}")))?;
        let host = parsed.host_str().unwrap_or_default().to_string();

        if host == gnnuhub_core::CAS_HOST {
            // 跳回认证平台说明 ticket 未被接受
            return Err(Error::SsoRedirect(
                "ticket 未被教务系统接受，跳回了认证平台".to_string(),
            ));
        }
        if host != gnnuhub_core::JWGL_HOST {
            return Err(Error::SsoRedirect(format!(
                "跳转目标域不在教务系统内: {next}"
            )));
        }

        tracing::trace!("SSO 跳转 {} -> {}", url, next);
        url = next;
    }

    if !cookies.contains_key("JSESSIONID") {
        tracing::warn!("未获得 JSESSIONID，后续请求可能失败");
    }

    Ok(cookies)
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

    let response = client
        .http()
        .get(&url)
        .headers(client.cas_headers())
        .send()
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

    /// 验证码错误响应能被识别
    #[test]
    fn parses_captcha_error() {
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

    /// data 形态的成功响应也能解析
    #[test]
    fn parses_data_form_success() {
        let body = r#"{"data":{"ticket":"ST-fromdata"}}"#;
        let outcome = parse_ticket_response(body).unwrap();
        match outcome {
            LoginOutcome::Success { ticket, .. } => assert_eq!(ticket, "ST-fromdata"),
            other => panic!("期望 Success，实际 {other:?}"),
        }
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
}
