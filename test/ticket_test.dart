/// 票据响应解析单测 —— 11 个黄金样本与 Rust 测试一致
library;

import 'package:flutter_test/flutter_test.dart';
import 'package:gnnuhub/api/ticket.dart';
import 'package:gnnuhub/core/error.dart';

void main() {
  test('成功响应携带 tgt 与 ticket', () {
    final outcome = parseTicketResponse('{"tgt":"TGT-abc","ticket":"ST-xyz"}');
    expect(outcome, isA<LoginSuccess>());
    final success = outcome as LoginSuccess;
    expect(success.tgt, 'TGT-abc');
    expect(success.ticket, 'ST-xyz');
  });

  test('data.code=CODEFALSE → 验证码错误（meta.success=true 也不影响）', () {
    final outcome = parseTicketResponse(
        '{"meta":{"success":true,"statusCode":200,"message":"ok"},"data":{"code":"CODEFALSE"}}');
    expect(outcome, isA<CaptchaIncorrect>());
  });

  test('statusCode 为字符串同样处理', () {
    final outcome = parseTicketResponse(
        '{"meta":{"success":true,"statusCode":"200","message":"ok"},"data":{"code":"CODEFALSE"}}');
    expect(outcome, isA<CaptchaIncorrect>());
  });

  test('meta 分支：statusCode=CODEFALSE', () {
    final outcome = parseTicketResponse(
        '{"meta":{"success":false,"statusCode":"CODEFALSE","message":"验证码错误"}}');
    expect(outcome, isA<CaptchaIncorrect>());
  });

  test('meta 分支：用户名或密码错误', () {
    final outcome = parseTicketResponse(
        '{"meta":{"success":false,"statusCode":"USERNAMEORPASSWORDERROR","message":"用户名或密码错误"}}');
    expect(outcome, isA<BadCredentials>());
  });

  test('data.code=PASSERROR 携带剩余次数', () {
    final outcome = parseTicketResponse(
        '{"meta":{"success":true,"statusCode":200,"message":"ok"},"data":{"code":"PASSERROR","data":"5,3"}}');
    expect(outcome, isA<BadCredentials>());
    expect((outcome as BadCredentials).detail, contains('5,3'));
  });

  test('data.code=PASSERROR 无详情', () {
    final outcome = parseTicketResponse('{"data":{"code":"PASSERROR"}}');
    expect(outcome, isA<BadCredentials>());
  });

  test('data.code=USERLOCK → 账号锁定', () {
    final outcome = parseTicketResponse(
        '{"meta":{"success":true,"statusCode":200,"message":"ok"},"data":{"code":"USERLOCK"}}');
    expect(outcome, isA<AccountLocked>());
  });

  test('data.data 存在但不影响 CODEFALSE 判定', () {
    final outcome = parseTicketResponse(
        '{"meta":{"success":true,"statusCode":200,"message":"ok"},"data":{"code":"CODEFALSE","data":"1,4"}}');
    expect(outcome, isA<CaptchaIncorrect>());
  });

  test('data.code=SUCCESS 但无 ticket → 报错', () {
    expect(
      () => parseTicketResponse('{"data":{"code":"SUCCESS"}}'),
      throwsA(isA<TicketMissingError>()),
    );
  });

  test('非 JSON 文本 → JsonError', () {
    expect(
      () => parseTicketResponse('not json at all'),
      throwsA(isA<JsonError>()),
    );
  });
}
