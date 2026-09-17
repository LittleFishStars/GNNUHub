//! HTTP 客户端构造与配置

use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue};

use gnnuhub_core::{Error, Result, CAS_HOST, JWGL_BASE_URL};

/// 默认请求超时
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// 默认 User-Agent
const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36";

/// 默认的相邻请求最小间隔
///
/// 取值偏保守：教务系统的 WAF 对突发请求较敏感，而正常使用场景
/// （登录 + 拉取课表）总共也就十几次请求，多等待一两秒完全可接受。
const DEFAULT_REQUEST_INTERVAL: Duration = Duration::from_millis(600);

/// 客户端配置
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// 请求超时
    pub timeout: Duration,
    /// User-Agent
    pub user_agent: String,
    /// 登录流程最多重试次数（含首次尝试之后的重试）
    ///
    /// Python 版在验证码错误时无限递归、在 SSO 跳转失败时 `while True`，
    /// 存在卡死风险。这里强制要求设置上限。
    pub max_login_retries: u32,
    /// 相邻两次请求之间的最小间隔
    ///
    /// 教务系统前置了 SafeDog WAF，短时间高频请求会触发按 IP 的封禁
    /// （表现为域名整体在数十毫秒内拒绝连接）。默认加一个温和的间隔
    /// 以免杀伤自己；设为 [`Duration::ZERO`] 可关闭。
    pub request_interval: Duration,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            user_agent: DEFAULT_USER_AGENT.to_string(),
            max_login_retries: 5,
            request_interval: DEFAULT_REQUEST_INTERVAL,
        }
    }
}

impl ClientConfig {
    /// 设置请求超时
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// 设置登录重试上限
    pub fn with_max_login_retries(mut self, retries: u32) -> Self {
        self.max_login_retries = retries.max(1);
        self
    }

    /// 设置相邻请求的最小间隔
    pub fn with_request_interval(mut self, interval: Duration) -> Self {
        self.request_interval = interval;
        self
    }
}

/// API 客户端，持有复用的 HTTP 连接池与 Cookie 容器
#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    config: ClientConfig,
    /// 上次发起请求的时刻，用于实现请求节流
    ///
    /// 用 `Arc<Mutex<_>>` 而非 `AtomicU64`，因为需要「读取→等待→写入」
    /// 这段临界区整体串行，避免并发请求各自算出一个间隔而叠加超发。
    last_request: std::sync::Arc<tokio::sync::Mutex<Option<std::time::Instant>>>,
}

impl Client {
    /// 按配置创建客户端
    ///
    /// # 错误
    ///
    /// User-Agent 含非法字符或底层 TLS 初始化失败时返回错误。
    pub fn new(config: ClientConfig) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::USER_AGENT,
            HeaderValue::from_str(&config.user_agent)
                .map_err(|e| Error::Config(format!("非法的 User-Agent: {e}")))?,
        );
        headers.insert(
            reqwest::header::ACCEPT_LANGUAGE,
            HeaderValue::from_static("zh-CN,zh;q=0.9,en;q=0.8"),
        );

        let http = reqwest::Client::builder()
            .timeout(config.timeout)
            .default_headers(headers)
            // 手动管理重定向：SSO 跳转的每一跳都需要检查目标域
            .redirect(reqwest::redirect::Policy::none())
            .cookie_store(true)
            .build()
            .map_err(Error::Network)?;

        Ok(Self {
            http,
            config,
            last_request: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
        })
    }

    /// 使用默认配置创建客户端
    pub fn with_defaults() -> Result<Self> {
        Self::new(ClientConfig::default())
    }

    /// 访问底层 HTTP 客户端
    ///
    /// 直接使用它会**绕过请求节流**。需要节流时请走
    /// [`Client::throttled`]。
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// 访问配置
    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    /// 发起一次受节流约束的请求
    ///
    /// 在真正发送前，若距离上一次请求不足
    /// [`ClientConfig::request_interval`]，会先等待补足。
    ///
    /// 教务系统的 WAF 会因突发请求封禁来源 IP，因此**所有**对校内
    /// 接口的访问都应经由本方法，而不是直接使用 [`Client::http`]。
    pub async fn throttled<F>(&self, build: F) -> Result<reqwest::Response>
    where
        F: FnOnce(&reqwest::Client) -> reqwest::RequestBuilder,
    {
        let mut last = self.last_request.lock().await;
        let interval = self.config.request_interval;

        if !interval.is_zero()
            && let Some(prev) = *last
        {
            let elapsed = prev.elapsed();
            if elapsed < interval {
                let wait = interval - elapsed;
                tracing::trace!("请求节流：等待 {} ms", wait.as_millis());
                tokio::time::sleep(wait).await;
            }
        }

        let response = build(&self.http).send().await.map_err(Error::Network)?;
        // 只有请求真正发出后才更新时刻，避免失败重试被误判为「已节流」
        *last = Some(std::time::Instant::now());
        Ok(response)
    }

    /// 构造统一认证平台接口所需的请求头
    ///
    /// 对应 Python 版 `login.py` 的 `_get_headers`。其中
    /// `loginUserToken` 需要对当前时间戳做 RSA 加密。
    ///
    /// # 重要
    ///
    /// 返回的头部**只能用于 `cas.gnnu.edu.cn`**：其中包含
    /// `Host: cas.gnnu.edu.cn`，若被复用到教务系统等其他域名的请求上，
    /// 会造成 Host 与 SNI 不匹配，服务端会直接关闭 TLS 连接
    /// （表现为 `peer closed connection without sending TLS close_notify`）。
    ///
    /// 这个 `Host` 覆盖是必需的：CAS 接口所在的反向代理只认
    /// `Host: cas.gnnu.edu.cn`，用默认推导值会被拒绝。
    ///
    /// 若确实需要跨域复用，请改用 [`Client::cas_headers_for`]。
    pub fn cas_headers(&self) -> HeaderMap {
        self.cas_headers_for(CAS_HOST)
    }

    /// 构造统一认证平台接口所需的请求头，并指定 `Host`
    ///
    /// 仅在需要覆盖 `Host` 时使用；一般场景请用 [`Client::cas_headers`]。
    pub fn cas_headers_for(&self, host: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Requested-With",
            HeaderValue::from_static("XMLHttpRequest"),
        );
        if let Ok(token) = HeaderValue::from_str(&gnnuhub_crypto::login_user_token()) {
            headers.insert("loginUserToken", token);
        }
        headers.insert("loginToken", HeaderValue::from_static("loginToken"));
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            HeaderValue::from_static("application/x-www-form-urlencoded;charset=utf-8"),
        );
        if let Ok(host) = HeaderValue::from_str(host) {
            headers.insert(reqwest::header::HOST, host);
        }
        headers.insert(reqwest::header::CONNECTION, HeaderValue::from_static("keep-alive"));
        headers
    }

    /// 登录并返回已认证的会话
    ///
    /// 完整执行「申请验证码 → 提交票据 → 换取教务系统 Cookie」三步，
    /// 验证码识别错误时自动重试，次数受
    /// [`ClientConfig::max_login_retries`] 限制。
    ///
    /// # 参数
    ///
    /// - `student_id`：学号
    /// - `password`：明文密码（内部会做 RSA 加密）
    /// - `ocr`：验证码识别器
    /// - `interactive`：需要人工输入验证码时使用的回调
    ///
    /// # 错误
    ///
    /// - [`Error::InvalidCredentials`]：学号或密码错误
    /// - [`Error::LoginRetriesExhausted`]：重试次数耗尽
    /// - [`Error::Unauthenticated`]：票据交换后未获得有效会话
    ///
    /// # 示例
    ///
    /// ```no_run
    /// use gnnuhub_api::Client;
    /// use gnnuhub_ocr::ManualOcr;
    ///
    /// async fn run() -> Result<(), Box<dyn std::error::Error>> {
    ///     let client = Client::with_defaults()?;
    ///     let session = client
    ///         .login(20250710088, "password", &ManualOcr::new(), None)
    ///         .await?;
    ///     println!("登录成功: {}", session.student_id());
    ///     Ok(())
    /// }
    /// ```
    pub async fn login(
        &self,
        student_id: u64,
        password: &str,
        ocr: &dyn gnnuhub_ocr::OcrEngine,
        interactive: Option<&gnnuhub_ocr::InteractiveFn>,
    ) -> Result<crate::session::Session> {
        // 教务系统的 service 参数固定指向教务系统首页
        let service = format!("{JWGL_BASE_URL}/");
        let student_id_str = student_id.to_string();

        let outcome = crate::login::login_with_retry(
            self,
            &student_id_str,
            password,
            &service,
            ocr,
            interactive,
        )
        .await?;

        let (tgt, ticket) = match outcome {
            crate::login::LoginOutcome::Success { tgt, ticket } => (tgt, ticket),
            crate::login::LoginOutcome::BadCredentials(_) => {
                return Err(Error::InvalidCredentials);
            }
            crate::login::LoginOutcome::CaptchaIncorrect => {
                return Err(Error::InvalidCaptcha);
            }
        };

        let cookies =
            crate::login::exchange_ticket_for_session(self, &ticket, &tgt, self.config.max_login_retries)
                .await?;

        if cookies.is_empty() {
            return Err(Error::Unauthenticated);
        }

        tracing::info!("登录成功，学号 {student_id}，获得 {} 个 Cookie", cookies.len());
        Ok(crate::session::Session::new(
            self.clone(),
            cookies,
            student_id_str,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 默认配置应能成功构造客户端
    #[test]
    fn builds_with_defaults() {
        let client = Client::with_defaults();
        assert!(client.is_ok());
    }

    /// 重试上限不应被设为 0
    #[test]
    fn retries_never_zero() {
        let cfg = ClientConfig::default().with_max_login_retries(0);
        assert_eq!(cfg.max_login_retries, 1);
    }

    /// 配置能正确覆盖超时
    #[test]
    fn overrides_timeout() {
        let cfg = ClientConfig::default().with_timeout(Duration::from_secs(5));
        assert_eq!(cfg.timeout.as_secs(), 5);
    }

    /// 默认配置应带有非零的请求间隔，避免触发 WAF 封禁
    #[test]
    fn default_interval_is_nonzero() {
        let cfg = ClientConfig::default();
        assert!(
            !cfg.request_interval.is_zero(),
            "默认必须节流，否则容易触发教务系统的 WAF 封禁"
        );
    }

    /// 请求间隔可被覆盖，也可显式关闭
    #[test]
    fn overrides_request_interval() {
        let cfg = ClientConfig::default().with_request_interval(Duration::from_millis(50));
        assert_eq!(cfg.request_interval.as_millis(), 50);

        let off = ClientConfig::default().with_request_interval(Duration::ZERO);
        assert!(off.request_interval.is_zero());
    }

    /// CAS 请求头应包含必需字段
    #[test]
    fn cas_headers_contain_required_fields() {
        let client = Client::with_defaults().unwrap();
        let headers = client.cas_headers();
        assert!(headers.contains_key("X-Requested-With"));
        assert!(headers.contains_key("loginUserToken"));
        assert!(headers.contains_key("loginToken"));
        assert!(headers.contains_key(reqwest::header::CONTENT_TYPE));
        assert!(headers.contains_key(reqwest::header::HOST));
    }

    /// `cas_headers` 的 Host 必须指向认证平台
    ///
    /// 这个头部只对 CAS 域有效；复用给其他域名会导致 TLS 握手被服务端关闭。
    #[test]
    fn cas_headers_host_is_cas() {
        let client = Client::with_defaults().unwrap();
        let headers = client.cas_headers();
        assert_eq!(
            headers.get(reqwest::header::HOST).unwrap(),
            CAS_HOST,
            "cas_headers 的 Host 必须是认证平台域名"
        );
    }

    /// `cas_headers_for` 应能覆盖 Host，供跨域场景使用
    #[test]
    fn cas_headers_for_overrides_host() {
        let client = Client::with_defaults().unwrap();
        let headers = client.cas_headers_for("example.com");
        assert_eq!(headers.get(reqwest::header::HOST).unwrap(), "example.com");
    }

    /// loginUserToken 应是合法的 HeaderValue（不含空格等非法字符）
    #[test]
    fn login_user_token_is_valid_header() {
        let token = gnnuhub_crypto::login_user_token();
        assert!(HeaderValue::from_str(&token).is_ok());
    }
}
