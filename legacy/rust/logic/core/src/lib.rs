//! 赣南师范大学教务系统接口 —— 核心类型与错误定义
//!
//! 本 crate 不包含任何网络请求逻辑，只提供被其他 crate 共享的
//! 数据类型、错误枚举与常量。

pub mod error;
pub mod model;

pub use error::{Error, Result};

/// 统一身份认证平台（CAS）主机
pub const CAS_HOST: &str = "cas.gnnu.edu.cn";

/// 教务系统主机
pub const JWGL_HOST: &str = "jwgl.gnnu.edu.cn";

/// 统一身份认证平台根地址
pub const CAS_BASE_URL: &str = "https://cas.gnnu.edu.cn";

/// 教务系统根地址
pub const JWGL_BASE_URL: &str = "https://jwgl.gnnu.edu.cn";
