//! 票据接口响应的解析

use serde::Deserialize;

use gnnuhub_core::{Error, Result};

use super::truncate;
use crate::login::LoginOutcome;

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
    /// 业务状态码，成功时为空或 "SUCCESS"，验证码错误时为 "CODEFALSE"，
    /// 密码错误时为 "PASSERROR"
    #[serde(default)]
    code: String,
    /// 业务附加数据。实测 `code == "PASSERROR"` 时这里是 `"5,3"`，
    /// 含义为「剩余可尝试次数,本次已用次数」。
    #[serde(default)]
    data: Option<String>,
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

/// 解析票据接口的响应体
///
/// 抽成独立函数以便单元测试覆盖各种响应形态。
///
/// # 判定逻辑
///
/// 1. 顶层同时存在 `tgt` 与 `ticket` → 成功
/// 2. 存在 `meta`：取 `statusCode` 与 `message` 判定失败原因
/// 3. 存在 `data.code`：`CODEFALSE` = 验证码错误，`PASSERROR` = 密码错误
///
/// # 两个业务码都不能漏
///
/// 这两个码**必须分开映射**，否则验证码识别率的真值验证无法进行：
///
/// - `CODEFALSE` → [`LoginOutcome::CaptchaIncorrect`]：**验证码读错了**
/// - `PASSERROR` → [`LoginOutcome::BadCredentials`]：**验证码读对了**，只是账密不符
///
/// 早期版本只处理了 `CODEFALSE`，`PASSERROR` 落到「未识别的业务码」分支
/// 报错。后果是拿错密码做真值验证时，第一轮就被当成异常中止，一个有效
/// 样本都拿不到。
pub fn parse_ticket_response(body: &str) -> Result<LoginOutcome> {
    // 先尝试「成功」形态：{"tgt": "...", "ticket": "..."}
    if let Ok(map) = serde_json::from_str::<std::collections::HashMap<String, serde_json::Value>>(body)
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
        // 实测形态：`{"data":{"code":"PASSERROR","data":"5,3"}}`
        // 说明**验证码是对的**，只是账密不匹配。`data` 里是剩余次数。
        if code.eq_ignore_ascii_case("PASSERROR") {
            let detail = match data.data.as_deref().map(str::trim) {
                Some(d) if !d.is_empty() => format!("密码错误（{d} = 剩余次数,已用次数）"),
                _ => "密码错误".to_string(),
            };
            return Ok(LoginOutcome::BadCredentials(detail));
        }
        // 实测形态：`{"data":{"code":"USERLOCK"}}`
        // 账号被锁定。**绝不可当作可重试失败**：继续提交会加重锁定。
        if code.eq_ignore_ascii_case("USERLOCK") {
            return Ok(LoginOutcome::AccountLocked(
                "账号已被锁定（USERLOCK），请等待解锁后再试，期间不要重复提交".to_string(),
            ));
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

    /// `PASSERROR` 必须识别为密码错误，而不是「未识别的业务码」
    ///
    /// 这是**真实抓到的响应原文**。漏掉它的后果：用错密码做验证码真值
    /// 验证时，第一轮就被当成异常中止，拿不到任何有效样本。
    #[test]
    fn parses_pass_error_from_real_response() {
        let body = r#"{"meta":{"success":true,"statusCode":200,"message":"ok"},"data":{"code":"PASSERROR","data":"5,3"}}"#;
        let outcome = parse_ticket_response(body).unwrap();
        match outcome {
            LoginOutcome::BadCredentials(detail) => {
                assert!(
                    detail.contains("5,3"),
                    "应把剩余次数带进详情，实际 {detail}"
                );
            }
            other => panic!("期望 BadCredentials，实际 {other:?}"),
        }
    }

    /// `USERLOCK` 必须识别为账号锁定，而不是「未识别的业务码」
    ///
    /// 这是**真实抓到的响应**。早期版本把它落到通用错误分支，调用方无法
    /// 区分「可重试」与「禁止重试」，会继续提交从而加重锁定。
    #[test]
    fn parses_account_lock_from_real_response() {
        let body = r#"{"meta":{"success":true,"statusCode":200,"message":"ok"},"data":{"code":"USERLOCK"}}"#;
        let outcome = parse_ticket_response(body).unwrap();
        assert!(
            matches!(outcome, LoginOutcome::AccountLocked(_)),
            "USERLOCK 必须判为账号锁定，实际 {outcome:?}"
        );
    }

    /// `PASSERROR` 不带附加 data 时也应正常识别
    #[test]
    fn parses_pass_error_without_extra_data() {
        let body = r#"{"data":{"code":"PASSERROR"}}"#;
        let outcome = parse_ticket_response(body).unwrap();
        assert!(matches!(outcome, LoginOutcome::BadCredentials(_)));
    }

    /// 带附加 data 的 CODEFALSE 仍应判为验证码错误
    ///
    /// 防止「加了 data 字段后误判分支」。CODEFALSE 与 PASSERROR 必须
    /// 走向相反的两个结论，这是真值验证的基石。
    #[test]
    fn codefalse_still_beats_pass_error_branch() {
        let body = r#"{"meta":{"success":true,"statusCode":200,"message":"ok"},"data":{"code":"CODEFALSE","data":"1,4"}}"#;
        let outcome = parse_ticket_response(body).unwrap();
        assert!(
            matches!(outcome, LoginOutcome::CaptchaIncorrect),
            "CODEFALSE 必须判为验证码错误，实际 {outcome:?}"
        );
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
}
