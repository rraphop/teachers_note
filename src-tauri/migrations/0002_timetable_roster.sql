-- 0.2: 학급명렬표 항목, 시간표, 업무 세부업무 통합

-- 학급명렬표: 필요한 항목만 화면에서 켜서 쓴다 (값이 없으면 빈 문자열)
ALTER TABLE students ADD COLUMN phone          TEXT NOT NULL DEFAULT '';
ALTER TABLE students ADD COLUMN guardian_phone TEXT NOT NULL DEFAULT '';
ALTER TABLE students ADD COLUMN address        TEXT NOT NULL DEFAULT '';
ALTER TABLE students ADD COLUMN gender         TEXT NOT NULL DEFAULT '';   -- '남' | '여' | ''
ALTER TABLE students ADD COLUMN birthday       TEXT;                       -- 'YYYY-MM-DD' 또는 'MM-DD'

-- 업무: '세부업무' 하나로 통일한다. 예전 '분류 설명'과 '핵심어'는 세부업무 뒤에 붙인다.
UPDATE duties SET detail = TRIM(detail);
UPDATE duties SET detail = CASE
  WHEN detail = '' THEN TRIM(guide)
  WHEN TRIM(guide) = '' THEN detail
  ELSE detail || ', ' || TRIM(guide) END;
UPDATE duties SET detail = CASE
  WHEN detail = '' THEN TRIM(keywords)
  WHEN TRIM(keywords) = '' THEN detail
  ELSE detail || ', ' || TRIM(keywords) END;
UPDATE duties SET guide = '', keywords = '';

-- 학기별 일과 시간 (등교·교시·쉬는 시간·점심·하교·방과후)
CREATE TABLE bell_schedules (
  id          INTEGER PRIMARY KEY,
  school_year INTEGER NOT NULL,
  semester    INTEGER NOT NULL CHECK (semester IN (1, 2)),
  config      TEXT NOT NULL,   -- 설정값 JSON
  periods     TEXT NOT NULL,   -- 교시별 시각 JSON (자동 계산 후 교사가 고칠 수 있음)
  updated_at  TEXT NOT NULL,
  UNIQUE (school_year, semester)
);

-- 시간표: 전체(학교) / 개인(교사) / 학급
CREATE TABLE timetables (
  id          INTEGER PRIMARY KEY,
  school_year INTEGER NOT NULL,
  semester    INTEGER NOT NULL CHECK (semester IN (1, 2)),
  kind        TEXT NOT NULL CHECK (kind IN ('school', 'personal', 'class')),
  class_label TEXT NOT NULL DEFAULT '',
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL,
  UNIQUE (school_year, semester, kind, class_label)
);

-- 칸: 요일(1=월 … 6=토) × 교시(0=0교시, 1~10, 방과후 101~)
-- 전체 시간표는 row_label(학급)별로 한 줄씩 저장한다.
CREATE TABLE timetable_cells (
  timetable_id INTEGER NOT NULL REFERENCES timetables(id) ON DELETE CASCADE,
  row_label    TEXT NOT NULL DEFAULT '',
  weekday      INTEGER NOT NULL CHECK (weekday BETWEEN 1 AND 6),
  period       INTEGER NOT NULL,
  subject      TEXT NOT NULL DEFAULT '',
  teacher      TEXT NOT NULL DEFAULT '',
  class_label  TEXT NOT NULL DEFAULT '',
  room         TEXT NOT NULL DEFAULT '',
  memo         TEXT NOT NULL DEFAULT '',
  PRIMARY KEY (timetable_id, row_label, weekday, period)
);
