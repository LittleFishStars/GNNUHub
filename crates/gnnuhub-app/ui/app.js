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

/** 给"查询中"按钮上锁，防止重复点击重复打接口 */
let busy = false;
async function withBusy(label, fn) {
  if (busy) return;
  busy = true;
  setStatus(label + "…");
  try {
    return await fn();
  } catch (e) {
    setStatus(String(e), true);
    throw e;
  } finally {
    busy = false;
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
    renderProfile(await invoke("student_info"));
    setStatus("学籍信息已加载");
  });
}

async function loadSchedule() {
  const year = Number($("sel-year").value);
  const term = Number($("sel-term").value);
  const week = Number($("sel-week").value);

  await withBusy(week ? "查询第 " + week + " 周课表" : "查询整学期课表", async () => {
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
    await Promise.allSettled([loadProfile(), loadSchedule()]);
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

$("btn-load-schedule").addEventListener("click", () => loadSchedule().catch(() => {}));

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
  });
});

/* ---------- 初始化 ---------- */

fillWeekOptions();
