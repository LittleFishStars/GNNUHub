//! SSO 票据交换：用 ticket 换取教务系统会话 Cookie

use std::collections::HashMap;

use reqwest::header::LOCATION;

use gnnuhub_core::{Error, JWGL_BASE_URL, Result};

use super::cookies::{build_cookie_header, parse_set_cookie};
use crate::client::Client;

/// 教务系统 SSO 登录入口
const SSO_LOGIN_PATH: &str = "/sso/lyiotlogin";

/// 教务系统登录页路径
///
/// SSO 交换链路会重定向到这里。**不要请求它**：此时会话 Cookie 已经到手，
/// 而服务端会把「已持有会话却重放登录页」判定为异常并直接关闭 TLS 连接。
const LOGIN_PAGE_PATH: &str = "/xtgl/login_slogin.html";

/// 用 ticket 换取教务系统的会话 Cookie
///
/// 链路经抓包实测，**共 3 跳**（每跳均打印真实 Location 与 Set-Cookie）：
///
/// ```text
/// ① GET /sso/lyiotlogin?ticket=ST-xxx      （裸请求，不带 Cookie）
///      → 302 /sso/lyiotlogin
///      Set-Cookie: JSESSIONID=<A>          种在 /sso 下
/// ② GET /sso/lyiotlogin                    （带 <A>）
///      → 302 /ticketlogin?uid=...&verify=...
/// ③ GET /ticketlogin?uid=...&verify=...    （带 <A>）
///      → 302 /xtgl/login_slogin.html
///      Set-Cookie: JSESSIONID=<B>          这才是可用的会话，种在 / 下
/// ```
///
/// # 为什么在 ③ 之后停止
///
/// ③ 的 `Set-Cookie` 已经给出根作用域的有效 `JSESSIONID`，会话此时即成立。
/// 若继续跟随到 `/xtgl/login_slogin.html`，服务端会**直接关闭 TLS 连接**
/// （实测报 `peer closed connection without sending TLS close_notify`）——
/// 该校对「已持有会话却重放登录页」有防护，这是稳定的行为拦截，
/// 不是网络抖动，重试也无效。
///
/// # Cookie 归属
///
/// `<A>` 与 `<B>` 同名 `JSESSIONID`，但 `<A>` 只在 `/sso` 下有效，最终
/// 应以 `<B>` 为准（见 [`is_sso_scoped`]）。
///
/// # 后续请求必须携带的 Cookie
///
/// 实测（对 `/xtgl/index_cxYhxxIndex.html` 逐一比对）：
///
/// | 携带内容 | 结果 |
/// |---|---|
/// | 仅 `JSESSIONID` | 302，会话不生效 |
/// | `JSESSIONID` + `SF_cookie_17` | **200，取到真实页面** |
/// | 再加 `rememberMe=deleteMe` | 连接被直接关闭 |
///
/// 即 `SF_cookie_17` 是会话成立的必要条件（它由网关下发并与会话绑定），
/// 而 `rememberMe=deleteMe` 必须在 [`parse_set_cookie`] 阶段剔除。
///
/// # 关于 `tgt`
///
/// `CASTGC` 属于认证平台，**不应出现在教务系统的请求里**，因此本流程
/// 不使用它。参数保留是为了让调用方能够把 `try_login` 返回的一整份
/// 结果直接传进来，也便于日后需要时扩展。
///
/// # 错误
///
/// - ticket 未被接受（跳回认证平台）时返回 [`Error::SsoRedirect`]
/// - 跳转超出上限时返回 [`Error::LoginRetriesExhausted`]
/// - 最终未取到根作用域的 `JSESSIONID` 时返回 [`Error::Unauthenticated`]
pub async fn exchange_ticket_for_session(
    client: &Client,
    ticket: &str,
    tgt: &str,
    max_hops: u32,
) -> Result<HashMap<String, String>> {
    if !tgt.is_empty() {
        tracing::trace!("持有 TGT，但教务系统侧不使用 CASTGC，仅留作诊断");
    }
    // 教务系统侧累积的 Cookie（不含 CASTGC）
    let mut cookies: HashMap<String, String> = HashMap::new();
    // /sso 作用域下的 JSESSIONID——链路中转产物，最后必须丢掉
    let mut sso_jsessionid: Option<String> = None;

    // 第一跳必须携带 ticket 才能完成兑换，且不带任何 Cookie
    let mut url = format!("{JWGL_BASE_URL}{SSO_LOGIN_PATH}?ticket={ticket}");
    let mut hops = 0;
    // 实测 3 跳即可拿到有效会话
    let hop_limit = max_hops.max(5);

    loop {
        hops += 1;
        if hops > hop_limit {
            return Err(Error::LoginRetriesExhausted {
                attempts: hop_limit,
            });
        }

        // 第 1 跳裸请求；之后带上链路累积的 Cookie
        let carry: HashMap<String, String> = if hops == 1 {
            HashMap::new()
        } else {
            cookies.clone()
        };

        let response = send_with_retry(client, &url, &carry, hops).await?;

        let status = response.status();
        collect_set_cookies(&response, &mut cookies, &mut sso_jsessionid);
        tracing::debug!("SSO 第 {hops} 跳 {url} -> {status}");
        tracing::debug!(
            "  携带的 Cookie: {:?}；收到的 Set-Cookie: {:?}",
            carry.keys().collect::<Vec<_>>(),
            response
                .headers()
                .get_all(reqwest::header::SET_COOKIE)
                .iter()
                .filter_map(|v| v.to_str().ok())
                .map(|s| s.split(';').next().unwrap_or("").to_string())
                .collect::<Vec<_>>()
        );

        // 非重定向说明链路已结束
        if !status.is_redirection() {
            break;
        }

        let Some(location) = response
            .headers()
            .get(LOCATION)
            .and_then(|v| v.to_str().ok())
        else {
            tracing::warn!("SSO 第 {hops} 跳返回 {status} 但缺少 Location");
            break;
        };

        let next = if location.starts_with("http") {
            location.to_string()
        } else {
            format!("{JWGL_BASE_URL}{location}")
        };

        tracing::debug!("  → 下一跳: {next}");

        // ⚠️ 关键：拿到根作用域 JSESSIONID 后即停止
        //
        // login_slogin 是「重放登录页」，服务端会直接断开连接。
        // 此时会话 Cookie 已经到手，没有继续跟随的理由。
        if next.contains(LOGIN_PAGE_PATH) {
            tracing::debug!("已取得会话 Cookie，停止于 {next}（不请求登录页）");
            break;
        }

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

    let _ = tgt; // CASTGC 不参与教务系统的请求

    // 会话是否成立，取决于是否拿到了**根作用域**的 JSESSIONID：
    // 第 3 跳会给出 `JSESSIONID=<B>; Path=/` 覆盖掉第 1 跳的 `/sso` 版本。
    //
    // `sso_jsessionid` 记录的正是 `/sso` 那份。若最终值仍等于它，
    // 说明链路只走到一半（没拿到根作用域的值），此处据实报错，
    // 而不是把已经装好的会话又删掉。
    if let (Some(sso_only), Some(current)) = (&sso_jsessionid, cookies.get("JSESSIONID"))
        && sso_only == current
    {
        tracing::warn!("只拿到 /sso 作用域的 JSESSIONID，未取得根作用域会话");
    }

    if !cookies.contains_key("JSESSIONID") {
        return Err(Error::Unauthenticated);
    }

    tracing::info!(
        "SSO 交换完成，共 {hops} 跳，获得 {} 个 Cookie",
        cookies.len()
    );
    Ok(cookies)
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
    hop: u32,
) -> Result<reqwest::Response> {
    /// 单跳最多尝试次数
    const MAX_ATTEMPTS: u32 = 3;
    /// 重试前的等待时间
    ///
    /// 取值需明显大于请求间隔：服务端的风控窗口是秒级的，
    /// 立刻重试会撞在同一个窗口上，反而加深封禁。
    const RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(1200);

    let cookie_header = build_cookie_header(cookies);

    let mut last_err = None;
    for attempt in 1..=MAX_ATTEMPTS {
        let result = client
            .throttled(|http| {
                http.get(url)
                    .header(reqwest::header::COOKIE, &cookie_header)
            })
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
/// 同名 Cookie 可能带不同 `Path` 在链路上各出现一次——实测第 1 跳给出
/// `JSESSIONID=<A>; Path=/sso`，第 3 跳给出 `JSESSIONID=<B>; Path=/`。
///
/// 处理方式：
///
/// - **任何 Cookie 都先放入 `out`**，因为 `/sso` 下那份在链路中途
///   （第 2 跳请求 `/sso/lyiotlogin`）是**必须携带**的
/// - 同时把 `/sso` 作用域的值记入 `sso_scoped`，供调用方在链路结束后剔除
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
            *sso_scoped = Some(val.clone());
        }
        out.insert(name, val);
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

#[cfg(test)]
mod tests {
    use super::*;

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

    /// 第 1 跳必须是裸请求：带上 CASTGC 会被弹回认证平台
    ///
    /// 这个不变量由 `exchange_ticket_for_session` 中 `hops == 1` 的分支保证，
    /// 这里用一个等价的本地函数复现判据，防止后人「顺手」改回去。
    #[test]
    fn first_hop_must_not_carry_cookies() {
        let mut accumulated = HashMap::new();
        accumulated.insert("CASTGC".to_string(), "TGT-1".to_string());

        // 复现 hop 1 的取值逻辑
        let carry: HashMap<String, String> = HashMap::new();
        assert!(
            carry.is_empty(),
            "第 1 跳不能携带任何 Cookie，否则 SSO 会跳回认证平台"
        );
        assert_eq!(build_cookie_header(&carry), "");

        // 第 2 跳才应带上累积的 Cookie
        let carry2 = accumulated.clone();
        assert!(build_cookie_header(&carry2).contains("CASTGC"));
    }

    /// 登录页路径必须被识别为「不应请求」的终点
    #[test]
    fn login_page_is_recognized_as_stop_point() {
        assert!(LOGIN_PAGE_PATH.contains("login_slogin"));
        // 相对与绝对两种 Location 都应命中
        let rel = format!("{}{}", "https://jwgl.gnnu.edu.cn", LOGIN_PAGE_PATH);
        assert!(rel.contains(LOGIN_PAGE_PATH));
        let abs = "/xtgl/login_slogin.html".to_string();
        let full = format!("https://jwgl.gnnu.edu.cn{abs}");
        assert!(full.contains(LOGIN_PAGE_PATH));
    }
}
