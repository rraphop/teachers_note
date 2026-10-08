import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { api, errorText } from "./api";
import { EditorHost } from "./components/Editors";
import { CHANGELOG, displayVersion } from "./changelog";
import {
  IconCalendar,
  IconDuty,
  IconHistory,
  IconNote,
  IconRecords,
  IconSearch,
  IconSettings,
  IconStudents,
  IconTask,
  IconTimetable,
  IconToday,
} from "./components/Icons";
import { ConfirmHost, Modal } from "./components/Modal";
import { QuickCapture } from "./components/QuickCapture";
import { badgeText } from "./lib/seen";
import { useLoad, useStore, type PageId } from "./lib/store";
import { CalendarPage } from "./pages/CalendarPage";
import { DutiesPage } from "./pages/DutiesPage";
import { NotesPage } from "./pages/NotesPage";
import { RecordsPage } from "./pages/RecordsPage";
import { SearchPage } from "./pages/SearchPage";
import { SettingsPage } from "./pages/SettingsPage";
import { StudentsPage } from "./pages/StudentsPage";
import { TasksPage } from "./pages/TasksPage";
import { TimetablePage } from "./pages/TimetablePage";
import { TodayPage } from "./pages/TodayPage";
import type { RestoreOutcome, RuntimeState } from "./types";

const MENU: Array<{ id: PageId; label: string; icon: ReactNode }> = [
  { id: "today", label: "오늘", icon: <IconToday /> },
  { id: "calendar", label: "일정표", icon: <IconCalendar /> },
  { id: "timetable", label: "시간표", icon: <IconTimetable /> },
  { id: "notes", label: "기록", icon: <IconNote /> },
  { id: "students", label: "학생 명렬표", icon: <IconStudents /> },
  { id: "records", label: "학생부 자료", icon: <IconRecords /> },
  { id: "duties", label: "교무 업무", icon: <IconDuty /> },
  { id: "tasks", label: "할 일", icon: <IconTask /> },
  { id: "search", label: "검색", icon: <IconSearch /> },
  { id: "settings", label: "설정", icon: <IconSettings /> },
];

function AiStatus() {
  const { navigate } = useStore();
  const cat = useLoad(() => api.modelCatalog(), []);
  const [rt, setRt] = useState<RuntimeState | null>(null);
  const [device, setDevice] = useState<string | null>(null);
  useEffect(() => {
    const status = () =>
      api
        .runtimeStatus()
        .then((s) => {
          setRt(s.state);
          setDevice(s.device);
        })
        .catch(() => undefined);
    void status();
    let un: (() => void) | null = null;
    let alive = true;
    void listen<RuntimeState>("runtime-state", (e) => {
      setRt(e.payload);
      void status();
    }).then((u) => (alive ? (un = u) : u()));
    return () => {
      alive = false;
      un?.();
    };
  }, []);
  const sel = cat.data?.entries.find((e) => e.selected && e.installed);
  let text = "AI 모델 없음 — 설치하기";
  let cls = "none";
  if (sel) {
    text = `${sel.model.tier_label} · ${sel.model.name}`;
    cls = rt?.state === "ready" ? "ready" : rt?.state === "starting" ? "starting" : rt?.state === "failed" ? "failed" : "idle";
  }
  const sub = !sel ? "기록은 저장됩니다" : rt?.state === "ready" ? `AI 준비됨${device ? ` · ${device === "CPU" ? "CPU" : "GPU"}` : ""}` : rt?.state === "starting" ? "AI 켜는 중…" : rt?.state === "failed" ? "AI 문제 — 눌러서 확인" : "필요할 때 자동으로 켜짐";
  return (
    <button className={`ai-status ${cls}`} onClick={() => navigate("settings", { section: "models" })}>
      <span className="dot" />
      <span className="ai-text">
        <b>{text}</b>
        <small>{sub}</small>
      </span>
    </button>
  );
}

/** 왼쪽 메뉴 ‘설정’ 바로 아래: 윈도우 시작 시 자동 실행, 항상 위에 표시, 투명도 */
function WindowOptions() {
  const { toast, bump } = useStore();
  const info = useLoad(() => api.appInfo(), []);
  const settings = useLoad(() => api.getSettings(), []);
  const [busy, setBusy] = useState(false);
  // 투명도: 끄는 동안 바로바로 보이게, 앞 요청이 끝나면 마지막 값만 보낸다
  const [alpha, setAlpha] = useState<number | null>(null);
  const pending = useRef<number | null>(null);
  const sending = useRef(false);
  useEffect(() => {
    if (settings.data && !sending.current && pending.current === null) setAlpha(settings.data.window_transparency);
  }, [settings.data]);
  const pushAlpha = (v: number) => {
    setAlpha(v);
    pending.current = v;
    if (sending.current) return;
    sending.current = true;
    void (async () => {
      while (pending.current !== null) {
        const x = pending.current;
        pending.current = null;
        try {
          await api.setWindowTransparency(x);
        } catch (e) {
          toast(errorText(e), "error");
        }
      }
      sending.current = false;
    })();
  };
  const change = async (job: () => Promise<unknown>, done: string) => {
    setBusy(true);
    try {
      await job();
      toast(done);
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setBusy(false);
      info.reload();
      settings.reload();
      bump();
    }
  };
  return (
    <div className="side-opts" id="side-opts">
      <label className="side-check" title="Windows에 로그인하면 교무수첩을 자동으로 켭니다">
        <input
          type="checkbox"
          checked={info.data?.autostart ?? false}
          disabled={!info.data || busy}
          onChange={(e) => {
            const on = e.target.checked;
            void change(() => api.setAutostart(on), on ? "Windows에 로그인하면 교무수첩이 자동으로 켜집니다." : "자동 실행을 껐습니다.");
          }}
        />
        윈도우 시작 시 자동 실행
      </label>
      <label className="side-check" title="교무수첩 창을 다른 프로그램 창보다 항상 위에 둡니다">
        <input
          type="checkbox"
          checked={settings.data?.always_on_top ?? false}
          disabled={!settings.data || busy}
          onChange={(e) => {
            const on = e.target.checked;
            void change(() => api.setAlwaysOnTop(on), on ? "교무수첩 창을 항상 위에 둡니다." : "항상 위에 두기를 껐습니다.");
          }}
        />
        항상 위에 표시
      </label>
      <label className="side-range" title="창을 비쳐 보이게 합니다 (0% = 불투명, 최대 70%)">
        <span>투명도</span>
        <input type="range" min={0} max={70} step={5} value={alpha ?? 0} disabled={alpha === null} onChange={(e) => pushAlpha(Number(e.target.value))} aria-label="창 투명도" />
        <b>{alpha ?? 0}%</b>
      </label>
    </div>
  );
}

/** 왼쪽 아래 업데이트 기록 */
function UpdateLog() {
  const [open, setOpen] = useState(false);
  const info = useLoad(() => api.appInfo(), []);
  const v = displayVersion(info.data?.version ?? "0.0.0");
  return (
    <>
      <button className="update-log-btn" onClick={() => setOpen(true)} title="업데이트 기록 보기">
        <IconHistory size={14} /> 업데이트 기록 <b>v{v}</b>
      </button>
      {open && (
        <Modal title={`업데이트 기록 — 지금 버전 v${v}`} onClose={() => setOpen(false)}>
          <ol className="changelog">
            {CHANGELOG.map((c) => (
              <li key={c.version}>
                <div className="cl-head">
                  <b>로컬 AI 교무수첩 v{c.version}</b> <span className="muted small">{c.date}</span>
                </div>
                {c.summary && <p className="cl-summary">{c.summary}</p>}
                {c.sections && (
                  <ul className="cl-sections">
                    {c.sections.map((sec) => (
                      <li key={sec.title}>
                        <b>{sec.title}:</b> {sec.text}
                      </li>
                    ))}
                  </ul>
                )}
                {c.features && c.features.length > 0 && (
                  <>
                    <h4 className="cl-kind">새 기능</h4>
                    <ul>
                      {c.features.map((it, i) => (
                        <li key={i}>{it}</li>
                      ))}
                    </ul>
                  </>
                )}
                {c.fixes && c.fixes.length > 0 && (
                  <>
                    <h4 className="cl-kind">고친 점</h4>
                    <ul>
                      {c.fixes.map((it, i) => (
                        <li key={i}>{it}</li>
                      ))}
                    </ul>
                  </>
                )}
              </li>
            ))}
          </ol>
        </Modal>
      )}
    </>
  );
}

/** 백업 되돌리기 뒤 다시 시작했을 때 결과를 한 번 알린다 */
function RestoreNotice() {
  const [outcome, setOutcome] = useState<RestoreOutcome | null>(null);
  useEffect(() => {
    api
      .takeRestoreOutcome()
      .then(setOutcome)
      .catch(() => undefined);
  }, []);
  if (!outcome) return null;
  const close = () => setOutcome(null);
  return (
    <Modal
      title={outcome.ok ? "백업으로 되돌렸습니다" : "되돌리지 못했습니다"}
      onClose={close}
      footer={
        <button className="primary" onClick={close}>
          확인
        </button>
      }
    >
      <p className={outcome.ok ? "" : "error-box"}>{outcome.message}</p>
      {outcome.ok && outcome.before && (
        <p className="hint">되돌리기 직전의 데이터는 설정 → 데이터의 백업 목록에 ‘되돌리기 전’으로 남아 있습니다. 필요하면 그 백업으로 다시 돌아갈 수 있습니다.</p>
      )}
    </Modal>
  );
}

function Badges({ page }: { page: PageId }) {
  const ov = useLoad(() => api.today(), []);
  if (!ov.data) return null;
  // 기록: 정리 중·미분류 + 교사가 답해야 할 것(특이사항 '어디에 둘까요?'·개인정보 확인)
  const notesCount = ov.data.unclassified_count + (ov.data.needs_answer_count ?? 0);
  if (page === "notes" && notesCount > 0)
    return (
      <span className="nav-badge pending" title={`정리 중·미분류 ${ov.data.unclassified_count} · 확인 필요 ${ov.data.needs_answer_count ?? 0}`}>
        {badgeText(notesCount)}
      </span>
    );
  if (page === "today" && ov.data.overdue.length > 0) return <span className="nav-badge danger">{ov.data.overdue.length}</span>;
  // 새로 정리된 항목: 초록 동그라미 숫자 (그 메뉴를 열면 사라짐)
  const menu = page === "records" ? "records" : page === "duties" ? "duties" : page === "tasks" ? "tasks" : page === "students" ? "students" : null;
  const n = menu && ov.data.new_counts ? ov.data.new_counts[menu] : 0;
  if (n > 0)
    return (
      <span className="nav-badge new" title={`새로 정리된 항목 ${n}개 — 열어서 확인하면 사라집니다`}>
        {badgeText(n)}
      </span>
    );
  return null;
}

export function App() {
  const { page, navigate, toasts, dismissToast } = useStore();
  useEffect(() => {
    void api.frontendReady().catch(() => undefined);
  }, []);
  return (
    <div className="shell">
      <nav className="sidebar" aria-label="주 메뉴">
        <div className="brand">
          <span className="brand-mark">📒</span>
          <span>
            로컬 AI
            <br />
            교무수첩
          </span>
        </div>
        <ul>
          {MENU.map((m) => (
            <li key={m.id}>
              <button className={page === m.id ? "on" : ""} onClick={() => navigate(m.id)} aria-current={page === m.id ? "page" : undefined}>
                {m.icon}
                <span className="grow">{m.label}</span>
                {["notes", "today", "records", "duties", "tasks", "students"].includes(m.id) && <Badges page={m.id} />}
              </button>
            </li>
          ))}
        </ul>
        <WindowOptions />
        <div className="sidebar-foot">
          <AiStatus />
          <p className="local-only">🔒 이 PC 안에서만 처리</p>
          <UpdateLog />
        </div>
      </nav>
      <main className="main">
        <div className="content">
          {page === "today" && <TodayPage />}
          {page === "calendar" && <CalendarPage />}
          {page === "timetable" && <TimetablePage />}
          {page === "notes" && <NotesPage />}
          {page === "students" && <StudentsPage />}
          {page === "records" && <RecordsPage />}
          {page === "duties" && <DutiesPage />}
          {page === "tasks" && <TasksPage />}
          {page === "search" && <SearchPage />}
          {page === "settings" && <SettingsPage />}
        </div>
        <QuickCapture />
      </main>
      <div className="toasts" role="status" aria-live="polite">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.kind}`} onClick={() => dismissToast(t.id)}>
            {t.text}
          </div>
        ))}
      </div>
      <EditorHost />
      <ConfirmHost />
      <RestoreNotice />
    </div>
  );
}
