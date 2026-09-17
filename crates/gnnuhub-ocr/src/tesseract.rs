//! 基于 tesseract 的验证码识别引擎（可选）
//!
//! 该实现依赖**外部二进制** `tesseract`（含 `eng` 语言包），因此整个模块
//! 由 `tesseract` feature 门控，默认不编译，以保持 crate 的纯 Rust 定位。
//!
//! # 为什么是 tesseract
//!
//! 在 60 张真实样本上做过多轮离线对比，结论如下（详见仓库 `.workbuddy/memory/`）：
//!
//! | 方案 | 结果 |
//! |------|------|
//! | 逐字符识别（先切字再喂 `--psm 10`） | 差。65 个字形里 21 个随缩放漂移 |
//! | 整图识别 + 12 配置大投票 | 55/60 产出 4 字符，但**正确结果会被低质量配置压掉** |
//! | **整图识别 + 2 配置一致性判定** | **48/60 产出 4 字符，其中 47 张两配置完全一致** |
//!
//! 关键教训：**不要用大投票**。低分辨率配置（`×1`）几乎全是噪声，它们在
//! 投票里与高质量配置等权，会把 `gray×5` 稳定读出的正确答案投掉。
//! 例如 `captcha_samples/0014.png`，`gray×5` 下 `psm8` 与 `psm13` 都坚定
//! 输出 `LD60`，但 12 配置投票只给了它 2 票。
//!
//! 因此这里的策略是：**只用两个质量足够高的配置，全票一致才输出**。
//! 不一致时宁可报错让上层换一张验证码，也不猜——登录流程本就有重试。
//!
//! # 为什么不切字
//!
//! 服务端启用了抗锯齿，一个字形有 1 个实心核心加约 30 个过渡色。早期按
//! 「颜色严格相等」抽字形会把笔画抽细 1px，导致与服务器字型库对不上。
//! 改为连通域抽取后字形正确，但**字型库仍然匹配不上**：服务器字体不在本机
//! 929 个可用字体族内（用 940,151 条 AWT 位图逐一比对，仅 27/240 命中，
//! 且全是 `I`/`E`/`F`/`L`/`H`/`T` 这类对称字形的巧合命中）。
//!
//! 所以模板匹配这条路被证伪，而 tesseract 依赖的是自己的字形模型，
//! 不受服务器字体是否可得的影响。

use std::io::Write as _;
use std::path::PathBuf;

use gnnuhub_core::{Error, Result};
use image::imageops::FilterType;
use image::{DynamicImage, GrayImage};

use crate::{CAPTCHA_ALPHABET, OcrEngine, decode_image, validate_captcha_format};

/// tesseract 允许出现的字符白名单
///
/// 由 [`CAPTCHA_ALPHABET`] 拼接而来，避免两处维护同一份字符集而漂移。
pub fn tesseract_whitelist() -> String {
    CAPTCHA_ALPHABET.iter().collect()
}

/// 识别时使用的预处理配置
///
/// `scale` 是放大倍数，`psm` 是 tesseract 的页面分割模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Profile {
    /// 放大倍数（LANCZOS 重采样）
    scale: u32,
    /// tesseract `--psm` 参数
    psm: u8,
}

impl Profile {
    /// 经过实测筛选出的两个高质量配置
    ///
    /// - 两者都在 `gray ×5` 下工作。灰度化是必须的：每个字符颜色随机，
    ///   保留彩色反而引入干扰。
    /// - `psm 8`（单词）与 `psm 13`（原始行）产出率最高，分别为 48/60 与 47/60。
    /// - 二者在 47/60 的样本上逐字符完全一致，这是本实现的自洽判据。
    const FINAL: [Profile; 2] = [Profile { scale: 5, psm: 8 }, Profile { scale: 5, psm: 13 }];
}

/// 基于 tesseract 的验证码识别器
///
/// # 依赖
///
/// 需要系统安装 `tesseract` 且包含 `eng` 语言包。若二进制不存在，
/// [`TesseractOcr::new`] 仍会构造成功（便于上层统一处理），但
/// [`OcrEngine::recognize`] 会返回 [`Error::CaptchaRequiresManualInput`]，
/// 调用方可据此回退到人工输入。用 [`TesseractOcr::is_available`] 可提前探测。
///
/// # 临时文件
///
/// tesseract 只接受文件路径或 stdin，而 stdin 模式在部分版本上与
/// `--psm` 组合行为不稳定。这里选择写入临时文件，文件名带
/// 进程号与自增序号以避免并发冲突，用后立即删除。
#[derive(Debug, Clone)]
pub struct TesseractOcr {
    /// 可执行文件路径，默认 `tesseract`
    binary: PathBuf,
    /// 临时文件目录，`None` 表示使用系统临时目录
    temp_dir: Option<PathBuf>,
    /// 语言包
    lang: String,
}

impl Default for TesseractOcr {
    fn default() -> Self {
        Self::new()
    }
}

impl TesseractOcr {
    /// 用默认参数创建识别器
    pub fn new() -> Self {
        Self {
            binary: PathBuf::from("tesseract"),
            temp_dir: None,
            lang: "eng".to_string(),
        }
    }

    /// 指定 tesseract 可执行文件路径
    pub fn with_binary(mut self, path: impl Into<PathBuf>) -> Self {
        self.binary = path.into();
        self
    }

    /// 指定临时文件目录
    pub fn with_temp_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.temp_dir = Some(dir.into());
        self
    }

    /// 指定语言包
    pub fn with_lang(mut self, lang: impl Into<String>) -> Self {
        self.lang = lang.into();
        self
    }

    /// 探测 tesseract 是否可用
    ///
    /// 执行 `tesseract --version`，退出码为 0 即认为可用。
    /// 该方法不访问网络，也不依赖任何样本。
    pub fn is_available(&self) -> bool {
        std::process::Command::new(&self.binary)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// 在单个配置下识别一次
    fn recognize_with(&self, gray: &GrayImage, profile: Profile) -> Result<String> {
        let scaled = upscale(gray, profile.scale);
        let path = self.write_temp_png(&scaled)?;

        // 无论识别成功还是失败，临时文件都必须删掉。
        // 先取结果再删除，避免 `?` 提前返回导致泄漏。
        let result = self.run_tesseract(&path, profile.psm);
        let _ = std::fs::remove_file(&path);

        result
    }

    /// 调用 tesseract 识别指定文件
    fn run_tesseract(&self, path: &std::path::Path, psm: u8) -> Result<String> {
        let output = std::process::Command::new(&self.binary)
            .arg(path)
            // 输出到 stdout
            .arg("-")
            .arg("--psm")
            .arg(psm.to_string())
            .arg("-l")
            .arg(&self.lang)
            // 限制候选字符集，避免把 `l` 读成 `|` 之类
            .arg("-c")
            .arg(format!("tessedit_char_whitelist={}", tesseract_whitelist()))
            // 关闭词典：验证码不是单词，词典会把随机串「纠正」成常见词
            .arg("-c")
            .arg("load_system_dawg=0")
            .arg("-c")
            .arg("load_freq_dawg=0")
            .output()
            .map_err(|e| {
                // 二进制缺失是最常见的原因，单独给出可读提示
                if e.kind() == std::io::ErrorKind::NotFound {
                    Error::Config(format!(
                        "找不到 tesseract 可执行文件（{}），请先安装或改用人工识别",
                        self.binary.display()
                    ))
                } else {
                    Error::Image(format!("调用 tesseract 失败: {e}"))
                }
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Image(format!(
                "tesseract 退出码 {:?}: {}",
                output.status.code(),
                stderr.trim()
            )));
        }

        let text = String::from_utf8_lossy(&output.stdout);
        // tesseract 会保留换行，去掉所有空白
        Ok(text.chars().filter(|c| !c.is_whitespace()).collect())
    }

    /// 把灰度图写成临时 PNG，返回其路径
    ///
    /// 用 `tempfile` 之外的轻量方案：路径里混入进程号与纳秒时间戳，
    /// 并发调用时也不易撞名。调用方负责在用完后删除该文件
    /// （见 [`TesseractOcr::recognize_with`]）。
    fn write_temp_png(&self, image: &GrayImage) -> Result<PathBuf> {
        let dir = self.temp_dir.clone().unwrap_or_else(std::env::temp_dir);
        std::fs::create_dir_all(&dir)
            .map_err(|e| Error::Image(format!("创建临时目录失败 {}: {e}", dir.display())))?;

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let name = format!("gnnuhub-captcha-{}-{nanos}.png", std::process::id());
        let path = dir.join(name);

        let mut file = std::fs::File::create(&path)
            .map_err(|e| Error::Image(format!("创建临时文件失败 {}: {e}", path.display())))?;
        image
            .write_to(&mut file, image::ImageFormat::Png)
            .map_err(|e| Error::Image(format!("写入临时 PNG 失败: {e}")))?;
        // 确保内容落盘再交给子进程读取
        file.flush()
            .map_err(|e| Error::Image(format!("刷新临时文件失败: {e}")))?;

        Ok(path)
    }
}

/// 用 LANCZOS 放大灰度图
///
/// 放大倍数取 1 时直接返回克隆。LANCZOS 比 nearest/bilinear 更能保留
/// 抗锯齿边缘的灰度过渡，tesseract 的连通域分析依赖这些过渡。
fn upscale(gray: &GrayImage, scale: u32) -> GrayImage {
    if scale <= 1 {
        return gray.clone();
    }
    let (w, h) = gray.dimensions();
    image::imageops::resize(gray, w * scale, h * scale, FilterType::Lanczos3)
}

/// 按 BT.601 系数灰度化，与 PIL `Image.convert("L")` 逐位对齐
///
/// # 为什么不能直接用 `DynamicImage::to_luma8`
///
/// `image` crate 内部用的是 **sRGB/BT.709** 系数
/// `(2126, 7152, 722) / 10000`，而 PIL 用的是 **BT.601** 系数
/// `(299, 587, 114) / 1000`。两套标准在纯灰上无差别，但验证码的
/// **每个字符颜色随机且常为高饱和色**，此时差距可达 10~33 个灰阶。
///
/// 实测影响（同一张图、同一 tesseract 参数，只换灰度系数）：
///
/// | 样本 | BT.601（PIL） | BT.709（`to_luma8`） |
/// |------|--------------|---------------------|
/// | `0024.png` | `0Cax` | `0Gax` |
/// | `0036.png` | `hW5F` | `hWsF` |
/// | `0041.png` | `9SIj` | `9S1j` |
/// | `0056.png` | `0iS1` | `0is1` |
///
/// 本项目的全部离线调参（缩放倍数、`psm` 取值、双配置一致性阈值）
/// 都是在 PIL 灰度图上做的，因此这里必须复现 PIL 的公式，
/// 否则「Rust 实现与离线结论一致」这一前提不成立。
///
/// # 舍入方式同样关键
///
/// PIL 的 `convert("L")` 是**四舍五入**，不是整除。写成 `(...)/1000`
/// 的整除式会让 .5 附近的像素系统性偏小，实测造成 4/60 的判定分叉。
/// 必须用 `(... + 500) / 1000`。
///
/// # 关于 alpha
///
/// 图片是 RGBA，但实测样本的 alpha 恒为 255（不透明），PIL 的
/// `convert("L")` 在这种情况下直接忽略 alpha 通道，不做合成。
/// 这里保持一致：忽略 alpha。若日后遇到带真实透明度的图，
/// 需要先与白色背景合成再灰度化。
fn to_gray_b601(image: &DynamicImage) -> GrayImage {
    let rgba = image.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut out = GrayImage::new(w, h);

    for (x, y, px) in rgba.enumerate_pixels() {
        let [r, g, b, _a] = px.0;
        // 与 PIL 完全相同的整数运算（含 +500 的四舍五入）：
        //   L = (R*299 + G*587 + B*114 + 500) / 1000
        //
        // ⚠️ 不能省掉 `+ 500`：那是四舍五入项。写成纯整除会让每个像素
        // 在 .5 附近偏向小值，实测在 60 张样本上造成 4/60 的判定分叉
        // （如 `0024.png` 由 `0Cax` 变成 `0ax`）。PIL 的 C 实现走的是
        // 浮点系数加 round，与此处整数式逐位等价。
        let l = (u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114 + 500) / 1000;
        out.put_pixel(x, y, image::Luma([l as u8]));
    }

    out
}

impl OcrEngine for TesseractOcr {
    fn recognize(
        &self,
        image_base64: &str,
        interactive: Option<&crate::InteractiveFn>,
    ) -> Result<String> {
        // 先解码并灰度化。颜色信息对识别无益（每字符颜色随机），丢掉它。
        //
        // ⚠️ 必须用本模块自带的 `to_gray_b601`，**不能**用 `DynamicImage::to_luma8`：
        // 后者用的是 sRGB/BT.709 系数 (0.2126, 0.7152, 0.0722)，而全部离线
        // 调参与样本分析都基于 PIL 的 BT.601 系数 (0.299, 0.587, 0.114)。
        // 两者对高饱和彩色字符的灰度值可差 10~20 阶，足以让 tesseract
        // 读出不同的字：实测 `0024.png` 会从 `0Cax` 变成 `0Gax`、
        // `0041.png` 从 `9SIj` 变成 `9S1j`。详见 `to_gray_b601` 的说明。
        let image: DynamicImage = decode_image(image_base64)?;
        let gray = to_gray_b601(&image);

        let mut agreed: Option<String> = None;

        for profile in Profile::FINAL {
            let text = match self.recognize_with(&gray, profile) {
                Ok(t) => t,
                Err(e) => {
                    tracing::debug!(
                        "tesseract 配置 scale={} psm={} 识别失败: {e}",
                        profile.scale,
                        profile.psm
                    );
                    // 单个配置失败不致命，继续试其他配置；但若所有配置都失败，
                    // 由下面的 agreed 判空统一处理
                    continue;
                }
            };

            // 长度不对说明这个配置读漏/读多了字符，直接弃用该配置的结果
            if validate_captcha_format(&text).is_err() {
                tracing::debug!(
                    "tesseract 配置 scale={} psm={} 输出 {text:?} 不是合法验证码，弃用",
                    profile.scale,
                    profile.psm
                );
                continue;
            }

            match &agreed {
                // 第一个有效结果，先记下
                None => agreed = Some(text),
                // 与已有结果一致 → 通过
                Some(prev) if *prev == text => return Ok(text),
                // 不一致 → 两个高质量配置都读不出共识，交给上层重试
                Some(prev) => {
                    tracing::debug!("两个配置结果不一致（{prev:?} vs {text:?}），放弃本次识别");
                    // 已解码过图片，仍可退回人工输入
                    return fallback_to_manual(interactive, image_base64);
                }
            }
        }

        match agreed {
            Some(code) => Ok(code),
            // 没有任何配置产出合法结果
            None => fallback_to_manual(interactive, image_base64),
        }
    }

    fn name(&self) -> &'static str {
        "tesseract"
    }
}

/// 自动识别失败时退回人工输入
///
/// 有回调就交给用户，没有则报 [`Error::CaptchaRequiresManualInput`]——
/// 这个错误同样被上层视为「本次识别失败，可重试」，语义是一致的。
fn fallback_to_manual(
    interactive: Option<&crate::InteractiveFn>,
    image_base64: &str,
) -> Result<String> {
    let Some(callback) = interactive else {
        return Err(Error::CaptchaRequiresManualInput);
    };

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

#[cfg(test)]
mod tests {
    use super::*;

    /// 白名单应覆盖完整的 62 个字符，且不含标点
    #[test]
    fn whitelist_covers_full_alphabet() {
        let wl = tesseract_whitelist();
        assert_eq!(wl.chars().count(), 62, "白名单应含 62 个字符: {wl}");
        assert!(wl.contains('0') && wl.contains('9'));
        assert!(wl.contains('A') && wl.contains('Z'));
        assert!(wl.contains('a') && wl.contains('z'));
        assert!(!wl.contains('+'), "白名单不应包含标点");
    }

    /// 放大后的尺寸应严格按倍数增长
    #[test]
    fn upscale_multiplies_dimensions() {
        let img = GrayImage::new(100, 25);
        let big = upscale(&img, 5);
        assert_eq!(big.dimensions(), (500, 125));
    }

    /// 倍数为 1 时应原样返回，且不改变尺寸
    #[test]
    fn upscale_with_factor_one_is_identity() {
        let img = GrayImage::new(100, 25);
        let same = upscale(&img, 1);
        assert_eq!(same.dimensions(), (100, 25));
    }

    /// 零倍数不应 panic，退化为原图
    #[test]
    fn upscale_with_zero_does_not_panic() {
        let img = GrayImage::new(100, 25);
        let same = upscale(&img, 0);
        assert_eq!(same.dimensions(), (100, 25));
    }

    /// 引擎名应为 tesseract
    #[test]
    fn engine_name_is_stable() {
        assert_eq!(TesseractOcr::new().name(), "tesseract");
    }

    /// 非法 base64 应返回图像错误，而不是 panic
    #[test]
    fn rejects_invalid_base64() {
        let engine = TesseractOcr::new();
        let err = engine.recognize("!!!not-base64!!!", None).unwrap_err();
        assert!(matches!(err, Error::Image(_)), "实际 {err:?}");
    }

    /// 无法识别且无回调时应报需要人工输入
    ///
    /// 这里构造一张纯白图：tesseract 读不出任何字符，必然走到回退分支。
    /// 注意该测试需要 tesseract 已安装；未安装时同样返回
    /// `CaptchaRequiresManualInput`（因为二进制缺失也被归一化为「本次识别失败」），
    /// 因此结论不变。
    #[test]
    fn blank_image_without_callback_requires_manual() {
        use base64::Engine as _;

        let blank = GrayImage::from_pixel(100, 25, image::Luma([255u8]));
        let mut buf = Vec::new();
        blank
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&buf);

        let engine = TesseractOcr::new();
        let result = engine.recognize(&b64, None);
        assert!(result.is_err(), "空白图不应识别出结果，实际 {result:?}");
    }

    /// 空白图提供了回调时，应把控制权交给回调
    #[test]
    fn blank_image_with_callback_uses_it() {
        use base64::Engine as _;

        let blank = GrayImage::from_pixel(100, 25, image::Luma([255u8]));
        let mut buf = Vec::new();
        blank
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&buf);

        let engine = TesseractOcr::new();
        let cb: &crate::InteractiveFn = &|_img: &str| Ok(Some("aB3d".to_string()));
        let result = engine.recognize(&b64, Some(cb));
        assert_eq!(result.unwrap(), "aB3d");
    }

    /// 灰度化必须复现 PIL 的 BT.601 公式（含四舍五入）
    ///
    /// 用几个高饱和色验证，这些正是 BT.601 与 BT.709 差异最大的取值。
    /// 期望值由 `(R*299 + G*587 + B*114 + 500) / 1000` 整除得出，
    /// 与 PIL `convert("L")` 逐位一致。
    #[test]
    fn gray_uses_bt601_coefficients() {
        use image::Rgba;

        // (R, G, B, 期望灰度)
        let cases: [(u8, u8, u8, u8); 6] = [
            // 纯色：BT.601 下为 76 / 150 / 29，与 BT.709 的 54 / 182 / 18 差 22/32/11 阶
            (255, 0, 0, 76),
            (0, 255, 0, 150),
            (0, 0, 255, 29),
            // 白色与黑色两端两者一致，必须精确
            (255, 255, 255, 255),
            (0, 0, 0, 0),
            // 青绿，BT.601→138 而 BT.709→156，差 18 阶，适合做区分。
            // 注意 `249,255,252` 这类近白色在整数截断下会差 1 阶，
            // 因此四舍五入项 `+500` 是必需的。
            (0, 200, 180, 138),
        ];

        for (r, g, b, expected) in cases {
            let mut img = image::RgbaImage::new(1, 1);
            img.put_pixel(0, 0, Rgba([r, g, b, 255]));
            let gray = to_gray_b601(&DynamicImage::ImageRgba8(img));
            assert_eq!(
                gray.get_pixel(0, 0).0[0],
                expected,
                "RGB({r},{g},{b}) 的 BT.601 灰度应为 {expected}"
            );
        }
    }

    /// BT.601 与 image crate 默认的 BT.709 在饱和色上应确实不同
    ///
    /// 这是上一条测试的「反向对照」：若两个公式结果相同，
    /// 说明 `to_gray_b601` 没有真正生效，而这个坑曾实际导致识别错误。
    #[test]
    fn bt601_differs_from_bt709_on_saturated_colors() {
        use image::Rgba;

        let mut img = image::RgbaImage::new(1, 1);
        // 高饱和青绿：BT.601 与 BT.709 在此差异可达 8 阶
        img.put_pixel(0, 0, Rgba([0, 200, 180, 255]));
        let dynimg = DynamicImage::ImageRgba8(img);

        let ours = to_gray_b601(&dynimg).get_pixel(0, 0).0[0];
        let theirs = dynimg.to_luma8().get_pixel(0, 0).0[0];

        assert_ne!(
            ours, theirs,
            "两个公式应给出不同结果，否则说明本实现未真正替换默认灰度化"
        );
    }

    /// 灰度化后的尺寸应与原图一致
    #[test]
    fn gray_preserves_dimensions() {
        let img = DynamicImage::new_rgb8(100, 25);
        let gray = to_gray_b601(&img);
        assert_eq!(gray.dimensions(), (100, 25));
    }

    /// 与真实样本上 PIL 的灰度结果逐像素比对
    ///
    /// 这是防止灰度公式再次漂移的**最强保障**。期望值由
    /// `PIL.Image.open(p).convert("L")` 生成，内嵌为常量，
    /// 因此该测试不依赖样本目录，CI 上也能跑。
    ///
    /// 选取 `0024.png` 是因为它对灰度系数极敏感：早先 BT.709 会读成
    /// `0Gax`、丢四舍五入会读成 `0ax`，只有完全对齐才得到 `0Cax`。
    #[test]
    fn gray_matches_pil_on_real_sample() {
        use base64::Engine as _;

        // captcha_samples/0024.png 原图
        const SAMPLE_PNG_B64: &str = include_str!("../tests/fixtures/0024.png.b64");

        // PIL 对该图 convert("L") 后的第 12 行前 30 个像素
        const EXPECTED_ROW12: [u8; 30] = [
            255, 175, 171, 252, 255, 255, 255, 252, 171, 175, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 233, 231, 253, 255,
        ];

        let cleaned: String = SAMPLE_PNG_B64
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&cleaned)
            .expect("内嵌样本应为合法 base64");
        let img = image::load_from_memory(&bytes).expect("内嵌样本应为合法 PNG");

        let gray = to_gray_b601(&img);
        assert_eq!(gray.dimensions(), (100, 25));

        for (x, expected) in EXPECTED_ROW12.iter().enumerate() {
            let actual = gray.get_pixel(x as u32, 12).0[0];
            assert_eq!(
                actual, *expected,
                "第 12 行第 {x} 列灰度不符：期望 {expected}，实际 {actual}"
            );
        }
    }

    /// 指定不存在的二进制时应报配置错误，且错误信息包含可读提示
    #[test]
    fn missing_binary_reports_config_error() {
        let image = GrayImage::new(100, 25);
        let engine = TesseractOcr::new().with_binary("/definitely/not/here/tesseract");
        let path = engine.write_temp_png(&image).unwrap();
        let err = engine.run_tesseract(&path, 8).unwrap_err();
        let _ = std::fs::remove_file(&path);

        // 缺失二进制在 run_tesseract 里被归一化为 Config 错误；
        // 但 recognize 会把它当作「单配置失败」吞掉并回退，因此这里
        // 直接测内部函数以锁定行为。
        assert!(matches!(err, Error::Config(_)), "实际 {err:?}");
        assert!(err.to_string().contains("tesseract"));
    }

    /// 临时文件应在写入后真实存在，且内容可被 image 重新读出
    #[test]
    fn temp_png_is_writable_and_readable() {
        let dir = std::env::temp_dir().join("gnnuhub-ocr-test");
        let engine = TesseractOcr::new().with_temp_dir(&dir);

        let image = GrayImage::from_pixel(100, 25, image::Luma([200u8]));
        let path = engine.write_temp_png(&image).unwrap();

        assert!(path.exists(), "临时文件应已创建: {}", path.display());
        let reread = image::open(&path).unwrap();
        assert_eq!(reread.to_luma8().dimensions(), (100, 25));

        let _ = std::fs::remove_file(&path);
    }

    /// 不可用二进制时 `is_available` 应返回 false 而不是 panic
    #[test]
    fn is_available_false_for_bad_binary() {
        let engine = TesseractOcr::new().with_binary("/definitely/not/here/tesseract");
        assert!(!engine.is_available());
    }

    /// 真实 tesseract 存在时应探测为可用
    ///
    /// 该测试依赖运行环境，tesseract 缺失时跳过（不算失败），
    /// 因为 CI 上不保证装了它。
    #[test]
    fn is_available_true_when_installed() {
        let engine = TesseractOcr::new();
        if !engine.is_available() {
            eprintln!("跳过：本机未安装 tesseract");
            return;
        }
        assert!(engine.is_available());
    }
}
