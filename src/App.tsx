import { listen } from "@tauri-apps/api/event";
import { useEffect, useState, type ReactNode } from "react";
import { api } from "./api";
import { EditorHost } from "./components/Editors";
import {
  IconCalendar,
  IconDuty,
  IconNote,
  IconRecords,
  IconSearch,
  IconSettings,
  IconStudents,
  IconTask,
  IconToday,
} from "./components/Icons";
import { ConfirmHost } from "./components/Modal";
import { QuickCapture } from "./components/QuickCapture";
import { useLoad, useStore, type PageId } from "./lib/store";
import { CalendarPage } from "./pages/CalendarPage";
import { DutiesPage } from "./pages/DutiesPage";
import { NotesPage } from "./pages/NotesPage";
import { RecordsPage } from "./pages/RecordsPage";
import { SearchPage } from "./pages/SearchPage";
import { SettingsPage } from "./pages/SettingsPage";
import { StudentsPage } from "./pages/StudentsPage";
import { TasksPage } from "./pages/TasksPage";
import { TodayPage } from "./pages/TodayPage";
import type { RuntimeState } from "./types";

const MENU: Array<{ id: PageId; label: string; icon: ReactNode }> = [
  { id: "today", label: "오늘", icon: <IconToday /> },
  { id: "calendar", label: "일정표", icon: <IconCalendar /> },
  { id: "notes", label: "기록", icon: <IconNote /> },
  { id: "students", label: "학생", icon: <IconStudents /> },
  { id: "records", label: "학생부 자료", icon: <IconRecords /> },
  { id: "duties", label: "업무", icon: <IconDuty /> },
  { id: "tasks", label: "할 일", icon: <IconTask /> },
  { id: "search", label: "검색", icon: <IconSearch /> },
  { id: "settings", label: "설정", icon: <IconSettings /> },
];

function AiStatus() {
  const { navigate } = useStore();
  const cat = useLoad(() => api.modelCatalog(), []);
  const [rt, setRt] = useState<RuntimeState | null>(null);
  useEffect(() => {
    api.runtimeStatus().then((s) => setRt(s.state)).catch(() => undefined);
    let un: (() => void) | null = null;
    let alive = true;
    void listen<RuntimeState>("runtime-state", (e) => setRt(e.payload)).then((u) => (alive ? (un = u) : u()));
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
  const sub = !sel ? "기록은 저장됩니다" : rt?.state === "ready" ? "AI 준비됨" : rt?.state === "starting" ? "AI 켜는 중…" : rt?.state === "failed" ? "AI 문제 — 눌러서 확인" : "필요할 때 자동으로 켜짐";
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

function Badges({ page }: { page: PageId }) {
  const ov = useLoad(() => api.today(), []);
  if (!ov.data) return null;
  if (page === "notes" && ov.data.unclassified_count > 0) return <span className="nav-badge">{ov.data.unclassified_count}</span>;
  if (page === "today" && ov.data.overdue.length > 0) return <span className="nav-badge danger">{ov.data.overdue.length}</span>;
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
                {(m.id === "notes" || m.id === "today") && <Badges page={m.id} />}
              </button>
            </li>
          ))}
        </ul>
        <div className="sidebar-foot">
          <AiStatus />
          <p className="local-only">🔒 이 PC 안에서만 처리</p>
        </div>
      </nav>
      <main className="main">
        <QuickCapture />
        <div className="content">
          {page === "today" && <TodayPage />}
          {page === "calendar" && <CalendarPage />}
          {page === "notes" && <NotesPage />}
          {page === "students" && <StudentsPage />}
          {page === "records" && <RecordsPage />}
          {page === "duties" && <DutiesPage />}
          {page === "tasks" && <TasksPage />}
          {page === "search" && <SearchPage />}
          {page === "settings" && <SettingsPage />}
        </div>
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
    </div>
  );
}
