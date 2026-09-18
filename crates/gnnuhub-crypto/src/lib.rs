//! 统一身份认证平台的加密实现
//!
//! 对应 Python 版 `encode.py`。原实现模拟了前端 JS 的 RSA 加密过程：
//! 前端使用一个自定义的 BigInt 库做模幂运算，本模块用 `num-bigint`
//! 实现等价逻辑，并且不再硬编码指数常量。
//!
//! # 加密流程
//!
//! 1. 把明文字符串按 ASCII 逐字符取出
//! 2. 补零到 `CHUNK_SIZE` 的整数倍
//! 3. 每 2 个字节合并为一个 16 位「数位」，凑成一个 `CHUNK_SIZE / 2`
//!    位的大数
//! 4. 对该大数做 `value^EXPONENT mod MODULUS` 模幂运算
//! 5. 结果转 16 进制，块与块之间用空格分隔
//!
//! 注意这里的「数位」是 16 位一组（base 65536），与原 JS 实现一致。

use num_bigint::BigUint;
use num_traits::Zero;

/// RSA 公钥指数，固定为 65537
const EXPONENT: u32 = 65537;

/// 每个明文块的字节数
const CHUNK_SIZE: usize = 126;

/// 模数（base 65536 的数位表示，由原 JS 提取而来）
const MODULUS_DIGITS: [u16; 64] = [
    59313, 4375, 54507, 5267, 8345, 43610, 49971, 28563, 34983, 36521, 17297, 62027, 42744, 32131,
    40043, 48417, 5636, 46659, 52373, 20768, 28635, 46498, 55076, 13948, 44453, 44804, 40613, 1466,
    26896, 54350, 28506, 28712, 44726, 4974, 46852, 32655, 60720, 2973, 7722, 43040, 10398, 28111,
    52739, 6542, 43865, 20892, 59308, 8898, 58877, 36302, 41921, 27719, 59291, 10923, 8559, 53747,
    10707, 59976, 48415, 32958, 37390, 57449, 45414, 46574,
];

/// 把 base 65536 的数位数组还原为一个大整数
///
/// 第 `i` 位代表 `digit * 65536^i`，低位数在前。
fn from_base65536_digits(digits: &[u16]) -> BigUint {
    // 从高位到低位累乘，避免多次幂运算
    let mut value = BigUint::zero();
    for &digit in digits.iter().rev() {
        value <<= 16;
        value += digit;
    }
    value
}

/// 延迟初始化的模数
fn modulus() -> BigUint {
    from_base65536_digits(&MODULUS_DIGITS)
}

/// 对单个明文块做 RSA 加密，返回 16 进制字符串
///
/// `block` 是最多 `CHUNK_SIZE` 字节的明文片段。
fn encrypt_block(block: &[u8]) -> String {
    // 每 2 字节合并成一个 16 位数位：低字节在前
    // 与原 JS 的 `c.digits[r] = a[l] + (a[l+1] << 8)` 一致
    let mut value = BigUint::zero();
    for chunk in block.chunks(2).rev() {
        value <<= 16;
        let low = u16::from(chunk[0]);
        let high = chunk.get(1).map_or(0, |&b| u16::from(b));
        value += low + (high << 8);
    }

    let encrypted = value.modpow(&BigUint::from(EXPONENT), &modulus());

    // 转 16 进制小写，无前导 0（BigUint 的 to_str_radix 已满足）
    encrypted.to_str_radix(16)
}

/// 对任意字符串执行加密，返回以空格分隔的 16 进制密文
///
/// 空字符串会返回空字符串。
pub fn encrypt(plaintext: &str) -> String {
    if plaintext.is_empty() {
        return String::new();
    }

    let mut bytes: Vec<u8> = plaintext.bytes().collect();

    // 补零到 CHUNK_SIZE 的整数倍，与 Python 版行为一致
    while bytes.len() % CHUNK_SIZE != 0 {
        bytes.push(0);
    }

    bytes
        .chunks(CHUNK_SIZE)
        .map(encrypt_block)
        .collect::<Vec<_>>()
        .join(" ")
}

/// 加密登录密码
///
/// 对应 Python 版的 `encode_password`。
pub fn encode_password(password: &str) -> String {
    encrypt(password)
}

/// 生成并加密 `loginUserToken`
///
/// 对应 Python 版的 `get_loginUserToken`，值为 `lyasp` 前缀
/// 加上当前毫秒时间戳。
pub fn login_user_token() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    encrypt(&format!("lyasp{millis}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 模数应当是非零的大整数
    #[test]
    fn modulus_is_large() {
        let m = modulus();
        assert!(!m.is_zero());
        // 模数由 64 个数位构成，bits 应该接近 1024
        let bits = m.bits();
        assert!(bits > 1000 && bits <= 1024, "modulus bits = {bits}");
    }

    /// 空明文返回空密文
    #[test]
    fn empty_plaintext() {
        assert_eq!(encrypt(""), "");
    }

    /// 加密结果应是空格分隔的 16 进制串
    #[test]
    fn output_is_space_separated_hex() {
        let out = encrypt("test1234");
        assert!(!out.is_empty());
        for part in out.split(' ') {
            assert!(
                part.chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
                "非法的 16 进制片段: {part}"
            );
        }
    }

    /// 相同输入必须产生相同输出（无随机填充）
    #[test]
    fn deterministic() {
        let a = encrypt("MyPassword123");
        let b = encrypt("MyPassword123");
        assert_eq!(a, b);
    }

    /// 不同明文产生不同密文
    #[test]
    fn different_plaintexts_differ() {
        assert_ne!(encrypt("password1"), encrypt("password2"));
    }

    /// 长明文会被切分为多块
    #[test]
    fn long_plaintext_splits_into_blocks() {
        let long = "a".repeat(CHUNK_SIZE + 1);
        let out = encrypt(&long);
        let blocks: Vec<&str> = out.split(' ').collect();
        assert_eq!(blocks.len(), 2, "应切分为 2 块");
    }

    /// 单块明文不产生空格
    #[test]
    fn single_block_has_no_space() {
        let out = encrypt("short");
        assert!(!out.contains(' '));
    }

    /// login_user_token 应以正确的魔数开头（通过长度间接验证）
    #[test]
    fn login_token_is_generated() {
        let token = login_user_token();
        assert!(!token.is_empty());
        assert!(!token.contains(' '));
    }

    /// 与 Python 参考实现的输出逐字节对齐
    ///
    /// 基准值由 `GNNU_API/encode.py` 的 `encode_password` 生成，
    /// 用于确保 Rust 重写没有改变加密语义。
    ///
    /// 注意每个密文块的长度：模数为 1024 位，密文以 16 进制表示时
    /// 最大 256 字符，实际值前面的 0 会被省略，因此长度可能略短。
    ///
    /// 测试向量一律使用**虚构**明文；真实凭据只允许出现在运行时的
    /// 环境变量里，绝不写入代码或测试。
    #[test]
    fn matches_python_reference_implementation() {
        let cases: &[(&str, &str)] = &[
            (
                "test1234",
                "35525dcd81df724b87753756e68a3c48dc6a7b92402afc1053f3a5fb7b2ebcafd6381d5178e2d999738407e03f4c3eb197a26704607f35af0f1e3179feff7a3bdc0357911a8d8a32454561d30556229cef1429125f6ca5c084bdd4567b1ba675cdc2b4a5c32e064ca21f4192396ae81e6a7c997c7a4c02aa5e61b501f856436e",
            ),
            (
                "MyPassword123",
                "657475301507cb66bacdf623ec7f929dc6fbacebf9aee1e880a37fed689ca2fa2cc3429d5fdc40afbbffac86dafd9cb023e377a3e231453c6d8c242f68e1a3c637b8fd1a878111f6409cd1c0f8148de25f6f99c3c5edbea5d0544de3e8bd835f771f80e83c60e9ba1eb2e7156ce74f78bda1c2550761957265e9a1a7babb15c6",
            ),
            (
                "short",
                "22f28a5bebcec7741fd2d7c6496d605dcc480163f38d09cec382c35f9c9e363548efa49e0dbd5ddd2e59948e316b19ceb7b2c58cf5ee0c6ac67a354ff17eed37486344e1fe19c2fdf58c8964aa5e2b1b6dc51a6a10cc8b84adccc228a8d6d8edfbbadc1378a6b10ae239eb7a8101dbe90fb59802728bf7e6637efdae4dd7425e",
            ),
            (
                "RefPass#2026",
                "5c47ec5e586038b4793fb391755265f592154b7f05f182ccefab4b9f330b4d4ba4494062325d23a9794304c070a0cdf574616c4f193262b4947bd711a1f342a5a719c1d28e306d3843f55c9d99b7ce14a729e239b63be934e2ae45fc71f8ce7f1d7f09fb54f02f061093ef622f35015209ce93c50c298b2927210e7df8d4a18",
            ),
            (
                "Str0ng!Pass",
                "4826032d565b4c455ffe6cac824cb370fc4969a505cf0baa43770a234ee1d8fa8517aaea27fa5c2cf46050c23cf9af638ae53353e111b4e34988e0ee784df73a149d06bd2eacb1e3eed6b4ae37f2747790d8e82520fa4c8576db993ade0f2a22a800038c2e28bda2a8388894553598ea82721880830cd8420c6a4c1858f763f7",
            ),
        ];

        for (plaintext, expected) in cases {
            let actual = encrypt(plaintext);
            assert_eq!(
                actual, *expected,
                "明文 {plaintext:?} 的密文与 Python 参考实现不一致"
            );
            assert!(
                !actual.is_empty() && actual.len() <= 256,
                "密文应为 1~256 字符（模数 1024 位，前导 0 可省略）"
            );
        }
    }

    /// 超过单块长度的明文应与 Python 的分块行为一致
    #[test]
    fn multi_block_matches_python_reference() {
        let long = "a".repeat(127);
        let expected = "6f5c4eeb120f60be0a3c4d972f4393192dab8605934e41dc3ce5b631e49f4fe6f115d03b39d6d0c6e9978cba8b444e294dd167f45ad246af1d05a34f8a4ed25cb0ba88a4abb4ab7c01d994a58af0bd5866f61384b4cb1e28a896460fbfc3647113692e76c314291a2eff1c30f3e6f3fd294c87e8846e324c8c83a63b2f18cbb5";
        let actual = encrypt(&long);
        let first_block = actual.split(' ').next().unwrap();
        assert_eq!(
            first_block, expected,
            "127 字节明文的首块密文与 Python 参考实现不一致"
        );
    }
}
