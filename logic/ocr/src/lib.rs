//! 验证码识别
//!
//! 实验表明赣南师范大学统一身份认证平台的验证码具有以下特征：
//!
//! - 尺寸固定 **100 × 25** 像素，PNG 格式
//! - 固定 **4 个字符**，字母数字混合，**大小写混用**
//! - 每个字符**颜色随机**且互不相同，白色背景
//! - **启用抗锯齿**（一个字形约 31 种颜色：1 个实心核心 + 约 30 个过渡色）
//! - 无旋转、无扭曲、无干扰线；实测**没有噪点**（`i`/`j` 的点是独立连通域，
//!   但那是字形的一部分，必须合并回主干）
//!
//! 由于字符颜色随机，识别前必须先丢弃颜色信息。
//!
//! # 只有一条识别路径：位图查表
//!
//! **字形渲染是确定性的** —— 同一 (字符, 字号) 组合在不同图片、不同颜色下
//! 产生逐像素相同的点阵。既然渲染确定，识别就退化成查表，
//! **准确率上限只取决于字库覆盖度**，与分类器精度无关。
//!
//! 实测真值验证 **72/72 = 100%**（见 `examples/ocr_verify.rs`）。
//! 因此这里不再保留通用 OCR（tesseract）与人工输入兜底：
//! 查表已经打满，别的路径只会引入「猜错」这一新的失败模式。
//!
//! # 未命中时的行为：报错，绝不猜
//!
//! 字库是采样得来的，必然存在未收录字形。此时 [`BitmapOcr`] **直接报错**，
//! 并把真实的宽高与位串带在错误信息里，便于事后补进字库。
//!
//! 之所以不让通用 OCR 兜底：拿一个猜出来的字符去登录，会白耗一次服务端
//! 尝试（且可能推进锁定计数），而报错是零成本的。**宁可报错也不猜。**
//!
//! 模糊匹配（汉明距离 ≤2）仍然保留，但它不是「另一种方式」——它针对的是
//! 同一字符在亚像素定位下的 1~3 像素抖动，最终落到**同一个字符**上。
//!
//! # 示例
//!
//! ```no_run
//! use gnnuhub_ocr::{BitmapOcr, OcrEngine};
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let engine = BitmapOcr::embedded()?;
//!     // 实际使用时把接口返回的 base64 图片传进来
//!     let base64_image = "data:image/png;base64,iVBORw0KGgo=";
//!     match engine.recognize(base64_image) {
//!         Ok(code) => println!("识别结果: {code}"),
//!         Err(e) => eprintln!("未命中字库（不猜，直接放弃本轮）: {e}"),
//!     }
//!     Ok(())
//! }
//! ```

use base64::Engine as _;
use gnnuhub_core::{Error, Result};
use image::DynamicImage;

pub mod bitmap;

pub use bitmap::{BitmapLibrary, BitmapOcr, EMBEDDED_LIBRARY_JSON, GlyphEntry, embedded_library};

/// 验证码识别引擎
///
/// 实现者需要把 base64 编码的图片转换为 4 位字符。
pub trait OcrEngine: Send + Sync {
    /// 识别验证码
    ///
    /// # 参数
    ///
    /// - `image_base64`：接口返回的图片，可能是 `data:image/png;base64,`
    ///   前缀的 data URL，也可能是裸 base64
    ///
    /// # 错误
    ///
    /// 实现无法识别该图片时返回错误。**不允许猜**：宁可报错也不给出
    /// 一个可能错误的字符，因为错误字符会白耗一次登录尝试。
    fn recognize(&self, image_base64: &str) -> Result<String>;

    /// 识别器的可读名称，用于日志与诊断
    fn name(&self) -> &'static str;
}

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

/// 校验验证码格式是否合法
///
/// 规则：恰好 [`CAPTCHA_LENGTH`] 个字符，且都落在 [`CAPTCHA_ALPHABET`] 之内。
///
/// 注意**不能**用 [`char::is_ascii_alphanumeric`] 校验：实测样本
/// （`captcha_samples/0009.png`）出现过 `+` 号字符，用字母数字规则会
/// 把合法验证码判为非法。
pub fn validate_captcha_format(input: &str) -> Result<()> {
    let chars: Vec<char> = input.chars().collect();
    if chars.len() != CAPTCHA_LENGTH {
        return Err(Error::InvalidCaptcha);
    }
    if !chars.iter().all(|c| CAPTCHA_ALPHABET.contains(c)) {
        return Err(Error::InvalidCaptcha);
    }
    Ok(())
}

/// 验证码字符数
pub const CAPTCHA_LENGTH: usize = 4;

/// 验证码可能出现的字符集合
///
/// `0-9` + `A-Z` + `a-z`，共 62 个，**不含标点**，**大小写都有意义**。
///
/// 该集合已由真值验证（`examples/ocr_verify.rs`）确认：
///
/// - 大小写区分由字形高度/基线承载（cap-height 顶 `y0=5`、x-height 顶
///   `y0=8`、ascender `y0=4`），不是服务器大小写不敏感。
/// - 早期以为出现过的 `+` 实为 `t` 的误读，服务器并未使用标点。
/// - **数字 `0`/`1` 确实存在**，且曾因字库把它们误标成 `O`/`L` 而导致
///   每次真值含 `0`/`1` 就必错。参见
///   [`BitmapLibrary::labels`] 与 `tests/real_samples.rs` 里的字符集检查。
///
/// 该集合是**实测归纳**而非服务端公布值，若后续遇到新字符应放宽
/// 而不是直接拒绝，见 [`validate_captcha_format`] 的说明。
pub const CAPTCHA_ALPHABET: &[char] = &[
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I',
    'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', 'a', 'b',
    'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u',
    'v', 'w', 'x', 'y', 'z',
];

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

    /// 大小写混合、纯数字、纯字母都应被接受
    ///
    /// 回归测试：早期版本用 `is_ascii_alphanumeric()` 校验，会连带把
    /// 合法输入判错；同时确认大小写不会被规范化。
    #[test]
    fn validate_accepts_mixed_case_and_digits() {
        for s in ["aB3d", "ABCD", "abcd", "1234", "a1B2", "Zz9Q"] {
            assert!(validate_captcha_format(s).is_ok(), "应接受 {s}");
        }
    }

    /// 长度不对或含标点/空格的输入应被拒绝
    #[test]
    fn validate_rejects_bad_input() {
        for s in ["abc", "abcde", "ab c", "ab+c", "ab.c", "", "ab中c"] {
            assert!(validate_captcha_format(s).is_err(), "应拒绝 {s:?}");
        }
    }

    /// 非法 base64 应产生图像错误
    #[test]
    fn decode_image_rejects_invalid_base64() {
        let err = decode_image("!!!not-base64!!!").unwrap_err();
        assert!(matches!(err, Error::Image(_)));
    }

    /// 位图引擎的名字应稳定，便于日志归因
    #[test]
    fn bitmap_engine_name_is_stable() {
        let engine = BitmapOcr::embedded().expect("内嵌字库应能构造");
        assert_eq!(engine.name(), "bitmap");
    }

    /// 无法识别的垃圾图必须**报错**，而不是退回某种猜测
    ///
    /// 这是删除兜底路径后最重要的行为契约：字库未命中时宁可报错，
    /// 也不给出一个可能错误的字符（错字符会白耗一次登录尝试）。
    #[test]
    fn bitmap_engine_errors_instead_of_guessing_on_garbage() {
        let engine = BitmapOcr::embedded().expect("内嵌字库应能构造");
        let err = engine
            .recognize("data:image/png;base64,iVBORw0KGgo=")
            .expect_err("垃圾图不应被识别出任何字符");
        // 图像解码失败或字形未命中都算「如实报错」，只要不是给出结果
        assert!(
            matches!(err, Error::Image(_)),
            "应报图像/字形错误，实际: {err:?}"
        );
    }

    /// 未命中时的错误信息要带上真实宽高与**完整可粘贴的位串**
    ///
    /// 这一条守着「诊断能力」：报错若只说"识别失败"，发现字库缺口的人
    /// 还得重新提取一遍才知道缺什么。现在错误信息里的 `rows` 直接就是
    /// `bitmap_lib.json` 里要填的内容。
    ///
    /// 位串**不截断**也是刻意的——截断后就失去了直接补库的价值。
    #[test]
    fn miss_error_message_is_paste_ready_for_the_library() {
        // 造一张纯白底 + 若干色块的图，得到字库必然没有的字形
        let mut img = image::RgbaImage::from_pixel(100, 25, image::Rgba([255, 255, 255, 255]));
        for y in 5..20u32 {
            for x in 40..50u32 {
                img.put_pixel(x, y, image::Rgba([10, 200, 60, 255]));
            }
        }
        for y in 5..14u32 {
            for x in 55..62u32 {
                img.put_pixel(x, y, image::Rgba([200, 10, 60, 255]));
            }
        }
        for y in 8..20u32 {
            for x in 68..80u32 {
                img.put_pixel(x, y, image::Rgba([10, 60, 200, 255]));
            }
        }
        for y in 4..22u32 {
            for x in 85..90u32 {
                img.put_pixel(x, y, image::Rgba([120, 120, 10, 255]));
            }
        }
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .expect("编码 PNG");
        let b64 = base64::engine::general_purpose::STANDARD.encode(buf.get_ref());

        let engine = BitmapOcr::embedded().expect("内嵌字库应能构造");
        let err = engine
            .recognize(&format!("data:image/png;base64,{b64}"))
            .expect_err("人为构造的色块不应命中字库");
        let msg = err.to_string();

        // 必须给出宽高，否则不知道按什么尺寸去找
        assert!(
            msg.contains("字形") && msg.contains('x'),
            "错误信息应包含字形宽高，实际: {msg}"
        );
        // 必须给出可粘贴的位串（每行形如 `"1010...",`，与原 JSON 同格式）
        assert!(
            msg.contains('"'),
            "错误信息应包含可直接补进字库的 rows 片段，实际: {msg}"
        );
        // 位串必须完整：应有多行，不能被截断
        let quoted_rows = msg.matches("\",").count();
        assert!(
            quoted_rows > 5,
            "位串不应被截断（应有多个 rows 行），实际只有 {quoted_rows} 行"
        );
        // 每个 rows 行的长度应等于字形宽度，否则粘进字库也解析不了
        for line in msg.lines().filter(|l| l.contains("\",")) {
            let bits = line.trim().trim_matches(|c| c == '"' || c == ',');
            assert!(
                bits.len() == 10,
                "rows 行宽应为 10（字形宽度），实际 {} 位: {bits}",
                bits.len()
            );
            assert!(
                bits.chars().all(|c| c == '0' || c == '1'),
                "rows 行应只含 0/1，实际: {bits}"
            );
        }
    }
}
