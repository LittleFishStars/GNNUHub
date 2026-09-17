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
use gnnuhub_core::{Error, Result, JWGL_BASE_URL};

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
    /// 由登录流程内部调用，创建会话
    pub(crate) fn new(
        client: Client,
        cookies: HashMap<String, String>,
        student_id: String,
    ) -> Self {
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

    /// 发起一个带会话 Cookie 的 GET 请求
    async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<String> {
        let url = format!("{JWGL_BASE_URL}{path}");
        let response = self
            .client
            .http()
            .get(&url)
            .query(query)
            .header(reqwest::header::COOKIE, self.cookie_header())
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            return Err(Error::UnexpectedStatus {
                status: status.as_u16(),
                url,
            });
        }
        Ok(response.text().await?)
    }

    /// 发起一个带会话 Cookie 的 POST 请求（表单）
    async fn post_form(
        &self,
        path: &str,
        query: &[(&str, &str)],
        form: &[(&str, &str)],
    ) -> Result<String> {
        let url = format!("{JWGL_BASE_URL}{path}");
        let response = self
            .client
            .http()
            .post(&url)
            .query(query)
            .header(reqwest::header::COOKIE, self.cookie_header())
            .form(form)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            return Err(Error::UnexpectedStatus {
                status: status.as_u16(),
                url,
            });
        }
        Ok(response.text().await?)
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

        let parsed = parse::parse_basic_info(&html)?;
        let mut state = self.state.lock().await;
        merge_info(&mut state.info, parsed.clone());
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
        merge_info(&mut state.info, parsed.clone());
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
        merge_info(&mut state.info, basic);
        merge_info(&mut state.info, detail);
        state.info.student_id = self.student_id.clone();
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

        let body = self
            .post_form(path, &[("gnmkdm", gnmkdm)], &form)
            .await?;

        let (mut schedule, info) = parse::parse_class_schedule(&body)?;
        parse::attach_academic_term(&mut schedule, term);

        if week.is_none() {
            let mut state = self.state.lock().await;
            state.schedules.insert(term, schedule.clone());
            merge_info(&mut state.info, info);
        }

        Ok(schedule)
    }

    /// 获取指定课程在给定学期的开课信息
    pub async fn course(&self, term: AcademicTerm, name: &str) -> Result<Vec<gnnuhub_core::model::CourseEntry>> {
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
                &[
                    ("xnm", &year_str),
                    ("xqm", &xqm_str),
                    ("xqh_id", "1"),
                ],
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

/// 把新解析出的信息合并进已有信息
///
/// 只覆盖 `Some` 的字段，避免后拉取的页面把已有数据清空。
fn merge_info(target: &mut StudentInfo, incoming: StudentInfo) {
    macro_rules! take {
        ($field:ident) => {
            if incoming.$field.is_some() {
                target.$field = incoming.$field;
            }
        };
    }

    take!(name);
    take!(identity);
    take!(college);
    take!(class_name);
    take!(avatar);
    take!(instructor);
    take!(major);
    take!(gender);
    take!(birthday);
    take!(ethnicity);
    take!(political_status);
    take!(address);
    take!(enrollment_year);

    if incoming.document.is_some() {
        target.document = incoming.document;
    }
    if !incoming.student_id.is_empty() {
        target.student_id = incoming.student_id;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合并时不应覆盖已有的 Some 值
    #[test]
    fn merge_keeps_existing_values() {
        let mut target = StudentInfo {
            name: Some("张三".into()),
            ..Default::default()
        };
        let incoming = StudentInfo {
            college: Some("信息学院".into()),
            ..Default::default()
        };
        merge_info(&mut target, incoming);
        assert_eq!(target.name.as_deref(), Some("张三"));
        assert_eq!(target.college.as_deref(), Some("信息学院"));
    }

    /// 后到的 Some 值应覆盖
    #[test]
    fn merge_overwrites_with_new_value() {
        let mut target = StudentInfo {
            name: Some("旧名字".into()),
            ..Default::default()
        };
        let incoming = StudentInfo {
            name: Some("新名字".into()),
            ..Default::default()
        };
        merge_info(&mut target, incoming);
        assert_eq!(target.name.as_deref(), Some("新名字"));
    }

    /// 后到的 None 不应清空已有值
    #[test]
    fn merge_does_not_clear_with_none() {
        let mut target = StudentInfo {
            name: Some("张三".into()),
            gender: Some("男".into()),
            ..Default::default()
        };
        merge_info(&mut target, StudentInfo::default());
        assert_eq!(target.name.as_deref(), Some("张三"));
        assert_eq!(target.gender.as_deref(), Some("男"));
    }
}
