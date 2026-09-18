//! UI 无关的应用服务层
//!
//! # 分层规则
//!
//! ```text
//! logic/core / logic/api / logic/ocr
//!         业务核心：领域模型 + 教务系统客户端 + 验证码识别
//!                    （不知道任何 UI 的存在）
//!                       ↑
//! logic/service（本 crate）
//!         应用用例：会话生命周期、用例编排、错误分类
//!                    （不知道任何 UI 的存在，只依赖 tokio）
//!                       ↑
//! ui/desktop 等各平台 UI 实现
//!         gnnuhub-desktop（Tauri 桌面）、未来的 CLI / 移动端 /
//!         Web 服务端等，各占 ui/ 下一个目录
//! ```
//!
//! 各 UI 实现**只应依赖本 crate**（加上 [`model`] 里的领域模型），
//! 不要绕过它直接调用 `gnnuhub-api`——否则会话状态、登录流程与
//! 错误分类就会在每个 UI 里各自为政、越写越散。
//!
//! 展示层面的决定（错误文案如何呈现、重试按钮是否可用）由各 UI
//! 自行处理；[`ServiceError::retry_safe`] 提供统一的「重试是否有害」
//! 判据，特别是**账号锁定时重试会加重锁定**这一硬约束。
//!
//! # 异步约定
//!
//! 所有方法都是 `async`，内部互斥锁用 tokio（可跨 `.await` 持有），
//! 与 Tauri 内置的 tokio 运行时天然兼容；CLI / 服务端复用时用
//! `#[tokio::main]` 或自带 runtime 驱动即可，本层不做任何运行时假设。

use gnnuhub_api::{Client, Session};
use gnnuhub_core::model::{AcademicTerm, ClassSchedule, StudentInfo};
use gnnuhub_core::{Error, Result};
use gnnuhub_ocr::BitmapOcr;
use tokio::sync::Mutex;

// 再导出领域模型：UI 实现依赖本 crate 即可拿到全部数据类型，
// 不必（也不该）直接依赖 gnnuhub-core。
pub use gnnuhub_core::model;

/// 应用服务的统一错误
///
/// 在 [`Error`]（业务核心错误）之上补充了两个 UI 才关心的情形：
/// 未登录就调用受保护的操作、学号格式不合法。
///
/// Display 文案是中文、面向最终用户的，各 UI 可直接展示；
/// 需要程序化分类时用变体匹配或 [`ServiceError::retry_safe`]。
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    /// 未登录就调用了需要会话的操作
    #[error("尚未登录，请先登录")]
    NotLoggedIn,

    /// 学号格式不合法（应为纯数字）
    #[error("学号必须是纯数字")]
    InvalidStudentId,

    /// 业务核心错误（网络、凭据、验证码、账号锁定等）
    #[error(transparent)]
    Core(#[from] Error),
}

impl ServiceError {
    /// 此时重试同一操作是否安全
    ///
    /// - **不安全**：凭据错误（重试必然再错）、账号锁定
    ///   （重试会加重锁定，[`Error::TicketMissing`] 承载该语义）、
    ///   未登录 / 学号非法（调用本身有问题，重试无意义）；
    /// - **安全**：网络抖动、验证码识别错误（下次登录会换新码）等。
    pub fn retry_safe(&self) -> bool {
        match self {
            Self::NotLoggedIn | Self::InvalidStudentId => false,
            Self::Core(Error::InvalidCredentials | Error::TicketMissing(_)) => false,
            Self::Core(_) => true,
        }
    }
}

/// UI 无关的应用服务
///
/// 持有登录会话并提供用例级操作。设计上**可克隆共享**是没必要的——
/// 它内部只有 `Arc` 化的锁，各 UI 直接把它放进自己的状态管理
/// （例如 Tauri 的 `.manage()`）即可。
pub struct HubService {
    /// 验证码识别器（内嵌字库，构造一次全程复用）
    ocr: BitmapOcr,
    /// 登录后的会话；`None` 表示未登录
    session: Mutex<Option<Session>>,
}

impl HubService {
    /// 创建服务
    ///
    /// 只做本地初始化（加载内嵌验证码字库），**不发起任何网络请求**；
    /// 字库损坏时返回 [`Error::Image`] 类错误。
    pub fn new() -> Result<Self> {
        Ok(Self {
            ocr: BitmapOcr::embedded()?,
            session: Mutex::new(None),
        })
    }

    /// 用学号 + 密码登录，成功后返回学号
    ///
    /// 验证码由内嵌位图字库自动识别，识别错误自动重试（上限见
    /// `ClientConfig::max_login_retries`）。密码只在本调用内使用，
    /// 不落盘、不进日志。
    ///
    /// # 错误
    ///
    /// - [`ServiceError::InvalidStudentId`]：学号不是纯数字；
    /// - [`ServiceError::Core`]：凭据错误、验证码失败、**账号锁定**
    ///   （此时 [`ServiceError::retry_safe`] 为 `false`，UI 不应提供
    ///   「重试」）等，见 [`Error`] 各变体。
    pub async fn login(
        &self,
        student_id: &str,
        password: &str,
    ) -> std::result::Result<String, ServiceError> {
        let id: u64 = student_id
            .trim()
            .parse()
            .map_err(|_| ServiceError::InvalidStudentId)?;

        let client = Client::with_defaults()?;
        let session = client
            .login(id, password.trim(), &self.ocr)
            .await
            .map_err(ServiceError::from)?;

        let logged_in_as = session.student_id().to_string();
        *self.session.lock().await = Some(session);
        tracing::info!("用户登录成功: {logged_in_as}");
        Ok(logged_in_as)
    }

    /// 退出登录（丢弃会话；幂等）
    pub async fn logout(&self) {
        *self.session.lock().await = None;
        tracing::info!("用户已退出登录");
    }

    /// 当前登录的学号（未登录返回 `None`）
    pub async fn logged_in_as(&self) -> Option<String> {
        self.session
            .lock()
            .await
            .as_ref()
            .map(|s| s.student_id().to_string())
    }

    /// 取出会话的克隆（锁只保护读取，不跨网络请求持有）
    async fn active_session(&self) -> std::result::Result<Session, ServiceError> {
        self.session
            .lock()
            .await
            .as_ref()
            .cloned()
            .ok_or(ServiceError::NotLoggedIn)
    }

    /// 获取合并后的完整学籍信息（内部有缓存与回退策略）
    pub async fn student_info(&self) -> std::result::Result<StudentInfo, ServiceError> {
        Ok(self.active_session().await?.student_info().await?)
    }

    /// 获取课表
    ///
    /// `week` 为 `None` 时返回整学期课表；传具体周次则返回该周视图
    /// （客户端过滤，不产生额外网络请求）。
    pub async fn class_schedule(
        &self,
        term: AcademicTerm,
        week: Option<u8>,
    ) -> std::result::Result<ClassSchedule, ServiceError> {
        Ok(self
            .active_session()
            .await?
            .class_schedule(term, week)
            .await?)
    }

    /// 获取当前教学周
    pub async fn this_week(&self) -> std::result::Result<u8, ServiceError> {
        Ok(self.active_session().await?.this_week().await?)
    }

    /// 获取考试安排
    ///
    /// 实测服务端不按学期过滤、返回学生全部考试记录；学期参数仅作
    /// 为缓存分桶的键（见 [`gnnuhub_api::Session::exam_schedule`]）。
    pub async fn exam_schedule(
        &self,
        term: AcademicTerm,
    ) -> std::result::Result<Vec<model::ExamRecord>, ServiceError> {
        Ok(self.active_session().await?.exam_schedule(term).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 未登录时调用受保护操作应返回 NotLoggedIn，而不是panic 或伪数据
    #[tokio::test]
    async fn protected_ops_fail_without_login() {
        let service = HubService::new().unwrap();

        assert!(matches!(
            service.student_info().await,
            Err(ServiceError::NotLoggedIn)
        ));
        assert!(matches!(
            service
                .class_schedule(AcademicTerm::first(2026), None)
                .await,
            Err(ServiceError::NotLoggedIn)
        ));
        assert!(matches!(
            service.this_week().await,
            Err(ServiceError::NotLoggedIn)
        ));
        assert!(service.logged_in_as().await.is_none());
    }

    /// 学号格式校验应在发起任何网络请求**之前**完成
    #[tokio::test]
    async fn login_rejects_non_numeric_id_before_any_request() {
        let service = HubService::new().unwrap();
        let error = service.login("不是数字", "whatever").await.unwrap_err();
        assert!(matches!(error, ServiceError::InvalidStudentId));
    }

    /// 重试安全性分类：锁定与凭据错误绝不能重试
    #[test]
    fn retry_safety_classification() {
        assert!(!ServiceError::NotLoggedIn.retry_safe());
        assert!(!ServiceError::InvalidStudentId.retry_safe());
        assert!(!ServiceError::Core(Error::InvalidCredentials).retry_safe());
        assert!(
            !ServiceError::Core(Error::TicketMissing("账号已锁定".into())).retry_safe(),
            "账号锁定后重试有害"
        );
        // 网络类错误允许重试
        assert!(ServiceError::Core(Error::PageParse("x".into())).retry_safe());
    }
}
