//! 已认证会话与业务接口
//!
//! 对应 Python 版 `api.py` 的 `Student` 类。持有登录后获得的
//! Cookie，并在此基础上提供课表、个人信息等查询。
//!
//! 与 Python 版的差异：
//!
//! - 所有接口都是 `async` 的
//! - 课表等信息在首次请求后缓存于内存，避免重复打接口
//! - 缓存用 `tokio::sync::Mutex` 保护，`Session` 可安全跨任务共享

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use gnnuhub_core::model::{AcademicTerm, ClassSchedule, PeriodTime, StudentInfo};
use gnnuhub_core::{Error, JWGL_BASE_URL, Result};

use crate::client::Client;
use crate::parse;

/// 会话内部可变状态
#[derive(Debug, Default)]
struct SessionState {
    /// 已获取的学生信息
    info: StudentInfo,
    /// 按学年学期缓存的课表
    schedules: HashMap<AcademicTerm, ClassSchedule>,
    /// 节次时间表
    timetable: Option<Vec<PeriodTime>>,
    /// 当前教学周
    this_week: Option<u8>,
}

/// 已认证的教务系统会话
///
/// 由 [`crate::Client::login`] 创建。内部状态用 `Arc<Mutex<_>>`
/// 包装，因此可以克隆后跨任务共享。
#[derive(Debug, Clone)]
pub struct Session {
    client: Client,
    cookies: Arc<HashMap<String, String>>,
    student_id: String,
    state: Arc<Mutex<SessionState>>,
}

impl Session {
    /// 由已有的 Cookie 直接构造会话
    ///
    /// 一般场景请用 [`crate::Client::login`]；当需要复用外部持久化的
    /// Cookie（例如把登录状态缓存到磁盘后恢复）时才用本方法。
    ///
    /// 传入的 Cookie 必须包含有效的 `JSESSIONID`，否则后续请求会被
    /// 重定向到登录页，表现为 [`crate::Error::Unauthenticated`]。
    pub fn new(client: Client, cookies: HashMap<String, String>, student_id: String) -> Self {
        Self {
            client,
            cookies: Arc::new(cookies),
            student_id,
            state: Arc::new(Mutex::new(SessionState::default())),
        }
    }

    /// 学号
    pub fn student_id(&self) -> &str {
        &self.student_id
    }

    /// 构造带 Cookie 的请求头
    fn cookie_header(&self) -> String {
        crate::login::build_cookie_header(&self.cookies)
    }

    /// 向教务系统发起一次请求并返回响应体
    ///
    /// 这是本模块唯一的出口：统一负责补全域名、注入会话 Cookie、
    /// 校验状态码，并保证请求经由 [`Client::throttled`] 节流。
    ///
    /// `build` 在拿到 `RequestBuilder` 后继续追加方法特有的内容
    /// （查询串、表单体等），因此三种请求形态共用同一条路径。
    async fn send<F>(&self, url: &str, build: F) -> Result<String>
    where
        F: FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    {
        let cookie = self.cookie_header();
        let response = self
            .client
            .throttled(|http| build(http.get(url)).header(reqwest::header::COOKIE, &cookie))
            .await?;

        let status = response.status();
        if !status.is_success() {
            return Err(Error::UnexpectedStatus {
                status: status.as_u16(),
                url: url.to_string(),
            });
        }
        Ok(response.text().await?)
    }

    /// 把站内路径补全为绝对地址
    ///
    /// 已经是 `http(s)://` 开头时原样返回，便于直接抓取外部资源。
    fn absolute_url(path: &str) -> String {
        if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{JWGL_BASE_URL}{path}")
        }
    }

    /// 按原样请求一个站内路径，返回响应体文本
    ///
    /// 与 [`Session::get`] 的区别：`path` 已经包含查询串，不再额外拼接。
    /// 主要用于抓取静态资源（前端 JS / HTML）做离线分析。
    ///
    /// 请求仍然经过客户端节流，避免触发网关风控。
    pub async fn fetch_raw(&self, path: &str) -> Result<String> {
        let url = Self::absolute_url(path);
        self.send(&url, |req| req).await
    }

    /// 发起一个带会话 Cookie 的 GET 请求
    async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<String> {
        let url = format!("{JWGL_BASE_URL}{path}");
        self.send(&url, |req| req.query(query)).await
    }

    /// 发起一个带会话 Cookie 的 POST 请求（表单）
    async fn post_form(
        &self,
        path: &str,
        query: &[(&str, &str)],
        form: &[(&str, &str)],
    ) -> Result<String> {
        let url = format!("{JWGL_BASE_URL}{path}");
        self.send(&url, |req| req.query(query).form(form)).await
    }

    /// 原样发一次表单 POST，供接口探测工具使用
    ///
    /// 与 [`Session::post_form`] 行为一致，区别只是**公开**且允许任意
    /// 查询串/表单体。存在的理由是：教务系统有些接口能否返回数据
    /// 取决于一个难以离线推断的参数组合，只能靠少量实测确定；
    /// 这类实验不应该污染正式的业务方法。
    ///
    /// 仍然经过客户端节流，不会绕过 [`Client::throttled`]。
    ///
    /// 调用方应遵守项目礼仪：一次探测不要超过个位数请求，
    /// 并且**优先先抓页面自带的 JS 离线分析**，不要盲目枚举参数。
    pub async fn post_form_for_probe(
        &self,
        path: &str,
        query: &[(&str, &str)],
        form: &[(&str, &str)],
    ) -> Result<String> {
        self.post_form(path, query, form).await
    }

    /// 拉取首页基本资料（姓名、身份、学院、班级、头像）
    ///
    /// 对应 Python 版 `_get_basic_info`。
    pub async fn fetch_basic_info(&self) -> Result<StudentInfo> {
        let html = self
            .get(
                "/xtgl/index_cxYhxxIndex.html",
                &[("xt", "jw"), ("localeKey", "zh_CN")],
            )
            .await?;

        let mut parsed = parse::parse_basic_info(&html)?;
        // 首页不带学号，用登录时的学号补齐
        parsed.student_id = Some(self.student_id.clone());

        let mut state = self.state.lock().await;
        state.info.merge_from(parsed.clone());
        Ok(parsed)
    }

    /// 拉取学籍详细信息
    ///
    /// 对应 Python 版 `_get_student_info`。
    pub async fn fetch_student_info(&self) -> Result<StudentInfo> {
        let html = self
            .get(
                "/xsxxxggl/xsgrxxwh_cxXsgrxx.html",
                &[("gnmkdm", "N100801"), ("layout", "default")],
            )
            .await?;

        let parsed = parse::parse_student_info(&html)?;
        let mut state = self.state.lock().await;
        state.info.merge_from(parsed.clone());
        Ok(parsed)
    }

    /// 获取合并后的完整学生信息
    ///
    /// 优先返回缓存；缓存不足以覆盖关键字段时会补齐。
    pub async fn student_info(&self) -> Result<StudentInfo> {
        {
            let state = self.state.lock().await;
            if state.info.name.is_some() && state.info.major.is_some() {
                return Ok(state.info.clone());
            }
        }

        // 两个页面提供的信息互补，都拉一遍再合并
        let basic = self.fetch_basic_info().await?;
        let detail = self.fetch_student_info().await.unwrap_or_default();

        let mut state = self.state.lock().await;
        state.info.merge_from(basic);
        state.info.merge_from(detail);
        state.info.student_id = Some(self.student_id.clone());
        Ok(state.info.clone())
    }

    /// 获取指定学年学期的课表
    ///
    /// `week` 为 `None` 时返回整学期课表（对应原实现的 `week = 0`），
    /// 传入具体周次则只返回该周的课表（走移动端接口）。
    ///
    /// 对应 Python 版 `get_class_schedule`。
    pub async fn class_schedule(
        &self,
        term: AcademicTerm,
        week: Option<u8>,
    ) -> Result<ClassSchedule> {
        // 整学期课表可以缓存；单周课表不缓存
        if week.is_none() {
            let state = self.state.lock().await;
            if let Some(cached) = state.schedules.get(&term) {
                return Ok(cached.clone());
            }
        }

        let (path, gnmkdm, form): (&str, &str, Vec<(&str, &str)>) = match week {
            None => (
                "/kbcx/xskbcx_cxXsgrkb.html",
                "N2151",
                vec![
                    ("xnm", ""),
                    ("xqm", ""),
                    ("kzlx", "ck"),
                    ("xsdm", ""),
                    ("kclbdm", ""),
                ],
            ),
            Some(_) => (
                "/kbcx/xskbcxMobile_cxXsKb.html",
                "N2154",
                vec![
                    ("xnm", ""),
                    ("xqm", ""),
                    ("zs", ""),
                    ("doType", "app"),
                    ("kblx", "1"),
                    ("xh", ""),
                ],
            ),
        };

        // 学年学期参数需要动态填充
        let year_str = term.as_xnm();
        let xqm_str = term.as_xqm().to_string();
        let week_str = week.map(|w| w.to_string()).unwrap_or_default();
        let form: Vec<(&str, &str)> = form
            .into_iter()
            .map(|(k, v)| match k {
                "xnm" => (k, year_str.as_str()),
                "xqm" => (k, xqm_str.as_str()),
                "zs" => (k, week_str.as_str()),
                _ => (k, v),
            })
            .collect();

        let body = self.post_form(path, &[("gnmkdm", gnmkdm)], &form).await?;

        let (mut schedule, info) = parse::parse_class_schedule(&body)?;
        parse::attach_academic_term(&mut schedule, term);

        if week.is_none() {
            let mut state = self.state.lock().await;
            state.schedules.insert(term, schedule.clone());
            state.info.merge_from(info);
        }

        Ok(schedule)
    }

    /// 获取指定课程在给定学期的开课信息
    pub async fn course(
        &self,
        term: AcademicTerm,
        name: &str,
    ) -> Result<Vec<gnnuhub_core::model::CourseEntry>> {
        let schedule = self.class_schedule(term, None).await?;
        Ok(schedule.course(name).to_vec())
    }

    /// 获取节次时间表
    ///
    /// 对应 Python 版 `get_timetable`。
    pub async fn timetable(&self, term: AcademicTerm) -> Result<Vec<PeriodTime>> {
        {
            let state = self.state.lock().await;
            if let Some(cached) = &state.timetable {
                return Ok(cached.clone());
            }
        }

        let year_str = term.as_xnm();
        let xqm_str = term.as_xqm().to_string();
        let body = self
            .post_form(
                "/kbcx/xskbcx_cxRjc.html",
                &[("gnmkdm", "N2151")],
                &[("xnm", &year_str), ("xqm", &xqm_str), ("xqh_id", "1")],
            )
            .await?;

        let times = parse::parse_timetable(&body)?;
        let mut state = self.state.lock().await;
        state.timetable = Some(times.clone());
        Ok(times)
    }

    /// 获取当前教学周
    ///
    /// 对应 Python 版 `this_week`。
    pub async fn this_week(&self) -> Result<u8> {
        {
            let state = self.state.lock().await;
            if let Some(week) = state.this_week {
                return Ok(week);
            }
        }

        let html = self
            .get(
                "/kbcx/xskbcxZccx_cxXskbcxIndex.html",
                &[("gnmkdm", "N2154"), ("layout", "default")],
            )
            .await?;

        let week = parse::parse_this_week(&html)?;
        let mut state = self.state.lock().await;
        state.this_week = Some(week);
        Ok(week)
    }

    /// 当前周课表（便捷方法）
    pub async fn current_week_schedule(&self, term: AcademicTerm) -> Result<ClassSchedule> {
        let week = self.this_week().await?;
        self.class_schedule(term, Some(week)).await
    }
}
