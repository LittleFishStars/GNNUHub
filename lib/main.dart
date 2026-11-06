/// GNNUHub —— 赣南师范大学教务系统客户端（Flutter 版）
///
/// 应用入口：Provider 注入 HubService，登录态驱动 登录页 / 主页 切换。
library;

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import 'service/hub_service.dart';
import 'ui/home_page.dart';
import 'ui/login_page.dart';
import 'ui/theme.dart';

void main() {
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
