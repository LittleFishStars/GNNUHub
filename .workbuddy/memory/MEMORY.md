# GNNUHub 项目长期备忘

赣南师范大学教务系统 Rust 重写。参考实现：`/home/ylxc/Projects/python/GNNU_API`。

## 硬性约束（来自用户明确要求，务必遵守）

- **控制请求量**：用户曾多次因 IP 被封而强烈不满。每轮对话最多 ~20 次
  网络请求；一旦发现被拦（连接被关闭、域名整体拒连）立即停止。
- **优先离线逆向**：用户明确指示「抓包返回的 js 脚本进行逆向分析，而不是
  一直尝试」。要探接口时先用 `cargo run -p gnnuhub-api --example js_recon`
  抓一次前端资源，之后全部离线分析，不要再反复打接口试探。
- **一次只改一个变量**：做实验时不要同时改多个条件。
- 真实账号测试已获授权（学号 `250710078`）。

## 工具链与代码规范

- **stable 工具链**（见 `rust-toolchain.toml`）。`rustfmt.toml` 里
  **不能**放 nightly-only 选项（`imports_granularity`、`group_imports`），
  否则被静默忽略且每次运行都报警告。
- 提交前跑这三条：
  ```bash
  cargo test --workspace --all-targets
  cargo clippy --workspace --all-targets -- -D warnings   # 必须带 -D warnings
  cargo fmt --all --check
  ```
  注意 clippy **默认跑不够**：`derivable_impls` 等 lint 只在 `-D warnings`
  下才会暴露。
- 注释与文档一律中文。
- 代码风格：`Session` 的所有 HTTP 出口收敛到私有 `send()`，新增请求方法
  走它而不是各自拼 URL。

## 已验证的关键事实（逆向 + 实测）

- **SSO 交换共 3 跳**，且**必须在 `/xtgl/login_slogin.html` 之前停止**——
  继续跟随会被服务端直接关闭 TLS 连接（`peer closed connection without
  sending TLS close_notify`）。这是稳定的行为拦截，重试无效。
- 后续请求必须同时携带 `JSESSIONID` **与** `SF_cookie_17`（网关下发）；
  只带前者会 302。`rememberMe=deleteMe` 必须剔除，带上会被断连。
- 同名 `JSESSIONID` 有作用域：`/sso` 那份只是链路中转物，最终用根作用域那份。
- `Client::cas_headers()` 带 `Host: cas.gnnu.edu.cn`，**不可跨域复用**，
  否则 Host 与 SNI 不匹配会被断连。
- `csrftoken` 对课表/学籍查询接口**不需要**。
- 验证码：100×25 PNG，固定 4 位，字母数字大小写混用，无干扰线。
- 认证平台「地址改为服务部署地址」这种提示与实际登录失败无必然关系。
- `xqm` 取值是 **3（第一学期）/ 12（第二学期）**，不是 1/2。

## 踩过的坑

- **字段 id 极易数错字符**：政治面貌的真实 id 是 `col_zzmmm`（**两个 z**，
  是「政治面貌」的拼音首字母 z-z-m-m-m），不是 `col_zzzmmm`。教训：
  一律用 `grep -o 'id="col_[^"]*"'` 机器提取，别用肉眼数。
  `tests/real_page_check.rs` 里有一条测试专门守着这一点。
- 不要给 reqwest 开 `cookie_store(true)`：会与手动设置的 COOKIE 头叠加，
  发出两个同名 `JSESSIONID`，服务端判定异常后直接关闭连接。
- `/tmp` 在工具调用之间不持久，临时脚本放项目内 `.scratch/`（已 gitignore）。
- 沙箱里 `python3 - << 'PYEOF'` 形式的 heredoc 容易被破坏，改为写脚本文件再执行。

## 待办

- 单周课表接口 `/kbcx/xskbcxMobile_cxXsKb.html` 未实测
- 学籍字段仅映射 12/84，完整对照表在 `tools/js-recon/out/verification.md` 附一
- 成绩查询未实现（`/cjcx/cjcx_cxDgXscj.html?gnmkdm=N305005`）
- 验证码自动识别未实现（当前 `ManualOcr` 需人工）
- 界面层框架未选（用户明确表示稍后再定）
