// Rust 쪽 DTO 와 같은 모양 (snake_case)

export type Area =
  | "attendance"
  | "subject"
  | "autonomous"
  | "career"
  | "volunteer"
  | "club"
  | "behavior"
  | "free_semester"
  | "daily"
  | "none";

export type ItemKind = "student" | "task" | "event" | "duty" | "memo";
export type ItemOrigin = "ai" | "rule" | "manual";
export type NoteStatus = "pending" | "processing" | "classified" | "failed" | "manual";

export interface ReminderSpec {
  enabled: boolean;
  offsets: number[];
  time: string;
  custom_at: string | null;
}

export interface Chip {
  /** timetable: 시간표 칸에 반영 / tt_conflict: 다른 수업이 있어 확인 필요 / tt_change: 하루짜리 시간표 변경 */
  kind: "student" | "task" | "event" | "duty" | "timetable" | "tt_conflict" | "tt_change";
  label: string;
  area: string | null;
  date: string | null;
  done: boolean;
  ref_id: number | null;
}

/** 기록 입력창에서 찾은 시간표 (정규 시간표 반영 또는 하루짜리 변경) */
export interface TtEntry {
  id: number;
  note_id: number;
  kind: "regular" | "change";
  school_year: number | null;
  semester: number | null;
  weekday: number | null;
  date: string | null;
  period: number;
  class_label: string;
  subject: string;
  room: string;
  change_type: string;
  text: string;
  status: "applied" | "conflict" | "replaced" | "kept" | "change";
  previous: string;
  time: string | null;
  label: string;
}

export interface Note {
  id: number;
  raw_text: string;
  created_at: string;
  updated_at: string;
  raw_edited_at: string | null;
  status: NoteStatus;
  status_reason: string | null;
  summary: string | null;
  summary_source: string | null;
  classified_at: string | null;
  classified_by: string | null;
  guard_notes: string[];
  unknown_names: string[];
  chips: Chip[];
  /** 교사가 직접 옮긴 갈래 (없으면 AI 분류를 따름) */
  track: NoteTrack | null;
  /** 정리된 업무기록인가 */
  is_work: boolean;
}

export type NoteTrack = "general" | "work";

export interface PlanStudent {
  student_id: number | null;
  name: string;
  class_label: string | null;
  number: number | null;
  candidates: number[];
}

export interface PlanItem {
  kind: ItemKind;
  text: string;
  students: PlanStudent[];
  area: Area;
  duty_id: number | null;
  date: string | null;
  time: string | null;
  date_expr: string | null;
  approximate: boolean;
  remind: ReminderSpec | null;
  origin: ItemOrigin;
  done: boolean;
  edited: boolean;
  /** 일정의 메모 (장소·방문자 같은 설명) */
  memo?: string | null;
}

export interface Plan {
  summary: string;
  items: PlanItem[];
  guard_notes: string[];
  unknown_names: string[];
}

export interface AiRun {
  model_id: string;
  started_at: string;
  ok: boolean;
  error_kind: string | null;
  duration_ms: number | null;
}

export interface NoteDetail {
  note: Note;
  plan: Plan;
  has_user_changes: boolean;
  runs: AiRun[];
}

export interface NoteFilter {
  status?: "all" | "unclassified" | "classified";
  track?: NoteTrack | null;
  date?: string | null;
  student_id?: number | null;
  limit?: number;
  offset?: number;
}

export interface ResolvedDate {
  date: string;
  label: string;
  expr: string;
  approximate: boolean;
  deadline: boolean;
}

export interface Student {
  id: number;
  name: string;
  class_id: number | null;
  class_label: string | null;
  number: number | null;
  memo: string;
  needs_review: boolean;
  note_count: number;
  observation_count: number;
  last_noted_at: string | null;
  area_counts: Record<string, number>;
  phone: string;
  guardian_phone: string;
  address: string;
  gender: string;
  birthday: string | null;
  /** 나이스 명렬표에서 가져온 항목 */
  student_no: string;
  postal_code: string;
  prev_record: string;
  remark: string;
  /** 설정에서 켠 경우에만, 뒷자리를 가린 모양 */
  rrn: string;
}

export type RosterField =
  | "class_label"
  | "number"
  | "name"
  | "phone"
  | "guardian_phone"
  | "address"
  | "gender"
  | "birthday"
  | "memo"
  | "student_no"
  | "postal_code"
  | "prev_record"
  | "remark"
  | "rrn";

export interface RosterColumns {
  phone: boolean;
  guardian_phone: boolean;
  address: boolean;
  gender: boolean;
  birthday: boolean;
  memo: boolean;
  student_no: boolean;
  prev_record: boolean;
  remark: boolean;
}

// ───── 시간표 ─────

export interface AfterSchool {
  name: string;
  start: string;
  end: string;
  /** 하는 요일 (1=월 … 6=토). 비어 있으면 매일 */
  days: number[];
}

export interface BellConfig {
  arrival: string;
  zero_period: boolean;
  zero_start: string;
  first_start: string;
  period_minutes: number;
  break_minutes: number;
  lunch_after: number;
  lunch_minutes: number;
  period_count: number;
  dismissal: string;
  after_school: AfterSchool[];
  saturday: boolean;
}

export interface PeriodSlot {
  no: number;
  label: string;
  start: string;
  end: string;
  kind: "class" | "lunch" | "after" | "arrival" | "dismissal";
  /** 방과후를 하는 요일 (비어 있으면 매일) */
  days?: number[];
}

export interface BellSchedule {
  school_year: number;
  semester: number;
  config: BellConfig;
  periods: PeriodSlot[];
  saved: boolean;
  updated_at: string | null;
}

export type TimetableKind = "personal" | "class" | "school";

export interface TimetableCell {
  row_label: string;
  weekday: number;
  period: number;
  subject: string;
  teacher: string;
  class_label: string;
  room: string;
  memo: string;
  /** '' = 교사가 입력, 'neis' = 나이스에서 받음 */
  source?: string;
}

export interface TimetableView {
  school_year: number;
  semester: number;
  kind: TimetableKind;
  class_label: string;
  cells: TimetableCell[];
  rows: string[];
  updated_at: string | null;
}

export interface TodaySlot {
  period: PeriodSlot;
  cell: TimetableCell | null;
  status: "before" | "now" | "after";
  change: TtEntry | null;
}

export interface TodayTimetable {
  school_year: number;
  semester: number;
  weekday: number;
  has_timetable: boolean;
  slots: TodaySlot[];
}

export interface CopyResult {
  timetables: number;
  cells: number;
  bell: boolean;
}

export interface StudentInput {
  name: string;
  class_label?: string | null;
  number?: number | null;
  memo?: string | null;
}

export interface ClassInfo {
  id: number;
  school_year: number;
  label: string;
  student_count: number;
}

export interface Observation {
  id: number;
  note_id: number | null;
  student_id: number | null;
  student_label: string | null;
  area: Area;
  area_label: string;
  content: string;
  observed_on: string;
  source: string;
  edited: boolean;
  created_at: string;
  note_raw: string | null;
  note_created_at: string | null;
  /** 교과는 과목, 자유학기는 활동 */
  subject: string;
  /** 학생부 참고자료에서 뺌 */
  excluded: boolean;
}

export interface ObservationInput {
  student_id: number | null;
  area: Area;
  content: string;
  observed_on: string;
  /** 교과 과목·자유학기 활동 (없으면 원문·시간표로 정함) */
  subject?: string | null;
  excluded?: boolean | null;
}

export interface DraftCheck {
  novel_words: string[];
  chars: number;
  neis_bytes: number;
  limit_chars: number | null;
  limit_bytes: number | null;
  over: boolean;
}

export interface Draft {
  id: number;
  student_id: number;
  area: Area;
  subject: string;
  body: string;
  generated_by: "template" | "ai" | "manual";
  reviewed: boolean;
  created_at: string;
  updated_at: string;
  source_ids: number[];
  check: DraftCheck;
}

/** 학생부 자료 칸 하나: 영역(교과는 과목, 자유학기는 활동) */
export interface RecordTab {
  area: Area;
  label: string;
  subject: string;
  limit_chars: number | null;
  refs: Observation[];
  text: Draft | null;
  status: RecordStatus;
}

export type RecordStatus = "none" | "refs" | "draft" | "final";

export interface StatusRow {
  student_id: number;
  status: RecordStatus;
  refs: number;
}

export interface FinalsExport {
  path: string;
  rows: number;
  students: number;
}

export interface StudentDetail {
  student: Student;
  observations: Observation[];
  notes: Note[];
  drafts: Draft[];
}

export interface ImportResult {
  added: number;
  skipped: number;
  problems: string[];
}

/** 나이스 명렬표 가져오기 결과 (파일마다) */
export interface NeisFileResult {
  file: string;
  classes: string[];
  total: number;
  added: number;
  updated: number;
  merged: number;
  unchanged: number;
  optional_columns: string[];
  rrn_skipped: boolean;
  error: string | null;
}

export interface NeisImportResult {
  files: NeisFileResult[];
  problems: string[];
}

export interface Duty {
  id: number;
  name: string;
  detail: string;
  guide: string;
  keywords: string;
  active: boolean;
  sort_order: number;
  log_count: number;
  open_task_count: number;
}

export interface DutyInput {
  name: string;
  detail: string;
  guide: string;
  keywords: string;
  active: boolean;
}

export interface Task {
  id: number;
  note_id: number | null;
  duty_id: number | null;
  duty_label: string | null;
  title: string;
  due_date: string | null;
  due_time: string | null;
  date_expr: string | null;
  approximate: boolean;
  done: boolean;
  done_at: string | null;
  source: string;
  edited: boolean;
  remind: ReminderSpec;
  remind_text: string;
  next_reminder: string | null;
  created_at: string;
}

export interface TaskInput {
  title: string;
  due_date: string | null;
  due_time: string | null;
  date_expr: string | null;
  duty_id: number | null;
  done?: boolean | null;
  remind?: ReminderSpec | null;
}

export interface DutyLog {
  id: number;
  note_id: number | null;
  duty_id: number | null;
  duty_label: string | null;
  content: string;
  date: string | null;
  time: string | null;
  date_expr: string | null;
  done: boolean;
  source: string;
  edited: boolean;
  remind: ReminderSpec;
  remind_text: string;
  next_reminder: string | null;
  created_at: string;
}

export interface DutyLogInput {
  duty_id: number | null;
  content: string;
  date: string | null;
  time: string | null;
  date_expr: string | null;
  done?: boolean | null;
  remind?: ReminderSpec | null;
}

export interface DutyBoard {
  logs: DutyLog[];
  tasks: Task[];
}

export type Layer = "annual" | "monthly" | "weekly" | "note";

export interface EventItem {
  id: number;
  layer: Layer;
  note_id: number | null;
  title: string;
  start_date: string;
  end_date: string;
  time: string | null;
  date_expr: string | null;
  memo: string;
  source: string;
  edited: boolean;
  remind: ReminderSpec;
  remind_text: string;
  next_reminder: string | null;
  created_at: string;
}

export interface EventInput {
  layer: Layer;
  title: string;
  start_date: string;
  end_date: string | null;
  time: string | null;
  date_expr: string | null;
  memo: string | null;
  remind?: ReminderSpec | null;
}

export interface AgendaItem {
  kind: "task" | "duty_log" | "event";
  id: number;
  title: string;
  date: string | null;
  end_date: string | null;
  time: string | null;
  date_expr: string | null;
  approximate: boolean;
  done: boolean;
  remind_text: string;
  remind_enabled: boolean;
  next_reminder: string | null;
  source: string;
  note_id: number | null;
  duty_label: string | null;
  layer: Layer | null;
  /** 일정 메모 */
  memo: string;
  /** 학사력 종류 (holiday | closure | vacation | work | 빈 값) */
  day_kind: string;
  /** 휴일·휴업일·방학이면 설정에서 고른 색 이름 */
  day_color: string | null;
}

export interface ReminderLog {
  id: number;
  target_kind: string;
  target_id: number;
  fire_at: string;
  label: string;
  status: "fired" | "missed" | "failed";
  fired_at: string | null;
  error: string | null;
  title: string;
  date: string | null;
}

export interface TodayOverview {
  today: string;
  overdue: AgendaItem[];
  due_today: AgendaItem[];
  upcoming: AgendaItem[];
  undated: AgendaItem[];
  events_today: AgendaItem[];
  events_upcoming: AgendaItem[];
  unclassified_count: number;
  notes_today: number;
  reminder_log: ReminderLog[];
  birthdays_today: string[];
}

export type CalendarView = "week" | "month" | "year";

export interface CalendarItem {
  kind: "event" | "task" | "duty_log" | "birthday" | "tt_change";
  id: number;
  layer: Layer | "task" | "duty" | "birthday" | "tt_change";
  title: string;
  start_date: string;
  end_date: string;
  time: string | null;
  done: boolean;
  source: string;
  note_id: number | null;
  memo: string;
  /** 학사력 종류 */
  day_kind: DayKind | "";
  /** 학년 태그 ("1,3") */
  grades: string;
  /** 목록에 보이지 않고 날짜 색만 칠함 (토요휴업일) */
  hidden: boolean;
}

export interface SearchQuery {
  text: string;
  student_id?: number | null;
  duty_id?: number | null;
  status?: string | null;
  from?: string | null;
  to?: string | null;
  limit?: number;
}

export interface NoteHit {
  note: Note;
  snippet: string;
}

export interface SearchResult {
  terms: string[];
  notes: NoteHit[];
  students: Student[];
  tasks: Task[];
  events: EventItem[];
}

export interface CatalogModel {
  id: string;
  tier: number;
  tier_label: string;
  name: string;
  params: string;
  quant: string;
  file_name: string;
  url: string;
  size_bytes: number;
  sha256: string;
  license: string;
  license_url: string;
  publisher: string;
  min_ram_gb: number;
  recommended_ram_gb: number;
  description: string;
  alternative: boolean;
}

export interface DlProgress {
  model_id: string;
  status: "downloading" | "paused" | "verifying" | "failed" | "done" | "copying" | "idle";
  received: number;
  total: number;
  bytes_per_sec: number;
  message: string | null;
}

export interface CatalogEntry {
  model: CatalogModel;
  installed: boolean;
  verified_at: string | null;
  selected: boolean;
  loaded: boolean;
  recommended: boolean;
  part_bytes: number;
  progress: DlProgress | null;
  fit: "ok" | "tight" | "too_big" | "unknown";
}

export interface GpuInfo {
  name: string;
  vram_bytes: number;
  shared_bytes: number;
  integrated: boolean;
}

export interface DeviceInfo {
  cpu_name: string;
  physical_cores: number;
  logical_cores: number;
  ram_total_bytes: number;
  ram_available_bytes: number;
  gpus: GpuInfo[];
  disk_free_bytes: number | null;
  os: string;
  checked_at: string;
  problems: string[];
  recommended_model: string;
}

export interface ModelsView {
  entries: CatalogEntry[];
  selected: string | null;
  device: DeviceInfo | null;
  models_dir: string;
}

export type FailKind =
  | "runtime_missing"
  | "model_missing"
  | "model_corrupt"
  | "out_of_memory"
  | "port_conflict"
  | "cpu_unsupported"
  | "crashed"
  | "timeout"
  | "request";

export type RuntimeState =
  | { state: "stopped" }
  | { state: "starting"; model_id: string }
  | { state: "ready"; model_id: string; port: number; pid: number }
  | { state: "stopping" }
  | { state: "failed"; kind: FailKind; message: string };

export interface RuntimeStatus {
  state: RuntimeState;
  runtime_found: boolean;
  runtime_dir: string | null;
  loaded_model: string | null;
  listening: string[];
  local_only: boolean | null;
  diagnostics: string[];
}

export interface AppSettings {
  ai_auto_classify: boolean;
  selected_model: string | null;
  idle_unload_minutes: number;
  remind_default: ReminderSpec;
  close_to_tray: boolean;
  /** 창을 항상 위에 (setAlwaysOnTop 으로만 바꾼다) */
  always_on_top: boolean;
  /** 창 투명도(%) 0~70 (setWindowTransparency 로만 바꾼다) */
  window_transparency: number;
  threads: number | null;
  onboarding_done: boolean;
  roster_columns: RosterColumns;
  birthdays_on_calendar: boolean;
  store_rrn: boolean;
  school: SchoolInfo | null;
  homeroom: string;
  neis_key: string;
  my_subjects: string[];
  my_classes_off: string[];
  day_colors: DayColors;
  day_rules: DayRules;
}

export type DayKind = "holiday" | "closure" | "vacation" | "work";

export interface DayColors {
  holiday: string;
  closure: string;
  vacation: string;
  work: string;
}

export interface DayRules {
  holiday: string[];
  closure: string[];
  vacation: string[];
}

// ───── 나이스 ─────
export interface SchoolInfo {
  office_code: string;
  office_name: string;
  code: string;
  name: string;
  kind: string;
  region: string;
  address: string;
}

export interface FetchInfo {
  what: string;
  scope: string;
  fetched_at: string;
  status: "ok" | "empty" | "need_key" | "error";
  message: string;
}

export interface NeisStatus {
  school: SchoolInfo | null;
  has_key: boolean;
  timetable: FetchInfo | null;
  schedule: FetchInfo | null;
  meals: FetchInfo | null;
  subjects: string[];
  classes: string[];
}

export interface SyncReport {
  status: "ok" | "empty" | "need_key" | "no_school" | "error";
  message: string;
  count: number;
  fetched_at: string | null;
}

export interface Dish {
  name: string;
  allergens: number[];
}

export interface MealInfo {
  meal: string;
  dishes: Dish[];
  calories: string;
  nutrition: string[];
  origin: string[];
  people: string;
}

export interface MealsView {
  date: string;
  school_name: string | null;
  meals: MealInfo[];
  next_date: string | null;
  next_meals: MealInfo[];
  fetched_at: string | null;
  message: string | null;
}

/** 오늘 업무 요약 한 줄 */
export interface SummaryLine {
  text: string;
  time: string | null;
  target_kind: "event" | "task" | "duty_log" | "timetable" | "tasks";
  target_id: number | null;
  top: boolean;
}

export interface Briefing {
  date: string;
  lines: SummaryLine[];
  by: "rule" | "ai";
  generated_at: string | null;
  empty: boolean;
}

export interface TtColors {
  cells: Record<string, string>;
  subjects: Record<string, string>;
  teachers: Record<string, string>;
  classes: Record<string, string>;
  rooms: Record<string, string>;
}

export interface AppInfo {
  version: string;
  data_dir: string;
  models_dir: string;
  exe_dir: string;
  /** 파일 하나 실행기로 켰으면 그 파일 위치 */
  launcher_path: string | null;
  db_path: string;
  autostart: boolean;
  wipe_pending: boolean;
  data_dir_name: string;
}

export interface BackupFile {
  name: string;
  size: number;
  /** 'YYYY-MM-DD HH:MM' */
  taken_at: string;
  /** auto | manual | before-restore (같은 초에 두 번이면 manual_2 처럼 붙음) */
  kind: string;
}

export interface BackupCounts {
  version: number;
  notes: number;
  students: number;
  observations: number;
  tasks: number;
  events: number;
  timetable_cells: number;
}

export interface RestoreOutcome {
  ok: boolean;
  name: string;
  message: string;
  before: string | null;
}

export type ExportSheet = "roster" | "notes" | "observations" | "drafts" | "tasks" | "events" | "duties" | "duty_logs" | "timetable";

export interface ExportOptions {
  format: "xlsx" | "csv";
  path: string;
  sheets: ExportSheet[];
  class_id?: number | null;
  mask_contacts?: boolean;
  roster_columns?: RosterColumns | null;
  school_year?: number | null;
}

export interface YearInfo {
  school_year: number;
  current: boolean;
  notes: number;
  students: number;
  observations: number;
  events: number;
  timetable_cells: number;
  archived_at: string | null;
}

export interface ArchiveOptions {
  school_year: number;
  path: string;
  business_pdf: boolean;
  student_pdf: boolean;
  excel: boolean;
  db_copy: boolean;
  school_name: string;
  teacher_name: string;
}

export interface ArchiveFile {
  name: string;
  size: number;
  sha256: string;
  pages: number | null;
  pdfa: boolean | null;
  note: string | null;
}

export interface ArchiveResult {
  folder: string;
  files: ArchiveFile[];
}

export interface NewYearPreview {
  new_year: number;
  prev_year: number;
  students_to_archive: number;
  prev_classes: string[];
  bell_source: string | null;
  new_has_bell: boolean;
  timetables_source: number;
  active_duties: number;
  prev_archived_at: string | null;
  started_year: number | null;
}

export interface NewYearOptions {
  new_year: number;
  archive_students: boolean;
  copy_bell: boolean;
  copy_timetables: boolean;
  keep_duties: boolean;
}

export interface NewYearResult {
  new_year: number;
  archived_students: number;
  bell_copied: boolean;
  timetables_copied: number;
  duties_paused: number;
  backup: string;
}

export interface ExportResult {
  path: string;
  files: string[];
  sheets: Array<{ title: string; rows: number }>;
}

export interface VerifyResult {
  ok: boolean;
  message: string;
}

/** 오늘 화면 메모 (날이 바뀌어도 그대로 남는다) */
export interface Memo {
  id: number;
  body: string;
  color: MemoColor;
  pos: number;
  created_at: string;
  updated_at: string;
}
export type MemoColor = "yellow" | "green" | "pink" | "purple" | "blue" | "gray";

/** 오늘 화면 칸 */
export type TodayPanelId = "tasks" | "briefing" | "memo" | "timetable" | "meals" | "notes" | "extras";

/** 오늘 화면 칸 배치: 자동(화면 너비에 맞춤) / 직접 */
export interface TodayLayout {
  mode: "auto" | "custom";
  columns: TodayPanelId[][];
  wide: TodayPanelId[];
  hidden: TodayPanelId[];
}
