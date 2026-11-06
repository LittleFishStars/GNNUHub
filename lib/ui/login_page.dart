/// 登录页
library;

import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:provider/provider.dart';

import '../core/error.dart';
import '../service/hub_service.dart';
import '../state/term_store.dart';
import 'widgets/window_title_bar.dart';

class LoginPage extends StatefulWidget {
  const LoginPage({super.key});

  @override
  State<LoginPage> createState() => _LoginPageState();
}

class _LoginPageState extends State<LoginPage> {
  final _studentId = TextEditingController();
  final _password = TextEditingController();
  bool _busy = false;
  bool _obscure = true;
  String? _error;

  @override
  void initState() {
    super.initState();
    TermStore.loadStudentId().then((saved) {
      if (saved != null && mounted) {
        setState(() => _studentId.text = saved);
      }
    });
  }

  @override
  void dispose() {
    _studentId.dispose();
    _password.dispose();
    super.dispose();
  }

  Future<void> _login() async {
    final id = _studentId.text.trim();
    final password = _password.text;
    if (id.isEmpty || password.isEmpty) {
      setState(() => _error = '请填写学号与密码。');
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final service = context.read<HubService>();
      await service.login(id, password);
      await TermStore.saveStudentId(id);
      // 登录成功后串行拉取首页数据（WAF 礼仪：一次只做一件事）
    } on HubError catch (e) {
      setState(() => _error = e.message);
    } catch (e) {
      setState(() => _error = e.toString());
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Scaffold(
      appBar: needsCustomTitleBar
          ? const PreferredSize(
              preferredSize: Size.fromHeight(32),
              child: WindowTitleBar(),
            )
          : null,
      body: Center(
        child: SingleChildScrollView(
          padding: const EdgeInsets.all(24),
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 380),
            child: Card(
              child: Padding(
                padding:
                    const EdgeInsets.symmetric(horizontal: 28, vertical: 32),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Center(
                      child: Container(
                        width: 60,
                        height: 60,
                        decoration: BoxDecoration(
                          color: scheme.primary,
                          borderRadius: BorderRadius.circular(16),
                        ),
                        child: const Icon(
                          LucideIcons.graduationCap,
                          color: Colors.white,
                          size: 30,
                        ),
                      ),
                    ),
                    const SizedBox(height: 14),
                    const Center(
                      child: Text(
                        'GNNUHub',
                        style: TextStyle(
                            fontSize: 20, fontWeight: FontWeight.w700),
                      ),
                    ),
                    Center(
                      child: Text(
                        '赣南师范大学教务系统',
                        style: TextStyle(
                          fontSize: 12.5,
                          color: scheme.onSurfaceVariant,
                        ),
                      ),
                    ),
                    const SizedBox(height: 26),
                    TextField(
                      controller: _studentId,
                      keyboardType: TextInputType.number,
                      enabled: !_busy,
                      decoration: const InputDecoration(
                        labelText: '学号',
                        prefixIcon: Icon(LucideIcons.userRound, size: 18),
                      ),
                    ),
                    const SizedBox(height: 14),
                    TextField(
                      controller: _password,
                      obscureText: _obscure,
                      enabled: !_busy,
                      onSubmitted: (_) => _login(),
                      decoration: InputDecoration(
                        labelText: '密码',
                        prefixIcon:
                            const Icon(LucideIcons.lock, size: 18),
                        suffixIcon: IconButton(
                          icon: Icon(
                            _obscure
                                ? LucideIcons.eye
                                : LucideIcons.eyeOff,
                            size: 18,
                          ),
                          onPressed: () =>
                              setState(() => _obscure = !_obscure),
                        ),
                      ),
                    ),
                    const SizedBox(height: 10),
                    if (_error != null)
                      Padding(
                        padding: const EdgeInsets.only(top: 4, bottom: 4),
                        child: Row(
                          children: [
                            Icon(LucideIcons.circleAlert,
                                size: 15, color: scheme.error),
                            const SizedBox(width: 6),
                            Expanded(
                              child: Text(
                                _error!,
                                style:
                                    TextStyle(color: scheme.error, fontSize: 12.5),
                              ),
                            ),
                          ],
                        ),
                      ),
                    const SizedBox(height: 16),
                    FilledButton(
                      onPressed: _busy ? null : _login,
                      child: _busy
                          ? const Row(
                              mainAxisAlignment: MainAxisAlignment.center,
                              children: [
                                SizedBox(
                                  width: 16,
                                  height: 16,
                                  child: CircularProgressIndicator(
                                    strokeWidth: 2,
                                    color: Colors.white70,
                                  ),
                                ),
                                SizedBox(width: 10),
                                Text('登录中（验证码自动识别）…'),
                              ],
                            )
                          : const Text('登 录'),
                    ),
                    const SizedBox(height: 10),
                    Text(
                      '仅供学习交流使用，请自觉遵守国家相关法律法规，谨慎使用。',
                      textAlign: TextAlign.center,
                      style: TextStyle(
                        fontSize: 11,
                        color: scheme.onSurfaceVariant.withValues(alpha: 0.7),
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}
