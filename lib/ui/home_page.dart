/// 主页：底部导航（窄屏）/ 侧边导航（宽屏）+ 六个功能页
library;

import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:provider/provider.dart';

import '../service/hub_service.dart';
import 'academic_page.dart';
import 'exam_page.dart';
import 'notice_page.dart';

import 'retake_page.dart';
import 'room_page.dart';
import 'schedule_page.dart';
import 'widgets/panels.dart';

class HomePage extends StatefulWidget {
  const HomePage({super.key});

  @override
  State<HomePage> createState() => _HomePageState();
}

class _HomePageState extends State<HomePage> {
  int _index = 0;

  static const _pages = [
    SchedulePage(),
    ExamPage(),
    RetakePage(),
    NoticePage(),
    AcademicPage(),
    RoomPage(),
    ProfilePage(),
  ];

  static const _items = [
    (icon: LucideIcons.calendarDays, label: '课表'),
    (icon: LucideIcons.clipboardList, label: '考试'),
    (icon: LucideIcons.bookCopy, label: '重修'),
    (icon: LucideIcons.megaphone, label: '通知'),
    (icon: LucideIcons.chartColumn, label: '学业'),
    (icon: LucideIcons.doorOpen, label: '教室'),
    (icon: LucideIcons.idCard, label: '学籍'),
  ];

  Future<void> _logout() async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('退出登录'),
        content: const Text('确定要退出当前账号吗？'),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('取消'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('退出'),
          ),
        ],
      ),
    );
    if (confirmed == true && mounted) {
      context.read<HubService>().logout();
    }
  }

  @override
  Widget build(BuildContext context) {
    final service = context.watch<HubService>();
    final width = MediaQuery.sizeOf(context).width;
    final wide = width >= 840;

    return Scaffold(
      appBar: AppBar(
        title: Text(
          'GNNUHub · ${service.loggedInAs ?? ''}',
          style: const TextStyle(fontSize: 16),
        ),
        actions: [
          IconButton(
            tooltip: '退出登录',
            icon: const Icon(LucideIcons.logOut, size: 19),
            onPressed: _logout,
          ),
          const SizedBox(width: 8),
        ],
      ),
      body: Row(
        children: [
          if (wide)
            NavigationRail(
              selectedIndex: _index,
              onDestinationSelected: (i) => setState(() => _index = i),
              destinations: [
                for (final item in _items)
                  NavigationRailDestination(
                    icon: Icon(item.icon, size: 20),
                    selectedIcon: Icon(item.icon, size: 20),
                    label: Text(item.label),
                  ),
              ],
            ),
          if (wide) const VerticalDivider(width: 1),
          Expanded(child: _pages[_index]),
        ],
      ),
      bottomNavigationBar: wide
          ? null
          : NavigationBar(
              selectedIndex: _index,
              onDestinationSelected: (i) => setState(() => _index = i),
              destinations: [
                for (final item in _items)
                  NavigationDestination(
                    icon: Icon(item.icon, size: 20),
                    label: item.label,
                  ),
              ],
            ),
    );
  }
}

/// 带异步状态管理的页脚手架：加载中 / 错误 / 数据三态
class AsyncPage<T> extends StatefulWidget {
  const AsyncPage({
    super.key,
    required this.title,
    required this.load,
    required this.builder,
    this.retrySafe = true,
    this.actions = const [],
    this.emptyIcon,
    this.emptyLabel = '暂无数据',
    this.emptyWhen,
  });

  final String title;
  final Future<T> Function() load;
  final Widget Function(BuildContext, T) builder;
  final bool retrySafe;
  final List<Widget> actions;
  final IconData? emptyIcon;
  final String emptyLabel;
  final bool Function(T)? emptyWhen;

  @override
  State<AsyncPage<T>> createState() => _AsyncPageState<T>();
}

class _AsyncPageState<T> extends State<AsyncPage<T>> {
  T? _data;
  Object? _error;
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    _reload();
  }

  Future<void> _reload() async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final data = await widget.load();
      if (mounted) setState(() => _data = data);
    } catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: Text(widget.title),
        actions: [
          ...widget.actions,
          IconButton(
            tooltip: '刷新',
            icon: _busy
                ? const SizedBox(
                    width: 16,
                    height: 16,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Icon(LucideIcons.refreshCw, size: 18),
            onPressed: _busy ? null : _reload,
          ),
          const SizedBox(width: 8),
        ],
      ),
      body: RefreshIndicator(
        onRefresh: _reload,
        child: _error != null
            ? ListView(
                children: [
                  SizedBox(
                    height: MediaQuery.sizeOf(context).height * 0.6,
                    child: ErrorPanel(
                      message: errorText(_error!),
                      onRetry: _reload,
                      retrySafe: widget.retrySafe,
                    ),
                  ),
                ],
              )
            : _busy && _data == null
                ? const LoadingPanel()
                : _data == null
                    ? const EmptyPanel()
                    : (widget.emptyWhen != null && widget.emptyWhen!(_data as T))
                        ? EmptyPanel(
                            icon: widget.emptyIcon,
                            label: widget.emptyLabel,
                          )
                        : _buildData(context),
      ),
    );
  }

  Widget _buildData(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final width = constraints.maxWidth;
        final pad = width > 720 ? 24.0 : 14.0;
        return ListView(
          physics: const AlwaysScrollableScrollPhysics(),
          padding: EdgeInsets.all(pad),
          children: [
            Center(
              child: ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 980),
                child: widget.builder(context, _data as T),
              ),
            ),
          ],
        );
      },
    );
  }
}
