# GNNUHub

赣南师范大学教务系统接口的 Rust 重新实现。

> 仅供学习交流使用，请自觉遵守国家相关法律法规，谨慎使用。
> 任何因使用本项目造成的后果均与作者无关。
>
> 本项目所有接口均由分析前端请求得到，作者不对其稳定性做任何保证。

## 项目定位

本项目将学校教务系统的接口重新打包为 Rust 库，核心逻辑与界面层解耦：

- **核心层（Rust）**：负责加密、登录、接口调用与数据解析，编译为库供上层链接
- **界面层**：技术栈待定，可以是 Tauri、egui/iced，或其他原生框架

## 目录结构

```
GNNUHub/
├── Cargo.toml                  # 工作区配置
├── crates/
│   ├── gnnuhub-core/           # 核心类型、错误、常量
│   ├── gnnuhub-crypto/         # RSA 密码与令牌加密
│   ├── gnnuhub-ocr/            # 验证码识别抽象层
│   └── gnnuhub-api/            # 教务系统接口客户端
└── tools/
    └── captcha-collector/      # 验证码样本采集工具
```

### 各 crate 职责

| crate | 职责 | 依赖 |
|-------|------|------|
| `gnnuhub-core` | 数据模型（课表、学生信息）、错误枚举、主机常量 | 无网络依赖 |
| `gnnuhub-crypto` | 复刻前端 JS 的 RSA 加密，用于密码与 `loginUserToken` | `num-bigint` |
| `gnnuhub-ocr` | 验证码识别 trait 与实现 | `image`、`base64` |
| `gnnuhub-api` | 登录流程、会话管理、接口调用与解析 | `reqwest`、`scraper` |

## 技术选型

| 方面 | 选择 | 说明 |
|------|------|------|
| 版本 | Rust 2024 edition | 需要 1.85+ |
| 异步运行时 | `tokio` | 多线程运行时 |
| HTTP | `reqwest` | 使用 `rustls-tls`，不依赖系统 OpenSSL |
| HTML 解析 | `scraper` | 基于 `html5ever` |
| 大数运算 | `num-bigint` | RSA 模幂 |
| 图像处理 | `image` | 验证码解码与预处理 |
| 错误处理 | `thiserror` | 库对外统一错误类型 |

## 接口分析

### 登录流程

教务系统通过统一身份认证平台（CAS）接入，完整链路：

```
1. GET  cas.gnnu.edu.cn/lyuapServer/kaptcha?id={random}
        → 返回 { kaptchaTile, uid, content(base64图片), timeout }

2. POST cas.gnnu.edu.cn/lyuapServer/v1/tickets
        表单: username / password(RSA加密) / service / code / id(用上一步的 uid)
        → 成功: { tgt, ticket }
        → 失败: { meta: { success, statusCode, message } }

3. GET  jwgl.gnnu.edu.cn/sso/lyiotlogin?ticket={ticket}
        → 302 跳转，Set-Cookie 里带 JSESSIONID
```

### 关键细节

**验证码申请与提交的 id 不是同一个值**。申请时传入的 `id` 是客户端随机数，
但接口响应里会返回一个服务端生成的 `uid`，提交登录时必须使用 `uid`。
原 Python 实现在 `Captcha` 类中误用了请求时的 id。

**验证码特征**（基于实际抓样的分析）：

| 属性 | 值 |
|------|-----|
| 尺寸 | 100 × 25 px |
| 字符数 | 4 |
| 字符集 | 字母数字混合，**大小写混用** |
| 颜色 | 每个字符颜色随机且不同，白底 |
| 干扰 | 无旋转、无扭曲、无干扰线 |
| 有效期 | 300 秒 |

由于字符颜色随机，识别前必须先灰度化丢弃颜色信息。

### 已实现的接口

| 方法 | 说明 | 原 Python 对应 |
|------|------|----------------|
| `fetch_basic_info` | 姓名、身份、学院、班级、头像 | `_get_basic_info` |
| `fetch_student_info` | 学籍详细信息 | `_get_student_info` |
| `student_info` | 合并后的完整信息 | `get_all_info` |
| `class_schedule` | 课表（整学期或单周） | `get_class_schedule` |
| `course` | 指定课程的开课信息 | `get_course` |
| `timetable` | 节次时间表 | `get_timetable` |
| `this_week` | 当前教学周 | `this_week` |
| `current_week_schedule` | 本周课表 | — |

## 与 Python 参考实现的差异

原项目 [GNNU_API](https://github.com/LittleFishStars/GNNU_API) 是本次重写的参考，
以下是重写过程中修正的问题：

| 问题 | 原实现 | 本实现 |
|------|--------|--------|
| 验证码错误重试 | 递归调用，无上限，可能栈溢出 | 有界循环，可配置次数 |
| SSO 跳转 | `while True`，无上限 | 有界循环 + 目标域校验 |
| 验证码 uid | 误用请求 id | 使用响应返回的 uid |
| Barrett 类 | 名为 Barrett，实为直接 `pow()` | 移除伪实现，直接用 `num-bigint` |
| 错误处理 | 返回元组，语义模糊 | 统一 `Error` 枚举 |
| 个人信息缓存 | 各字段独立 `if` 判断，重复请求 | 统一状态，一次拉取 |

## 开发

### 环境要求

- Rust 1.85+（edition 2024）
- 无需系统 OpenSSL（使用 rustls）

### 常用命令

```bash
# 编译
cargo build

# 运行测试
cargo test

# 只测试加密模块（与 Python 输出对齐的验证在这里）
cargo test -p gnnuhub-crypto

# 文档
cargo doc --open
```

### 验证码样本采集

`tools/captcha-collector` 用于批量抓取验证码，为后续训练字型库做准备：

```bash
python3 tools/captcha-collector/collect.py --count 300 --out captcha_samples
```

## 许可证

MIT
