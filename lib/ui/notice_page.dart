/// 通知页 + 通知详情页
library;

import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:provider/provider.dart';

import '../core/model.dart';
import '../service/hub_service.dart';
import 'home_page.dart';
import 'widgets/panels.dart';

/// 首页数据包：通知 + 系统提醒
typedef NoticesData = (List<Notice>, List<String>);

class NoticePage extends StatelessWidget {
  const NoticePage({super.key});

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return AsyncPage<NoticesData>(
      title: '通知公告',
      emptyIcon: LucideIcons.megaphoneOff,
      emptyLabel: '没有通知',
      load: () async {
        final service = context.read<HubService>();
        final notices = await service.notices();
        final messages = await service.systemMessages().catchError((_) => <String>[]);
        return (notices, messages);
      },
      builder: (context, data) {
        final (notices, messages) = data;
        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (messages.isNotEmpty)
              Card(
                color: scheme.secondaryContainer,
                margin: const EdgeInsets.only(bottom: 12),
                child: Padding(
                  padding: const EdgeInsets.all(12),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(
                        children: [
                          Icon(LucideIcons.bellRing,
                              size: 15, color: scheme.onSecondaryContainer),
                          const SizedBox(width: 6),
                          Text(
                            '系统提醒',
                            style: TextStyle(
                              fontSize: 13,
                              fontWeight: FontWeight.w600,
                              color: scheme.onSecondaryContainer,
                            ),
                          ),
                        ],
                      ),
                      const SizedBox(height: 6),
                      for (final message in messages)
                        Padding(
                          padding: const EdgeInsets.only(top: 2),
                          child: Text(
                            message,
                            style: TextStyle(
                              fontSize: 12.5,
                              color: scheme.onSecondaryContainer,
                            ),
                          ),
                        ),
                    ],
                  ),
                ),
              ),
            for (final notice in notices)
              Card(
                margin: const EdgeInsets.only(bottom: 8),
                child: ListTile(
                  dense: true,
                  onTap: () => _openDetail(context, notice),
                  title: Row(
                    children: [
                      if (notice.pinned)
                        Padding(
                          padding: const EdgeInsets.only(right: 6),
                          child: Icon(
                            LucideIcons.pin,
                            size: 14,
                            color: scheme.error,
                          ),
                        ),
                      Expanded(
                        child: Text(
                          notice.title,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: const TextStyle(
                            fontSize: 13.5,
                            fontWeight: FontWeight.w500,
                          ),
                        ),
                      ),
                    ],
                  ),
                  subtitle: Text(
                    [notice.category, notice.date]
                        .where((s) => s.isNotEmpty)
                        .join(' · '),
                    style: TextStyle(
                      fontSize: 11.5,
                      color: scheme.onSurfaceVariant,
                    ),
                  ),
                  trailing: Icon(
                    LucideIcons.chevronRight,
                    size: 16,
                    color: scheme.onSurfaceVariant,
                  ),
                ),
              ),
          ],
        );
      },
    );
  }

  void _openDetail(BuildContext context, Notice notice) {
    Navigator.of(context).push(
      MaterialPageRoute<void>(
        builder: (_) => NoticeDetailPage(notice: notice),
      ),
    );
  }
}

class NoticeDetailPage extends StatefulWidget {
  const NoticeDetailPage({super.key, required this.notice});

  final Notice notice;

  @override
  State<NoticeDetailPage> createState() => _NoticeDetailPageState();
}

class _NoticeDetailPageState extends State<NoticeDetailPage> {
  NoticeDetail? _detail;
  Object? _error;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final detail = await context
          .read<HubService>()
          .noticeContent(widget.notice.detailPath);
      if (mounted) setState(() => _detail = detail);
    } catch (e) {
      if (mounted) setState(() => _error = e);
    }
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final detail = _detail;
    return Scaffold(
      appBar: AppBar(
        title: Text(
          detail?.title ?? widget.notice.title,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
        ),
        actions: [
          IconButton(
            tooltip: '刷新',
            icon: const Icon(LucideIcons.refreshCw, size: 18),
            onPressed: detail == null && _error == null ? null : _load,
          ),
          const SizedBox(width: 8),
        ],
      ),
      body: _error != null
          ? ErrorPanel(message: errorText(_error!), onRetry: _load)
          : detail == null
              ? const LoadingPanel()
              : SingleChildScrollView(
                  padding: const EdgeInsets.all(16),
                  child: Center(
                    child: ConstrainedBox(
                      constraints: const BoxConstraints(maxWidth: 760),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: [
                          Text(
                            detail.title,
                            style: const TextStyle(
                              fontSize: 17,
                              fontWeight: FontWeight.w700,
                              height: 1.4,
                            ),
                          ),
                          const SizedBox(height: 8),
                          Text(
                            [
                              if (detail.publisher.isNotEmpty)
                                '发布人：${detail.publisher}',
                              if (detail.publishTime.isNotEmpty)
                                detail.publishTime,
                              if (detail.views.isNotEmpty)
                                '浏览 ${detail.views}',
                            ].join(' · '),
                            style: TextStyle(
                              fontSize: 12,
                              color: scheme.onSurfaceVariant,
                            ),
                          ),
                          const Divider(height: 24),
                          Text(
                            detail.content.isEmpty ? '（无正文）' : detail.content,
                            style: const TextStyle(
                              fontSize: 14.5,
                              height: 1.75,
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),
                ),
    );
  }
}
