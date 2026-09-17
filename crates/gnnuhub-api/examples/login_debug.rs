//! 两阶段登录调试工具
//!
//! 因为验证码识别器尚未实现，把登录流程拆成两步便于调试：
//!
//! - `captcha`：抓一张验证码，保存图片并打印 uid
//! - `submit <uid> <code>`：用给定的验证码提交登录
//!
//! 运行:
//!
//! ```bash
//! GNNU_STUDENT_ID=xxx GNNU_PASSWORD='xxx' cargo run -p gnnuhub-api --example login_debug -- captcha
//! GNNU_STUDENT_ID=xxx GNNU_PASSWORD='xxx' cargo run -p gnnuhub-api --example login_debug -- submit <uid> <code>
//! ```

use gnnuhub_api::Client;

const CAPTCHA_PNG: &str = "captcha_login.png";
const CAPTCHA_STATE: &str = "captcha_state.json";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 输出内部诊断日志；用 RUST_LOG=warn 可只看关键信息
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_target(false)
        .init();

    let args: Vec<String> = std::env::args().collect();
    let command = args.get(1).map(String::as_str).unwrap_or("captcha");

    match command {
        "captcha" => fetch_captcha().await,
        "submit" => {
            let uid = args.get(2).ok_or("缺少 uid 参数")?;
            let code = args.get(3).ok_or("缺少验证码参数")?;
            submit_login(uid, code).await
        }
        "auto" => {
            let code = args.get(2).ok_or("缺少验证码参数")?;
            auto_login(code).await
        }
        other => {
            eprintln!("未知命令: {other}");
            eprintln!("用法: captcha | submit <uid> <code> | auto <code>");
            std::process::exit(1);
        }
    }
}

/// 抓取验证码并保存到文件
async fn fetch_captcha() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::with_defaults()?;
    let captcha = gnnuhub_api::login::fetch_captcha(&client).await?;

    // 保存图片
    let img = gnnuhub_ocr::decode_image(&captcha.image)?;
    img.save(CAPTCHA_PNG)?;

    // 保存 uid 供下一步使用
    std::fs::write(CAPTCHA_STATE, &captcha.uid)?;

    println!("验证码已保存: {CAPTCHA_PNG}");
    println!("图片尺寸: {} x {}", img.width(), img.height());
    println!("uid: {}", captcha.uid);
    println!("有效期: {} 秒", captcha.timeout_secs);

    Ok(())
}

/// 用给定的验证码提交登录
async fn submit_login(uid: &str, code: &str) -> Result<(), Box<dyn std::error::Error>> {
    let (student_id, password) = read_credentials()?;
    let client = Client::with_defaults()?;

    let service = "https://jwgl.gnnu.edu.cn/";
    let encrypted = gnnuhub_crypto::encode_password(&password);

    println!("提交登录: 学号={student_id} 验证码={code}");
    let outcome = gnnuhub_api::login::try_login(
        &client,
        &student_id.to_string(),
        &encrypted,
        code,
        uid,
        service,
    )
    .await?;

    match outcome {
        gnnuhub_api::LoginOutcome::Success { tgt, ticket } => {
            println!("认证成功");
            println!("  TGT: {}...", &tgt[..tgt.len().min(30)]);
            println!("  ticket: {}...", &ticket[..ticket.len().min(30)]);

            // 继续换取教务系统会话
            println!("\n换取教务系统会话...");
            match gnnuhub_api::login::exchange_ticket_for_session(&client, &ticket, &tgt, 10).await
            {
                Ok(cookies) => {
                    println!("获得 {} 个 Cookie:", cookies.len());
                    for (k, v) in &cookies {
                        println!("  {k} = {}...", &v[..v.len().min(20)]);
                    }

                    // 会话是否真的可用，要以能否取到数据为准
                    println!("\n验证会话（拉取学籍信息）...");
                    let session =
                        gnnuhub_api::Session::new(client.clone(), cookies, student_id.to_string());
                    match session.fetch_basic_info().await {
                        Ok(info) => {
                            println!("会话有效");
                            println!("  学号: {}", info.student_id.as_deref().unwrap_or("(无)"));
                            println!("  姓名: {}", info.name.as_deref().unwrap_or("(无)"));
                            println!("  身份: {}", info.identity.as_deref().unwrap_or("(无)"));
                            println!("  学院: {}", info.college.as_deref().unwrap_or("(无)"));
                            println!("  专业: {}", info.major.as_deref().unwrap_or("(无)"));
                            println!("  班级: {}", info.class_name.as_deref().unwrap_or("(无)"));
                        }
                        Err(e) => println!("会话无效: {e}"),
                    }
                }
                Err(e) => println!("会话交换失败: {e}"),
            }
        }
        gnnuhub_api::LoginOutcome::CaptchaIncorrect => {
            println!("验证码错误");
        }
        gnnuhub_api::LoginOutcome::BadCredentials(msg) => {
            println!("凭据错误: {msg}");
        }
    }

    Ok(())
}

/// 自动完成整个流程（验证码由外部识别后传入）
async fn auto_login(code: &str) -> Result<(), Box<dyn std::error::Error>> {
    let (student_id, password) = read_credentials()?;
    let client = Client::with_defaults()?;

    // 先抓新验证码
    let captcha = gnnuhub_api::login::fetch_captcha(&client).await?;
    let img = gnnuhub_ocr::decode_image(&captcha.image)?;
    img.save(CAPTCHA_PNG)?;
    println!("验证码图片: {CAPTCHA_PNG} (uid={})", captcha.uid);

    let service = "https://jwgl.gnnu.edu.cn/";
    let encrypted = gnnuhub_crypto::encode_password(&password);

    let outcome = gnnuhub_api::login::try_login(
        &client,
        &student_id.to_string(),
        &encrypted,
        code,
        &captcha.uid,
        service,
    )
    .await?;

    match outcome {
        gnnuhub_api::LoginOutcome::Success { tgt, ticket } => {
            println!("认证成功");
            let cookies =
                gnnuhub_api::login::exchange_ticket_for_session(&client, &ticket, &tgt, 10).await?;
            println!("会话已建立，{} 个 Cookie", cookies.len());
        }
        other => println!("登录未成功: {other:?}"),
    }

    Ok(())
}

/// 从环境变量读取凭据
fn read_credentials() -> Result<(u64, String), Box<dyn std::error::Error>> {
    let id: u64 = std::env::var("GNNU_STUDENT_ID")?.parse()?;
    let pw = std::env::var("GNNU_PASSWORD")?;
    Ok((id, pw))
}
