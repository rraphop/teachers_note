// 1.0 E2E: 오늘 화면 칸 배치(자동 1~4열로 꽉 채우기, 직접 배치: 화살표·끌기·열 수·넓게·숨기기, 다시 켜도 기억, 좁으면 접기),
//          메모(윈도우 스티커 메모처럼 날이 바뀌어도 남는 메모: 여러 장·색·자동 저장·순서·지우기와 되돌리기)
// 실행: node scripts/e2e/run-v10.mjs <exe> <데이터폴더> <스크린샷폴더>
import { execFileSync } from "node:child_process";
import fs from "node:fs/promises";
import path from "node:path";
import { connect, launch, sleep, waitFor } from "./cdp.mjs";

const [exe, dataRoot, shots] = process.argv.slice(2);
await fs.mkdir(shots, { recursive: true });
const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok: !!ok, detail });
  console.log(`${ok ? "OK " : "NG "} ${name}${detail ? " — " + detail : ""}`);
};
const pad = (n) => String(n).padStart(2, "0");
const iso = (d) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const now = new Date();
const plus = (n) => iso(new Date(now.getFullYear(), now.getMonth(), now.getDate() + n));

// 화면 크기 바꾸기 (창 크기 대신 보이는 영역을 바꾼다)
const view = async (app, width, height = 1000) => {
  await app.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
  await sleep(500);
};
const board = (app) =>
  app.evaluate(`(() => {
    const b = document.querySelector('.board');
    const m = document.querySelector('.main');
    const cols = [...document.querySelectorAll('.board-col')].map(col => [...col.querySelectorAll(':scope > .board-slot')].map(s => s.dataset.panel));
    const wide = [...document.querySelectorAll('.board-wide > .board-slot')].map(s => s.dataset.panel);
    const br = b.getBoundingClientRect();
    const bottoms = [...document.querySelectorAll('.board-col')].map(col => {
      const slots = [...col.querySelectorAll(':scope > .board-slot')].filter(s => s.offsetHeight > 0);
      const last = slots[slots.length - 1];
      return last ? Math.round(last.getBoundingClientRect().bottom) : null;
    }).filter(x => x !== null);
    return { mode: b.dataset.mode, n: Number(b.dataset.columns), cols, wide, rightGap: Math.round(m.getBoundingClientRect().right - br.right), width: Math.round(br.width), bottoms };
  })()`);
const autoN = (w) => (w < 700 ? 1 : w < 1250 ? 2 : w < 1700 ? 3 : 4);
const strip = (app, panel, label) => app.evaluate(`document.querySelector('.board-slot[data-panel="${panel}"] .edit-strip button[aria-label="${label}"]').click(), true`);
const stripText = (app, panel, text) => app.evaluate(`[...document.querySelectorAll('.board-slot[data-panel="${panel}"] .edit-strip button')].find(b => b.innerText.trim() === ${JSON.stringify(text)}).click(), true`);
const layoutOf = (app) => app.invoke("get_today_layout");

// 창 상태 읽기 (Win32): 계층 창 여부·알파·맨 위. capture 이면 창 가운데 300×200 을 화면에서 읽어
// 불투명(A)·지금(B)·창을 잠깐 숨긴 뒤 뒤 화면(C)을 비교한다 (B ≈ A·α + C·(1-α) 이면 실제로 비쳐 보이는 것).
// 화면에서 읽은 값은 숫자로만 계산하고 파일로 남기지 않는다 (뒤에 있는 다른 창 내용이 들어갈 수 있다).
const winState = (pid, mode = "") => {
  const ps = `
Add-Type -AssemblyName System.Drawing
Add-Type -Name W -Namespace U -MemberDefinition @'
[DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
[DllImport("user32.dll")] public static extern bool GetLayeredWindowAttributes(IntPtr h, out uint key, out byte alpha, out uint flags);
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
public struct RECT { public int L; public int T; public int R; public int B; }
'@
$h = (Get-Process -Id ${pid}).MainWindowHandle
$ex = [U.W]::GetWindowLong($h, -20)
$k = 0; $a = [byte]0; $f = 0
$ok = [U.W]::GetLayeredWindowAttributes($h, [ref]$k, [ref]$a, [ref]$f)
function Grab() {
  $r = New-Object U.W+RECT; [void][U.W]::GetWindowRect($h, [ref]$r)
  $x = [int](($r.L + $r.R) / 2 - 150); $y = [int](($r.T + $r.B) / 2 - 100)
  $bmp = New-Object System.Drawing.Bitmap 300, 200
  $g = [System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($x, $y, 0, 0, $bmp.Size); $g.Dispose()
  $v = New-Object System.Collections.Generic.List[int]
  for ($i = 5; $i -lt 300; $i += 10) { for ($j = 5; $j -lt 200; $j += 10) { $c = $bmp.GetPixel($i, $j); $v.Add($c.R); $v.Add($c.G); $v.Add($c.B) } }
  $bmp.Dispose(); return ,$v.ToArray()
}
$out = @{ layered = [bool]($ex -band 0x80000); topmost = [bool]($ex -band 8); attr = $ok; alpha = [int]$a; flags = [int]$f }
if ('${mode}' -eq 'grab') { $out.px = Grab }
if ('${mode}' -eq 'blend') {
  $out.px = Grab
  [void][U.W]::ShowWindow($h, 0); Start-Sleep -Milliseconds 400
  $out.behind = Grab
  [void][U.W]::ShowWindow($h, 8); Start-Sleep -Milliseconds 300
}
$out | ConvertTo-Json -Compress -Depth 3
`;
  return JSON.parse(execFileSync("powershell", ["-NoProfile", "-Command", ps], { encoding: "utf8" }).trim());
};
const mean = (v) => v.reduce((a, b) => a + b, 0) / Math.max(1, v.length);

async function open(dataRootDir) {
  const child = launch(exe, { dataRoot: dataRootDir });
  const app = await connect(9333);
  await waitFor(() => app.evaluate(`!!document.querySelector('.capture textarea') && !!document.querySelector('.board')`), { label: "첫 화면" });
  return { child, app };
}

let { child, app } = await open(dataRoot);
try {
  const info = await app.invoke("app_info");
  check("버전 1.0.x", /^1\.0\.\d+$/.test(info.version), info.version);

  // ── 준비: 할 일·일정 ──
  await app.invoke("create_task", { input: { title: "수행평가 채점", due_date: plus(0) } });
  await app.invoke("create_task", { input: { title: "가정통신문 회수", due_date: plus(2) } });
  await app.invoke("create_event", { input: { layer: "annual", title: "1차 지필평가", start_date: plus(3) } });
  await app.nav("일정표");
  await sleep(300);
  await app.nav("오늘");
  await waitFor(() => app.evaluate(`(document.querySelector('#today-tasks')?.innerText || '').includes('가정통신문')`), { label: "할 일" });

  // ── 1. 자동 배치: 화면 너비에 맞춰 열 수, 오른쪽 빈 곳 없음, 열 바닥 맞춤 ──
  const sizes = [
    [1920, 3],
    [2400, 4],
    [1280, 2],
    [760, 1],
  ];
  for (const [w, want] of sizes) {
    await view(app, w);
    const b = await waitFor(async () => {
      const x = await board(app);
      return x.n === want ? x : null;
    }, { label: `${w}px 화면 ${want}열`, timeout: 5000 }).catch(() => null);
    const got = b ?? (await board(app));
    check(`자동 배치: 화면 ${w}px → ${want}열`, got.mode === "auto" && got.n === want && got.n === autoN(got.width), `열 ${got.n}, 칸 너비 ${got.width}px`);
    if (w >= 1280) {
      check(`  ${w}px: 오른쪽에 남는 빈 곳 없음`, got.rightGap <= 40, `오른쪽 여백 ${got.rightGap}px`);
      check(`  ${w}px: 열마다 마지막 칸이 같은 높이에서 끝남`, got.bottoms.length === got.n && Math.max(...got.bottoms) - Math.min(...got.bottoms) <= 2, JSON.stringify(got.bottoms));
      check(`  ${w}px: 오늘의 기록은 맨 아래 넓은 줄`, JSON.stringify(got.wide) === '["notes"]', JSON.stringify(got.wide));
    }
    if (w === 1920) {
      check("  3열 자리: 할 일·메모 / 업무 요약·시간표 / 급식", JSON.stringify(got.cols.map((c) => c.filter((p) => p !== "extras"))) === JSON.stringify([["tasks", "memo"], ["briefing", "timetable"], ["meals"]]), JSON.stringify(got.cols));
      await app.screenshot(path.join(shots, "90-today-auto-1920.png"));
    }
    if (w === 1280) await app.screenshot(path.join(shots, "91-today-auto-1280.png"));
  }

  // ── 2. 메모 ──
  await view(app, 1920);
  await app.click("새 메모", "#today-memo button");
  await waitFor(() => app.evaluate(`document.querySelectorAll('#today-memo .memo-card').length === 1`), { label: "새 메모" });
  check("[새 메모] → 메모 한 장, 바로 쓸 수 있게 커서", await app.evaluate(`document.activeElement?.classList.contains('memo-text') === true`), "");
  const m1Text = "3교시 전 프린터 토너 확인\n학부모 상담 자료 출력";
  await app.evaluate(`window.__e2e.setValue(document.querySelector('#today-memo .memo-text'), ${JSON.stringify(m1Text)})`);
  // 저장 전에 다른 자료가 바뀌어(화면 다시 읽기) 쓰던 글이 지워지면 안 된다
  await app.invoke("create_task", { input: { title: "연수 신청", due_date: plus(1) } });
  await sleep(300);
  check("쓰는 중에 화면을 다시 읽어도 쓴 글은 그대로", (await app.evaluate(`document.querySelector('#today-memo .memo-text').value`)) === m1Text, "");
  const saved = await waitFor(async () => {
    const v = await app.invoke("list_memos");
    return v[0]?.body === m1Text ? v : null;
  }, { label: "메모 자동 저장", timeout: 5000 }).catch(() => null);
  check("쓰는 대로 자동 저장 (따로 저장 버튼 없음)", !!saved, "");
  const m1 = saved?.[0];
  await app.evaluate(`document.querySelector('#today-memo .memo-card button[aria-label="분홍 메모"]').click(), true`);
  await waitFor(async () => ((await app.invoke("list_memos"))[0]?.color === "pink" ? true : null), { label: "색 저장", timeout: 4000 }).catch(() => null);
  const m1b = (await app.invoke("list_memos"))[0];
  check("메모 색 바꾸기 (6가지 중 분홍)", m1b?.color === "pink" && (await app.evaluate(`document.querySelector('#today-memo .memo-card').classList.contains('memo-pink')`)), m1b?.color);
  await app.click("새 메모", "#today-memo button");
  await waitFor(() => app.evaluate(`document.querySelectorAll('#today-memo .memo-card').length === 2`), { label: "두 번째 메모" });
  await app.evaluate(`window.__e2e.setValue(document.querySelector('#today-memo .memo-text'), '급식 지도 당번 (목)')`);
  await app.evaluate(`document.querySelector('#today-memo .memo-text').blur(), true`);
  const two = await waitFor(async () => {
    const v = await app.invoke("list_memos");
    return v.length === 2 && v[0].body === "급식 지도 당번 (목)" ? v : null;
  }, { label: "두 번째 메모 저장", timeout: 5000 });
  check("새 메모는 맨 앞", two[1].id === m1.id, two.map((m) => m.body.split("\n")[0]).join(" | "));
  await app.evaluate(`document.querySelector('#today-memo .memo-card button[aria-label="뒤로"]').click(), true`);
  const moved = await waitFor(async () => {
    const v = await app.invoke("list_memos");
    return v[0]?.id === m1.id ? v : null;
  }, { label: "순서 바꾸기", timeout: 4000 }).catch(() => null);
  check("메모 순서 바꾸기 (뒤로)", !!moved, "");
  const m2id = two[0].id;
  await app.evaluate(`document.querySelector('#today-memo .memo-card[data-memo="${m2id}"] button[aria-label="메모 지우기"]').click(), true`);
  await waitFor(() => app.evaluate(`!!document.querySelector('#today-memo .memo-undo')`), { label: "되돌리기 안내" });
  check("메모 지우기 → 목록에서 빠지고 [되돌리기] 안내", (await app.invoke("list_memos")).length === 1, await app.text("#today-memo .memo-undo"));
  await app.click("되돌리기", "#today-memo .memo-undo button");
  // 화면에도 두 장이 다시 보일 때까지 기다린다
  const back = await waitFor(async () => {
    const v = await app.invoke("list_memos");
    const cards = await app.evaluate(`document.querySelectorAll('#today-memo .memo-card').length`);
    return v.length === 2 && cards === 2 ? v : null;
  }, { label: "되돌리기", timeout: 5000 }).catch(() => null);
  const backM2 = back?.find((m) => m.id === m2id);
  check("[되돌리기] → 글까지 그대로 돌아옴", backM2?.body === "급식 지도 당번 (목)" && !(await app.evaluate(`!!document.querySelector('#today-memo .memo-undo')`)), JSON.stringify(backM2 ?? null));
  await app.evaluate(`document.getElementById('today-memo').scrollIntoView({ block: 'center' }); true`);
  await sleep(200);
  await app.screenshot(path.join(shots, "92-today-memo.png"));
  await app.evaluate(`window.scrollTo(0, 0); true`);

  // ── 3. 직접 배치 ──
  await app.click("칸 배치", ".page-head button");
  await waitFor(() => app.evaluate(`!!document.querySelector('.board-bar')`), { label: "칸 배치 막대" });
  const strips = await app.evaluate(`[document.querySelectorAll('.board-slot').length, document.querySelectorAll('.edit-strip').length]`);
  check("[칸 배치] → 칸마다 옮기기 막대 (⠿ ← ↑ ↓ → 넓게 숨기기)", strips[0] > 0 && strips[0] === strips[1], JSON.stringify(strips));
  await strip(app, "tasks", "오른쪽 열로");
  await sleep(500);
  let lay = await layoutOf(app);
  let b = await board(app);
  check("화살표로 옮기면 직접 배치로 (할 일 → 둘째 열)", lay.mode === "custom" && lay.columns[1].includes("tasks") && b.cols[1].includes("tasks"), JSON.stringify(lay.columns));
  await strip(app, "tasks", "위로");
  await sleep(400);
  lay = await layoutOf(app);
  check("위로 옮기기 (둘째 열 맨 위)", lay.columns[1][0] === "tasks", JSON.stringify(lay.columns[1]));
  await app.click("4", '.board-bar [aria-label="열 수"] button');
  await sleep(500);
  lay = await layoutOf(app);
  b = await board(app);
  check("열 수 4로", lay.columns.length === 4 && b.n === 4, `${lay.columns.length} / 화면 ${b.n}`);

  // 끌어서 옮기기: 메모를 맨 아래 넓은 줄의 맨 앞으로
  const drag = await app.evaluate(`(() => {
    const h = document.querySelector('.board-slot[data-panel="memo"] .drag-handle').getBoundingClientRect();
    const w = document.querySelector('.board-wide').getBoundingClientRect();
    return { hx: h.left + h.width / 2, hy: h.top + h.height / 2, tx: w.left + 60, ty: w.top + 4 };
  })()`);
  await app.evaluate(`(() => {
    const h = document.querySelector('.board-slot[data-panel="memo"] .drag-handle');
    h.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, clientX: ${drag.hx}, clientY: ${drag.hy}, pointerId: 1, button: 0 }));
    window.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, clientX: ${drag.tx}, clientY: ${drag.ty}, pointerId: 1 }));
    return true;
  })()`);
  await sleep(150);
  const markShown = await app.evaluate(`!!document.querySelector('.board-wide > .drop-mark')`);
  await app.evaluate(`window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, clientX: ${drag.tx}, clientY: ${drag.ty}, pointerId: 1 })), true`);
  await sleep(500);
  lay = await layoutOf(app);
  check("⠿ 끌어서 옮기기: 메모 → 맨 아래 넓은 줄 (놓을 자리 표시)", markShown && JSON.stringify(lay.wide) === '["memo","notes"]', `${markShown} ${JSON.stringify(lay.wide)}`);
  await stripText(app, "memo", "열로");
  await sleep(500);
  lay = await layoutOf(app);
  check("[열로] → 다시 열 안으로", JSON.stringify(lay.wide) === '["notes"]' && lay.columns.some((c) => c.includes("memo")), JSON.stringify(lay));
  await stripText(app, "meals", "숨기기");
  await sleep(500);
  lay = await layoutOf(app);
  const mealsGone = !(await app.evaluate(`!!document.querySelector('.board-slot[data-panel="meals"]')`));
  check("[숨기기] → 급식 칸 숨김, ‘숨긴 칸’에 표시", mealsGone && lay.hidden.includes("meals") && (await app.text(".board-bar")).includes("급식"), JSON.stringify(lay.hidden));
  await app.evaluate(`[...document.querySelectorAll('.board-hidden button')].find(b => b.innerText.includes('급식')).click(), true`);
  await sleep(500);
  lay = await layoutOf(app);
  check("숨긴 칸 다시 보이기", !lay.hidden.includes("meals") && (await app.evaluate(`!!document.querySelector('.board-slot[data-panel="meals"]')`)), "");
  await app.screenshot(path.join(shots, "93-today-edit.png"));
  await app.click("완료", ".board-bar button");
  await sleep(300);
  check("[완료] → 옮기기 막대가 사라짐", !(await app.evaluate(`!!document.querySelector('.edit-strip') || !!document.querySelector('.board-bar')`)), "");
  const custom = await board(app);

  // ── 3-1. 투명도 (왼쪽 메뉴 ‘항상 위에 표시’ 아래) ──
  const sideOrder = await app.evaluate(`[...document.querySelectorAll('#side-opts > label')].map(l => (l.querySelector('span')?.innerText || l.innerText).trim())`);
  check("‘항상 위에 표시’ 바로 아래 투명도 막대 (0~70%)", sideOrder.join("|") === "윈도우 시작 시 자동 실행|항상 위에 표시|투명도" && (await app.evaluate(`(() => { const r = document.querySelector('#side-opts input[type=range]'); return r.min + '-' + r.max + '/' + r.value; })()`)) === "0-70/0", sideOrder.join("|"));
  // 화면에서 비교하려면 창이 맨 앞에 있어야 한다 → 항상 위에 켜기
  await app.evaluate(`document.querySelectorAll('#side-opts input[type=checkbox]')[1].click(), true`);
  await waitFor(async () => ((await app.invoke("get_settings")).always_on_top ? true : null), { label: "항상 위", timeout: 6000 });
  await sleep(600);
  const opaque = winState(child.pid, "grab");
  check("처음엔 불투명 (계층 창 아님)", !opaque.layered && opaque.topmost, JSON.stringify({ layered: opaque.layered, topmost: opaque.topmost }));
  await app.evaluate(`window.__e2e.setValue(document.querySelector('#side-opts input[type=range]'), '50')`);
  await waitFor(async () => ((await app.invoke("get_settings")).window_transparency === 50 ? true : null), { label: "투명도 50 저장", timeout: 6000 });
  await sleep(500);
  const half = winState(child.pid, "blend");
  check("투명도 50% → 창 알파 128 (Windows 계층 창)", half.layered && half.attr && half.alpha === 128 && (await app.text("#side-opts .side-range b")) === "50%", JSON.stringify({ layered: half.layered, alpha: half.alpha }));
  const grabbed = !!half.px?.length && half.px.length === opaque.px?.length && half.px.length === half.behind?.length;
  if (!grabbed) {
    check("화면에서 실제로 반쯤 비쳐 보임 (교무수첩 내용 + 뒤 화면)", false, "화면을 읽지 못했습니다 (화면 잠금 등)");
  } else {
    // 실제로 비쳐 보이는지: 지금 화면 ≈ 불투명 화면 × 0.5 + 뒤 화면 × 0.5
    const errBlend = mean(half.px.map((v, i) => Math.abs(v - (opaque.px[i] * 128 + half.behind[i] * 127) / 255)));
    const errOpaque = mean(half.px.map((v, i) => Math.abs(v - opaque.px[i])));
    const errBehind = mean(half.px.map((v, i) => Math.abs(v - half.behind[i])));
    check("화면에서 실제로 반쯤 비쳐 보임 (교무수첩 내용 + 뒤 화면)", errBlend < 12 && (errOpaque > errBlend || errBehind > errBlend), `섞인 값과 차이 ${errBlend.toFixed(1)} / 불투명과 ${errOpaque.toFixed(1)} / 뒤 화면과 ${errBehind.toFixed(1)}`);
  }
  await app.evaluate(`window.__e2e.setValue(document.querySelector('#side-opts input[type=range]'), '30')`);
  await waitFor(async () => ((await app.invoke("get_settings")).window_transparency === 30 ? true : null), { label: "투명도 30 저장", timeout: 6000 });
  await app.screenshot(path.join(shots, "95-side-transparency.png"));
  const bad = await app.invoke("set_window_transparency", { value: 95 });
  check("70%보다 크게는 못 함 (창이 안 보이게 되는 일 막기)", bad === 70, String(bad));
  await app.invoke("set_window_transparency", { value: 30 });
  const stale2 = await app.invoke("get_settings");
  await app.invoke("update_settings", { value: { ...stale2, window_transparency: 0 } });
  check("다른 설정을 저장해도 투명도는 그대로", (await app.invoke("get_settings")).window_transparency === 30, "");

  // ── 4. 다시 켜도 배치·메모 그대로 (메모는 날짜와 상관없이 남는다) ──
  app.close();
  child.kill();
  await sleep(1500);
  ({ child, app } = await open(dataRoot));
  await view(app, 1920);
  const again = await waitFor(async () => {
    const x = await board(app);
    return x.mode === "custom" ? x : null;
  }, { label: "다시 켠 뒤 배치", timeout: 8000 }).catch(() => null);
  check("다시 켜도 직접 배치 그대로", !!again && JSON.stringify(again.cols) === JSON.stringify(custom.cols), JSON.stringify(again?.cols));
  const memosAgain = await waitFor(() => app.evaluate(`document.querySelectorAll('#today-memo .memo-card').length === 2 ? [...document.querySelectorAll('#today-memo .memo-text')].map(t => t.value) : null`), { label: "다시 켠 뒤 메모", timeout: 8000 }).catch(() => null);
  check("다시 켜도 메모 그대로 (글·순서)", !!memosAgain && memosAgain[0] === m1Text && memosAgain[1] === "급식 지도 당번 (목)", JSON.stringify(memosAgain));
  const reopened = winState(child.pid);
  const slider = await app.evaluate(`document.querySelector('#side-opts input[type=range]').value`);
  check("다시 켜도 투명도 30%·항상 위 그대로", reopened.layered && reopened.alpha === 179 && reopened.topmost && slider === "30", JSON.stringify({ alpha: reopened.alpha, topmost: reopened.topmost, slider }));
  // 항상 위를 꺼도(창 틀이 스타일을 다시 씀) 투명도는 그대로
  await app.evaluate(`document.querySelectorAll('#side-opts input[type=checkbox]')[1].click(), true`);
  await waitFor(async () => ((await app.invoke("get_settings")).always_on_top ? null : true), { label: "항상 위 끄기", timeout: 6000 });
  await sleep(700);
  const aotOff = winState(child.pid);
  check("항상 위를 꺼도 투명도 30% 그대로", aotOff.layered && aotOff.alpha === 179 && !aotOff.topmost, JSON.stringify({ layered: aotOff.layered, alpha: aotOff.alpha, topmost: aotOff.topmost }));
  await app.evaluate(`window.__e2e.setValue(document.querySelector('#side-opts input[type=range]'), '0')`);
  await waitFor(async () => ((await app.invoke("get_settings")).window_transparency === 0 ? true : null), { label: "투명도 0", timeout: 6000 });
  await sleep(300);
  const back0 = winState(child.pid);
  check("투명도 0% → 다시 불투명 (알파 255)", !back0.layered || back0.alpha === 255, JSON.stringify({ layered: back0.layered, alpha: back0.alpha }));

  // ── 5. 좁은 화면에서는 직접 배치(4열)를 접어 보여 줌 ──
  await view(app, 1280);
  const narrow = await waitFor(async () => {
    const x = await board(app);
    return x.n < 4 ? x : null;
  }, { label: "접기", timeout: 5000 }).catch(() => null);
  check("좁은 화면: 4열 직접 배치를 접어 보여 줌 (칸은 모두 보임)", !!narrow && narrow.n === 2 && narrow.cols.flat().length === custom.cols.flat().length, JSON.stringify(narrow?.cols));
  await app.click("칸 배치", ".page-head button");
  await waitFor(() => app.evaluate(`!!document.querySelector('.board-bar')`), { label: "칸 배치 막대" });
  check("  접었을 때 안내 문구", (await app.text(".board-bar")).includes("접어 보여 줍니다"), "");
  await app.screenshot(path.join(shots, "94-today-folded.png"));

  // ── 6. 자동 배치로 되돌리기 ──
  await app.click("자동 배치", ".board-bar button");
  await sleep(500);
  lay = await layoutOf(app);
  b = await board(app);
  check("[자동 배치] → 처음 배치로", lay.mode === "auto" && b.n === 2 && JSON.stringify(b.cols.map((c) => c.filter((p) => p !== "extras"))) === JSON.stringify([["tasks", "memo", "timetable"], ["briefing", "meals"]]), JSON.stringify(b.cols));
  await app.click("완료", ".board-bar button");
  await app.send("Emulation.clearDeviceMetricsOverride");
  await sleep(300);

  // ── 업데이트 기록 ──
  await app.evaluate(`document.querySelector('.update-log-btn').click()`);
  await waitFor(() => app.evaluate(`!!document.querySelector('.changelog')`), { label: "업데이트 기록" });
  const log = await app.text(".dialog");
  check("업데이트 기록: v1.0 (칸 배치·메모·투명도)", log.includes("v1.0") && log.includes("칸 배치") && log.includes("메모") && log.includes("투명도"), "");
} catch (e) {
  check("E2E 실행", false, e instanceof Error ? e.stack ?? e.message : String(e));
} finally {
  try {
    app?.close();
  } catch {}
  child.kill();
  await fs.writeFile(path.join(shots, "results-v10.json"), JSON.stringify(results, null, 2));
  const failed = results.filter((r) => !r.ok).length;
  console.log(`\n결과: ${results.length - failed}/${results.length} 통과`);
  process.exit(failed ? 1 : 0);
}
