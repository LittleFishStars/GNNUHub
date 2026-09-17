//! 赣南师范大学教务系统 API 客户端
//!
//! 本 crate 负责所有网络交互，对应 Python 版 `api.py` 与 `login.py`。
//!
//! # 登录流程
//!
//! 教务系统通过统一身份认证平台（CAS）接入，完整链路为：
//!
//! 1. 向 `cas.gnnu.edu.cn/lyuapServer/kaptcha` 申请验证码，得到
//!    图片与一个**服务端 uid**
//! 2. 对密码做 RSA 加密，连同验证码一起提交到
//!    `cas.gnnu.edu.cn/lyuapServer/v1/tickets`
//! 3. 认证成功后拿到 `TGT` 与 `ticket`
//! 4. 带着 `ticket` 请求 `jwgl.gnnu.edu.cn/sso/lyiotlogin`，
//!    跟随跳转换取教务系统的 `JSESSIONID`
//!
//! 第 4 步的跳转链共 3 跳，详见
//! [`login::exchange_ticket_for_session`] 的文档——其中有两处容易踩的坑：
//! 跳转必须在 `login_slogin` 之前停下（继续跟随会被服务端断开连接），
//! 且后续请求必须同时携带 `JSESSIONID` 与网关下发的 `SF_cookie_17`。
//!
//! 第 1 步也有个坑：申请验证码时传入的 `id` 与接口返回的
//! `uid` **不是同一个值**，提交时必须使用返回的 `uid`。
//!
//! # 示例
//!
//! 完整用法见 `examples/` 目录下的可运行示例。
//!
//! ```no_run
//! use gnnuhub_api::{Client, ClientConfig};
//! use gnnuhub_ocr::ManualOcr;
//!
//! async fn run() -> Result<(), Box<dyn std::error::Error>> {
//!     let client = Client::new(ClientConfig::default())?;
//!     let session = client
//!         .login(2500000001, "password", &ManualOcr::new(), None)
//!         .await?;
//!     println!("当前教学周: {:?}", session.this_week().await?);
//!     Ok(())
//! }
//! ```
//!
//! # 礼仪
//!
//! 教务系统前置了 SafeDog WAF，会因突发请求封禁来源 IP。客户端默认对
//! 请求做节流（见 [`ClientConfig::request_interval`]），**请勿关闭**，
//! 也请勿在无必要时反复登录试探接口。

pub mod captcha;
pub mod client;
pub mod login;
pub mod parse;
pub mod session;

pub use client::{Client, ClientConfig};
pub use login::LoginOutcome;
pub use session::Session;

pub use gnnuhub_core::{Error, Result};
