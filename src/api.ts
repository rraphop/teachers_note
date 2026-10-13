import { invoke } from "@tauri-apps/api/core";
import type {
  Contact,
  NewSince,
  Memo,
  MemoColor,
  TodayLayout,
  AppInfo,
  AppSettings,
  ArchiveOptions,
  ArchiveResult,
  NewYearOptions,
  NewYearPreview,
  NewYearResult,
  YearInfo,
  TtEntry,
  RecordTab,
  StatusRow,
  FinalsExport,
  BackupCounts,
  MirrorStatus,
  BackupFile,
  BellConfig,
  BellSchedule,
  CopyResult,
  PeriodSlot,
  RosterField,
  TimetableCell,
  TimetableKind,
  TimetableView,
  TodayTimetable,
  CalendarItem,
  CalendarView,
  ClassInfo,
  DeviceInfo,
  DraftCheck,
  Duty,
  DutyBoard,
  DutyInput,
  DutyLog,
  DutyLogInput,
  EventInput,
  EventItem,
  ExportOptions,
  ExportResult,
  ImportResult,
  NeisImportResult,
  MealsView,
  Briefing,
  NeisStatus,
  SchoolInfo,
  SyncReport,
  TtColors,
  NoteTrack,
  ModelsView,
  Note,
  NoteDetail,
  NoteFilter,
  Observation,
  ObservationInput,
  Plan,
  ResolvedDate,
  RestoreOutcome,
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
  openQuickWindow: () => call<void>("open_quick_window"),
  closeQuickWindow: () => call<void>("close_quick_window"),
  quitApp: () => call<void>("quit_app"),
  listNotes: (filter: NoteFilter) => call<Note[]>("list_notes", { filter }),
  noteDetail: (id: number) => call<NoteDetail>("get_note_detail", { id }),
  updateNoteRaw: (id: number, raw: string) => call<Note>("update_note_raw", { id, raw }),
  deleteNote: (id: number) => call<void>("delete_note", { id }),
  setNoteTrack: (id: number, track: NoteTrack | null) => call<void>("set_note_track", { id, track }),
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
  importNeisRoster: (paths: string[]) => call<NeisImportResult>("import_neis_roster", { paths }),

  // 학생부 자료
  studentRecord: (studentId: number) => call<RecordTab[]>("student_record", { studentId }),
  recordStatus: (area: string, subject: string) => call<StatusRow[]>("record_status", { area, subject }),
  recordGather: (studentId: number, area: string, subject: string) => call<RecordTab>("record_gather", { studentId, area, subject }),
  recordAiDraft: (studentId: number, area: string, subject: string) => call<RecordTab>("record_ai_draft", { studentId, area, subject }),
  saveRecordText: (studentId: number, area: string, subject: string, body: string, finalText: boolean) =>
    call<RecordTab>("save_record_text", { studentId, area, subject, body, finalText }),
  checkRecordText: (studentId: number, area: string, subject: string, body: string) => call<DraftCheck>("check_record_text", { studentId, area, subject, body }),
  exportRecordFinals: (classId: number | null, path: string) => call<FinalsExport>("export_record_finals", { classId, path }),
  unassignedObservations: () => call<Observation[]>("unassigned_observations"),
  createObservation: (input: ObservationInput) => call<number>("create_observation", { input }),
  updateObservation: (id: number, input: ObservationInput) => call<void>("update_observation", { id, input }),
  deleteObservation: (id: number) => call<void>("delete_observation", { id }),


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

  // 학급명렬표
  setStudentField: (id: number, field: RosterField, value: string) => call<Student>("set_student_field", { id, field, value }),

  // 시간표
  currentSemester: () => call<{ school_year: number; semester: number }>("current_semester"),
  getBell: (year: number, semester: number) => call<BellSchedule>("get_bell", { year, semester }),
  computeBell: (config: BellConfig) => call<PeriodSlot[]>("compute_bell", { config }),
  saveBell: (year: number, semester: number, config: BellConfig, periods: PeriodSlot[] | null) =>
    call<BellSchedule>("save_bell", { year, semester, config, periods }),
  getTimetable: (year: number, semester: number, kind: TimetableKind, classLabel: string | null) =>
    call<TimetableView>("get_timetable", { year, semester, kind, classLabel }),
  classTimetables: (year: number, semester: number) => call<string[]>("class_timetables", { year, semester }),
  setTimetableCell: (year: number, semester: number, kind: TimetableKind, classLabel: string | null, cell: TimetableCell) =>
    call<void>("set_timetable_cell", { year, semester, kind, classLabel, cell }),
  addSchoolRow: (year: number, label: string) => call<string>("add_school_row", { year, label }),
  deleteTimetable: (year: number, semester: number, kind: TimetableKind, classLabel: string | null) =>
    call<void>("delete_timetable", { year, semester, kind, classLabel }),
  copySemester: (fromYear: number, fromSemester: number, toYear: number, toSemester: number) =>
    call<CopyResult>("copy_semester", { fromYear, fromSemester, toYear, toSemester }),
  todayTimetable: () => call<TodayTimetable>("today_timetable"),

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
  setAlwaysOnTop: (enabled: boolean) => call<boolean>("set_always_on_top", { enabled }),
  // 기록 속 개인정보 → 학생 명렬표
  resolveProfileChange: (id: number, accept: boolean, replaceId: number | null) => call<void>("resolve_profile_change", { id, accept, replaceId }),
  answerNoteQuestion: (id: number, choice: "record" | "roster") => call<void>("answer_note_question", { id, choice }),
  setStudentContacts: (studentId: number, contacts: Contact[]) => call<Contact[]>("set_student_contacts", { studentId, contacts }),
  // 메뉴의 새 항목
  markSeen: (menu: "records" | "duties" | "tasks" | "students") => call<string | null>("mark_seen", { menu }),
  newSince: (since: string) => call<NewSince>("new_since", { since }),
  setWindowTransparency: (value: number) => call<number>("set_window_transparency", { value }),
  // 오늘 화면 메모·칸 배치
  listMemos: () => call<Memo[]>("list_memos"),
  createMemo: (color?: MemoColor) => call<Memo>("create_memo", { color: color ?? null }),
  updateMemo: (id: number, patch: { body?: string; color?: MemoColor }) => call<Memo>("update_memo", { id, body: patch.body ?? null, color: patch.color ?? null }),
  deleteMemo: (id: number) => call<void>("delete_memo", { id }),
  restoreMemo: (id: number) => call<Memo>("restore_memo", { id }),
  shiftMemo: (id: number, dir: -1 | 1) => call<Memo[]>("shift_memo", { id, dir }),
  getTodayLayout: () => call<TodayLayout>("get_today_layout"),
  setTodayLayout: (layout: TodayLayout) => call<TodayLayout>("set_today_layout", { layout }),
  testNotification: () => call<string>("test_notification"),
  openFolder: (which: "data" | "models" | "backups" | "mirror" | "logs" | "licenses") => call<void>("open_folder", { which }),
  licenseTexts: () => call<{ license: string; notices: string }>("license_texts"),
  neisSearchSchools: (name: string) => call<SchoolInfo[]>("neis_search_schools", { name }),
  neisStatus: () => call<NeisStatus>("neis_status"),
  neisSync: (what: "timetable" | "schedule" | "classes" | "all") => call<SyncReport[]>("neis_sync", { what }),
  getMeals: (date: string) => call<MealsView>("get_meals", { date }),
  todayBriefing: () => call<Briefing>("today_briefing"),
  refreshBriefing: () => call<Briefing>("refresh_briefing"),
  timetableColors: () => call<TtColors>("timetable_colors"),
  setTimetableColor: (scope: "cell" | "subject" | "teacher" | "class" | "room", target: string, color: string | null) => call<TtColors>("set_timetable_color", { scope, target, color }),
  listBackups: () => call<BackupFile[]>("list_backups"),
  backupNow: () => call<string>("backup_now"),
  inspectBackup: (name: string, location?: string) => call<BackupCounts>("inspect_backup", { name, location: location ?? null }),
  requestRestore: (name: string, location?: string) => call<BackupCounts>("request_restore", { name, location: location ?? null }),
  backupMirrorStatus: () => call<MirrorStatus>("backup_mirror_status"),
  setBackupMirror: (dir: string | null) => call<MirrorStatus>("set_backup_mirror", { dir }),
  takeRestoreOutcome: () => call<RestoreOutcome | null>("take_restore_outcome"),
  exportData: (options: ExportOptions) => call<ExportResult>("export_data", { options }),
  revealExport: (path: string) => call<void>("reveal_export", { path }),
  ttConflicts: () => call<TtEntry[]>("tt_conflicts"),
  resolveTtConflict: (id: number, replace: boolean) => call<TtEntry>("resolve_tt_conflict", { id, replace }),
  schoolYears: () => call<YearInfo[]>("school_years"),
  makeYearArchive: (options: ArchiveOptions) => call<ArchiveResult>("make_year_archive", { options }),
  newYearPreview: (newYear: number | null) => call<NewYearPreview>("new_year_preview", { newYear }),
  startNewYear: (options: NewYearOptions) => call<NewYearResult>("start_new_year", { options }),
  requestWipe: (confirmText: string) => call<boolean>("request_wipe", { request: { confirm_text: confirmText } }),
  cancelWipe: () => call<void>("cancel_wipe"),
  restartApp: () => call<void>("restart_app"),
  frontendReady: () => call<void>("frontend_ready"),
};
