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
//!
//! # 内部划分
//!
//! - [`ticket`]：票据接口响应的解析（成功 / 验证码错 / 密码错 / 锁定）
//! - [`sso`]：用 ticket 换取教务系统会话 Cookie 的 3 跳交换
//! - [`cookies`]：Set-Cookie 解析与 Cookie 头拼接
//!
//! 以上子模块的公开项在本模块根再导出，调用方统一走
//! `login::函数名`。

mod cookies;
mod sso;
mod ticket;

pub use cookies::{build_cookie_header, parse_set_cookie};
pub use sso::exchange_ticket_for_session;
pub use ticket::parse_ticket_response;

use std::collections::HashMap;

use gnnuhub_core::{CAS_BASE_URL, Error, Result};
use gnnuhub_ocr::OcrEngine;

use crate::captcha::{Captcha, CaptchaResponse};
use crate::client::Client;

/// 票据接口路径
const TICKETS_PATH: &str = "/lyuapServer/v1/tickets";

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
    /// **账号已被锁定**，任何重试都会延长锁定时间
    ///
    /// 实测服务端返回 `{"data":{"code":"USERLOCK"}}`。触发条件是短时间内
    /// 连续多次账密错误。这不是「可以再试一次」的普通失败：继续提交只会
    /// 加重锁定，因此**必须**作为独立分支显式暴露给调用方，让上层立刻
    /// 停手，而不是混进通用错误里被当成可重试情形。
    AccountLocked(String),
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
        .throttled(|http| http.post(&url).headers(headers).form(&form))
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

/// 申请一张新验证码
pub async fn fetch_captcha(client: &Client) -> Result<Captcha> {
    let request_id = Captcha::random_request_id();
    let url = Captcha::request_url(&request_id);

    let headers = client.cas_headers();
    let response = client
        .throttled(|http| http.get(&url).headers(headers))
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
///
/// # 验证码识别失败时的行为
///
/// 识别器未命中字库时会返回错误，此处**直接向上抛**，不消耗重试次数。
/// 这是因为识别失败是本地问题（字库缺条目），与服务端无关：重试只会
/// 再取一张同样可能不认识的图，纯属浪费请求。
///
/// 真正需要重试的是服务端判定「验证码错误」——那说明这张图被读错了，
/// 换一张才有意义。
pub async fn login_with_retry(
    client: &Client,
    student_id: &str,
    password: &str,
    service: &str,
    ocr: &dyn OcrEngine,
) -> Result<LoginOutcome> {
    let max_attempts = client.config().max_login_retries;
    let encrypted_password = gnnuhub_crypto::encode_password(password);

    for attempt in 1..=max_attempts {
        let captcha = fetch_captcha(client).await?;
        tracing::debug!("第 {attempt} 次尝试登录，验证码 uid = {}", captcha.uid);

        let code = ocr.recognize(&captcha.image)?;

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
            LoginOutcome::AccountLocked(msg) => {
                // 账号已锁定，**绝不能重试**：继续提交只会加重锁定。
                tracing::error!("账号被锁定，立即停止: {msg}");
                return Ok(LoginOutcome::AccountLocked(msg));
            }
        }
    }

    Err(Error::LoginRetriesExhausted {
        attempts: max_attempts,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 截断函数不应切断 UTF-8
    #[test]
    fn truncate_respects_utf8_boundary() {
        let s = "中文测试";
        let out = truncate(s, 5);
        assert!(s.starts_with(out));
    }
}
