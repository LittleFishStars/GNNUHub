//! 首页门户组件解析：通知、系统提醒、校历
//!
//! 这些组件都来自 `/xtgl/index_*.html`（服务端渲染的首页区块），
//! 通知正文则来自详情页 `xwck_ckXw.html`。

use gnnuhub_core::model::{AcademicCalendar, Notice, NoticeDetail};
use gnnuhub_core::{Error, Result};

use super::selector;

/// 解析首页通知栏（`index_cxNews.html` 的 HTML 片段）
///
/// 每条通知是 `a.list-group-item`：`title` 属性为完整标题，
/// `span.time` 为日期，`span.title` 内的 `font[color=red]` 表示
/// 置顶，`href` 携带详情页编号（`xwbh`）。
pub fn parse_notices(html: &str) -> Result<Vec<Notice>> {
    let doc = scraper::Html::parse_fragment(html);
    let item = selector("a.list-group-item")?;

    let mut notices = Vec::new();
    for el in doc.select(&item) {
        let title = el
            .value()
            .attr("title")
            .unwrap_or_default()
            .trim()
            .to_string();
        if title.is_empty() {
            continue;
        }

        let date = el
            .select(&selector("span.time")?)
            .next()
            .map(|t| t.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        let span_text = el
            .select(&selector("span.title")?)
            .next()
            .map(|t| t.text().collect::<String>())
            .unwrap_or_default();

        let pinned = span_text.contains("【置顶】");

        // 栏目标记：【通知】/【新闻】等，去掉后剩余部分为纯标题
        let mut category = String::new();
        let mut clean = span_text.replace("【置顶】", "");
        if let Some(start) = clean.find('【')
            && let Some(rel) = clean[start..].find('】')
        {
            let end = start + rel + '】'.len_utf8();
            let tag = clean[start..end].to_string();
            if tag != "【置顶】" {
                category = tag.clone();
            }
            clean = clean.replace(&tag, "");
        }
        clean = clean.trim().to_string();

        let href = el.value().attr("href").unwrap_or_default().to_string();
        let detail_path = if href.starts_with('/') {
            href
        } else {
            format!("/{href}")
        };

        notices.push(Notice {
            title,
            date,
            detail_path,
            pinned,
            category,
        });
    }
    Ok(notices)
}

/// 解析系统提醒消息（`index_cxXsKttsxx` 类接口的响应）
///
/// 响应是一个 JSON 字符串，多条消息以 `|` 分隔，可能带尾部空段：
/// `"消息一|消息二|"`.
pub fn parse_system_messages(body: &str) -> Result<Vec<String>> {
    let text: String = serde_json::from_str(body).map_err(Error::Json)?;
    Ok(text
        .split('|')
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(str::to_string)
        .collect())
}

/// 解析通知正文页（`xwck_ckXw.html?xwbh=...`）
///
/// 页面结构：`h3.text-center` 标题、`h5.news_title1` 内的三个
/// `span`（发布人/发布时间/浏览人数，`key：value` 形式）、
/// `div.news_con` 正文富文本。
pub fn parse_notice_detail(html: &str) -> Result<NoticeDetail> {
    let doc = scraper::Html::parse_document(html);
    let text_of = |sel: &str| -> String {
        match selector(sel) {
            Ok(sel) => doc
                .select(&sel)
                .next()
                .map(|el| el.text().collect::<String>())
                .unwrap_or_default(),
            Err(_) => String::new(),
        }
    };

    let title = text_of("h3.text-center").trim().to_string();

    // 发布信息：h5.news_title1 的 span 文本，按「：」拆键值
    let mut publisher = String::new();
    let mut publish_time = String::new();
    let mut views = String::new();
    if let Ok(meta_sel) = selector("h5.news_title1 span") {
        for el in doc.select(&meta_sel) {
            let span = el.text().collect::<String>();
            let span = span.trim();
            if let Some(v) = span.strip_prefix("发布人：") {
                publisher = v.trim().to_string();
            } else if let Some(v) = span.strip_prefix("发布时间：") {
                publish_time = v.trim().to_string();
            } else if let Some(v) = span.strip_prefix("浏览人数：") {
                views = v.trim().to_string();
            }
        }
    }

    // 正文：news_con 内提取纯文本，图片位置标注为「图片: <路径>」
    let con = doc
        .select(&selector("div.news_con")?)
        .next()
        .ok_or_else(|| Error::PageParse("通知正文页缺少 news_con 容器".to_string()))?;

    let mut content = String::new();
    for node in con.descendants() {
        match node.value() {
            scraper::Node::Text(t) => {
                let seg = t.text.trim();
                if !seg.is_empty() {
                    content.push_str(seg);
                    content.push(' ');
                }
            }
            scraper::Node::Element(el) if el.name() == "img" => {
                if let Some(src) = el.attr("src") {
                    content.push_str(&format!("图片: {src} "));
                }
            }
            scraper::Node::Element(el) if matches!(el.name(), "p" | "br" | "div" | "tr") => {
                content.push('\n');
            }
            _ => {}
        }
    }
    // 压缩空白：按行去多余空格，丢弃空行
    let content = content
        .split('\n')
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    Ok(NoticeDetail {
        title,
        publisher,
        publish_time,
        views,
        content,
    })
}

/// 从首页校历组件（`index_cxAreaFive.html`）解析学年学期信息
///
/// 页面为服务端渲染文本，学期信息形如
/// `2026-2027学年1学期(2026-08-31至2027-01-17)`。为避免引入
/// regex 依赖，这里用定位字符串的方式提取。
pub fn parse_calendar(html: &str) -> Result<AcademicCalendar> {
    let err = || Error::PageParse("校历页中未找到学期信息".to_string());

    // 形如 `2026-2027学年1学期(2026-08-31至2027-01-17)`
    // 学年字段取「学年」前数字段的第一段（2026-2027 → 2026，起始年）
    let year_pos = html.find("学年").ok_or_else(err)?;
    let before = &html[..year_pos];
    let digits_start = before
        .trim_end_matches(|c: char| c.is_ascii_digit() || c == '-')
        .len();
    let year: u16 = before[digits_start..]
        .split('-')
        .next()
        .and_then(|seg| seg.parse().ok())
        .ok_or_else(err)?;

    let paren = html[year_pos..].find('(').ok_or_else(err)? + year_pos;
    let close = html[paren..].find(')').ok_or_else(err)? + paren;
    let inner = &html[paren + 1..close];

    let (start_date, end_date) = inner.split_once('至').ok_or_else(err)?;

    // 学期数字在「学年」与「学期(」之间，形如 `1学期`
    let seg = &html[year_pos + "学年".len()..paren];
    let term: u8 = seg
        .trim_end_matches("学期")
        .trim()
        .parse()
        .map_err(|_| err())?;

    Ok(AcademicCalendar {
        year,
        term,
        start_date: start_date.trim().to_string(),
        end_date: end_date.trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 通知栏解析（脱敏夹具，DOM 结构对齐实测响应）
    #[test]
    fn parses_notices() {
        let html = r#"<div>
          <a title="【学生】教务系统使用说明" href="/xtgl/xwck_ckXw.html?xwbh=AAA111&doType=save" class="list-group-item">
            <span class="title xwxx">
              <font color="red">【置顶】</font>
              <font color="black">【通知】</font>
              【学生】教务系统使用说明
            </span>
            <span class="time">2020-12-31</span>
          </a>
          <a title="关于转专业计划的通..." href="/xtgl/xwck_ckXw.html?xwbh=BBB222&doType=save" class="list-group-item">
            <span class="title xwxx">
              <font color="black">【通知】</font>
              关于转专业计划的通...
            </span>
            <span class="time">2026-09-01</span>
          </a>
        </div>"#;
        let notices = parse_notices(html).unwrap();
        assert_eq!(notices.len(), 2);

        let first = &notices[0];
        assert_eq!(first.title, "【学生】教务系统使用说明");
        assert_eq!(first.date, "2020-12-31");
        assert!(first.pinned);
        assert_eq!(first.category, "【通知】");
        assert_eq!(
            first.detail_path,
            "/xtgl/xwck_ckXw.html?xwbh=AAA111&doType=save"
        );

        let second = &notices[1];
        assert!(!second.pinned);
        assert_eq!(second.title, "关于转专业计划的通...");
        assert_eq!(second.date, "2026-09-01");
    }

    /// 通知正文解析（脱敏夹具，DOM 对齐实测响应）
    #[test]
    fn parses_notice_detail() {
        let html = r#"<html><head><title>通知详情</title></head><body>
          <h3 class="text-center">【学生】教务系统使用说明</h3>
          <h5 class="text-center news_title1">
            <span>发布人：温老师</span>
            <span>发布时间：2020-12-31 08:56:24</span>
            <span>浏览人数：10577</span>
          </h5>
          <div class="news_con">
            <p>第一段说明文字。</p>
            <p>第二段：<img src="/upload/pic.png" />附图说明。</p>
          </div>
        </body></html>"#;
        let d = parse_notice_detail(html).unwrap();
        assert_eq!(d.title, "【学生】教务系统使用说明");
        assert_eq!(d.publisher, "温老师");
        assert_eq!(d.publish_time, "2020-12-31 08:56:24");
        assert_eq!(d.views, "10577");
        assert!(d.content.contains("第一段说明文字。"), "{}", d.content);
        assert!(d.content.contains("第二段："), "{}", d.content);
        assert!(d.content.contains("图片: /upload/pic.png"), "{}", d.content);
    }

    /// 校历解析（夹具文本对齐首页 AreaFive 实测渲染）
    #[test]
    fn parses_calendar() {
        let html = "<div>2026-2027学年1学期(2026-08-31至2027-01-17)</div>";
        let cal = parse_calendar(html).unwrap();
        assert_eq!(cal.year, 2026);
        assert_eq!(cal.term, 1);
        assert_eq!(cal.start_date, "2026-08-31");
        assert_eq!(cal.end_date, "2027-01-17");

        // 当前周推算：起始日当天为第 1 周，8 天后为第 2 周，未开始为 None
        let start = chrono::NaiveDate::from_ymd_opt(2026, 8, 31).unwrap();
        assert_eq!(cal.current_week(start), Some(1));
        assert_eq!(cal.current_week(start + chrono::Duration::days(8)), Some(2));
        assert_eq!(cal.current_week(start - chrono::Duration::days(1)), None);
    }

    /// 系统提醒消息解析：`|` 分隔、去空段
    #[test]
    fn parses_system_messages() {
        let body = r#""你可能有应选未选的主修课程，请核实！|""#;
        assert_eq!(
            parse_system_messages(body).unwrap(),
            vec!["你可能有应选未选的主修课程，请核实！".to_string()]
        );
        assert!(parse_system_messages(r#""""#).unwrap().is_empty());
    }
}
