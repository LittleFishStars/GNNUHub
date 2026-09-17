//! 验证码识别抽象层
//!
//! 实验表明赣南师范大学统一身份认证平台的验证码具有以下特征：
//!
//! - 尺寸固定 **100 × 25** 像素，PNG 格式
//! - 固定 **4 个字符**，字母数字混合，**大小写混用**
//! - 每个字符**颜色随机**且互不相同，白色背景
//! - 无旋转、无扭曲、无干扰线
//!
//! 由于字符颜色随机，识别前必须先做灰度化丢弃颜色信息。
//!
//! # 可插拔设计
//!
//! 识别器通过 [`OcrEngine`] trait 抽象，上层登录逻辑只依赖该 trait，
//! 因此可以自由替换实现：
//!
//! - [`ManualOcr`]：不做识别，把图片交给调用方人工输入（默认实现）
//! - 模板匹配 OCR：需要预先采集样本训练字型库
//! - 云打码服务：网络调用第三方识别接口
//!
//! # 示例
//!
//! ```no_run
//! use gnnuhub_ocr::{InteractiveFn, ManualOcr, OcrEngine};
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let engine = ManualOcr::new();
//!     // 实际使用时把接口返回的 base64 图片传进来
//!     let base64_image = "data:image/png;base64,iVBORw0KGgo=";
//!
//!     // 交互回调：界面层在这里展示图片并取得用户输入
//!     let callback: &InteractiveFn = &|_image: &str| {
//!         // 真实场景中把图片交给界面渲染，这里直接返回示例值
//!         Ok(Some("aB3d".to_string()))
//!     };
//!
//!     let result = engine.recognize(base64_image, Some(callback))?;
//!     assert_eq!(result, "aB3d");
//!     Ok(())
//! }
//! ```

use base64::Engine as _;
use gnnuhub_core::{Error, Result};
use image::DynamicImage;

/// 验证码识别引擎
///
/// 实现者需要把 base64 编码的图片转换为 4 位字符。
/// `interactive` 参数提供给需要人工介入的实现使用；纯自动的
/// 实现可以忽略它。
pub trait OcrEngine: Send + Sync {
    /// 识别验证码
    ///
    /// # 参数
    ///
    /// - `image_base64`：接口返回的图片，可能是 `data:image/png;base64,`
    ///   前缀的 data URL，也可能是裸 base64
    /// - `interactive`：可选的交互回调。需要人工输入时调用它，
    ///   传入可展示给用户的图片（data URL），返回用户输入的文本
    ///
    /// # 错误
    ///
    /// 当实现无法完成识别（例如需要人工输入但没有提供回调）时，
    /// 返回 [`Error::CaptchaRequiresManualInput`]。
    fn recognize(
        &self,
        image_base64: &str,
        interactive: Option<&InteractiveFn>,
    ) -> Result<String>;

    /// 识别器的可读名称，用于日志与诊断
    fn name(&self) -> &'static str;
}

/// 人工输入回调的类型别名
///
/// 接收验证码图片的 data URL，返回用户输入的文本。
/// 回调返回 `Ok(None)` 表示用户取消。
pub type InteractiveFn = dyn Fn(&str) -> Result<Option<String>> + Send + Sync;

/// 把可能的 data URL 前缀剥离，得到裸 base64
pub fn strip_data_url_prefix(input: &str) -> &str {
    match input.split_once(";base64,") {
        Some((_, rest)) => rest,
        None => input,
    }
}

/// 从 base64（或 data URL）解码为图片
///
/// # 错误
///
/// base64 解码失败或图片格式不被支持时返回错误。
pub fn decode_image(input: &str) -> Result<DynamicImage> {
    let raw = strip_data_url_prefix(input);
    // 去掉可能存在的空白字符，部分接口返回值带有换行
    let cleaned: String = raw.chars().filter(|c| !c.is_whitespace()).collect();

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(cleaned.as_bytes())
        .map_err(|e| Error::Image(format!("base64 解码失败: {e}")))?;

    image::load_from_memory(&bytes).map_err(|e| Error::Image(format!("图片解析失败: {e}")))
}

/// 手动输入识别器
///
/// 不执行任何自动识别，而是把验证码图片通过回调交给调用方
/// （通常是界面层），由用户肉眼识别后输入。
///
/// 这是默认实现，适合在自动识别尚未就绪时打通整条登录链路。
#[derive(Debug, Default, Clone, Copy)]
pub struct ManualOcr;

impl ManualOcr {
    /// 创建手动识别器
    pub const fn new() -> Self {
        Self
    }
}

impl OcrEngine for ManualOcr {
    fn recognize(
        &self,
        image_base64: &str,
        interactive: Option<&InteractiveFn>,
    ) -> Result<String> {
        let callback =
            interactive.ok_or(Error::CaptchaRequiresManualInput)?;

        // 规范化成 data URL 形式再交给界面层，避免前缀缺失导致渲染失败
        let data_url = if image_base64.starts_with("data:") {
            image_base64.to_string()
        } else {
            format!("data:image/png;base64,{image_base64}")
        };

        let input = callback(&data_url)?.ok_or(Error::CaptchaRequiresManualInput)?;
        let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();

        validate_captcha_format(&cleaned)?;
        Ok(cleaned)
    }

    fn name(&self) -> &'static str {
        "manual"
    }
}

/// 校验验证码格式是否符合「4 位字母数字」
fn validate_captcha_format(input: &str) -> Result<()> {
    let len = input.chars().count();
    if len != CAPTCHA_LENGTH {
        return Err(Error::InvalidCaptcha);
    }
    if !input.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(Error::InvalidCaptcha);
    }
    Ok(())
}

/// 验证码字符数
pub const CAPTCHA_LENGTH: usize = 4;

/// 验证码图片的固定宽度
pub const CAPTCHA_WIDTH: u32 = 100;

/// 验证码图片的固定高度
pub const CAPTCHA_HEIGHT: u32 = 25;

#[cfg(test)]
mod tests {
    use super::*;

    /// data URL 前缀应被正确剥离
    #[test]
    fn strips_data_url_prefix() {
        assert_eq!(strip_data_url_prefix("data:image/png;base64,AAAA"), "AAAA");
        assert_eq!(strip_data_url_prefix("AAAA"), "AAAA");
    }

    /// 手动识别器在没有回调时必须报错而不是 panic
    #[test]
    fn manual_ocr_without_callback_errors() {
        let engine = ManualOcr::new();
        let err = engine.recognize("AAAA", None).unwrap_err();
        assert!(matches!(err, Error::CaptchaRequiresManualInput));
    }

    /// 回调返回的合法输入应被接受
    #[test]
    fn manual_ocr_accepts_valid_input() {
        let engine = ManualOcr::new();
        let cb: &InteractiveFn = &|_img: &str| Ok(Some("aB3d".to_string()));
        let result = engine.recognize("AAAA", Some(cb)).unwrap();
        assert_eq!(result, "aB3d");
    }

    /// 回调返回的输入会被去除空白字符
    #[test]
    fn manual_ocr_trims_whitespace() {
        let engine = ManualOcr::new();
        let cb: &InteractiveFn = &|_img: &str| Ok(Some(" aB3d \n".to_string()));
        let result = engine.recognize("AAAA", Some(cb)).unwrap();
        assert_eq!(result, "aB3d");
    }

    /// 长度不符的输入应被拒绝
    #[test]
    fn manual_ocr_rejects_wrong_length() {
        let engine = ManualOcr::new();
        let cb: &InteractiveFn = &|_img: &str| Ok(Some("abc".to_string()));
        assert!(engine.recognize("AAAA", Some(cb)).is_err());
    }

    /// 含非字母数字字符的输入应被拒绝
    #[test]
    fn manual_ocr_rejects_non_alphanumeric() {
        let engine = ManualOcr::new();
        let cb: &InteractiveFn = &|_img: &str| Ok(Some("ab-c".to_string()));
        assert!(engine.recognize("AAAA", Some(cb)).is_err());
    }

    /// 用户取消应返回错误
    #[test]
    fn manual_ocr_handles_cancel() {
        let engine = ManualOcr::new();
        let cb: &InteractiveFn = &|_img: &str| Ok(None);
        assert!(engine.recognize("AAAA", Some(cb)).is_err());
    }

    /// 缺少前缀的图片会被补全为 data URL
    #[test]
    fn manual_ocr_normalizes_data_url() {
        let engine = ManualOcr::new();
        let cb: &InteractiveFn = &|img: &str| {
            assert!(img.starts_with("data:image/png;base64,"));
            Ok(Some("aB3d".to_string()))
        };
        engine.recognize("AAAA", Some(cb)).unwrap();
    }

    /// 非法 base64 应产生图像错误
    #[test]
    fn decode_image_rejects_invalid_base64() {
        let err = decode_image("!!!not-base64!!!").unwrap_err();
        assert!(matches!(err, Error::Image(_)));
    }
}
