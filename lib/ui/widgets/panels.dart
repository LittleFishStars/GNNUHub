/// 通用小组件：加载/错误/空状态视图
library;

import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

/// 页面内错误提示（含重试按钮；[retrySafe] 为 false 时隐藏）
class ErrorPanel extends StatelessWidget {
  const ErrorPanel({
    super.key,
    required this.message,
    this.onRetry,
    this.retrySafe = true,
  });

  final String message;
  final VoidCallback? onRetry;
  final bool retrySafe;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(LucideIcons.circleAlert, size: 36, color: scheme.error),
            const SizedBox(height: 12),
            Text(
              message,
              textAlign: TextAlign.center,
              style: TextStyle(color: scheme.onSurfaceVariant, fontSize: 14),
            ),
            if (onRetry != null && retrySafe) ...[
              const SizedBox(height: 16),
              FilledButton.icon(
                onPressed: onRetry,
                icon: const Icon(LucideIcons.rotateCw, size: 16),
                label: const Text('重试'),
              ),
            ],
          ],
        ),
      ),
    );
  }
}

/// 页面内加载指示
class LoadingPanel extends StatelessWidget {
  const LoadingPanel({super.key, this.label = '加载中…'});

  final String label;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          const CircularProgressIndicator(strokeWidth: 2.4),
          const SizedBox(height: 14),
          Text(
            label,
            style: TextStyle(color: scheme.onSurfaceVariant, fontSize: 13),
          ),
        ],
      ),
    );
  }
}

/// 空状态
class EmptyPanel extends StatelessWidget {
  const EmptyPanel({super.key, this.icon, this.label = '暂无数据'});

  final IconData? icon;
  final String label;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(
            icon ?? LucideIcons.inbox,
            size: 36,
            color: scheme.onSurfaceVariant.withValues(alpha: 0.5),
          ),
          const SizedBox(height: 12),
          Text(
            label,
            style: TextStyle(color: scheme.onSurfaceVariant, fontSize: 13),
          ),
        ],
      ),
    );
  }
}

/// HubError → 面向用户的文案（未知错误兜底）
String errorText(Object error) => switch (error) {
      final String s => s,
      _ => error.toString(),
    };

/// 异步任务包装：捕获错误转 SnackBar（失败时调用方自行刷新状态）
Future<void> runCatched(
  BuildContext context,
  Future<void> Function() task,
) async {
  final messenger = ScaffoldMessenger.of(context);
  try {
    await task();
  } catch (e) {
    messenger.showSnackBar(
      SnackBar(content: Text(errorText(e))),
    );
  }
}
