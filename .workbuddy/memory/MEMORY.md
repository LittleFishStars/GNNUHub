# GNNUHub 项目长期备忘

赣南师范大学教务系统 Rust 重写。参考实现：`/home/ylxc/Projects/python/GNNU_API`。

## 仓库架构（2026-09-18 用户定稿，勿再放回 crates/）

- `logic/`（按业务功能分 crate）：`core/`（模型与错误）、`crypto/`、`ocr/`、
  `api/`（教务客户端）、`service/`（UI 无关应用服务层，`HubService` +
  `ServiceError`，**各 UI 的唯一入口**）
- `ui/`（按平台分实现）：`desktop/`（gnnuhub-desktop，Tauri 2），
  将来 CLI / 移动端等放 `ui/` 下新目录。
- **分层纪律**：`ui/` 只依赖 `gnnuhub-service`（及其 re-export 的 model），
  不得直接依赖 `gnnuhub-api`/`gnnuhub-core`。

## 硬性约束（用户明确要求）

- **控制请求量**（用户因 IP 被封强烈不满过）：每轮对话最多 ~20 次网络请求；
  发现被拦（连接被关闭、域名拒连）立即停止。
- **优先离线逆向**：探接口先 `cargo run -p gnnuhub-api --example js_recon`
  抓一次前端资源，之后离线分析，不反复打接口。
- **一次只改一个变量**；真实账号测试已授权（学号 `250710078`）。
- 密码只从环境变量读，**绝不写进代码或提交**。

## 工具链与代码规范

- stable 工具链；`rustfmt.toml` 不能放 nightly-only 选项。
- 提交前三件套（clippy 必须带 `-D warnings`，否则 lint 不全暴露）：
  `cargo test --workspace --all-targets` /
  `cargo clippy --workspace --all-targets -- -D warnings` /
  `cargo fmt --all --check`
- 注释与文档一律中文；`Session` 所有 HTTP 出口走私有 `send()`。
- `/tmp` 是 10M tmpfs，构建前必须 `export TMPDIR=$PWD/.scratch/tmp`；
  临时脚本放项目内 `.scratch/`（已 gitignore）。

## 关键事实（逆向 + 实测，均已验证）

- **SSO 交换 3 跳，必须在 `/xtgl/login_slogin.html` 之前停止**，继续跟随会被
  服务端断 TLS。后续请求须同时带 `JSESSIONID` + `SF_cookie_17`；
  `rememberMe=deleteMe` 必须剔除。`/sso` 作用域的 JSESSIONID 只是中转，
  最终用根作用域那份。`cas_headers()` 的 Host 不可跨域复用。
- `csrftoken` 对课表/学籍查询不需要。`xqm` = 3（第一学期）/ 12（第二学期）。
- **HTTP 栈**：reqwest + native-tls（系统 OpenSSL）；UA 诚实标识
  `gnnuhub/<版本>`（实验证明 UA/头组合无关）。不要开 `cookie_store(true)`：
  会与手动 COOKIE 头叠加出两个 JSESSIONID 导致断连。
- **🔴 历史教训（2026-09-19）**：`Session::send` 曾写死 `http.get(url)`，
  所有 POST 实际是 GET+body——课表接口 null 的真因，14 组对照实验被
  单一混杂变量污染。**对照实验必须先验证已知成功基底仍成功**。
- **正方权限过滤器**：查询串显式带 `gnmkdm` 即可，无需模拟 Referer。
- **学籍 JSON 接口（已切换）**：POST `/xsxxxggl/xsxxwh_cxCkDgxsxx.html?gnmkdm=N100801`，
  最小表单 `{xh_id, fromXh_id:""}`，64 键响应 = HTML 字段 id 去 `col_` 前缀。
- **移动端课表接口可用**：`xskbcxMobile_cxXsKb` POST（xnm/xqm/zs/doType=app/
  kblx/xh），客户端过滤方案已上线暂不切换，按周远程查询可兜底。
- **正方 jqGrid 通用规律**：数据接口常是菜单页 + `?doType=query`；
  必带 `zd_fzdm=<模块码>-<角色>`（学生 `-xs`）；分页参数被框架重映射
  `rows→queryModel.showCount`、`page→queryModel.currentPage`，无 `sidx/sord`。
  菜单 URL ≠ 数据接口，要进对应 JS 确认；学生侧不发 `xh_id`（传了被忽略）。
- **CAS 业务码**（真实抓包原文）：`PASSERROR`（data 为「剩余次数,已用次数"）
  → 密码错；`USERLOCK` → 账号锁定**禁止重试**；`CODEFALSE` → 验证码错。
  走 `data.code` 不是 `meta.statusCode`。**错密码 3 次即锁号，正确密码
  永不进计数**——量准确率只跑正确密码模式。
- 成绩查询：该账号恒定返回固定空信封，已停止调参（探针
  `example probe_grade`，module doc 列了已排除的 7 种假设）。
  判定「接口是否被调坏」用非法值对照比较信封指纹（已实现
  `envelope_fingerprint()`）。及格判定只能用 `bfzcj`。

## 验证码识别（2026-09-18 定稿）

- **100×25 RGBA，4 位，`0-9A-Za-z` 大小写混用**，开抗锯齿，有噪点。
  服务端字体 = DejaVu Sans 常规体字号约 18。字形渲染**确定性**：
  同一 (字符,字号) 逐像素相同 → **位图查表是唯一识别路径**。
- **只有一条识别路径（定稿）**：已删除 tesseract、ManualOcr、FailoverOcr、
  ChainOcr。位图查表实测 100%（72/72，含模糊匹配兜住 4/400 变体），
  **未命中就报错，绝不猜**——猜错会拿去登录白耗一次尝试。
  `Client::login(student_id, password, ocr)`；`BitmapOcr::recognize(image_base64)`。
- 字库：70 条覆盖 62 字符，`logic/ocr/assets/bitmap_lib.json`（内嵌资产）。
  `captcha_samples/` 下同名 json 只是副本，代码只读内嵌资产库。
  错误信息可粘贴：`Error::Image` 带完整宽高位串，直接补进 json。
  识别失败**不消耗重试**（本地问题），该重试的是 `CODEFALSE`。
- 提取要点：按颜色就近+保留边缘（非严格相等）；噪点是独立连通块，
  保留 ≥8 像素可分离 4 字符；**但 i/j 的点是小块，必须按 x 重叠 +
  主干上方合并回**，否则 i/l 混淆；槽位固定 `(0,20)(21,46)(47,73)(74,99)`。
- **诊断信号**：字形级 100% 但整图偏低 = 字库漏字符或标错，不是提取器坏。
  同一字符有 1~4 种抗锯齿抖动变体（`Q/Y/e/u/v/w/x/m` 各 2 条）。
- **定标纪律（0/O 事故教训）**：绝不用最近邻当标签证据，至少两重独立证据
  （几何高度硬约束：cap-height y0=5/h13、x-height y0=8/h10、ascender y0=4/h14、
  降部底 y=22；距离排名要有断层；tesseract 整图读数需几何交叉否决）。
  新字形入库必须配回归测试点名位串（`tests/real_samples.rs`）。
- 字库空间 Chao1 估计 ≈67（基于 60 样本的下界参考）。长尾收割：跑
  `ocr_verify` 后查 `kind=="miss"` 条目。

## 踩过的坑

- 字段 id 极易数错：政治面貌是 `col_zzmmm`（两个 z）。一律
  `grep -o 'id="col_[^"]*"'` 机器提取，`tests/real_page_check.rs` 守着。
- 灰度化要复现 PIL（若将来再接 OCR 引擎）：BT.601 系数 + 四舍五入
  `(...+500)/1000`，有回归测试守着。
- 不要靠肉眼标 8~13px 点阵（失败三次）；放大 ASCII 图后 ≥8x13 可靠，
  仅 I/l（w≤3 窄竖条）不可判——DejaVu Sans 下 I 与 l 逐像素相同。
- 沙箱里 python heredoc 易被破坏，写脚本文件再执行。

## 待办

- 学籍字段扩展（JSON 源 64 键，对照表 `tools/js-recon/out/verification.md`）
- `fetch_basic_info` / `timetable` JSON 化候选
- 成绩查询卡在账号数据上（非代码问题）

## 2026-09-21 代码优化重构

见当日日志。核心：api crate 按域拆模块（`model`/`endpoints`/`parse` 内分
`student`/`schedule`/`exam`/`misc` 等），公共 HTTP 参数构造收敛，
examples 全部按用途归目录，删除已废弃探针。行为不变，三件套全绿。
