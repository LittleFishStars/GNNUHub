//! GNNUHub 桌面客户端（Tauri 2）
//!
//! 本 crate 是**各平台 UI 实现之一**（见仓库根 `Cargo.toml` 的分层
//! 说明与 `logic/service` 的分层文档）。职责被刻意压到最薄：
//!
//! - 把前端可调用的 Tauri 命令一对一转发给 [`gnnuhub_service::HubService`]；
//! - 把 [`gnnuhub_service::ServiceError`] 转成字符串给前端展示
//!   （Display 文案已是面向用户的中文）。
//!
//! **不包含任何业务逻辑**：会话生命周期、验证码识别、请求节流、
//! 错误分类全部在服务层。将来新增 CLI / 移动端 / Web 服务端等
//! 平台实现时，应复用同一个服务层而不是复制这里的任何代码。
//!
//! 设计取舍：
//!
//! - **成绩暂不提供**：成绩接口对测试账号恒定返回空信封（无法实测），
//!   项目纪律是「无法实测的接口不转正」，见
//!   `logic/api/examples/probe_grade.rs` 的结论文档。
//! - **验证码自动识别**：内嵌位图字库实测准确率 100%，无需用户手输；
//!   识别失败由登录流程自动重试。

use gnnuhub_service::HubService;
use gnnuhub_service::model::{AcademicTerm, ClassSchedule, StudentInfo, Term};

/// Tauri 命令的统一返回类型
///
/// 服务层的错误已经是面向用户的中文文案，直接 `to_string()` 即可。
/// 需要按错误类别定制交互（如禁用重试按钮）时，可改用
/// [`gnnuhub_service::ServiceError::retry_safe`] 再序列化成结构体。
type CmdResult<T> = Result<T, String>;

fn cmd_error(error: gnnuhub_service::ServiceError) -> String {
    error.to_string()
}

/// 把学年 + 学期编号组装成 [`AcademicTerm`]
fn academic_term(year: u16, term: u8) -> CmdResult<AcademicTerm> {
    let term = Term::from_number(u32::from(term))
        .ok_or_else(|| format!("非法的学期编号 {term}（只接受 1 或 2）"))?;
    Ok(AcademicTerm::new(year, term))
}

/// 用学号 + 密码登录，成功返回学号
///
/// 验证码自动识别；密码仅用于本次登录，不落盘、不进日志。
#[tauri::command]
async fn login(
    state: tauri::State<'_, HubService>,
    student_id: String,
    password: String,
) -> CmdResult<String> {
    state.login(&student_id, &password).await.map_err(cmd_error)
}

/// 退出登录（丢弃会话；幂等）
#[tauri::command]
async fn logout(state: tauri::State<'_, HubService>) -> CmdResult<()> {
    state.logout().await;
    Ok(())
}

/// 当前登录的学号（未登录返回空串）
#[tauri::command]
async fn current_student(state: tauri::State<'_, HubService>) -> CmdResult<String> {
    Ok(state.logged_in_as().await.unwrap_or_default())
}

/// 获取合并后的完整学籍信息
#[tauri::command]
async fn student_info(state: tauri::State<'_, HubService>) -> CmdResult<StudentInfo> {
    state.student_info().await.map_err(cmd_error)
}

/// 获取课表（`week` 省略或为 0 = 整学期；传具体周次 = 该周视图）
#[tauri::command]
async fn class_schedule(
    state: tauri::State<'_, HubService>,
    year: u16,
    term: u8,
    week: Option<u8>,
) -> CmdResult<ClassSchedule> {
    let term = academic_term(year, term)?;
    state
        .class_schedule(term, week.filter(|w| *w > 0))
        .await
        .map_err(cmd_error)
}

/// 获取当前教学周
#[tauri::command]
async fn this_week(state: tauri::State<'_, HubService>) -> CmdResult<u8> {
    state.this_week().await.map_err(cmd_error)
}

/// Tauri 应用入口
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("gnnuhub_api=info,gnnuhub_service=info")
            }),
        )
        .with_target(false)
        .init();

    // 服务初始化只做本地加载（内嵌验证码字库），不会失败到需要退出
    // 的程度；万一失败则属于构建产物损坏，直接终止并给出原因。
    let service =
        gnnuhub_service::HubService::new().expect("应用服务初始化失败（内嵌验证码字库损坏？）");

    tauri::Builder::default()
        .manage(service)
        .invoke_handler(tauri::generate_handler![
            login,
            logout,
            current_student,
            student_info,
            class_schedule,
            this_week,
        ])
        .run(tauri::generate_context!())
        .expect("GNNUHub 桌面应用启动失败");
}
