/// 应用主题 —— 填充色阶画层级、无边框、颜色只给状态、
/// 阴影只给浮层、大圆角（参照 FlClash 界面体系）
library;

import 'package:flutter/material.dart';

/// 品牌主色（延续 Tauri 版的 #1c549e）
const Color brandPrimary = Color(0xFF1C549E);
const Color brandPrimaryDark = Color(0xFF14417C);

/// 亮色方案
final ColorScheme lightScheme = ColorScheme.fromSeed(
  seedColor: brandPrimary,
  brightness: Brightness.light,
);

/// 暗色方案
final ColorScheme darkScheme = ColorScheme.fromSeed(
  seedColor: brandPrimary,
  brightness: Brightness.dark,
);

/// 主题配置
ThemeData buildTheme(Brightness brightness) {
  final scheme = brightness == Brightness.light ? lightScheme : darkScheme;
  final isLight = brightness == Brightness.light;

  return ThemeData(
    useMaterial3: true,
    colorScheme: scheme,
    scaffoldBackgroundColor: scheme.surfaceContainerLowest,
    fontFamilyFallback: const ['Noto Sans CJK SC', 'PingFang SC', 'Microsoft YaHei'],

    // 无边框输入框：填充色阶画层级
    inputDecorationTheme: InputDecorationTheme(
      filled: true,
      fillColor: isLight
          ? const Color(0xFFF1F4F8)
          : scheme.surfaceContainerHigh,
      border: OutlineInputBorder(
        borderRadius: BorderRadius.circular(14),
        borderSide: BorderSide.none,
      ),
      enabledBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(14),
        borderSide: BorderSide.none,
      ),
      focusedBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(14),
        borderSide: BorderSide(color: scheme.primary, width: 1.6),
      ),
      errorBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(14),
        borderSide: BorderSide.none,
      ),
      focusedErrorBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(14),
        borderSide: BorderSide(color: scheme.error, width: 1.6),
      ),
      contentPadding:
          const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
    ),

    filledButtonTheme: FilledButtonThemeData(
      style: FilledButton.styleFrom(
        backgroundColor: scheme.primary,
        foregroundColor: scheme.onPrimary,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(14),
        ),
        padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 14),
        textStyle: const TextStyle(fontSize: 15, fontWeight: FontWeight.w600),
      ),
    ),

    outlinedButtonTheme: OutlinedButtonThemeData(
      style: OutlinedButton.styleFrom(
        side: BorderSide.none,
        backgroundColor: isLight
            ? const Color(0xFFE9EEF5)
            : scheme.surfaceContainerHigh,
        foregroundColor: scheme.onSurface,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(14),
        ),
        padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 14),
      ),
    ),

    textButtonTheme: TextButtonThemeData(
      style: TextButton.styleFrom(
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(12),
        ),
      ),
    ),

    iconButtonTheme: IconButtonThemeData(
      style: IconButton.styleFrom(
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(12),
        ),
      ),
    ),

    // 卡片：填充色阶 + 无阴影（阴影只给浮层）
    cardTheme: CardThemeData(
      elevation: 0,
      color: isLight ? Colors.white : scheme.surfaceContainer,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(18),
      ),
      margin: EdgeInsets.zero,
      clipBehavior: Clip.antiAlias,
    ),

    // 浮层才有阴影
    dialogTheme: DialogThemeData(
      elevation: 8,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(20),
      ),
      backgroundColor: isLight ? Colors.white : scheme.surfaceContainer,
    ),

    snackBarTheme: SnackBarThemeData(
      elevation: 6,
      behavior: SnackBarBehavior.floating,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(14),
      ),
    ),

    appBarTheme: AppBarTheme(
      elevation: 0,
      scrolledUnderElevation: 0,
      backgroundColor: scheme.surfaceContainerLowest,
      centerTitle: false,
      titleTextStyle: TextStyle(
        fontSize: 17,
        fontWeight: FontWeight.w600,
        color: scheme.onSurface,
      ),
    ),

    navigationBarTheme: NavigationBarThemeData(
      elevation: 0,
      backgroundColor: scheme.surfaceContainerLowest,
      indicatorColor: scheme.primaryContainer,
      height: 64,
      labelBehavior: NavigationDestinationLabelBehavior.alwaysShow,
    ),

    navigationRailTheme: NavigationRailThemeData(
      elevation: 0,
      backgroundColor: scheme.surfaceContainerLowest,
      indicatorColor: scheme.primaryContainer,
      labelType: NavigationRailLabelType.all,
    ),

    menuTheme: MenuThemeData(
      style: MenuStyle(
        elevation: WidgetStateProperty.all(8),
        shape: WidgetStateProperty.all(
          RoundedRectangleBorder(borderRadius: BorderRadius.circular(14)),
        ),
      ),
    ),

    chipTheme: ChipThemeData(
      elevation: 0,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(10),
      ),
    ),

    dividerTheme: DividerThemeData(
      color: isLight ? const Color(0xFFE5EAF1) : scheme.surfaceContainerHigh,
      thickness: 1,
      space: 1,
    ),

    listTileTheme: ListTileThemeData(
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(14),
      ),
    ),

    progressIndicatorTheme: ProgressIndicatorThemeData(
      color: scheme.primary,
      linearTrackColor:
          isLight ? const Color(0xFFE9EEF5) : scheme.surfaceContainerHigh,
    ),
  );
}
