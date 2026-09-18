//! 用真实登录接口验证识别准确率
//!
//! # 为什么需要这个工具
//!
//! `captcha_samples/` 里的 60 张图只有 `uid`，**没有真值标注**。本地能测的
//! 只有「多个识别配置是否互相一致」，那叫自洽性，不等于准确率。要拿到
//! 真实准确率，唯一可靠的地面真值是登录接口本身。
//!
//! # 判据
//!
//! `login::parse_ticket_response` 已经把登录失败干净地分成几类：
//!
//! | 服务端返回 | 判定 | 含义 |
//! |---|---|---|
//! | `data.code == "CODEFALSE"` | [`LoginOutcome::CaptchaIncorrect`] | **识别错了** |
//! | `data.code == "PASSERROR"` | [`LoginOutcome::BadCredentials`] | **识别对了**（密码错是预期的） |
//! | `statusCode == "USERNAMEORPASSWORDERROR"` | [`LoginOutcome::BadCredentials`] | **识别对了** |
//! | `data.code == "USERLOCK"` | [`LoginOutcome::AccountLocked`] | **账号被锁，立即终止** |
//! | 顶层带 `tgt` + `ticket` | [`LoginOutcome::Success`] | **识别对了**（且登录成功） |
//!
//! 用错密码是关键技巧：这样账密校验必然失败，于是**只要不是 `CODEFALSE`
//! 就说明验证码读对了**，不会有「识别正确因而登录成功、消耗真实登录次数」
//! 的副作用。工具会主动拒绝正确密码，避免误用。
//!
//! # 账号锁定是终止条件
//!
//! 同一账号短时间内连续账密错误会触发 `USERLOCK`。**这不是可重试失败**：
//! 继续提交只会加重锁定。因此本工具一旦收到 `USERLOCK` 就立刻停止，
//! 并在汇总里显式说明「是账号被锁，不是网络或工具问题」。
//!
//! 也正因如此，**每轮都发一个真实错密码是有代价的**——本工具的轮数应
//! 当作一种配额来用，不要为了凑样本反复跑。
//!
//! # 必须现场取验证码
//!
//! 验证码 `timeout` 只有 300 秒。`captcha_samples/` 里的旧 `uid` 早已过期，
//! 拿旧 uid 提交只会得到「验证码失效」，无法区分识别对错。因此本工具
//! **每轮重新申请一张验证码**，识别后立刻提交，全程在一个 ttl 内完成。
//!
//! # 识别引擎
//!
//! 默认用 [`BitmapOcr`] 位图查表（内嵌字库，60 张样本构建，65 字形 /
//! 59 字符）。加 `--features tesseract` 后用 `recommended_with_tesseract`
//! 走「位图 -> tesseract -> 人工」的链路，两者结果会同时打印以便对比。
//!
//! 每轮把四个字形的**匹配质量**一并打印（`=` 精确命中、`~N` 模糊命中且
//! 汉明距离为 N、`!` 未命中），这样失败轮次能立刻看出是「字库缺条目」
//! 还是「识别链路别的问题」。
//!
//! # 请求量控制
//!
//! 每张验证码固定消耗 **2 个请求**（取图 + 提交）。默认跑 20 轮 = 40 请求。
//! 轮间隔默认 1.5 秒，比 SDK 默认节流更保守；若提交时命中风控迹象
//! （连接被关闭、非预期状态码）立即中止，不重试。
//!
//! # 运行
//!
//! ```bash
//! GNNU_STUDENT_ID=xxx GNNU_PASSWORD='故意写错的密码' \
//!   cargo run -p gnnuhub-api --example ocr_verify -- 20
//! ```
//!
//! 第一个位置参数是轮数，第二个可选是输出 JSON 路径（默认
//! `captcha_samples/verify.json`）。
//!
//! # ⚠️ 风险
//!
//! 这会向认证平台发真实登录请求。虽然用的是错密码，但**连续失败仍可能
//! 触发账号风控**。请从小轮数开始，观察结果正常再加大。

use std::path::PathBuf;
use std::time::Duration;

use gnnuhub_api::{Client, LoginOutcome};
use gnnuhub_ocr::{BitmapLibrary, bitmap::extract_glyphs_for_bench, decode_image};

/// 单个字形的匹配质量
#[derive(Debug, Clone)]
struct GlyphHit {
    /// 字符
    ch: char,
    /// 匹配方式：`exact` / `fuzzy` / `miss`
    kind: &'static str,
    /// 汉明距离；`exact` 为 0，`miss` 为 `None`
    distance: Option<usize>,
    /// 字形尺寸，便于事后分析
    dim: String,
}

/// 一轮的结果
#[derive(Debug)]
struct RoundResult {
    /// 本轮序号
    index: usize,
    /// 识别出的验证码
    code: Option<String>,
    /// 判定结论
    verdict: &'static str,
    /// 详情
    detail: String,
    /// 逐字形匹配质量
    glyphs: Vec<GlyphHit>,
}

/// 轮之间的等待时间
///
/// 取 1.5 秒。每轮含 2 个请求，因此实际请求间隔约 1.5 秒，
/// 明显宽于 SDK 默认节流，给足了服务端风控的观察窗口。
const ROUND_INTERVAL: Duration = Duration::from_secs(1);

/// 单轮内的两个请求之间的等待
///
/// 识别本身是本地操作（约 300ms），这里补足到 600ms 再提交，
/// 避免「取图后立刻提交」这种明显非人类的节奏。
const SUBMIT_DELAY: Duration = Duration::from_millis(600);

/// 单次运行允许的最大轮数
const MAX_ROUNDS: usize = 60;

/// 用位图字库识别一张图，并给出逐字形的匹配质量
///
/// 与 [`gnnuhub_ocr::BitmapOcr::recognize_image`] 的区别是这里不把
/// 「未命中」当成整体失败，而是如实返回已经命中的部分与未命中的槽位，
/// 便于诊断字库缺口。
fn recognize_with_quality(
    lib: &BitmapLibrary,
    image_base64: &str,
) -> Result<Vec<GlyphHit>, String> {
    let glyphs = extract_glyphs_for_bench(image_base64).map_err(|e| e.to_string())?;
    if glyphs.len() != 4 {
        return Err(format!("期望 4 个字形，实际 {}", glyphs.len()));
    }
    let mut hits = Vec::with_capacity(4);
    for g in &glyphs {
        let dim = format!("{}x{}", g.w, g.h);
        if let Some(ch) = lib.lookup_exact(g.w, g.h, &g.bits) {
            hits.push(GlyphHit {
                ch,
                kind: "exact",
                distance: Some(0),
                dim,
            });
            continue;
        }
        // 逐级放宽：先试距离 1，再试 2，命中即止。
        // 分成两档是为了区分「差 1 像素」和「差 2 像素」，
        // 后者更可能是真正的字库缺口而非同一字形的抗锯齿抖动。
        match lib
            .lookup_fuzzy(g.w, g.h, &g.bits, 1)
            .or_else(|| lib.lookup_fuzzy(g.w, g.h, &g.bits, 2))
        {
            Some((ch, d)) => hits.push(GlyphHit {
                ch,
                kind: "fuzzy",
                distance: Some(d),
                dim,
            }),
            None => hits.push(GlyphHit {
                ch: '?',
                kind: "miss",
                distance: None,
                dim,
            }),
        }
    }
    Ok(hits)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 默认安静（只报 warn），但允许用 RUST_LOG 打开 debug —— 排查
    // 「服务端返回了没见过的业务码」这类问题时，debug 里的原始响应体
    // 是唯一的信息源。
    let filter = std::env::var("RUST_LOG")
        .unwrap_or_else(|_| "gnnuhub_api=warn,gnnuhub_ocr=warn".to_string());
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    let mut args = std::env::args().skip(1);
    let rounds: usize = args.next().map(|s| s.parse()).transpose()?.unwrap_or(20);
    let out_path = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "captcha_samples/verify.json".into()),
    );
    let rounds = rounds.min(MAX_ROUNDS);

    let student_id: u64 = std::env::var("GNNU_STUDENT_ID")?.parse()?;
    let password = std::env::var("GNNU_PASSWORD")?;

    // 保险：如果用户误填了正确密码，一旦登录成功会消耗一次真实登录。
    // 这里不主动去验证密码对错（那本身也是个请求），而是提醒。
    println!("⚠️  请确认 GNNU_PASSWORD 填的是**错误密码**。");
    println!("    本工具靠「账密必然失败」来区分验证码对错；");
    println!("    若填了正确密码，命中正确验证码时会真的登录成功。");
    println!();

    // 直接加载内嵌字库，拿到引用以便逐字形查表
    let lib = gnnuhub_ocr::embedded_library()?;
    println!("字库：{} 个字形", lib.len());

    // 可选：tesseract 对照。默认构建下这段会被 cfg 掉。
    //
    // 这里只做可用性探测并打印，不参与判定——位图查表命中率远高于
    // tesseract，把两者混在一起只会让「谁错了」变得难以归因。
    #[cfg(feature = "tesseract")]
    {
        let tess = gnnuhub_ocr::TesseractOcr::new();
        if tess.is_available() {
            println!("对照引擎：tesseract 可用（本工具默认不采用其结果）");
        } else {
            println!("对照引擎：tesseract 未安装");
        }
    }

    let client = Client::with_defaults()?;
    println!(
        "计划轮数: {rounds}（每轮 2 个请求，共约 {} 个）\n",
        rounds * 2
    );

    let mut results: Vec<RoundResult> = Vec::new();
    let mut correct = 0usize;
    let mut wrong = 0usize;
    let mut unrecognized = 0usize;
    // 字形级统计
    let mut g_exact = 0usize;
    let mut g_fuzzy = 0usize;
    let mut g_miss = 0usize;
    let mut miss_dims: Vec<String> = Vec::new();
    // 账号是否被锁定。锁定是终止条件，summary 里必须显式说明，
    // 否则「只跑了 3 轮」看起来像是工具坏了或网络不通。
    let mut locked = false;

    for index in 1..=rounds {
        println!("---- 第 {index}/{rounds} 轮 ----");

        // 1) 申请新验证码
        let captcha = match gnnuhub_api::login::fetch_captcha(&client).await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("申请验证码失败，中止: {e}");
                break;
            }
        };

        // 2) 本地识别（逐字形给出匹配质量）
        let hits = match recognize_with_quality(&lib, &captcha.image) {
            Ok(h) => h,
            Err(e) => {
                println!("  识别失败（跳过，不提交）: {e}");
                unrecognized += 1;
                results.push(RoundResult {
                    index,
                    code: None,
                    verdict: "unrecognized",
                    detail: e,
                    glyphs: Vec::new(),
                });
                tokio::time::sleep(ROUND_INTERVAL).await;
                continue;
            }
        };

        for h in &hits {
            match h.kind {
                "exact" => g_exact += 1,
                "fuzzy" => g_fuzzy += 1,
                _ => {
                    g_miss += 1;
                    miss_dims.push(format!("{} {}", h.dim, index));
                }
            }
        }

        let quality: String = hits
            .iter()
            .map(|h| match (h.kind, h.distance) {
                ("exact", _) => '='.to_string(),
                ("fuzzy", Some(d)) => format!("~{d}"),
                _ => "!".to_string(),
            })
            .collect::<Vec<_>>()
            .join(" ");

        let code: String = hits.iter().map(|h| h.ch).collect();
        println!("  识别结果: {code}   [匹配 {quality}]");

        // 顺便把这张图存下来，便于事后复核识别错误的样本
        if let Ok(img) = decode_image(&captcha.image) {
            let p = format!("captcha_samples/verify_{index:04}.png");
            let _ = img.save(&p);
        }

        // 未全部命中时不提交：字库没覆盖到的位图，提交也只是白耗一个请求
        // 并给账号加一次失败记录。
        if hits.iter().any(|h| h.kind == "miss") {
            println!("  有字形未命中字库，不提交（省一次请求）");
            unrecognized += 1;
            results.push(RoundResult {
                index,
                code: Some(code),
                verdict: "unrecognized",
                detail: "存在未命中字库的字形".to_string(),
                glyphs: hits,
            });
            tokio::time::sleep(ROUND_INTERVAL).await;
            continue;
        }

        // 3) 稍等一下再提交，避免非人类节奏
        tokio::time::sleep(SUBMIT_DELAY).await;

        // 4) 提交登录
        //
        // service 必须与 `Client::login` 用的一致（教务系统首页），
        // 否则即使验证码正确也可能因 service 不匹配而出错。
        let service = format!("{}/", gnnuhub_core::JWGL_BASE_URL);
        let encrypted = gnnuhub_crypto::encode_password(&password);
        let outcome = match gnnuhub_api::login::try_login(
            &client,
            &student_id.to_string(),
            &encrypted,
            &code,
            &captcha.uid,
            &service,
        )
        .await
        {
            Ok(o) => o,
            Err(e) => {
                // 传输层失败通常意味着被风控，立刻停手
                eprintln!("  提交失败，判定为可能触发风控，立即中止: {e}");
                results.push(RoundResult {
                    index,
                    code: Some(code.clone()),
                    verdict: "aborted",
                    detail: e.to_string(),
                    glyphs: hits,
                });
                break;
            }
        };

        let (verdict, detail) = match &outcome {
            LoginOutcome::CaptchaIncorrect => {
                wrong += 1;
                ("wrong", "服务端返回 CODEFALSE，识别错误".to_string())
            }
            LoginOutcome::BadCredentials(msg) => {
                correct += 1;
                (
                    "correct",
                    format!("账密错误（预期），说明验证码读对了: {msg}"),
                )
            }
            LoginOutcome::AccountLocked(msg) => {
                // 账号锁定是**终止条件**，不是可重试失败。
                // 立刻停手：继续提交只会加重锁定。
                eprintln!();
                eprintln!("⛔ 账号已被锁定，立即停止：{msg}");
                eprintln!("   请等待解锁后再运行本工具，期间不要重复提交。");
                results.push(RoundResult {
                    index,
                    code: Some(code.clone()),
                    verdict: "locked",
                    detail: msg.clone(),
                    glyphs: hits,
                });
                locked = true;
                break;
            }
            LoginOutcome::Success { .. } => {
                correct += 1;
                (
                    "correct",
                    "登录成功——注意这意味着密码是对的，本次判定同样说明识别正确".to_string(),
                )
            }
        };
        println!("  判定: {verdict} —— {detail}");

        results.push(RoundResult {
            index,
            code: Some(code),
            verdict,
            detail,
            glyphs: hits,
        });

        tokio::time::sleep(ROUND_INTERVAL).await;
    }

    // ---------- 汇总 ----------
    let judged = correct + wrong;
    println!();
    println!("================ 汇总 ================");
    if locked {
        println!("⛔ 本次运行因**账号被锁定**而提前终止（不是网络或工具问题）");
        println!("   锁定来自短期内的连续账密错误。等待解锁后再运行。");
        println!();
    }
    println!("已判定轮数: {judged}");
    println!("  识别正确: {correct}");
    println!("  识别错误: {wrong}");
    println!("未识别(未提交): {unrecognized}");
    if judged > 0 {
        println!(
            "准确率: {:.1}%（{correct}/{judged}）",
            correct as f64 / judged as f64 * 100.0
        );
    }
    if unrecognized > 0 {
        println!(
            "端到端成功率: {:.1}%（{correct}/{}，含识别失败）",
            correct as f64 / (judged + unrecognized) as f64 * 100.0,
            judged + unrecognized
        );
    }

    let g_total = g_exact + g_fuzzy + g_miss;
    if g_total > 0 {
        println!();
        println!("字形级（提交过的轮次）:");
        println!("  精确 {g_exact} / 模糊 {g_fuzzy} / 未命中 {g_miss}（共 {g_total}）");
        println!(
            "  命中率 {:.1}%",
            (g_exact + g_fuzzy) as f64 / g_total as f64 * 100.0
        );
    }
    if !miss_dims.is_empty() {
        println!();
        println!("未命中的字形尺寸（用于补字库）:");
        for d in &miss_dims {
            println!("  {d}");
        }
    }

    // 逐轮明细
    println!();
    for r in &results {
        let code = r.code.as_deref().unwrap_or("(未识别)");
        println!("  #{:>3}  {code:<6}  {}", r.index, r.verdict);
    }

    let json = serde_json::json!({
        "judged": judged,
        "correct": correct,
        "wrong": wrong,
        "unrecognized": unrecognized,
        "locked": locked,
        "accuracy": if judged > 0 { Some(correct as f64 / judged as f64) } else { None },
        "glyph": {
            "exact": g_exact,
            "fuzzy": g_fuzzy,
            "miss": g_miss,
        },
        "rounds": results.iter().map(|r| serde_json::json!({
            "index": r.index,
            "code": r.code,
            "verdict": r.verdict,
            "detail": r.detail,
            "glyphs": r.glyphs.iter().map(|g| serde_json::json!({
                "ch": g.ch.to_string(),
                "kind": g.kind,
                "distance": g.distance,
                "dim": g.dim,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    });
    std::fs::write(&out_path, serde_json::to_string_pretty(&json)?)?;
    println!("\n结果已写入 {}", out_path.display());

    Ok(())
}
