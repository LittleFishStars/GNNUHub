//! 接口连通性检查示例
//!
//! 只验证到「能拿到验证码」这一步，不需要账号密码，
//! 用于确认网络与接口结构是否正常。
//!
//! 运行:
//!
//! ```bash
//! cargo run -p gnnuhub-api --example probe
//! ```

use gnnuhub_api::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let client = Client::with_defaults()?;

    println!("1. 检查统一认证平台可达性");
    match client
        .throttled(|http| http.get("https://cas.gnnu.edu.cn/"))
        .await
    {
        Ok(resp) => println!("   cas.gnnu.edu.cn -> HTTP {}", resp.status()),
        Err(e) => println!("   cas.gnnu.edu.cn 不可达: {e}"),
    }

    println!("2. 检查教务系统可达性");
    match client
        .throttled(|http| http.get("https://jwgl.gnnu.edu.cn/"))
        .await
    {
        Ok(resp) => println!("   jwgl.gnnu.edu.cn -> HTTP {}", resp.status()),
        Err(e) => println!("   jwgl.gnnu.edu.cn 不可达: {e}"),
    }

    println!("3. 申请验证码");
    let captcha = gnnuhub_api::captcha::Captcha::random_request_id();
    let url = gnnuhub_api::captcha::Captcha::request_url(&captcha);
    let headers = client.cas_headers();
    let resp = client
        .throttled(|http| http.get(&url).headers(headers.clone()))
        .await?;

    println!("   {} -> HTTP {}", url, resp.status());
    let body = resp.text().await?;
    let parsed: gnnuhub_api::captcha::CaptchaResponse = serde_json::from_str(&body)?;
    println!("   服务端 uid: {}", parsed.uid);
    println!("   验证码类型: {}", parsed.kaptcha_type);
    println!("   有效期: {} 秒", parsed.timeout);
    println!("   图片长度: {} 字符（base64）", parsed.content.len());

    // 校验图片确实能解码
    let img = gnnuhub_ocr::decode_image(&parsed.content)?;
    println!("   图片尺寸: {} x {}", img.width(), img.height());
    if img.width() != gnnuhub_ocr::CAPTCHA_WIDTH || img.height() != gnnuhub_ocr::CAPTCHA_HEIGHT {
        println!(
            "   注意: 尺寸与预期的 {}x{} 不一致，接口可能已变更",
            gnnuhub_ocr::CAPTCHA_WIDTH,
            gnnuhub_ocr::CAPTCHA_HEIGHT
        );
    }

    println!("\n接口探测完成");
    Ok(())
}
