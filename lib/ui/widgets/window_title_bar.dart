/// 桌面窗口操作区（仅 Linux 生效）
///
/// Linux 壳已用 `gtk_window_set_decorated(FALSE)` 去掉系统标题栏。
/// 这里只保留一条 32px 的透明拖动区 + 右上角图标按钮，不显示标题文字、
/// 不画色块与边框，视觉上不再是一条标题栏（FlClash 风格）。
/// 非 Linux 平台返回空组件，不影响 Android / Web / 其它桌面。
library;

import 'dart:io';

import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:window_manager/window_manager.dart';

/// 是否需要自绘标题栏
///
/// 只有 Linux 壳改成了 `gtk_window_set_decorated(FALSE)`；
/// Windows / macOS / Android 保留各自的系统标题栏，避免双重装饰。
bool get needsCustomTitleBar => !kIsWeb && Platform.isLinux;

/// 桌面窗口控制条
class WindowTitleBar extends StatefulWidget {
  const WindowTitleBar({super.key, this.leading = const [], this.trailing = const []});

  /// 左侧附加操作（例如退出登录）
  final List<Widget> leading;

  /// 右侧附加操作（放在窗口按钮之前）
  final List<Widget> trailing;

  @override
  State<WindowTitleBar> createState() => _WindowTitleBarState();
}

class _WindowTitleBarState extends State<WindowTitleBar> {
  bool _maximized = false;
  ListenerId? _listenerId;

  @override
  void initState() {
    super.initState();
    if (!needsCustomTitleBar) return;
    _readState();
    // 监听最大化状态以切换图标（tryNative 在 widget test 的纯 Dart VM 下安全降级）
    _listenerId = _tryNative(() => WindowManager.instance.addListener((event) {
          switch (event) {
            case WindowMaximizedEvent():
            case WindowRestoredEvent():
            case WindowResizedEvent():
              _readState();
            default:
              break;
          }
        }));
  }

  @override
  void dispose() {
    final id = _listenerId;
    if (id != null) {
      _tryNative(() => WindowManager.instance.removeListener(id));
    }
    super.dispose();
  }

  void _readState() {
    final maximized = _tryNative<bool>(() => _window?.isMaximized ?? false) ?? false;
    if (maximized != _maximized && mounted) {
      setState(() => _maximized = maximized);
    }
  }

  Window? get _window =>
      _tryNative<Window>(() => WindowManager.instance.getCurrent()!);

  @override
  Widget build(BuildContext context) {
    if (!needsCustomTitleBar) return const SizedBox.shrink();

    final scheme = Theme.of(context).colorScheme;
    final window = _window;

    Widget button({
      required IconData icon,
      required VoidCallback onPressed,
      required String tooltip,
      Color? hoverColor,
    }) {
      return Tooltip(
        message: tooltip,
        child: IconButton(
          onPressed: onPressed,
          icon: Icon(icon, size: 15),
          visualDensity: VisualDensity.compact,
          hoverColor: hoverColor,
          style: IconButton.styleFrom(
            minimumSize: const Size(38, 32),
            maximumSize: const Size(38, 32),
            tapTargetSize: MaterialTapTargetSize.shrinkWrap,
            shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(8),
            ),
          ),
        ),
      );
    }

    // DragToMoveArea 内部用 GestureDetector 处理拖动与双击；
    // 整条透明、无文字、无边框，只有角落的图标按钮可点。
    return DragToMoveArea(
      child: SizedBox(
        height: 32,
        child: Row(
          children: [
            const SizedBox(width: 6),
            ...widget.leading,
            const Spacer(),
            ...widget.trailing,
            button(
              icon: LucideIcons.minus,
              tooltip: '最小化',
              onPressed: () => _tryNative(() => window?.minimize()),
            ),
            button(
              icon: _maximized ? LucideIcons.minimize2 : LucideIcons.maximize2,
              tooltip: _maximized ? '还原' : '最大化',
              onPressed: () {
                _tryNative(() {
                  if (_maximized) {
                    window?.unmaximize();
                  } else {
                    window?.maximize();
                  }
                });
                _readState();
              },
            ),
            button(
              icon: LucideIcons.x,
              tooltip: '关闭',
              hoverColor: scheme.errorContainer,
              onPressed: () => _tryNative(() => window?.close()),
            ),
            const SizedBox(width: 4),
          ],
        ),
      ),
    );
  }
}

/// 包内没有导出 native_guard，这里做等价的纯 Dart VM 安全降级
T? _tryNative<T>(T Function() body) {
  try {
    return body();
  } on ArgumentError {
    return null;
  } catch (_) {
    return null;
  }
}
