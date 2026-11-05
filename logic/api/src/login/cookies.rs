//! Cookie 解析与拼接
//!
//! 登录链路（[`super::sso`]）与会话请求（`Session::send`）共用的
//! Cookie 工具。所有 Cookie 都是**显式管理**的——reqwest 的
//! cookie_store 保持关闭（见 `Client::new` 的说明），这里的两个
//! 函数就是唯一的拼装/拆解入口。

use std::collections::HashMap;

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
///
/// # 以下情况返回 `None`（表示该 Cookie 不应被携带）
///
/// - 名称为空
/// - 值为 `DELETED`：教务系统用它标记失效
/// - 值为 `deleteMe`：实测把 `rememberMe=deleteMe` 带给服务端会导致
///   连接被直接关闭，必须剔除（见 `SESSION_REQUIRED_COOKIES` 相关测试）
pub fn parse_set_cookie(header: &str) -> Option<(String, String)> {
    let first = header.split(';').next()?;
    let (name, value) = first.split_once('=')?;
    let name = name.trim().to_string();
    let value = value.trim().to_string();

    if name.is_empty()
        || value.eq_ignore_ascii_case("DELETED")
        || value.eq_ignore_ascii_case("deleteMe")
    {
        return None;
    }
    Some((name, value))
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// `deleteMe` 的 Cookie 必须丢弃
    ///
    /// 实测把 `rememberMe=deleteMe` 带给服务端会导致连接被直接关闭，
    /// 因此不能在解析阶段放行。
    #[test]
    fn drops_deleteme_cookie() {
        assert!(
            parse_set_cookie("rememberMe=deleteMe; Path=/; Max-Age=0").is_none(),
            "deleteMe 是删除标记，携带它会触发服务端断开连接"
        );
        // 大小写不敏感
        assert!(parse_set_cookie("rememberMe=DELETEME; Path=/").is_none());
        // 但值只是「包含」这个词时不应误伤
        assert!(parse_set_cookie("rememberMe=deleteMeLater; Path=/").is_some());
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
}
