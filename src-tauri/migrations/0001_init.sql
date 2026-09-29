-- 로컬 AI 교무수첩 — 초기 스키마
-- 날짜: 'YYYY-MM-DD', 시각: 'HH:MM', 일시: 'YYYY-MM-DDTHH:MM:SS' (이 PC의 현지 시각)
-- 원문(notes.raw_text)은 교사만 고칠 수 있고, AI가 만든 내용은 별도 표에 파생 데이터로 저장한다.

CREATE TABLE app_meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

-- 학급 (학년도별)
CREATE TABLE classes (
  id          INTEGER PRIMARY KEY,
  school_year INTEGER NOT NULL,
  grade       INTEGER NOT NULL,
  class_no    INTEGER NOT NULL,
  label       TEXT NOT NULL,              -- 예: "2-1"
  UNIQUE (school_year, grade, class_no)
);

-- 학생: 동명이인을 위해 학급·번호와 별도의 id를 쓴다.
CREATE TABLE students (
  id           INTEGER PRIMARY KEY,
  class_id     INTEGER REFERENCES classes(id) ON DELETE SET NULL,
  number       INTEGER,
  name         TEXT NOT NULL,
  memo         TEXT NOT NULL DEFAULT '',
  needs_review INTEGER NOT NULL DEFAULT 0,  -- 기록에서 자동 등록되어 교사 확인이 필요한 학생
  archived     INTEGER NOT NULL DEFAULT 0,
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL
);
CREATE INDEX idx_students_name ON students(name);
CREATE INDEX idx_students_class ON students(class_id);

-- 기록 원문
CREATE TABLE notes (
  id             INTEGER PRIMARY KEY,
  raw_text       TEXT NOT NULL,
  created_at     TEXT NOT NULL,              -- 입력 시각 (상대 날짜 해석 기준)
  updated_at     TEXT NOT NULL,
  raw_edited_at  TEXT,                       -- 교사가 원문을 고친 시각
  status         TEXT NOT NULL DEFAULT 'pending'
                 CHECK (status IN ('pending','processing','classified','failed','manual')),
  status_reason  TEXT,                       -- 미분류 이유 (쉬운 문장)
  summary        TEXT,                       -- 파생 데이터
  summary_source TEXT CHECK (summary_source IN ('ai','manual','extract')),
  classified_at  TEXT,
  classified_by  TEXT,                       -- 모델 id 또는 'manual'
  guard_notes    TEXT NOT NULL DEFAULT '[]', -- 안전 규칙이 고친 내역 (JSON 배열)
  unknown_names  TEXT NOT NULL DEFAULT '[]'  -- 확인이 필요한 이름 (JSON 배열)
);
CREATE INDEX idx_notes_created ON notes(created_at);
CREATE INDEX idx_notes_status ON notes(status);

-- AI 처리 이력 (이 PC의 DB 안에만 보관, 로그 파일에는 남기지 않음)
CREATE TABLE ai_runs (
  id          INTEGER PRIMARY KEY,
  note_id     INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
  model_id    TEXT NOT NULL,
  started_at  TEXT NOT NULL,
  finished_at TEXT,
  ok          INTEGER NOT NULL DEFAULT 0,
  error_kind  TEXT,
  duration_ms INTEGER,
  raw_output  TEXT
);
CREATE INDEX idx_ai_runs_note ON ai_runs(note_id);

CREATE TABLE note_students (
  note_id    INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
  student_id INTEGER NOT NULL REFERENCES students(id) ON DELETE CASCADE,
  source     TEXT NOT NULL CHECK (source IN ('ai','manual')),
  PRIMARY KEY (note_id, student_id)
);
CREATE INDEX idx_note_students_student ON note_students(student_id);

-- 학생 관찰 기록 → 학생부 자료 영역 연결. student_id 가 없으면 "학생 미지정" 수업·학급 관찰.
CREATE TABLE observations (
  id          INTEGER PRIMARY KEY,
  note_id     INTEGER REFERENCES notes(id) ON DELETE CASCADE,
  student_id  INTEGER REFERENCES students(id) ON DELETE CASCADE,
  area        TEXT NOT NULL CHECK (area IN ('attendance','subject','autonomous','career','volunteer','club','behavior','none')),
  content     TEXT NOT NULL,
  observed_on TEXT NOT NULL,
  source      TEXT NOT NULL CHECK (source IN ('ai','manual')),
  edited      INTEGER NOT NULL DEFAULT 0,
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL
);
CREATE INDEX idx_obs_student_area ON observations(student_id, area);
CREATE INDEX idx_obs_note ON observations(note_id);

-- 학생부 자료 초안 (교사 검토용, 자동 입력 없음)
CREATE TABLE record_drafts (
  id           INTEGER PRIMARY KEY,
  student_id   INTEGER NOT NULL REFERENCES students(id) ON DELETE CASCADE,
  area         TEXT NOT NULL CHECK (area IN ('attendance','subject','autonomous','career','volunteer','club','behavior')),
  body         TEXT NOT NULL,
  generated_by TEXT NOT NULL CHECK (generated_by IN ('template','ai','manual')),
  reviewed     INTEGER NOT NULL DEFAULT 0,
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL
);
CREATE INDEX idx_drafts_student ON record_drafts(student_id, area);

CREATE TABLE record_draft_sources (
  draft_id       INTEGER NOT NULL REFERENCES record_drafts(id) ON DELETE CASCADE,
  observation_id INTEGER NOT NULL REFERENCES observations(id) ON DELETE CASCADE,
  PRIMARY KEY (draft_id, observation_id)
);

-- 담당업무 프로필 (교사가 직접 등록)
CREATE TABLE duties (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL,
  detail     TEXT NOT NULL DEFAULT '',
  guide      TEXT NOT NULL DEFAULT '',
  keywords   TEXT NOT NULL DEFAULT '',
  active     INTEGER NOT NULL DEFAULT 1,
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

-- 알림 설정 열(remind_*)은 업무 기록·할 일·일정에 공통으로 둔다.
CREATE TABLE duty_logs (
  id               INTEGER PRIMARY KEY,
  note_id          INTEGER REFERENCES notes(id) ON DELETE CASCADE,
  duty_id          INTEGER REFERENCES duties(id) ON DELETE SET NULL,
  content          TEXT NOT NULL,
  date             TEXT,
  time             TEXT,
  date_expr        TEXT,
  done             INTEGER NOT NULL DEFAULT 0,
  source           TEXT NOT NULL CHECK (source IN ('ai','manual')),
  edited           INTEGER NOT NULL DEFAULT 0,
  remind_enabled   INTEGER NOT NULL DEFAULT 0,
  remind_offsets   TEXT NOT NULL DEFAULT '[]',
  remind_time      TEXT NOT NULL DEFAULT '08:30',
  remind_custom_at TEXT,
  created_at       TEXT NOT NULL,
  updated_at       TEXT NOT NULL
);
CREATE INDEX idx_duty_logs_duty ON duty_logs(duty_id);
CREATE INDEX idx_duty_logs_note ON duty_logs(note_id);
CREATE INDEX idx_duty_logs_date ON duty_logs(date);

CREATE TABLE tasks (
  id               INTEGER PRIMARY KEY,
  note_id          INTEGER REFERENCES notes(id) ON DELETE CASCADE,
  duty_id          INTEGER REFERENCES duties(id) ON DELETE SET NULL,
  title            TEXT NOT NULL,
  due_date         TEXT,
  due_time         TEXT,
  date_expr        TEXT,
  approximate      INTEGER NOT NULL DEFAULT 0,
  done             INTEGER NOT NULL DEFAULT 0,
  done_at          TEXT,
  source           TEXT NOT NULL CHECK (source IN ('ai','manual')),
  edited           INTEGER NOT NULL DEFAULT 0,
  remind_enabled   INTEGER NOT NULL DEFAULT 0,
  remind_offsets   TEXT NOT NULL DEFAULT '[]',
  remind_time      TEXT NOT NULL DEFAULT '08:30',
  remind_custom_at TEXT,
  created_at       TEXT NOT NULL,
  updated_at       TEXT NOT NULL
);
CREATE INDEX idx_tasks_due ON tasks(done, due_date);
CREATE INDEX idx_tasks_note ON tasks(note_id);

-- 일정표: layer 로 반영 범위를 정한다.
--   annual  : 연간 학사력  → 연간·월간·주간
--   monthly : 월간 세부 계획 → 월간·주간
--   weekly  : 주간 세부 계획 → 주간
--   note    : 기록(AI 또는 교사 분류)에서 찾은 일정 → 월간·주간 (연간 학사력에는 올리지 않음)
CREATE TABLE events (
  id               INTEGER PRIMARY KEY,
  layer            TEXT NOT NULL CHECK (layer IN ('annual','monthly','weekly','note')),
  note_id          INTEGER REFERENCES notes(id) ON DELETE CASCADE,
  title            TEXT NOT NULL,
  start_date       TEXT NOT NULL,
  end_date         TEXT NOT NULL,
  time             TEXT,
  date_expr        TEXT,
  memo             TEXT NOT NULL DEFAULT '',
  source           TEXT NOT NULL CHECK (source IN ('ai','manual')),
  edited           INTEGER NOT NULL DEFAULT 0,
  remind_enabled   INTEGER NOT NULL DEFAULT 0,
  remind_offsets   TEXT NOT NULL DEFAULT '[]',
  remind_time      TEXT NOT NULL DEFAULT '08:30',
  remind_custom_at TEXT,
  created_at       TEXT NOT NULL,
  updated_at       TEXT NOT NULL,
  CHECK (end_date >= start_date)
);
CREATE INDEX idx_events_range ON events(start_date, end_date);
CREATE INDEX idx_events_note ON events(note_id);

-- 알림 대기열: 대상의 날짜·알림 설정으로부터 다시 만든다.
CREATE TABLE reminders (
  id          INTEGER PRIMARY KEY,
  target_kind TEXT NOT NULL CHECK (target_kind IN ('task','event','duty_log')),
  target_id   INTEGER NOT NULL,
  fire_at     TEXT NOT NULL,
  label       TEXT NOT NULL,
  status      TEXT NOT NULL DEFAULT 'scheduled' CHECK (status IN ('scheduled','fired','missed','failed')),
  fired_at    TEXT,
  error       TEXT
);
CREATE INDEX idx_reminders_due ON reminders(status, fire_at);
CREATE INDEX idx_reminders_target ON reminders(target_kind, target_id);

-- 설치된 AI 모델
CREATE TABLE installed_models (
  model_id     TEXT PRIMARY KEY,
  file_name    TEXT NOT NULL,
  size_bytes   INTEGER NOT NULL,
  sha256       TEXT NOT NULL,
  installed_at TEXT NOT NULL,
  verified_at  TEXT
);

-- 원문·요약 전문 검색 (trigram: 한국어 부분 일치 검색)
CREATE VIRTUAL TABLE notes_fts USING fts5(
  raw_text, summary,
  content='notes', content_rowid='id',
  tokenize='trigram'
);

CREATE TRIGGER notes_ai AFTER INSERT ON notes BEGIN
  INSERT INTO notes_fts(rowid, raw_text, summary) VALUES (new.id, new.raw_text, COALESCE(new.summary, ''));
END;
CREATE TRIGGER notes_ad AFTER DELETE ON notes BEGIN
  INSERT INTO notes_fts(notes_fts, rowid, raw_text, summary) VALUES ('delete', old.id, old.raw_text, COALESCE(old.summary, ''));
END;
CREATE TRIGGER notes_au AFTER UPDATE OF raw_text, summary ON notes BEGIN
  INSERT INTO notes_fts(notes_fts, rowid, raw_text, summary) VALUES ('delete', old.id, old.raw_text, COALESCE(old.summary, ''));
  INSERT INTO notes_fts(rowid, raw_text, summary) VALUES (new.id, new.raw_text, COALESCE(new.summary, ''));
END;
