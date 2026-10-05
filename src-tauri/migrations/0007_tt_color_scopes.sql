-- 시간표 색: 교과별·교사별 색 추가 (우선순위: 칸 → 교과 → 교사 → 학급 → 교실)
--   scope='subject' : target = "국어"
--   scope='teacher' : target = "김선생"
-- 대상 제한(CHECK)을 넓히려면 표를 다시 만들어야 한다 (다른 표가 참조하지 않는 작은 표)
CREATE TABLE timetable_colors_new (
  scope  TEXT NOT NULL CHECK (scope IN ('cell', 'subject', 'teacher', 'class', 'room')),
  target TEXT NOT NULL,
  color  TEXT NOT NULL,
  PRIMARY KEY (scope, target)
);
INSERT INTO timetable_colors_new(scope, target, color) SELECT scope, target, color FROM timetable_colors;
DROP TABLE timetable_colors;
ALTER TABLE timetable_colors_new RENAME TO timetable_colors;
