// Rust 쪽 DTO 와 같은 모양 (snake_case)

export type Area =
  | "attendance"
  | "subject"
  | "autonomous"
  | "career"
  | "volunteer"
  | "club"
  | "behavior"
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
  kind: "student" | "task" | "event" | "duty";
  label: string;
  area: string | null;
  date: string | null;
  done: boolean;
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
}

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
}

export interface ObservationInput {
  student_id: number | null;
  area: Area;
  content: string;
  observed_on: string;
}

export interface DraftCheck {
  novel_words: string[];
  chars: number;
  neis_bytes: number;
}

export interface Draft {
  id: number;
  student_id: number;
  area: Area;
  body: string;
  generated_by: "template" | "ai" | "manual";
  reviewed: boolean;
  created_at: string;
  updated_at: string;
  source_ids: number[];
  check: DraftCheck;
}

export interface AreaBlock {
  area: Area;
  label: string;
  observations: Observation[];
  drafts: Draft[];
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
}

export type CalendarView = "week" | "month" | "year";

export interface CalendarItem {
  kind: "event" | "task" | "duty_log";
  id: number;
  layer: Layer | "task" | "duty";
  title: string;
  start_date: string;
  end_date: string;
  time: string | null;
  done: boolean;
  source: string;
  note_id: number | null;
  memo: string;
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
  threads: number | null;
  onboarding_done: boolean;
}

export interface AppInfo {
  version: string;
  data_dir: string;
  models_dir: string;
  exe_dir: string;
  db_path: string;
  autostart: boolean;
  wipe_pending: boolean;
  data_dir_name: string;
}

export interface BackupFile {
  name: string;
  size: number;
  modified: string;
}

export interface VerifyResult {
  ok: boolean;
  message: string;
}
