/// 位图查表验证码识别 —— 逐行复刻 Rust `logic/ocr/src/bitmap.rs`
///
/// 三大「反直觉」事实（文档注释与实现不一致，以代码为准）：
/// 1. **没有灰度化**：判据是 `(r,g,b) != (255,255,255)`，抗锯齿浅色边缘
///    （灰度最低 153）都算墨迹；
/// 2. **没有缩放归一化**：只是包围盒裁剪到原点，字库条目尺寸本身不统一；
/// 3. **没有平移容差搜索**：平移不变性由包围盒裁剪隐式实现，
///    1~3 像素抖动由汉明距离 ≤2 被动吸收。
library;

import 'dart:convert';
import 'dart:typed_data';

import 'package:image/image.dart' as img;

import '../core/error.dart';
import 'library.dart';

/// 验证码长度（固定 4 位）
const int captchaLength = 4;

/// 验证码字符集（大小写敏感）
const String captchaAlphabet =
    '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz';

/// 连通域最少像素数
const int minBlobPixels = 4;

/// i/j 点判定阈值（三者同时满足）
const int dotMaxW = 3;
const int dotMaxH = 4;
const int dotMaxPixels = 8;

/// 验证码识别器接口
abstract interface class OcrEngine {
  String recognize(String imageBase64);
  String get name;
}

/// 位图查表识别器
class BitmapOcr implements OcrEngine {
  BitmapOcr(this.library, {this.fuzzyDistance = 2});

  final BitmapLibrary library;

  /// 模糊匹配允许的最大汉明距离（0 = 仅精确匹配）
  final int fuzzyDistance;

  @override
  String get name => 'bitmap';

  /// 剥掉 data URL 前缀、剔除全部空白、base64 标准解码
  static List<int> decodeBase64(String input) {
    var payload = input;
    final sep = input.indexOf(';base64,');
    if (sep >= 0) payload = input.substring(sep + ';base64,'.length);
    payload = payload.replaceAll(RegExp(r'\s'), '');
    try {
      return base64.decode(payload);
    } on FormatException catch (e) {
      throw ImageError('base64 解码失败: $e');
    }
  }

  /// 解码 PNG/JPEG → RGBA 像素墨迹矩阵
  ///
  /// 严格判据：三通道同时等于 255 才是背景；alpha 完全忽略，
  /// 不做白底合成、不做灰度化、不做缩放。
  static ({int width, int height, List<bool> ink}) decodeImage(String imageBase64) {
    final bytes = decodeBase64(imageBase64);
    final img.Image decoded;
    try {
      decoded = img.decodeImage(Uint8List.fromList(bytes))!;
    } catch (e) {
      throw ImageError('图片解析失败: $e');
    }
    final w = decoded.width;
    final h = decoded.height;
    final ink = List<bool>.filled(w * h, false);
    for (var y = 0; y < h; y++) {
      for (var x = 0; x < w; x++) {
        final p = decoded.getPixel(x, y);
        // package:image 4.x 的通道是 num（0..255）
        final r = p.r.toInt();
        final g = p.g.toInt();
        final b = p.b.toInt();
        ink[y * w + x] = !(r == 255 && g == 255 && b == 255);
      }
    }
    return (width: w, height: h, ink: ink);
  }

  /// 从图像提取字形（连通域 → 点合并 → 稳定排序 → 栅格化）
  static List<Glyph> extractGlyphs(String imageBase64) {
    final decoded = decodeImage(imageBase64);
    final w = decoded.width;
    final h = decoded.height;
    final ink = decoded.ink;
    final seen = List<bool>.filled(w * h, false);

    // 按行优先扫描发现 8 邻域连通域（栈式 DFS）
    final blobs = <_Blob>[];
    for (var sy = 0; sy < h; sy++) {
      for (var sx = 0; sx < w; sx++) {
        final start = sy * w + sx;
        if (!ink[start] || seen[start]) continue;

        final cells = <int>[];
        final stack = <int>[start];
        seen[start] = true;
        while (stack.isNotEmpty) {
          final i = stack.removeLast();
          cells.add(i);
          final cx = i % w;
          final cy = i ~/ w;
          for (var dy = -1; dy <= 1; dy++) {
            for (var dx = -1; dx <= 1; dx++) {
              final nx = cx + dx;
              final ny = cy + dy;
              if (nx < 0 || ny < 0 || nx >= w || ny >= h) continue;
              final ni = ny * w + nx;
              if (seen[ni] || !ink[ni]) continue;
              seen[ni] = true; // 入栈时标记
              stack.add(ni);
            }
          }
        }
        if (cells.length >= minBlobPixels) {
          blobs.add(_Blob(cells, blobs.length));
        }
      }
    }

    // 分离 i/j 点与主干（保持发现顺序）
    final dots = <_Blob>[];
    final mains = <_Blob>[];
    for (final blob in blobs) {
      final box = blob.bbox(w);
      final bw = box.$3 - box.$1 + 1;
      final bh = box.$4 - box.$2 + 1;
      if (bw <= dotMaxW && bh <= dotMaxH && blob.cells.length <= dotMaxPixels) {
        dots.add(blob);
      } else {
        mains.add(blob);
      }
    }

    // 每个点挂到 x 区间重叠、位于其下方、gap 最小的主干
    for (final dot in dots) {
      final d = dot.bbox(w);
      _Blob? best;
      var bestGap = 1 << 62;
      for (final m in mains) {
        final box = m.bbox(w); // 每次重新计算（主干可能已合并前一个点）
        if (box.$3 < d.$1 || box.$1 > d.$3) continue; // x 区间不重叠
        if (box.$2 <= d.$4) continue; // 主干必须在点下方（my0 > dy1）
        final gap = box.$2 - d.$4;
        if (gap < bestGap) {
          bestGap = gap;
          best = m;
        }
      }
      if (best != null) {
        best.cells.addAll(dot.cells);
      }
      // 没匹配到主干的点静默丢弃
    }

    if (mains.isEmpty) throw const ImageError('未提取到任何字形');

    // 稳定排序：按 x0 升序，平局保持发现顺序（Dart sort 不稳定）
    final order = mains
        .map((b) => (x0: b.bbox(w).$1, index: b.discoveryIndex, blob: b))
        .toList()
      ..sort((a, b) {
        final c = a.x0.compareTo(b.x0);
        return c != 0 ? c : a.index.compareTo(b.index);
      });

    return [for (final o in order) _rasterize(o.blob, w)];
  }

  /// 包围盒裁剪到原点并栅格化为 0/1 位串
  static Glyph _rasterize(_Blob blob, int width) {
    final box = blob.bbox(width);
    final x0 = box.$1, y0 = box.$2, x1 = box.$3, y1 = box.$4;
    final gw = x1 - x0 + 1;
    final gh = y1 - y0 + 1;
    final grid = List<int>.filled(gw * gh, 0x30); // '0'
    for (final cell in blob.cells) {
      final x = cell % width;
      final y = cell ~/ width;
      grid[(y - y0) * gw + (x - x0)] = 0x31; // '1'
    }
    return Glyph(w: gw, h: gh, bits: String.fromCharCodes(grid));
  }

  @override
  String recognize(String imageBase64) {
    final glyphs = extractGlyphs(imageBase64);
    if (glyphs.length != captchaLength) {
      throw ImageError(
          '期望提取到 $captchaLength 个字形，实际得到 ${glyphs.length} 个');
    }
    final out = StringBuffer();
    for (final glyph in glyphs) {
      final ch = library.lookup(glyph.w, glyph.h, glyph.bits, fuzzyDistance);
      if (ch == null) {
        // 错误信息直接给出可粘贴进字库 JSON 的 rows，便于「补库」运维
        final rows = <String>[];
        for (var i = 0; i < glyph.bits.length; i += glyph.w) {
          rows.add(glyph.bits.substring(i, i + glyph.w));
        }
        final block = rows.map((r) => '      "$r",').join('\n');
        throw ImageError(
            '字形 ${glyph.w}x${glyph.h} 不在字库中，未命中位串'
            '（可直接补进 bitmap_lib.json 的 rows）：\n$block');
      }
      out.write(ch);
    }
    final result = out.toString();
    if (result.length != captchaLength ||
        result.split('').any((c) => !captchaAlphabet.contains(c))) {
      throw const InvalidCaptchaError();
    }
    return result;
  }
}

/// 内部：一个连通域（可变的 cells 用于点合并）
class _Blob {
  _Blob(this.cells, this.discoveryIndex);
  final List<int> cells;
  final int discoveryIndex;

  /// (x0, y0, x1, y1)
  (int, int, int, int) bbox(int width) {
    var x0 = 1 << 62, y0 = 1 << 62, x1 = 0, y1 = 0;
    for (final cell in cells) {
      final x = cell % width;
      final y = cell ~/ width;
      if (x < x0) x0 = x;
      if (y < y0) y0 = y;
      if (x > x1) x1 = x;
      if (y > y1) y1 = y;
    }
    return (x0, y0, x1, y1);
  }
}
