//! 统一错误类型定义

use thiserror::Error;

/// 项目统一 Result 别名
pub type Result<T> = std::result::Result<T, Error>;

/// GNNUHub 全部错误的统一枚举
#[derive(Debug, Error)]
pub enum Error {
    /// 网络请求失败（DNS、连接、超时等）
    #[error("网络请求失败: {0}")]
    Network(#[from] reqwest::Error),

    /// 服务端返回了非预期状态码
    #[error("服务端返回异常状态码 {status}: {url}")]
    UnexpectedStatus {
        /// HTTP 状态码
        status: u16,
        /// 请求的地址
        url: String,
    },

    /// 响应体不是合法的 JSON
    #[error("响应解析失败: {0}")]
    Json(#[from] serde_json::Error),

    /// 学号或密码错误
    #[error("学号或密码错误")]
    InvalidCredentials,

    /// 验证码识别错误或已过期
    #[error("验证码错误或已过期")]
    InvalidCaptcha,

    /// 登录票据获取失败（CAS 返回了非预期的结构）
    #[error("登录票据获取失败: {0}")]
    TicketMissing(String),

    /// SSO 跳转未能落在教务系统域内
    #[error("SSO 跳转异常，未落在教务系统域内: {0}")]
    SsoRedirect(String),

    /// 登录流程重试次数耗尽
    #[error("登录重试 {attempts} 次后仍然失败")]
    LoginRetriesExhausted {
        /// 已尝试的次数
        attempts: u32,
    },

    /// 验证码接口返回了空 uid 或空图片
    #[error("验证码接口响应异常: {0}")]
    CaptchaResponse(String),

    /// 会话未认证或已过期
    #[error("会话未认证或已过期，请重新登录")]
    Unauthenticated,

    /// 页面结构与预期不符（HTML 解析失败）
    #[error("页面解析失败，结构可能已变更: {0}")]
    PageParse(String),

    /// 缺少必需的个人信息字段
    #[error("缺少必需字段: {0}")]
    MissingField(String),

    /// 图像处理失败
    #[error("图像处理失败: {0}")]
    Image(String),

    /// 配置问题
    #[error("配置错误: {0}")]
    Config(String),

    /// 其他未归类的错误
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}
