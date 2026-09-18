//! GNNUHub 桌面客户端（Tauri 2）
//!
//! 后端职责：持有登录后的 [`Session`]，把它包装成前端可调用的命令
//! （登录 / 课表 / 学籍）。所有对教务系统的访问都经由 `gnnuhub-api`
//! （自带请求节流与 Cookie 管理），前端永远不直接触网。
//!
//! 设计取舍：
//!
//! - **成绩暂不提供**：成绩接口对测试账号恒定返回空信封（无法实测），
//!   项目纪律是「无法实测的接口不转正」，见
//!   `gnnuhub-api/examples/probe_grade.rs` 的结论文档。
//! - **验证码自动识别**：内嵌位图字库实测准确率 100%，无需用户手输；
//!   识别失败由登录流程自动重试。

use gnnuhub_api::{Client, Session};
use gnnuhub_core::model::{AcademicTerm, ClassSchedule, StudentInfo, Term};
use gnnuhub_ocr::BitmapOcr;
use tokio::sync::Mutex;

/// 应用全局状态
#[derive(Default)]
struct AppState {
    /// 登录后的会话；`None` 表示未登录
    session: Mutex<Option<Session>>,
}

/// 把学年 + 学期编号组装成 [`AcademicTerm`]
fn academic_term(year: u16, term: u8) -> Result<AcademicTerm, String> {
    let term = Term::from_number(u32::from(term))
        .ok_or_else(|| format!("非法的学期编号 {term}（只接受 1 或 2）"))?;
    Ok(AcademicTerm::new(year, term))
}

/// 从状态里取出会话的克隆
///
/// [`Session`] 内部状态由 `Arc<Mutex<_>>` 保护且可克隆，因此锁只在
/// 读取时短暂持有，业务请求不阻塞并发的状态切换（如退出登录）。
async fn active_session(state: &tauri::State<'_, AppState>) -> Result<Session, String> {
    state
        .session
        .lock()
        .await
        .as_ref()
        .cloned()
        .ok_or_else(|| "尚未登录，请先登录".to_string())
}

/// 用学号 + 密码登录
///
/// 验证码由内嵌位图字库自动识别，识别失败自动重试，上限见
/// `ClientConfig::max_login_retries`。密码只在本调用内使用，
/// 不落盘、不进日志。
#[tauri::command]
async fn login(
    state: tauri::State<'_, AppState>,
    student_id: String,
    password: String,
) -> Result<String, String> {
    let id: u64 = student_id
        .trim()
        .parse()
        .map_err(|_| "学号必须是纯数字".to_string())?;

    let client = Client::with_defaults().map_err(|e| e.to_string())?;
    let ocr = BitmapOcr::embedded().map_err(|e| e.to_string())?;
    let session = client
        .login(id, password.trim(), &ocr)
        .await
        .map_err(|e| e.to_string())?;

    let logged_in_as = session.student_id().to_string();
    *state.session.lock().await = Some(session);
    tracing::info!("前端登录成功: {logged_in_as}");
    Ok(logged_in_as)
}

/// 退出登录（丢弃会话）
#[tauri::command]
async fn logout(state: tauri::State<'_, AppState>) -> Result<(), String> {
    *state.session.lock().await = None;
    Ok(())
}

/// 获取合并后的完整学籍信息
///
/// 首次调用会请求首页与学籍 JSON 接口（JSON 失败自动回退 HTML），
/// 之后走内存缓存。
#[tauri::command]
async fn student_info(state: tauri::State<'_, AppState>) -> Result<StudentInfo, String> {
    active_session(&state)
        .await?
        .student_info()
        .await
        .map_err(|e| e.to_string())
}

/// 获取课表
///
/// `week` 省略时返回整学期课表；传具体周次则返回该周视图
/// （由整学期缓存客户端过滤派生，0 额外请求）。
#[tauri::command]
async fn class_schedule(
    state: tauri::State<'_, AppState>,
    year: u16,
    term: u8,
    week: Option<u8>,
) -> Result<ClassSchedule, String> {
    let term = academic_term(year, term)?;
    active_session(&state)
        .await?
        .class_schedule(term, week)
        .await
        .map_err(|e| e.to_string())
}

/// 获取当前教学周
#[tauri::command]
async fn this_week(state: tauri::State<'_, AppState>) -> Result<u8, String> {
    active_session(&state)
        .await?
        .this_week()
        .await
        .map_err(|e| e.to_string())
}

/// Tauri 应用入口
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("gnnuhub_api=info,gnnuhub_app=info")
            }),
        )
        .with_target(false)
        .init();

    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            login,
            logout,
            student_info,
            class_schedule,
            this_week,
        ])
        .run(tauri::generate_context!())
        .expect("GNNUHub 桌面应用启动失败");
}
