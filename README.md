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
│       ├── src/                # 登录、会话、解析
│       ├── examples/           # 可运行的调试与抓取工具
│       └── tests/              # 真实页面回归测试与夹具
└── tools/
    ├── captcha-collector/      # 验证码样本采集脚本（Python）
    └── js-recon/               # 前端资源离线逆向（抓取 + 分析）
```

### 各 crate 职责

| crate | 职责 | 依赖 |
|-------|------|------|
| `gnnuhub-core` | 数据模型（课表、学生信息）、错误枚举、主机常量 | 无网络依赖 |
| `gnnuhub-crypto` | 复刻前端 JS 的 RSA 加密，用于密码与 `loginUserToken` | `num-bigint` |
| `gnnuhub-ocr` | 验证码识别（位图查表） | `image`、`base64` |
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

教务系统通过统一身份认证平台（CAS）接入，完整链路分四步：

```
① GET  cas.gnnu.edu.cn/lyuapServer/kaptcha?id={random}
        → { kaptchaType, uid, content(base64 图片), timeout }

② POST cas.gnnu.edu.cn/lyuapServer/v1/tickets
        表单: username / password(RSA加密) / service / code / id(用上一步的 uid)
        → 成功: { tgt, ticket }
        → 失败: { meta: {...}, data: { code: "CODEFALSE" } }

③ 带着 ticket 进入教务系统，共 3 跳换取会话：

   GET /sso/lyiotlogin?ticket=ST-xxx      （裸请求，不带 Cookie）
        → 302 /sso/lyiotlogin
        Set-Cookie: JSESSIONID=<A>          种在 /sso 下
   GET /sso/lyiotlogin                    （带 <A>）
        → 302 /ticketlogin?uid=...&verify=...
   GET /ticketlogin?uid=...&verify=...    （带 <A>）
        → 302 /xtgl/login_slogin.html
        Set-Cookie: JSESSIONID=<B>          这才是可用会话，种在 / 下
```

### 关键细节

**必须停在 `login_slogin` 之前。** 第 3 跳的 `Set-Cookie` 已经给出根作用域的
有效 `JSESSIONID`，会话此时即告成立。若继续跟随到 `/xtgl/login_slogin.html`，
服务端会**直接关闭 TLS 连接**（实测报 `peer closed connection without sending
TLS close_notify`）——这是针对「已持有会话却重放登录页」的稳定行为拦截，
不是网络抖动，重试也无效。

**后续请求必须同时携带 `JSESSIONID` 与 `SF_cookie_17`。** 后者由网关下发并与
会话绑定。实测对 `/xtgl/index_cxYhxxIndex.html` 的结果：

| 携带内容 | 结果 |
|---|---|
| 仅 `JSESSIONID` | 302，会话不生效 |
| `JSESSIONID` + `SF_cookie_17` | **200，取到真实页面** |
| 再加 `rememberMe=deleteMe` | 连接被直接关闭 |

**同名 Cookie 有作用域之分。** `/sso` 下的 `JSESSIONID` 只在链路中途必需，
最终应以根作用域那份为准；`rememberMe=deleteMe` 这类删除标记必须在解析阶段剔除。

**验证码申请与提交的 id 不是同一个值。** 申请时传入的 `id` 是客户端随机数，
但接口响应里会返回一个服务端生成的 `uid`，提交登录时必须使用 `uid`。
原 Python 实现在 `Captcha` 类中误用了请求时的 id。

**`Host` 头覆盖只对认证平台有效。** CAS 接口所在的反向代理只认
`Host: cas.gnnu.edu.cn`，用默认推导值会被拒绝；但该头部若被复用到教务系统
等其他域名的请求上，会造成 Host 与 SNI 不匹配，服务端直接关闭 TLS 连接。
因此 `Client::cas_headers()` 返回的头部**不可跨域复用**。

**`csrftoken` 无需携带。** 实测课表与学籍查询接口均不校验它。

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
| `current_week_schedule` | 本周课表（`this_week` + `class_schedule` 组合） | — |
| `fetch_raw` | 按原样抓取站内路径，用于离线逆向 | — |

### 已知限制

- **课表单周接口未验证**：`class_schedule(term, Some(week))` 走的移动端接口
  `/kbcx/xskbcxMobile_cxXsKb.html` 尚未实测。
- **学籍字段仅映射 12 个**：页面实际提供 84 个字段，完整对照表见
  `tools/js-recon/out/verification.md` 附一，可按需扩展。
- **成绩查询尚未实现**。

### 验证码识别

只有一条路径：**位图查表**。服务端的字形渲染是确定性的——同一
`(字符, 字号)` 组合在不同图片、不同颜色下产生逐像素相同的点阵，因此识别
退化成查表，准确率上限只取决于字库覆盖度。

- 字库内嵌于 `crates/gnnuhub-ocr/assets/bitmap_lib.json`（70 个字形、
  覆盖 `0-9A-Za-z` 共 62 个字符），在 100 张真实样本上**字形级
  400/400 = 100%**、整图 100/100 = 100%。
- **真值实测 72/72 = 100%**（用真实登录验证识别对错，工具见
  `examples/ocr_verify.rs`）。
- **未命中时直接报错，不做任何猜测**：错误信息里带上真实的宽高与完整位串，
  可原样粘进字库补充。之所以不让通用 OCR 兜底——猜错的字符会被拿去登录，
  白耗一次尝试；报错是零成本的。

> 早期还挂过 tesseract 作为兜底，现已移除。真值验证显示查表已达 100%，
> 而 tesseract 对这类抗锯齿彩色验证码只有 78%，一个更差的兜底只会引入
> 「猜错」这一新的失败模式。

## 与 Python 参考实现的差异

原项目 [GNNU_API](https://github.com/LittleFishStars/GNNU_API) 是本次重写的参考，
以下是重写过程中修正的问题：

| 问题 | 原实现 | 本实现 |
|------|--------|--------|
| 验证码错误重试 | 递归调用，无上限，可能栈溢出 | 有界循环，可配置次数 |
| SSO 跳转 | `while True`，无上限 | 有界循环 + 目标域校验 + 明确的停止点 |
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

# 运行测试（含真实页面回归测试）
cargo test --workspace --all-targets

# 静态检查（CI 以此为门槛）
cargo clippy --workspace --all-targets -- -D warnings

# 格式检查
cargo fmt --all --check

# 文档
cargo doc --open
```

### 验证码样本采集

`tools/captcha-collector` 用于批量抓取验证码，为后续训练字型库做准备：

```bash
python3 tools/captcha-collector/collect.py --count 300 --out captcha_samples
```

### 前端资源离线逆向

登录逻辑（加密、跳转、Cookie 作用域、请求头要求）都写在前端 JS 里，
读代码比反复发请求试探高效得多，也不会触发风控。抓取**只跑一次**，
产物落盘后全部离线分析：

```bash
# 抓取（需凭据，会写 4.2 MB 左右的产物）
GNNU_STUDENT_ID=xxx GNNU_PASSWORD='xxx' \
  cargo run -p gnnuhub-api --example js_recon

# 追加 once 参数可启用文件桥，在单进程内完成交互式验证码输入
GNNU_STUDENT_ID=xxx GNNU_PASSWORD='xxx' \
  cargo run -p gnnuhub-api --example js_recon -- once

# 离线分析（不联网）
./tools/js-recon/analyze.sh
```

### 请求礼仪

教务系统前置了 SafeDog WAF，会因**请求量**与**行为模式**封禁来源 IP。
本项目默认对请求做节流（`ClientConfig::request_interval`，默认 600 ms），
**请勿关闭**。调试接口时优先用 `js_recon` 抓一次前端资源做离线分析，
而不是反复打接口试探。

## 许可证

MIT
