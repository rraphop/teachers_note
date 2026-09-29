import { invoke } from "@tauri-apps/api/core";
import type {
  AppInfo,
  AppSettings,
  AreaBlock,
  BackupFile,
  CalendarItem,
  CalendarView,
  ClassInfo,
  DeviceInfo,
  Draft,
  DraftCheck,
  Duty,
  DutyBoard,
  DutyInput,
  DutyLog,
  DutyLogInput,
  EventInput,
  EventItem,
  ImportResult,
  ModelsView,
  Note,
  NoteDetail,
  NoteFilter,
  Observation,
  ObservationInput,
  Plan,
  ResolvedDate,
  RuntimeStatus,
  SearchQuery,
  SearchResult,
  Student,
  StudentDetail,
  StudentInput,
  Task,
  TaskInput,
  TodayOverview,
  VerifyResult,
} from "./types";

/** 백엔드 오류는 쉬운 한국어 문자열로 온다 */
export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  try {
    return JSON.stringify(e);
  } catch {
    return "알 수 없는 오류";
  }
}

const call = <T>(cmd: string, args?: Record<string, unknown>) => invoke<T>(cmd, args);

export const api = {
  // 기록
  createNote: (raw: string) => call<Note>("create_note", { raw }),
  listNotes: (filter: NoteFilter) => call<Note[]>("list_notes", { filter }),
  noteDetail: (id: number) => call<NoteDetail>("get_note_detail", { id }),
  updateNoteRaw: (id: number, raw: string) => call<Note>("update_note_raw", { id, raw }),
  deleteNote: (id: number) => call<void>("delete_note", { id }),
  reclassifyNote: (id: number) => call<void>("reclassify_note", { id }),
  reclassifyUnclassified: () => call<number>("reclassify_unclassified"),
  suggestPlan: (id: number) => call<Plan>("suggest_plan", { id }),
  savePlan: (id: number, plan: Plan) => call<Note>("save_plan", { id, plan }),
  resolveDate: (expr: string, base: string | null) => call<ResolvedDate | null>("resolve_date_expr", { expr, base }),

  // 학생
  listStudents: (query: string | null, classId: number | null) => call<Student[]>("list_students", { query, classId }),
  listClasses: () => call<ClassInfo[]>("list_classes"),
  studentDetail: (id: number) => call<StudentDetail>("get_student_detail", { id }),
  createStudent: (input: StudentInput) => call<number>("create_student", { input }),
  updateStudent: (id: number, input: StudentInput) => call<void>("update_student", { id, input }),
  deleteStudent: (id: number) => call<void>("delete_student", { id }),
  mergeStudents: (fromId: number, intoId: number) => call<void>("merge_students", { fromId, intoId }),
  importRoster: (text: string) => call<ImportResult>("import_roster", { text }),

  // 학생부 자료
  recordOverview: (studentId: number) => call<AreaBlock[]>("record_overview", { studentId }),
  unassignedObservations: () => call<Observation[]>("unassigned_observations"),
  createObservation: (input: ObservationInput) => call<number>("create_observation", { input }),
  updateObservation: (id: number, input: ObservationInput) => call<void>("update_observation", { id, input }),
  deleteObservation: (id: number) => call<void>("delete_observation", { id }),
  createDraftTemplate: (studentId: number, area: string, observationIds: number[]) =>
    call<Draft>("create_draft_template", { studentId, area, observationIds }),
  generateDraftAi: (studentId: number, area: string, observationIds: number[]) =>
    call<Draft>("generate_draft_ai", { studentId, area, observationIds }),
  updateDraft: (id: number, body: string, reviewed: boolean) => call<Draft>("update_draft", { id, body, reviewed }),
  deleteDraft: (id: number) => call<void>("delete_draft", { id }),
  checkDraft: (draftId: number, body: string) => call<DraftCheck>("check_draft_text", { draftId, body }),

  // 업무
  listDuties: () => call<Duty[]>("list_duties"),
  createDuty: (input: DutyInput) => call<number>("create_duty", { input }),
  updateDuty: (id: number, input: DutyInput) => call<void>("update_duty", { id, input }),
  deleteDuty: (id: number) => call<void>("delete_duty", { id }),
  dutyBoard: (dutyId: number | null) => call<DutyBoard>("duty_board", { dutyId }),
  createDutyLog: (input: DutyLogInput) => call<number>("create_duty_log", { input }),
  updateDutyLog: (id: number, input: DutyLogInput) => call<DutyLog>("update_duty_log", { id, input }),
  setDutyLogDone: (id: number, done: boolean) => call<void>("set_duty_log_done", { id, done }),
  deleteDutyLog: (id: number) => call<void>("delete_duty_log", { id }),
  getDutyLog: (id: number) => call<DutyLog>("get_duty_log", { id }),

  // 할 일
  listTasks: (state: "open" | "done" | "all", dutyId: number | null = null) =>
    call<Task[]>("list_tasks", { filter: { state, duty_id: dutyId } }),
  createTask: (input: TaskInput) => call<Task>("create_task", { input }),
  updateTask: (id: number, input: TaskInput) => call<Task>("update_task", { id, input }),
  setTaskDone: (id: number, done: boolean) => call<void>("set_task_done", { id, done }),
  deleteTask: (id: number) => call<void>("delete_task", { id }),
  getTask: (id: number) => call<Task>("get_task", { id }),

  // 일정
  calendar: (view: CalendarView, start: string, end: string) => call<CalendarItem[]>("calendar", { view, start, end }),
  getEvent: (id: number) => call<EventItem>("get_event", { id }),
  createEvent: (input: EventInput) => call<number>("create_event", { input }),
  updateEvent: (id: number, input: EventInput) => call<EventItem>("update_event", { id, input }),
  deleteEvent: (id: number) => call<void>("delete_event", { id }),

  // 오늘·검색
  today: () => call<TodayOverview>("today_overview"),
  search: (query: SearchQuery) => call<SearchResult>("search", { query }),

  // AI 모델·엔진
  modelCatalog: () => call<ModelsView>("model_catalog"),
  startDownload: (id: string) => call<void>("start_download", { id }),
  pauseDownload: (id: string) => call<void>("pause_download", { id }),
  cancelDownload: (id: string) => call<void>("cancel_download", { id }),
  selectModel: (id: string) => call<void>("select_model", { id }),
  deleteModel: (id: string) => call<void>("delete_model", { id }),
  verifyModel: (id: string) => call<VerifyResult>("verify_model", { id }),
  importModelFile: (path: string) => call<string>("import_model_file", { path }),
  aiAvailable: () => call<boolean>("ai_available"),
  runtimeStatus: () => call<RuntimeStatus>("runtime_status"),
  runtimeStart: () => call<void>("runtime_start"),
  runtimeStop: () => call<void>("runtime_stop"),
  deviceInfo: (refresh: boolean) => call<DeviceInfo>("device_info", { refresh }),

  // 설정·데이터
  appInfo: () => call<AppInfo>("app_info"),
  getSettings: () => call<AppSettings>("get_settings"),
  updateSettings: (value: AppSettings) => call<AppSettings>("update_settings", { value }),
  setAutostart: (enabled: boolean) => call<boolean>("set_autostart", { enabled }),
  testNotification: () => call<string>("test_notification"),
  openFolder: (which: "data" | "models" | "backups" | "logs") => call<void>("open_folder", { which }),
  listBackups: () => call<BackupFile[]>("list_backups"),
  backupNow: () => call<string>("backup_now"),
  requestWipe: (confirmText: string) => call<boolean>("request_wipe", { request: { confirm_text: confirmText } }),
  cancelWipe: () => call<void>("cancel_wipe"),
  restartApp: () => call<void>("restart_app"),
  frontendReady: () => call<void>("frontend_ready"),
};
