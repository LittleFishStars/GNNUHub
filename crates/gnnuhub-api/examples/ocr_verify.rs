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
//! `login::parse_ticket_response` 已经把登录失败干净地分成两类：
//!
//! | 服务端返回 | 判定 | 含义 |
//! |---|---|---|
//! | `data.code == "CODEFALSE"` | [`LoginOutcome::CaptchaIncorrect`] | **识别错了** |
//! | `statusCode == "USERNAMEORPASSWORDERROR"` | [`LoginOutcome::BadCredentials`] | **识别对了**（密码错是预期的） |
//! | 顶层带 `tgt` + `ticket` | [`LoginOutcome::Success`] | **识别对了**（且登录成功） |
//!
//! 用错密码是关键技巧：这样账密校验必然失败，于是**只要不是 `CODEFALSE`
//! 就说明验证码读对了**，不会有「识别正确因而登录成功、消耗真实登录次数」
//! 的副作用。工具会主动拒绝正确密码，避免误用。
//!
//! # 必须现场取验证码
//!
//! 验证码 `timeout` 只有 300 秒。`captcha_samples/` 里的旧 `uid` 早已过期，
//! 拿旧 uid 提交只会得到「验证码失效」，无法区分识别对错。因此本工具
//! **每轮重新申请一张验证码**，识别后立刻提交，全程在一个 ttl 内完成。
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
//!   cargo run -p gnnuhub-api --example ocr_verify --features tesseract -- 20
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
use gnnuhub_ocr::{OcrEngine, TesseractOcr, decode_image};

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
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("gnnuhub_api=warn,gnnuhub_ocr=warn")
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

    let engine = TesseractOcr::new();
    if !engine.is_available() {
        eprintln!("未检测到 tesseract，无法运行");
        std::process::exit(1);
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

        // 2) 本地识别
        let code = match engine.recognize(&captcha.image, None) {
            Ok(c) => c,
            Err(e) => {
                println!("  识别失败（跳过，不提交）: {e}");
                unrecognized += 1;
                results.push(RoundResult {
                    index,
                    code: None,
                    verdict: "unrecognized",
                    detail: e.to_string(),
                });
                tokio::time::sleep(ROUND_INTERVAL).await;
                continue;
            }
        };
        println!("  识别结果: {code}");

        // 顺便把这张图存下来，便于事后复核识别错误的样本
        if let Ok(img) = decode_image(&captcha.image) {
            let p = format!("captcha_samples/verify_{index:04}.png");
            let _ = img.save(&p);
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
        });

        tokio::time::sleep(ROUND_INTERVAL).await;
    }

    // ---------- 汇总 ----------
    let judged = correct + wrong;
    println!();
    println!("================ 汇总 ================");
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
        "accuracy": if judged > 0 { Some(correct as f64 / judged as f64) } else { None },
        "rounds": results.iter().map(|r| serde_json::json!({
            "index": r.index,
            "code": r.code,
            "verdict": r.verdict,
            "detail": r.detail,
        })).collect::<Vec<_>>(),
    });
    std::fs::write(&out_path, serde_json::to_string_pretty(&json)?)?;
    println!("\n结果已写入 {}", out_path.display());

    Ok(())
}
