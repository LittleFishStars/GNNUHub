/// GNNUHub —— 赣南师范大学教务系统客户端（Flutter 版）
///
/// 应用入口：Provider 注入 HubService，登录态驱动 登录页 / 主页 切换。
library;

import 'dart:io' show Platform;

import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:window_manager/window_manager.dart';

import 'service/hub_service.dart';
import 'ui/home_page.dart';
import 'ui/login_page.dart';
import 'ui/theme.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  // Linux 壳已去掉 GTK 装饰，这里同步 nativeapi 的标题栏策略并设置
  // 无边框窗口的边缘缩放手柄（由 window_manager 在 builder 中注入）。
  if (!kIsWeb && Platform.isLinux) {
    try {
      WindowManager.instance.getCurrent()?.titleBarStyle = TitleBarStyle.hidden;
    } catch (_) {
      // 纯 Dart VM（测试）或无原生库时忽略
    }
  }
  runApp(const GnnuHubApp());
}

class GnnuHubApp extends StatelessWidget {
  const GnnuHubApp({super.key});

  @override
  Widget build(BuildContext context) {
    return ChangeNotifierProvider(
      create: (_) => HubService(),
      child: MaterialApp(
        title: 'GNNUHub',
        debugShowCheckedModeBanner: false,
        theme: buildTheme(Brightness.light),
        darkTheme: buildTheme(Brightness.dark),
        builder: (context, child) {
          // 无装饰窗口需要原生边缘拖拽缩放（不改变视觉，不加边框）
          if (!kIsWeb && Platform.isLinux) {
            return DragToResizeArea(child: child ?? const SizedBox.shrink());
          }
          return child ?? const SizedBox.shrink();
        },
        home: const _AuthGate(),
      ),
    );
  }
}

/// 登录态门卫：未登录 → 登录页；已登录 → 主页
class _AuthGate extends StatelessWidget {
  const _AuthGate();

  @override
  Widget build(BuildContext context) {
    final service = context.watch<HubService>();
    return AnimatedSwitcher(
      duration: const Duration(milliseconds: 240),
      child: service.isLoggedIn ? const HomePage(key: ValueKey('home')) : const LoginPage(key: ValueKey('login')),
    );
  }
}
