//! 验证码获取
//!
//! 对应 Python 版 `login.py` 中的 `get_captcha` / `Captcha` 类。
//!
//! 接口返回结构（实测）：
//!
//! ```json
//! {
//!   "kaptchaType": "2",
//!   "uid": "34ae36e5ca2045a485d8ef781c19556f",
//!   "content": "data:image/png;base64,iVBORw0KGgo...",
//!   "timeout": 300
//! }
//! ```
//!
//! **注意**：请求时传入的 `id` 与响应中的 `uid` 不同，提交登录时必须
//! 使用响应里的 `uid`。Python 版在 `Captcha` 类中误用了请求时的 uid，
//! 这里已修正。

use serde::Deserialize;

use gnnuhub_core::{CAS_BASE_URL, Error, Result};

/// 验证码接口路径
const KAPTCHA_PATH: &str = "/lyuapServer/kaptcha";

/// 验证码接口的响应
#[derive(Debug, Clone, Deserialize)]
pub struct CaptchaResponse {
    /// 验证码类型，实测为 `"2"`
    #[serde(rename = "kaptchaType", default)]
    pub kaptcha_type: String,

    /// 服务端分配的验证码标识，提交登录时必须带上
    pub uid: String,

    /// 图片内容，`data:image/png;base64,...` 形式
    pub content: String,

    /// 有效期（秒）
    #[serde(default)]
    pub timeout: u64,
}

/// 一次验证码申请的完整信息
#[derive(Debug, Clone)]
pub struct Captcha {
    /// 服务端返回的验证码标识
    pub uid: String,
    /// 图片的 data URL
    pub image: String,
    /// 有效期
    pub timeout_secs: u64,
}

impl Captcha {
    /// 生成一个随机的请求 id
    ///
    /// 与原 Python 版 `token_hex(16)` 等价，即 16 字节随机数的
    /// 十六进制表示（32 个字符）。
    pub fn random_request_id() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};

        // 不引入额外依赖，用时间戳 + 地址熵混合出一个足够唯一的 id
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let stack_hint = &nanos as *const _ as usize as u128;
        let mut state = nanos ^ (stack_hint << 17);

        let mut out = String::with_capacity(32);
        for _ in 0..8 {
            // xorshift64 风格的简单混淆
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            out.push_str(&format!("{:08x}", (state & 0xFFFF_FFFF) as u32));
        }
        out.truncate(32);
        out
    }

    /// 从接口响应构造
    pub fn from_response(resp: CaptchaResponse) -> Result<Self> {
        if resp.uid.is_empty() || resp.content.is_empty() {
            return Err(Error::CaptchaResponse(
                "接口未返回 uid 或图片内容，可能是风控拦截或接口变更".to_string(),
            ));
        }
        Ok(Self {
            uid: resp.uid,
            image: resp.content,
            timeout_secs: resp.timeout,
        })
    }

    /// 构造验证码接口的完整 URL
    pub fn request_url(request_id: &str) -> String {
        format!("{CAS_BASE_URL}{KAPTCHA_PATH}?id={request_id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 请求 id 应为 32 位十六进制字符串
    #[test]
    fn request_id_is_32_hex_chars() {
        let id = Captcha::random_request_id();
        assert_eq!(id.len(), 32, "id 长度应为 32，实际 {}", id.len());
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// 连续生成的 id 不应相同
    #[test]
    fn request_ids_are_unique() {
        let a = Captcha::random_request_id();
        let b = Captcha::random_request_id();
        assert_ne!(a, b);
    }

    /// URL 拼接正确
    #[test]
    fn builds_request_url() {
        let url = Captcha::request_url("abc123");
        assert_eq!(url, "https://cas.gnnu.edu.cn/lyuapServer/kaptcha?id=abc123");
    }

    /// 空响应应被拒绝
    #[test]
    fn rejects_empty_response() {
        let resp = CaptchaResponse {
            kaptcha_type: "2".into(),
            uid: String::new(),
            content: String::new(),
            timeout: 300,
        };
        assert!(Captcha::from_response(resp).is_err());
    }

    /// 正常响应能被正确解析
    #[test]
    fn parses_valid_response() {
        let resp = CaptchaResponse {
            kaptcha_type: "2".into(),
            uid: "34ae36e5".into(),
            content: "data:image/png;base64,AAAA".into(),
            timeout: 300,
        };
        let cap = Captcha::from_response(resp).unwrap();
        assert_eq!(cap.uid, "34ae36e5");
        assert_eq!(cap.timeout_secs, 300);
    }

    /// JSON 反序列化应兼容真实响应结构
    #[test]
    fn deserializes_real_response_shape() {
        let raw = r#"{
            "kaptchaType": "2",
            "uid": "904fade5958c4468a37e883197ba3c99",
            "content": "data:image/png;base64,iVBORw0KGgoAAAANSU",
            "timeout": 300
        }"#;
        let resp: CaptchaResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(resp.uid, "904fade5958c4468a37e883197ba3c99");
        assert_eq!(resp.timeout, 300);
        assert_eq!(resp.kaptcha_type, "2");
    }

    /// 缺少可选字段时也应能解析
    #[test]
    fn tolerates_missing_optional_fields() {
        let raw = r#"{"uid":"abc","content":"data:image/png;base64,AA"}"#;
        let resp: CaptchaResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(resp.uid, "abc");
        assert_eq!(resp.timeout, 0);
        assert_eq!(resp.kaptcha_type, "");
    }
}
