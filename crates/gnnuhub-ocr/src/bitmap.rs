//! 位图查表识别引擎
//!
//! # 原理
//!
//! 实测发现赣南师范大学统一身份认证平台的验证码**字形渲染是确定性的**：
//! 同一个「字符 + 字号」组合，在不同图片、不同颜色下都会产生**逐像素相同**
//! 的二值点阵。证据：一个 `4x14` 位图在 7 张不同图片里复现 8 次，而来源
//! 颜色各不相同（`(162,229,60)`、`(106,118,49)`、`(135,58,188)`……），
//! 灰度化二值化后完全一致。
//!
//! 因此识别可以退化成**查表**：
//!
//! 1. 灰度化（BT.601，与 PIL 一致）并二值化；
//! 2. 连通域提取，得到 4 个字形（`i`/`j` 的点合并回主干）；
//! 3. 每个字形转成位图键，在字库中精确查找；
//! 4. 查不到时用汉明距离做模糊匹配，距离超过阈值则放弃。
//!
//! 这个方案的准确率上限**只取决于字库覆盖度**，与分类器能力无关。
//! 60 张样本（240 个字形）已得到 65 个唯一位图，Chao1 估计总体约 67 个，
//! 采样覆盖度 96.7%。
//!
//! # 与 tesseract 的关系
//!
//! tesseract 是通用 OCR，对这类抗锯齿彩色验证码只能到 78%，且对放大倍数
//! 敏感（`0024.png` 只在 scale 4~8 窗口内正确）。位图查表在字库覆盖到的
//! 范围内接近 100%，因此**优先用本引擎，tesseract 作为兜底**。
//!
//! # 字库格式
//!
//! 字库是 JSON，由 `captcha_samples/bitmap_lib.json` 提供（构建脚本见
//! 项目 `.scratch/`）：
//!
//! ```json
//! {
//!   "entries": [
//!     { "w": 9, "h": 13, "rows": ["0110...", ...], "label": "S" }
//!   ]
//! }
//! ```
//!
//! `rows` 是 `h` 个等长字符串，每个字符是 `'0'` 或 `'1'`。

use std::collections::HashMap;
use std::path::Path;

use base64::Engine as _;
use gnnuhub_core::{Error, Result};
use image::DynamicImage;

use crate::{InteractiveFn, OcrEngine, decode_image, validate_captcha_format};

// 二值化判据：**非纯白即墨迹**
//
// 这里必须与 Python 参考实现（`.scratch/extract4.py`）保持一致：
// 判据是 `(r, g, b) != (255, 255, 255)`，而不是「灰度 < 阈值」。
//
// 原因是字形启用抗锯齿，边缘像素是浅色（实测样本灰度最低 153，
// 大量像素落在 153~254 之间）。若用灰度阈值，这些边缘像素会被判为
// 背景，字形被削断成多个连通域，提取结果严重错误。
// 真实样本的连通域分析显示：非纯白判据下每张图恰好 4 个主干 + 0~2 个点，
// 非常干净。

/// 连通域提取时丢弃的孤立小块的像素上限
///
/// 注意：真实样本里的 `i`/`j` 点是 `2x2`/4 像素，**必须保留并合并回主干**。
/// 因此这里只丢弃 < 4 像素的碎片（实测样本中一个都没有）。
const MIN_BLOB_PIXELS: usize = 4;

/// 判定为 `i`/`j` 的点的小块上限
const DOT_MAX_W: usize = 3;
const DOT_MAX_H: usize = 4;
const DOT_MAX_PIXELS: usize = 8;

/// 字库中的一个字形
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphEntry {
    /// 宽度（像素）
    pub w: u32,
    /// 高度（像素）
    pub h: u32,
    /// `h` 个等长字符串，字符为 `'0'` / `'1'`
    pub rows: Vec<String>,
    /// 该字形对应的字符
    pub label: char,
}

impl GlyphEntry {
    /// 把点阵拍平成一维位串，用于哈希查找
    fn bits(&self) -> String {
        let mut s = String::with_capacity(self.rows.len() * self.w as usize);
        for r in &self.rows {
            s.push_str(r);
        }
        s
    }
}

/// 位图字库
#[derive(Debug, Clone)]
pub struct BitmapLibrary {
    /// 精确查找表：`(w, h, 位串)` -> 字符
    exact: HashMap<(u32, u32, String), char>,
    /// 按 `(w, h)` 分组的条目，用于模糊匹配
    by_size: HashMap<(u32, u32), Vec<GlyphEntry>>,
}

impl BitmapLibrary {
    /// 字库中的字形总数
    pub fn len(&self) -> usize {
        self.exact.len()
    }

    /// 字库是否为空
    pub fn is_empty(&self) -> bool {
        self.exact.is_empty()
    }

    /// 从 JSON 文本解析字库
    pub fn from_json(text: &str) -> Result<Self> {
        let parsed: serde_json::Value = serde_json::from_str(text)
            .map_err(|e| Error::Image(format!("字库 JSON 解析失败: {e}")))?;
        let entries = parsed
            .get("entries")
            .and_then(|v| v.as_array())
            .ok_or_else(|| Error::Image("字库缺少 entries 数组".to_string()))?;

        let mut lib = Self {
            exact: HashMap::new(),
            by_size: HashMap::new(),
        };
        for (i, e) in entries.iter().enumerate() {
            let w = e
                .get("w")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| Error::Image(format!("字库第 {i} 条缺少 w")))?
                as u32;
            let h = e
                .get("h")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| Error::Image(format!("字库第 {i} 条缺少 h")))?
                as u32;
            let label = e
                .get("label")
                .and_then(|v| v.as_str())
                .and_then(|s| s.chars().next())
                .ok_or_else(|| Error::Image(format!("字库第 {i} 条缺少 label")))?;
            let rows: Vec<String> = e
                .get("rows")
                .and_then(|v| v.as_array())
                .ok_or_else(|| Error::Image(format!("字库第 {i} 条缺少 rows")))?
                .iter()
                .map(|r| r.as_str().unwrap_or_default().to_string())
                .collect();

            if rows.len() != h as usize {
                return Err(Error::Image(format!(
                    "字库第 {i} 条 rows 行数 {} 与 h {h} 不符",
                    rows.len()
                )));
            }
            if rows.iter().any(|r| r.chars().count() != w as usize) {
                return Err(Error::Image(format!("字库第 {i} 条存在长度不为 {w} 的行")));
            }

            let entry = GlyphEntry { w, h, rows, label };
            lib.exact.insert((w, h, entry.bits()), label);
            lib.by_size.entry((w, h)).or_default().push(entry);
        }
        Ok(lib)
    }

    /// 从文件加载字库
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let text = std::fs::read_to_string(path.as_ref())
            .map_err(|e| Error::Image(format!("读取字库 {} 失败: {e}", path.as_ref().display())))?;
        Self::from_json(&text)
    }

    /// 精确查找
    pub fn lookup_exact(&self, w: u32, h: u32, bits: &str) -> Option<char> {
        self.exact.get(&(w, h, bits.to_string())).copied()
    }

    /// 模糊查找：在相同尺寸的条目中找汉明距离最小者
    ///
    /// 返回 `(字符, 汉明距离)`。只有距离不超过 `max_distance` 才认为可用。
    pub fn lookup_fuzzy(
        &self,
        w: u32,
        h: u32,
        bits: &str,
        max_distance: usize,
    ) -> Option<(char, usize)> {
        let cands = self.by_size.get(&(w, h))?;
        let mut best: Option<(char, usize)> = None;
        for c in cands {
            let cb = c.bits();
            if cb.len() != bits.len() {
                continue;
            }
            let d = cb.bytes().zip(bits.bytes()).filter(|(a, b)| a != b).count();
            if d > max_distance {
                continue;
            }
            match best {
                Some((_, bd)) if bd <= d => {}
                _ => best = Some((c.label, d)),
            }
        }
        best
    }
}

/// 提取出的字形（包围盒 + 位串）
#[derive(Debug, Clone)]
pub struct Glyph {
    /// 宽度（放大后的像素数）
    pub w: u32,
    /// 高度（放大后的像素数）
    pub h: u32,
    /// 按行拍平的位串，`'0'`/`'1'`，长度 `w * h`
    pub bits: String,
}

/// 位图查表识别器
///
/// 使用前需要提供字库。字库来源可以是文件，也可以直接内嵌。
#[derive(Debug, Clone)]
pub struct BitmapOcr {
    lib: BitmapLibrary,
    /// 模糊匹配允许的最大汉明距离；0 表示只做精确匹配
    fuzzy_distance: usize,
}

impl BitmapOcr {
    /// 用给定字库构造识别器
    pub fn new(lib: BitmapLibrary) -> Self {
        Self {
            lib,
            fuzzy_distance: 2,
        }
    }

    /// 从文件加载字库并构造识别器
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self::new(BitmapLibrary::from_path(path)?))
    }

    /// 用随 crate 内嵌的字库构造识别器（推荐）
    ///
    /// 内嵌字库由 60 张真实样本构建，无需任何外部文件。
    pub fn embedded() -> Result<Self> {
        Ok(Self::new(embedded_library()?))
    }

    /// 设置模糊匹配的最大汉明距离
    pub fn with_fuzzy_distance(mut self, d: usize) -> Self {
        self.fuzzy_distance = d;
        self
    }

    /// 字库引用
    pub fn library(&self) -> &BitmapLibrary {
        &self.lib
    }

    /// 对单张图片做识别，返回 4 个字符
    ///
    /// 与 [`OcrEngine::recognize`] 的区别是这里不做人工回退，失败就返回
    /// 具体原因，便于诊断。
    pub fn recognize_image(&self, image_base64: &str) -> Result<String> {
        let img = decode_image(image_base64)?;
        let glyphs = extract_glyphs(&img)?;
        if glyphs.len() != 4 {
            return Err(Error::Image(format!(
                "期望提取到 4 个字形，实际得到 {} 个",
                glyphs.len()
            )));
        }
        let mut out = String::with_capacity(4);
        for g in &glyphs {
            let ch = self
                .lib
                .lookup_exact(g.w, g.h, &g.bits)
                .or_else(|| {
                    self.lib
                        .lookup_fuzzy(g.w, g.h, &g.bits, self.fuzzy_distance)
                        .map(|(c, _)| c)
                })
                .ok_or_else(|| {
                    Error::Image(format!(
                        "字形 {}x{} 不在字库中（位串 {}）",
                        g.w,
                        g.h,
                        &g.bits[..g.bits.len().min(32)]
                    ))
                })?;
            out.push(ch);
        }
        Ok(out)
    }
}

impl OcrEngine for BitmapOcr {
    fn recognize(&self, image_base64: &str, interactive: Option<&InteractiveFn>) -> Result<String> {
        match self.recognize_image(image_base64) {
            Ok(code) => {
                validate_captcha_format(&code)?;
                Ok(code)
            }
            Err(e) => {
                tracing::debug!("位图查表识别失败（{e}），尝试人工输入");
                crate::ManualOcr::new().recognize(image_base64, interactive)
            }
        }
    }

    fn name(&self) -> &'static str {
        "bitmap"
    }
}

/// 提取字形：二值化 -> 连通域 -> 合并 i/j 的点 -> 按 x 排序
fn extract_glyphs(image: &DynamicImage) -> Result<Vec<Glyph>> {
    let rgba = image.to_rgba8();
    let (w, h) = rgba.dimensions();
    let (w, h) = (w as usize, h as usize);

    // 二值化：非纯白即墨迹（与 Python 参考实现一致）
    let mut ink = vec![false; w * h];
    for (i, p) in rgba.pixels().enumerate() {
        let [r, g, b, _a] = p.0;
        ink[i] = !(r == 255 && g == 255 && b == 255);
    }

    // 连通域（8 邻域）
    let mut seen = vec![false; w * h];
    let mut blobs: Vec<Vec<(usize, usize)>> = Vec::new();
    for sy in 0..h {
        for sx in 0..w {
            let idx = sy * w + sx;
            if !ink[idx] || seen[idx] {
                continue;
            }
            let mut stack = vec![(sx, sy)];
            seen[idx] = true;
            let mut cells = Vec::new();
            while let Some((cx, cy)) = stack.pop() {
                cells.push((cx, cy));
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        let nx = cx as i32 + dx;
                        let ny = cy as i32 + dy;
                        if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                            continue;
                        }
                        let ni = ny as usize * w + nx as usize;
                        if ink[ni] && !seen[ni] {
                            seen[ni] = true;
                            stack.push((nx as usize, ny as usize));
                        }
                    }
                }
            }
            if cells.len() >= MIN_BLOB_PIXELS {
                blobs.push(cells);
            }
        }
    }

    // 拆分主干与 i/j 的点
    let is_dot = |b: &Vec<(usize, usize)>| -> bool {
        let (x0, y0, x1, y1) = bbox(b);
        let bw = x1 - x0 + 1;
        let bh = y1 - y0 + 1;
        bw <= DOT_MAX_W && bh <= DOT_MAX_H && b.len() <= DOT_MAX_PIXELS
    };
    let dots: Vec<Vec<(usize, usize)>> = blobs.iter().filter(|b| is_dot(b)).cloned().collect();
    let mut mains: Vec<Vec<(usize, usize)>> = blobs.into_iter().filter(|b| !is_dot(b)).collect();

    // 把点合并回正上方且 x 区间重叠的主干
    for d in dots {
        let (dx0, _dy0, dx1, dy1) = bbox(&d);
        let mut best: Option<usize> = None;
        let mut best_gap = usize::MAX;
        for (i, m) in mains.iter().enumerate() {
            let (mx0, my0, mx1, _my1) = bbox(m);
            if mx1 < dx0 || mx0 > dx1 {
                continue; // x 不重叠
            }
            if my0 <= dy1 {
                continue; // 点必须在主干上方
            }
            let gap = my0 - dy1;
            if gap < best_gap {
                best_gap = gap;
                best = Some(i);
            }
        }
        if let Some(i) = best {
            mains[i].extend(d);
        }
    }

    if mains.is_empty() {
        return Err(Error::Image("未提取到任何字形".to_string()));
    }

    mains.sort_by_key(|b| bbox(b).0);

    Ok(mains
        .into_iter()
        .map(|cells| {
            let (x0, y0, x1, y1) = bbox(&cells);
            let gw = (x1 - x0 + 1) as u32;
            let gh = (y1 - y0 + 1) as u32;
            let mut grid = vec!['0'; (gw * gh) as usize];
            for (x, y) in &cells {
                let gx = x - x0;
                let gy = y - y0;
                grid[gy * gw as usize + gx] = '1';
            }
            Glyph {
                w: gw,
                h: gh,
                bits: grid.into_iter().collect(),
            }
        })
        .collect())
}

fn bbox(cells: &[(usize, usize)]) -> (usize, usize, usize, usize) {
    let mut x0 = usize::MAX;
    let mut y0 = usize::MAX;
    let mut x1 = 0;
    let mut y1 = 0;
    for &(x, y) in cells {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    (x0, y0, x1, y1)
}

/// 供离线基准工具使用：提取字形（点阵与坐标）
///
/// 这个函数把内部提取逻辑暴露给 `examples/`，便于用真实样本做比对。
pub fn extract_glyphs_for_bench(image_base64: &str) -> Result<Vec<Glyph>> {
    let img = decode_image(image_base64)?;
    extract_glyphs(&img)
}

/// 随 crate 一起打包的字库 JSON
///
/// 由 60 张真实样本构建（见 `assets/bitmap_lib.json`）：
/// 65 个字形、覆盖 59 个字符，对样本集字形级命中率 100%。
///
/// 字库是「数据」而非代码，把它内嵌进来的好处是 [`BitmapOcr::embedded`]
/// 无需文件系统即可工作，用在移动端/嵌入式场景时尤其方便。
pub const EMBEDDED_LIBRARY_JSON: &str = include_str!("../assets/bitmap_lib.json");

/// 从内嵌字库构造识别器
///
/// 这是最常用的入口：不需要任何外部文件。
///
/// # 示例
///
/// ```no_run
/// use gnnuhub_ocr::{BitmapOcr, OcrEngine};
///
/// let engine = BitmapOcr::embedded().expect("内嵌字库应能解析");
/// assert_eq!(engine.name(), "bitmap");
/// ```
pub fn embedded_library() -> Result<BitmapLibrary> {
    BitmapLibrary::from_json(EMBEDDED_LIBRARY_JSON)
}

/// 便捷函数：把 base64 图片的字节解码出来（供测试使用）
#[allow(dead_code)]
fn decode_b64_for_test(s: &str) -> Result<Vec<u8>> {
    let raw = crate::strip_data_url_prefix(s);
    base64::engine::general_purpose::STANDARD
        .decode(raw)
        .map_err(|e| Error::Image(format!("base64 解码失败: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_lib() -> BitmapLibrary {
        // 构造一个 2x2 的 "L" 形状字库
        let json = r#"{
          "entries": [
            {"w": 2, "h": 2, "rows": ["10", "11"], "label": "L"},
            {"w": 2, "h": 2, "rows": ["01", "11"], "label": "J"}
          ]
        }"#;
        BitmapLibrary::from_json(json).expect("字库应能解析")
    }

    #[test]
    fn parses_library_from_json() {
        let lib = tiny_lib();
        assert_eq!(lib.len(), 2);
        assert!(!lib.is_empty());
    }

    #[test]
    fn exact_lookup_hits() {
        let lib = tiny_lib();
        assert_eq!(lib.lookup_exact(2, 2, "1011"), Some('L'));
        assert_eq!(lib.lookup_exact(2, 2, "0111"), Some('J'));
        assert_eq!(lib.lookup_exact(2, 2, "1111"), None);
    }

    #[test]
    fn exact_lookup_is_size_sensitive() {
        let lib = tiny_lib();
        // 同样的位串但尺寸不同，不应命中
        assert_eq!(lib.lookup_exact(3, 2, "1011"), None);
    }

    #[test]
    fn fuzzy_lookup_respects_max_distance() {
        let lib = tiny_lib();
        // "1111" 距 "1011" 为 1，距 "0111" 也为 1
        let hit = lib.lookup_fuzzy(2, 2, "1111", 1);
        assert!(hit.is_some(), "距离 1 应能命中");
        assert_eq!(hit.unwrap().1, 1);

        // 距离设为 0 则不应命中
        assert_eq!(lib.lookup_fuzzy(2, 2, "1111", 0), None);
    }

    #[test]
    fn fuzzy_lookup_rejects_far_candidates() {
        let lib = tiny_lib();
        // "0000" 距任何候选都比较远
        assert!(lib.lookup_fuzzy(2, 2, "0000", 1).is_none());
    }

    #[test]
    fn rejects_wrong_row_count() {
        let json = r#"{"entries":[{"w":2,"h":3,"rows":["10","11"],"label":"L"}]}"#;
        let err = BitmapLibrary::from_json(json);
        assert!(err.is_err(), "行数与 h 不符应报错");
    }

    #[test]
    fn rejects_wrong_row_width() {
        let json = r#"{"entries":[{"w":3,"h":2,"rows":["10","111"],"label":"L"}]}"#;
        assert!(
            BitmapLibrary::from_json(json).is_err(),
            "行长与 w 不符应报错"
        );
    }

    #[test]
    fn rejects_missing_entries() {
        assert!(BitmapLibrary::from_json("{}").is_err());
        assert!(BitmapLibrary::from_json("not json").is_err());
    }

    #[test]
    fn binarize_treats_any_non_white_pixel_as_ink() {
        // 抗锯齿边缘是浅色（实测样本灰度最低 153），必须算作墨迹，
        // 否则字形会被削断。这里用 (255, 240, 240) 这种极浅的像素验证。
        let mut img = image::RgbImage::from_pixel(100, 25, image::Rgb([255, 255, 255]));
        // 4 个用极浅颜色画的方块，每个 4x4
        for x0 in [5u32, 25, 45, 65] {
            for y in 10..14 {
                for x in x0..(x0 + 4) {
                    img.put_pixel(x, y, image::Rgb([255, 240, 240]));
                }
            }
        }
        let glyphs = extract_glyphs(&DynamicImage::ImageRgb8(img)).expect("应能提取");
        assert_eq!(glyphs.len(), 4, "极浅像素也应被识别为字形");
    }

    #[test]
    fn binarize_drops_exactly_white_background() {
        // 纯白背景不应产生任何字形
        let img = image::RgbImage::from_pixel(100, 25, image::Rgb([255, 255, 255]));
        assert!(extract_glyphs(&DynamicImage::ImageRgb8(img)).is_err());
    }

    #[test]
    fn extract_finds_four_glyphs_on_synthetic_image() {
        // 造一张 100x25 的图，放 4 个分离的方块
        let mut img = image::RgbImage::from_pixel(100, 25, image::Rgb([255, 255, 255]));
        for (i, x0) in [5u32, 25, 45, 65].iter().enumerate() {
            let color = image::Rgb([(i as u8) * 40, 0, 0]);
            for y in 8..20 {
                for x in *x0..(*x0 + 8) {
                    img.put_pixel(x, y, color);
                }
            }
        }
        let glyphs = extract_glyphs(&DynamicImage::ImageRgb8(img)).expect("应能提取");
        assert_eq!(glyphs.len(), 4, "应提取到 4 个字形");
        // 不做放大，原图 8px 宽的方块提取后仍是 8px
        assert!(
            glyphs.iter().all(|g| g.w == 8),
            "宽度应为 8，实际 {:?}",
            glyphs.iter().map(|g| g.w).collect::<Vec<_>>()
        );
    }

    #[test]
    fn recognize_image_reports_missing_glyph() {
        let mut img = image::RgbImage::from_pixel(100, 25, image::Rgb([255, 255, 255]));
        for x0 in [5u32, 25, 45, 65] {
            for y in 8..20 {
                for x in x0..(x0 + 8) {
                    img.put_pixel(x, y, image::Rgb([0, 0, 0]));
                }
            }
        }
        let engine = BitmapOcr::new(tiny_lib());
        let err = engine
            .recognize_image(&encode_png(&DynamicImage::ImageRgb8(img)))
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("不在字库中"), "应提示字形缺失，实际: {msg}");
    }

    fn encode_png(img: &DynamicImage) -> String {
        use std::io::Cursor;
        let mut buf = Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png)
            .expect("编码 PNG");
        format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(buf.into_inner())
        )
    }

    #[test]
    fn engine_name_is_stable() {
        assert_eq!(BitmapOcr::new(tiny_lib()).name(), "bitmap");
    }

    #[test]
    fn full_pipeline_recovers_known_glyphs() {
        // 造图 -> 提取 -> 加入字库 -> 识别，应得到 "LJLJ"
        let mut img = image::RgbImage::from_pixel(100, 25, image::Rgb([255, 255, 255]));
        // 两个 "L" 形状（左竖 + 底横）与两个 "J" 形状（右竖 + 底横）
        let shapes: [(u32, bool); 4] = [(5, false), (25, true), (45, false), (65, true)];
        for (x0, is_j) in shapes {
            for y in 8..20 {
                let x = if is_j { x0 + 6 } else { x0 };
                img.put_pixel(x, y, image::Rgb([0, 0, 0]));
                if y == 19 {
                    for x in x0..(x0 + 7) {
                        img.put_pixel(x, y, image::Rgb([0, 0, 0]));
                    }
                }
            }
        }
        let dynimg = DynamicImage::ImageRgb8(img);
        let glyphs = extract_glyphs(&dynimg).expect("应能提取");
        assert_eq!(glyphs.len(), 4);

        // 用提取结果构造字库
        let mut entries = String::new();
        for (g, label) in glyphs.iter().zip(["L", "J", "L", "J"]) {
            if !entries.is_empty() {
                entries.push(',');
            }
            let rows: Vec<String> = (0..g.h)
                .map(|r| {
                    g.bits[r as usize * g.w as usize..(r as usize + 1) * g.w as usize].to_string()
                })
                .collect();
            entries.push_str(&format!(
                "{{\"w\":{},\"h\":{},\"rows\":{:?},\"label\":\"{}\"}}",
                g.w, g.h, rows, label
            ));
        }
        let json = format!("{{\"entries\":[{entries}]}}");

        let engine = BitmapOcr::new(BitmapLibrary::from_json(&json).expect("字库"));
        let code = engine
            .recognize_image(&encode_png(&dynimg))
            .expect("应识别成功");
        assert_eq!(code, "LJLJ");
    }
}
