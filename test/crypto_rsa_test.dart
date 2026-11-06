/// RSA 加密单测 —— 与 Python 参考实现（legacy encode.py）逐字节对拍
library;

import 'package:flutter_test/flutter_test.dart';
import 'package:gnnuhub/crypto/rsa.dart';

void main() {
  group('encrypt', () {
    test('空明文返回空密文', () {
      expect(encrypt(''), '');
    });

    test('输出是空格分隔的小写十六进制串', () {
      final out = encrypt('test1234');
      expect(out, isNotEmpty);
      for (final part in out.split(' ')) {
        expect(part, matches(RegExp(r'^[0-9a-f]+$')), reason: '片段: $part');
        expect(part.length, lessThanOrEqualTo(256));
      }
    });

    test('相同输入产生相同输出（无随机填充）', () {
      expect(encrypt('MyPassword123'), encrypt('MyPassword123'));
    });

    test('与 Python 参考实现对拍（test1234）', () {
      expect(
        encrypt('test1234'),
        '35525dcd81df724b87753756e68a3c48dc6a7b92402afc1053f3a5fb7b2ebcafd6381d5178e2d999738407e03f4c3eb197a26704607f35af0f1e3179feff7a3bdc0357911a8d8a32454561d30556229cef1429125f6ca5c084bdd4567b1ba675cdc2b4a5c32e064ca21f4192396ae81e6a7c997c7a4c02aa5e61b501f856436e',
      );
    });

    test('与 Python 参考实现对拍（MyPassword123）', () {
      expect(
        encrypt('MyPassword123'),
        '657475301507cb66bacdf623ec7f929dc6fbacebf9aee1e880a37fed689ca2fa2cc3429d5fdc40afbbffac86dafd9cb023e377a3e231453c6d8c242f68e1a3c637b8fd1a878111f6409cd1c0f8148de25f6f99c3c5edbea5d0544de3e8bd835f771f80e83c60e9ba1eb2e7156ce74f78bda1c2550761957265e9a1a7babb15c6',
      );
    });

    test('与 Python 参考实现对拍（short）', () {
      expect(
        encrypt('short'),
        '22f28a5bebcec7741fd2d7c6496d605dcc480163f38d09cec382c35f9c9e363548efa49e0dbd5ddd2e59948e316b19ceb7b2c58cf5ee0c6ac67a354ff17eed37486344e1fe19c2fdf58c8964aa5e2b1b6dc51a6a10cc8b84adccc228a8d6d8edfbbadc1378a6b10ae239eb7a8101dbe90fb59802728bf7e6637efdae4dd7425e',
      );
    });

    test('与 Python 参考实现对拍（RefPass#2026）', () {
      expect(
        encrypt('RefPass#2026'),
        '5c47ec5e586038b4793fb391755265f592154b7f05f182ccefab4b9f330b4d4ba4494062325d23a9794304c070a0cdf574616c4f193262b4947bd711a1f342a5a719c1d28e306d3843f55c9d99b7ce14a729e239b63be934e2ae45fc71f8ce7f1d7f09fb54f02f061093ef622f35015209ce93c50c298b2927210e7df8d4a18',
      );
    });

    test('与 Python 参考实现对拍（Str0ng!Pass）', () {
      expect(
        encrypt('Str0ng!Pass'),
        '4826032d565b4c455ffe6cac824cb370fc4969a505cf0baa43770a234ee1d8fa8517aaea27fa5c2cf46050c23cf9af638ae53353e111b4e34988e0ee784df73a149d06bd2eacb1e3eed6b4ae37f2747790d8e82520fa4c8576db993ade0f2a22a800038c2e28bda2a8388894553598ea82721880830cd8420c6a4c1858f763f7',
      );
    });

    test('长明文分块（127 字节首块与 Python 对拍）', () {
      final long = 'a' * 127;
      final actual = encrypt(long);
      final firstBlock = actual.split(' ').first;
      expect(
        firstBlock,
        '6f5c4eeb120f60be0a3c4d972f4393192dab8605934e41dc3ce5b631e49f4fe6f115d03b39d6d0c6e9978cba8b444e294dd167f45ad246af1d05a34f8a4ed25cb0ba88a4abb4ab7c01d994a58af0bd5866f61384b4cb1e28a896460fbfc3647113692e76c314291a2eff1c30f3e6f3fd294c87e8846e324c8c83a63b2f18cbb5',
      );
      expect(actual.split(' ').length, 2, reason: '应切分为 2 块');
    });

    test('单块明文不产生空格', () {
      expect(encrypt('short'), isNot(contains(' ')));
    });
  });

  group('loginUserToken', () {
    test('生成单块密文', () {
      final token = loginUserToken();
      expect(token, isNotEmpty);
      expect(token, isNot(contains(' ')));
    });
  });
}
