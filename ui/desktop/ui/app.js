/* GNNUHub 前端逻辑（无框架，直接调用 Tauri 命令） */
"use strict";

const { invoke } = window.__TAURI__.core;

/* ---------- 小工具 ---------- */

const $ = (id) => document.getElementById(id);

function setStatus(text, isError = false) {
  const bar = document.querySelector(".statusbar");
  $("status-text").textContent = text;
  bar.classList.toggle("error", isError);
}

function showError(id, message) {
  const el = $(id);
  el.textContent = message;
  el.classList.remove("hidden");
}

/**
 * 包一层"状态栏提示 + 错误上浮"。
 *
 * 注意：这里**没有全局互斥锁**——登录后会并发拉学籍与课表，
 * 之前的布尔锁会把后到的调用静默丢弃（表现为登录后课表空白、
 * 点查询前毫无动静）。防重复点击由各按钮自行 disabled。
 */
async function withBusy(label, fn) {
  setStatus(label + "…");
  try {
    return await fn();
  } catch (e) {
    setStatus(String(e), true);
    throw e;
  }
}

/** 把错误显示到某个面板区域内，比底部状态栏醒目得多 */
function showPanelError(panelId, message) {
  const panel = $(panelId);
  let tip = panel.querySelector(".error-tip");
  if (!tip) {
    tip = document.createElement("div");
    tip.className = "error-tip";
    panel.prepend(tip);
  }
  tip.textContent = "加载失败：" + message;
}

function clearPanelError(panelId) {
  document.getElementById(panelId)?.querySelectorAll(".error-tip").forEach((t) => t.remove());
}

/** 执行期间禁用按钮，防止重复点击重复打接口 */
async function withButton(btn, fn) {
  if (btn.disabled) return;
  btn.disabled = true;
  try {
    return await fn();
  } finally {
    btn.disabled = false;
  }
}

/** 证件号打码：保留前 6 后 4 */
function maskId(text) {
  if (!text || text.length < 11) return text || "—";
  return text.slice(0, 6) + "********" + text.slice(-4);
}

/* ---------- 课表渲染 ---------- */

const WEEKDAYS = ["星期一", "星期二", "星期三", "星期四", "星期五", "星期六", "星期日"];
const PERIOD_ROWS = ["1-2 节", "3-4 节", "5-6 节", "7-8 节", "9-11 节"];

/** 节次起点 → 网格行号（0..4）；畸形节次落回第 0 行 */
function periodRow(start) {
  const row = Math.floor((Math.max(start, 1) - 1) / 2);
  return Math.min(Math.max(row, 0), PERIOD_ROWS.length - 1);
}

/** 给每门课一个稳定的配色（按课程名哈希取色相） */
function courseHue(name) {
  let h = 0;
  for (const ch of name) h = (h * 31 + ch.codePointAt(0)) % 360;
  return h;
}

function renderSchedule(schedule) {
  const grid = $("schedule-grid");
  grid.innerHTML = "";

  // 表头
  grid.appendChild(Object.assign(document.createElement("div"), { className: "head", textContent: "节次" }));
  for (const day of WEEKDAYS) {
    grid.appendChild(Object.assign(document.createElement("div"), { className: "head", textContent: day }));
  }

  // 空网格
  const cells = Array.from({ length: PERIOD_ROWS.length * 7 }, () => []);
  let total = 0;

  for (const [name, entries] of Object.entries(schedule.courses ?? {})) {
    for (const entry of entries) {
      const col = WEEKDAYS.indexOf(entry.time.weekday);
      if (col < 0) continue; // 未知星期文本，跳过
      const row = periodRow(entry.time.periods.start);
      cells[row * 7 + col].push({ name, entry });
      total += 1;
    }
  }

  for (let r = 0; r < PERIOD_ROWS.length; r++) {
    grid.appendChild(Object.assign(document.createElement("div"), {
      className: "period-label", textContent: PERIOD_ROWS[r],
    }));
    for (let c = 0; c < 7; c++) {
      const cell = document.createElement("div");
      cell.className = "cell";
      for (const { name, entry } of cells[r * 7 + c]) {
        const card = document.createElement("div");
        card.className = "course";
        card.style.setProperty("--course-hue", courseHue(name));
        card.style.background = `hsl(${courseHue(name)}, 52%, 46%)`;

        const title = document.createElement("div");
        title.textContent = name;
        const room = document.createElement("div");
        room.className = "room";
        room.textContent = [entry.building, entry.position].filter(Boolean).join(" ") || "地点待定";
        const weeks = document.createElement("div");
        weeks.className = "weeks";
        weeks.textContent = entry.time.weeks;

        card.append(title, room, weeks);
        cell.appendChild(card);
      }
      grid.appendChild(cell);
    }
  }

  if (total === 0) {
    const tip = document.createElement("div");
    tip.className = "empty-tip";
    tip.textContent = "该学期暂无课程记录";
    grid.appendChild(tip);
  }
}

function fillWeekOptions() {
  const sel = $("sel-week");
  sel.innerHTML = '<option value="0">整学期</option>';
  for (let w = 1; w <= 25; w++) {
    const opt = document.createElement("option");
    opt.value = String(w);
    opt.textContent = `第 ${w} 周`;
    sel.appendChild(opt);
  }
}

/* ---------- 考试安排 ---------- */

/** 把 "2026-07-07(14:30-16:30)" 拆成日期与时间段（容错：拆不开就整段当日期） */
function splitExamTime(text) {
  const m = /^(.*?)\((.*?)\)\s*$/.exec(text || "");
  if (!m) return [text || "—", ""];
  return [m[1] || "—", m[2]];
}

function renderExams(exams) {
  const wrap = $("exam-list");
  wrap.innerHTML = "";

  if (!exams.length) {
    wrap.innerHTML = '<div class="empty-tip">该学期暂无考试安排</div>';
    return;
  }

  const table = document.createElement("table");
  table.className = "exam-table";
  table.innerHTML =
    "<thead><tr><th>课程</th><th>考试批次</th><th>日期</th><th>时间</th>" +
    "<th>地点</th><th>方式</th><th>教师</th><th>补考</th></tr></thead>";
  const tbody = document.createElement("tbody");

  for (const exam of exams) {
    const [date, clock] = splitExamTime(exam.exam_time);
    const tr = document.createElement("tr");
    if (exam.is_make_up) tr.classList.add("makeup");

    const cells = [
      [exam.course_name, exam.course_code],
      [exam.exam_name],
      [date],
      [clock],
      [exam.location, exam.campus],
      [exam.exam_mode],
      [(exam.teacher || "").split("/").pop() || exam.teacher],
      [exam.is_make_up ? "是" : "否"],
    ];
    for (const [main, sub] of cells) {
      const td = document.createElement("td");
      td.textContent = main || "—";
      if (sub) {
        const small = document.createElement("div");
        small.className = "sub";
        small.textContent = sub;
        td.appendChild(small);
      }
      tr.appendChild(td);
    }
    tbody.appendChild(tr);
  }

  table.appendChild(tbody);
  wrap.appendChild(table);
}

async function loadExams() {
  const year = Number($("sel-exam-year").value);
  const term = Number($("sel-exam-term").value);

  await withBusy("查询考试安排", async () => {
    clearPanelError("tab-exams");
    const exams = await invoke("exam_schedule", { year, term });
    renderExams(exams);
    setStatus(`考试安排已更新（${exams.length} 场）`);
  });
}

/* ---------- 空闲教室（学期+周次+节次） ---------- */

/** 初始化周次（1-25）与节次（1-11）下拉 */
function initRoomSelectors() {
  const week = $("sel-room-week");
  if (!week.options.length) {
    for (let w = 1; w <= 25; w++) {
      const o = document.createElement("option");
      o.value = String(w);
      o.textContent = `第 ${w} 周`;
      week.appendChild(o);
    }
    week.value = "3"; // 当前周附近，用户可改
  }
  for (const sel of [$("sel-room-jc-start"), $("sel-room-jc-end")]) {
    if (sel.options.length) continue;
    for (let j = 1; j <= 11; j++) {
      const o = document.createElement("option");
      o.value = String(j);
      o.textContent = `第 ${j} 节`;
      sel.appendChild(o);
    }
  }
  $("sel-room-jc-start").value = "1";
  $("sel-room-jc-end").value = "11";
}

/** 学期起始周一（UTC 日期，避免时区偏移） */
function termStartUtc(cal) {
  const [y, m, d] = cal.start_date.split("-").map(Number);
  return new Date(Date.UTC(y, m - 1, d));
}

/** 第 w 周的周一与周日（YYYY-MM-DD，UTC 计算） */
function weekDateRange(cal, w) {
  const base = termStartUtc(cal);
  const monday = new Date(base.getTime() + (w - 1) * 7 * 86400000);
  const sunday = new Date(monday.getTime() + 6 * 86400000);
  const fmt = (d) => d.toISOString().slice(0, 10);
  return { qssj: fmt(monday), jssj: fmt(sunday) };
}

/** 节次起止时段（timetable[0..n] 的 start/end，补秒） */
function periodTimeRange(periods, jcStart, jcEnd) {
  const first = periods[Math.min(jcStart, periods.length) - 1];
  const last = periods[Math.min(jcEnd, periods.length) - 1];
  return {
    qssd: (first?.start ?? "08:00") + ":00",
    jssd: (last?.end ?? "21:25") + ":00",
  };
}

async function loadRooms() {
  const year = Number($("sel-room-year").value);
  const term = Number($("sel-room-term").value);
  const week = Number($("sel-room-week").value);
  const jcStart = Number($("sel-room-jc-start").value);
  const jcEnd = Number($("sel-room-jc-end").value);

  await withBusy("查询空闲教室", async () => {
    clearPanelError("tab-rooms");

    // 校历：学期起始日 → 周次换算日期（校历组件只提供当前学期；
    // 选择历史学期时按其学期名近似换算，起始日沿用当前校历）
    const cal = await invoke("academic_calendar");
    const range = weekDateRange(cal, week);

    // 节次时间表：节次 → 时段
    const periods = await invoke("timetable", { year, term }).catch(() => []);
    const times = periodTimeRange(periods, jcStart, jcEnd);

    $("room-range").textContent = `第 ${week} 周（${range.qssj} ~ ${range.jssj}）· 第 ${jcStart}-${jcEnd} 节（${times.qssd.slice(0, 5)} ~ ${times.jssd.slice(0, 5)}）`;

    const rooms = await invoke("free_rooms", {
      year, term,
      qssj: range.qssj,
      jssj: range.jssj,
      qssd: times.qssd,
      jssd: times.jssd,
    });

    fillRoomCategories(rooms);
    const cat = $("sel-room-cat").value;
    const filtered = cat ? rooms.filter((r) => r.category === cat) : rooms;
    renderRooms(filtered);
    setStatus(`空闲教室已更新（${filtered.length} 间 / 共 ${rooms.length} 间）`);
  });
}

/** 填充类别下拉（首次查询后按数据去重生成） */
function fillRoomCategories(rooms) {
  const sel = $("sel-room-cat");
  if (sel.options.length > 1) return;
  const cats = [...new Set(rooms.map((r) => r.category).filter(Boolean))].sort();
  for (const c of cats) {
    const opt = document.createElement("option");
    opt.value = c;
    opt.textContent = c;
    sel.appendChild(opt);
  }
}

/** 渲染教室表格 */
function renderRooms(rooms) {
  const wrap = $("room-list");
  wrap.innerHTML = "";
  if (!rooms.length) {
    wrap.innerHTML = '<div class="empty-tip">该时段暂无空闲教室</div>';
    return;
  }
  const table = document.createElement("table");
  table.className = "exam-table";
  table.innerHTML =
    "<thead><tr><th>场地</th><th>类别</th><th>归属</th><th>教学楼</th><th>座位</th></tr></thead>";
  const tbody = document.createElement("tbody");
  for (const r of rooms) {
    const tr = document.createElement("tr");
    const cells = [
      [r.name, r.code],
      [r.category],
      [r.campus],
      [r.building],
      [r.seats ? String(r.seats) : "—"],
    ];
    for (const [main, sub] of cells) {
      const td = document.createElement("td");
      td.textContent = main || "—";
      if (sub) {
        const small = document.createElement("div");
        small.className = "sub";
        small.textContent = sub;
        td.appendChild(small);
      }
      tr.appendChild(td);
    }
    tbody.appendChild(tr);
  }
  table.appendChild(tbody);
  wrap.appendChild(table);
}

/* ---------- 学业情况（GPA） ---------- */

async function loadAcademic() {
  await withBusy("获取学业情况", async () => {
    clearPanelError("tab-academic");
    const items = await invoke("gpa_summary");
    const cards = $("academic-cards");
    cards.innerHTML = "";

    const addCard = (k, v) => {
      const card = document.createElement("div");
      card.className = "card";
      card.innerHTML = `<div class="k">${k}</div><div class="v">${v}</div>`;
      cards.appendChild(card);
    };

    // 校历卡片：学期起止 + 推算当前周（比 this_week 的 HTML 解析更稳）
    let calNote = "—";
    try {
      const cal = await invoke("academic_calendar");
      const start = new Date(cal.start_date);
      const days = Math.floor((Date.now() - start) / 86400000);
      const week = days >= 0 ? Math.floor(days / 7) + 1 : null;
      calNote = `${cal.start_date} ~ ${cal.end_date}` + (week ? `（第 ${week} 周）` : "（未开始）");
    } catch { /* 校历失败不影响主卡片 */ }

    addCard("总加权 GPA", GpaSummary_overall(items).toFixed(2));
    addCard("已获学分合计", GpaSummary_total(items).toFixed(1));
    addCard("本学期", calNote);

    const wrap = $("academic-table");
    wrap.innerHTML = "";
    if (!items.length) {
      wrap.innerHTML = '<div class="empty-tip">暂无学业数据</div>';
    } else {
      const table = document.createElement("table");
      table.className = "exam-table";
      table.innerHTML =
        "<thead><tr><th>课程性质</th><th>GPA</th><th>获得学分</th></tr></thead>";
      const tbody = document.createElement("tbody");
      for (const it of items) {
        const tr = document.createElement("tr");
        for (const v of [it.nature_name, it.gpa.toFixed(2), it.earned_credit.toFixed(1)]) {
          const td = document.createElement("td");
          td.textContent = v;
          tr.appendChild(td);
        }
        tbody.appendChild(tr);
      }
      table.appendChild(tbody);
      wrap.appendChild(table);
    }
    setStatus("学业情况已更新");
  });
}

/** 学分加权总 GPA（与后端 overall_gpa 一致） */
function GpaSummary_overall(items) {
  let total = 0, credits = 0;
  for (const it of items) {
    total += it.gpa * it.earned_credit;
    credits += it.earned_credit;
  }
  return credits > 0 ? total / credits : 0;
}
function GpaSummary_total(items) {
  return items.reduce((s, it) => s + it.earned_credit, 0);
}

/* ---------- 通知与系统消息 ---------- */

async function loadNotices() {
  await withBusy("获取通知与消息", async () => {
    clearPanelError("tab-notices");
    // 系统提醒消息与通知一起拉，消息横幅置顶展示
    const [notices, messages] = await Promise.all([
      invoke("notices"),
      invoke("system_messages").catch(() => []),
    ]);

    const banner = $("msg-banner");
    if (messages.length) {
      banner.textContent = "系统提醒：" + messages.join("　|　");
      banner.classList.remove("hidden");
    } else {
      banner.classList.add("hidden");
    }

    const wrap = $("notice-list");
    wrap.innerHTML = "";
    if (!notices.length) {
      wrap.innerHTML = '<div class="empty-tip">暂无通知</div>';
    } else {
      const table = document.createElement("table");
      table.className = "exam-table";
      table.innerHTML =
        "<thead><tr><th>标题</th><th>栏目</th><th>日期</th></tr></thead>";
      const tbody = document.createElement("tbody");
      for (const n of notices) {
        const tr = document.createElement("tr");
        tr.style.cursor = "pointer";
        tr.title = "点击查看正文";
        if (n.pinned) tr.classList.add("pinned");

        const titleTd = document.createElement("td");
        titleTd.textContent = (n.pinned ? "【置顶】" : "") + n.title;
        const catTd = document.createElement("td");
        catTd.textContent = n.category || "—";
        const dateTd = document.createElement("td");
        dateTd.textContent = n.date || "—";
        tr.append(titleTd, catTd, dateTd);

        // 点击行：展开/收起正文（全程状态跟踪，任何一步失败都可见）
        tr.addEventListener("click", async () => {
          try {
            setStatus("已触发通知点击");
            const existing = tr.nextElementSibling;
            if (existing && existing.classList.contains("notice-detail-row")) {
              existing.remove();
              setStatus("已收起正文");
              return;
            }
            const detailRow = document.createElement("tr");
            detailRow.className = "notice-detail-row";
            const detailTd = document.createElement("td");
            detailTd.colSpan = 3;
            detailTd.textContent = "加载中…";
            // 必须先把 td 挂进 tr 再插入 DOM——孤儿节点的内容永远不会显示
            detailRow.appendChild(detailTd);
            tr.after(detailRow);
            setStatus("调用 notice_content…");

            const d = await invoke("notice_content", { detailPath: n.detail_path });
            setStatus(`正文返回 ${d.content.length} 字，渲染中…`);

            detailTd.innerHTML =
              `<div class="notice-meta">${d.publisher} · ${d.publish_time} · 浏览 ${d.views}</div>` +
              `<div class="notice-body"></div>`;
            detailTd.querySelector(".notice-body").textContent = d.content;
            setStatus("正文已加载");
          } catch (e) {
            setStatus("点击处理错误：" + (e?.message ?? e), true);
          }
        });

        tbody.appendChild(tr);
      }
      table.appendChild(tbody);
      wrap.appendChild(table);
    }
    // 版本标记：用于确认前端资源是否随构建更新（点击展开正文为 v2 特性）
    setStatus(`通知已更新（${notices.length} 条）· 通知模块 v2（点击行查看正文）`);
  });
}

/* ---------- 重修课程 ---------- */

function renderRetakes(courses) {
  const wrap = $("retake-list");
  wrap.innerHTML = "";

  if (!courses.length) {
    wrap.innerHTML = '<div class="empty-tip">暂无重修课程记录</div>';
    return;
  }

  const table = document.createElement("table");
  table.className = "exam-table";
  table.innerHTML =
    "<thead><tr><th>课程</th><th>学分</th><th>开课学院</th><th>类别</th>" +
    "<th>重修学年</th><th>学期</th><th>状态</th></tr></thead>";
  const tbody = document.createElement("tbody");

  for (const c of courses) {
    const tr = document.createElement("tr");
    const cells = [
      [c.course_name, c.course_code],
      [String(c.credit)],
      [c.college],
      [c.category],
      [c.academic_year],
      [c.semester === "1" ? "第一学期" : c.semester === "2" ? "第二学期" : c.semester],
      [c.retake_status || "—"],
    ];
    for (const [main, sub] of cells) {
      const td = document.createElement("td");
      td.textContent = main || "—";
      if (sub) {
        const small = document.createElement("div");
        small.className = "sub";
        small.textContent = sub;
        td.appendChild(small);
      }
      tr.appendChild(td);
    }
    tbody.appendChild(tr);
  }

  table.appendChild(tbody);
  wrap.appendChild(table);
}

async function loadRetakes() {
  const year = Number($("sel-retake-year").value);
  const term = Number($("sel-retake-term").value);

  await withBusy("查询重修课程库", async () => {
    clearPanelError("tab-retakes");
    const courses = await invoke("retake_courses", { year, term });
    renderRetakes(courses);
    setStatus(`重修课程库已更新（${courses.length} 条）`);
  });
}

/* ---------- 学籍渲染 ---------- */

const PROFILE_FIELDS = [
  ["student_id", "学号", (v) => v],
  ["name", "姓名", (v) => v],
  ["gender", "性别", (v) => v],
  ["identity", "身份", (v) => v],
  ["college", "学院", (v) => v],
  ["major", "专业", (v) => v],
  ["class_name", "班级", (v) => v],
  ["instructor", "辅导员", (v) => v],
  ["enrollment_year", "入学年份", (v) => v],
  ["birthday", "出生日期", (v) => v],
  ["ethnicity", "民族", (v) => v],
  ["political_status", "政治面貌", (v) => v],
  ["address", "联系地址", (v) => v],
];

function renderProfile(info) {
  const wrap = $("profile-cards");
  wrap.innerHTML = "";

  const add = (k, v) => {
    const card = document.createElement("div");
    card.className = "card";
    const key = document.createElement("div");
    key.className = "k";
    key.textContent = k;
    const val = document.createElement("div");
    val.className = "v";
    val.textContent = v;
    card.append(key, val);
    wrap.appendChild(card);
  };

  for (const [field, label, fmt] of PROFILE_FIELDS) {
    add(label, info[field] ? fmt(info[field]) : "—");
  }

  if (info.document) {
    add("证件类型", info.document.kind || "—");
    add("证件号码", maskId(info.document.number));
  }
}

/* ---------- 视图切换 ---------- */

function showMain(studentId) {
  $("view-login").classList.add("hidden");
  $("view-main").classList.remove("hidden");
  $("user-badge").textContent = studentId;
}

function showLogin() {
  $("view-main").classList.add("hidden");
  $("view-login").classList.remove("hidden");
  $("in-password").value = "";
}

async function loadProfile() {
  await withBusy("加载学籍信息", async () => {
    clearPanelError("tab-profile");
    renderProfile(await invoke("student_info"));
    setStatus("学籍信息已加载");
  });
}

async function loadSchedule() {
  const year = Number($("sel-year").value);
  const term = Number($("sel-term").value);
  const week = Number($("sel-week").value);

  await withBusy(week ? "查询第 " + week + " 周课表" : "查询整学期课表", async () => {
    clearPanelError("tab-schedule");
    const schedule = await invoke("class_schedule", {
      year, term,
      week: week > 0 ? week : null, // null → 后端 None = 整学期
    });
    renderSchedule(schedule);
    setStatus(week ? `第 ${week} 周课表（${schedule.academic_term?.year ?? year} 学年）已更新`
                   : "整学期课表已更新");
  });
}

/* ---------- 事件绑定 ---------- */

$("btn-login").addEventListener("click", async () => {
  $("login-error").classList.add("hidden");
  const studentId = $("in-student-id").value.trim();
  const password = $("in-password").value;

  if (!studentId || !password) {
    return showError("login-error", "请填写学号与密码。");
  }

  $("btn-login").disabled = true;
  $("btn-login").textContent = "登录中（验证码自动识别）…";
  try {
    const id = await invoke("login", { studentId, password });
    showMain(id);
    setStatus("登录成功，正在拉取信息…");
    // 串行加载：课表依赖的请求节流在同一个客户端上，串行更符合
    // "一次只做一件事"的 WAF 礼仪；错误就地显示在各自面板里
    await loadProfile().catch((e) => showPanelError("tab-profile", String(e)));
    await loadSchedule().catch((e) => showPanelError("tab-schedule", String(e)));
    setStatus("就绪");
  } catch (e) {
    showError("login-error", String(e));
  } finally {
    $("btn-login").disabled = false;
    $("btn-login").textContent = "登 录";
  }
});

$("in-password").addEventListener("keydown", (ev) => {
  if (ev.key === "Enter") $("btn-login").click();
});

$("btn-load-schedule").addEventListener("click", (ev) =>
    withButton(ev.currentTarget, () => loadSchedule()).catch(() => {}));

$("btn-load-exams").addEventListener("click", (ev) =>
    withButton(ev.currentTarget, () => loadExams()).catch(() => {}));

$("btn-load-retakes").addEventListener("click", (ev) =>
    withButton(ev.currentTarget, () => loadRetakes()).catch(() => {}));

$("btn-load-notices").addEventListener("click", (ev) =>
    withButton(ev.currentTarget, () => loadNotices()).catch(() => {}));

$("btn-load-rooms").addEventListener("click", (ev) =>
    withButton(ev.currentTarget, () => loadRooms()).catch(() => {}));

$("sel-room-cat").addEventListener("change", () => {
  // 类别切换：仅前端过滤，需要重查时手动点查询
  loadRooms().catch(() => {});
});

$("btn-this-week").addEventListener("click", async () => {
  try {
    const week = await invoke("this_week");
    $("sel-week").value = String(week);
    await loadSchedule();
  } catch (e) {
    setStatus(String(e), true);
  }
});

$("btn-logout").addEventListener("click", async () => {
  try { await invoke("logout"); } catch { /* 会话已丢弃即可 */ }
  showLogin();
  setStatus("已退出登录");
});

// tab 切换；首次进入学籍页时若尚未加载过则自动加载
document.querySelectorAll(".tab").forEach((btn) => {
  btn.addEventListener("click", () => {
    document.querySelectorAll(".tab").forEach((b) => b.classList.remove("active"));
    btn.classList.add("active");
    document.querySelectorAll(".tab-panel").forEach((p) => p.classList.add("hidden"));
    $(`tab-${btn.dataset.tab}`).classList.remove("hidden");

    if (btn.dataset.tab === "profile" && !$("profile-cards").childElementCount) {
      loadProfile().catch(() => {});
    }
    if (btn.dataset.tab === "exams" && !$("exam-list").childElementCount) {
      loadExams().catch(() => {});
    }
    if (btn.dataset.tab === "retakes" && !$("retake-list").childElementCount) {
      loadRetakes().catch(() => {});
    }
    if (btn.dataset.tab === "notices" && !$("notice-list").childElementCount) {
      loadNotices().catch(() => {});
    }
    if (btn.dataset.tab === "academic" && !$("academic-table").childElementCount) {
      loadAcademic().catch(() => {});
    }
    if (btn.dataset.tab === "rooms" && !$("room-list").childElementCount) {
      initRoomSelectors();
      loadRooms().catch(() => {});
    }
  });
});

/* ---------- 初始化 ---------- */

fillWeekOptions();

// 兜底诊断：任何未捕获的错误都要可见，而不是"按了没反应"
window.addEventListener("unhandledrejection", (ev) => {
  setStatus("未处理的错误：" + (ev.reason?.message ?? ev.reason), true);
});
window.addEventListener("error", (ev) => {
  setStatus("脚本错误：" + ev.message, true);
});
