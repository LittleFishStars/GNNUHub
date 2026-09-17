# 前端 JS 逆向分析工具

## 目的

教务系统的登录逻辑（加密方式、跳转链路、Cookie 作用域、请求头要求）
**全部写在前端 JS 里**。与其反复发请求试探，不如一次性把相关 JS 抓下来
离线阅读——不产生试探流量，也不会触发风控。

## 为什么需要这个

本项目在打通 SSO 时走了弯路：先后误判为「网络抖动」「WAF 限流」，
累计发出上百次请求并触发了按 IP 的封禁。实际上前端 JS 里就有答案。
**先读代码，再发请求。**

## 使用方式

### 1. 抓取（需要有效会话，请求量固定且很小）

```bash
# 用现有凭据登录后抓取（约 15-25 次请求，一次性完成）
GNNU_STUDENT_ID=xxx GNNU_PASSWORD='xxx' \
  cargo run -p gnnuhub-api --example js_recon
```

产物落在 `tools/js-recon/out/`：
- `pages/*.html` — 各页面原始 HTML
- `scripts/*.js`  — 提取并下载的 JS 文件
- `index.json`    — 抓取清单与来源对应关系

### 2. 离线分析（不联网）

```bash
./analyze.sh
```

会在 `out/report.md` 生成关键词命中汇总，重点关注：

| 关键词 | 用途 |
|---|---|
| `login_slogin` | 登录页在流程中的真实角色 |
| `index_initMenu` | 登录成功后的落地页 |
| `lyiotlogin` | SSO 入口 |
| `kaptcha` | 验证码接口与参数 |
| `loginUserToken` | 自定义请求头，需逆向其生成逻辑 |
| `publicKey` / `modulus` / `exponent` | RSA 加密参数 |
| `JSESSIONID` / `SF_cookie` | Cookie 作用域相关 |
| `gnmkdm` | 各功能模块的菜单代码 |
| `/xtgl/` `/xsxxxggl/` `/kbcx/` | 接口路径 |

## 注意事项

- **抓取只需跑一次**。产物入库后，后续分析全部离线完成。
- JS 文件较多时优先读 `login` / `index` / `common` 命名的，以及体积最大的。
- 教务系统多用 jQuery + 拼接式 URL，搜索 `.html?` 和 `.do?` 能快速定位接口。
- 部分 JS 可能被压缩，先看有没有 `.min.js` 之外的同名未压缩版本。
