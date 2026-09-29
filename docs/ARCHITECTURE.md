# 아키텍처 결정 기록 — 로컬 AI 교무수첩 (Local Teacher Notebook) v2

이 문서는 독립 재구현에서 내린 주요 결정과 그 이유를 적는다. 판단 기준은 명세의 질문들이다:
교사가 매일 빠르게 쓸 수 있는가, AI가 틀려도 원문과 기록이 안전한가, RAM 8GB·외장 GPU 없는 학교 PC에서 도는가,
학생 개인정보가 PC 밖으로 나가지 않는가, 설치 권한 없이 압축만 풀어 실행되는가.

## 1. 전체 구조

```
TeacherNotebook.exe (Tauri 2, Rust)
 ├─ 화면: React + TypeScript strict (EXE 안에 내장, 개발 서버 불필요)
 ├─ 명령 계층 commands.rs  ──  화면이 부르는 70여 개 명령
 ├─ 도메인
 │   ├─ notes.rs        원문 저장(항상 먼저) · 분류 결과 반영(한 트랜잭션)
 │   ├─ classify.rs     분류 계획(Plan) · 규칙 제안 · AI 결과 안전 검증(guard)
 │   ├─ analysis.rs     규칙 분석: 출결 사실·학생 언급·행동/기한·과거 사실·영역 단서·업무 핵심어
 │   ├─ dates.rs        한국어 날짜·시각 해석 (AI에게 날짜 계산을 맡기지 않음)
 │   ├─ text.rs         문장·절 분리, 원문 근거(2-gram 포함률)
 │   ├─ students.rs / records.rs / duties.rs / schedule.rs / reminders.rs / search.rs
 ├─ AI
 │   ├─ runtime.rs      llama-server sidecar 수명주기 (127.0.0.1 전용, 임시 인증키, Job Object)
 │   ├─ llm.rs          프롬프트·JSON 스키마·긴 기록 나누기
 │   └─ models.rs       카탈로그·다운로드(이어받기)·검증·가져오기·삭제
 ├─ 백그라운드 app.rs   AI 분류 대기열(한 번에 하나), 알림 확인(20초), AI 엔진 자동 내리기, 하루 1회 자동 백업
 └─ SQLite (rusqlite bundled, WAL, FTS5 trigram)

runtime\llama.cpp\  llama-server.exe + DLL (llama.cpp b11000 을 llvm-mingw 로 직접 빌드)
%LOCALAPPDATA%\TeacherNotebookV2\  data\notebook.db, models\, backups\, rules\, logs\, webview\, settings.json
```

## 2. 결정과 이유

### D1. 원문 먼저, AI는 나중 (비동기 대기열)
- `create_note`는 원문을 `pending` 상태로 저장하고 즉시 돌아온다 (실측 5~20ms). 분류는 별도 스레드 대기열에서 한 번에 하나씩 한다.
- AI가 없거나, 엔진이 실패하거나, 앱이 분류 중에 꺼져도 원문은 남는다. 다음 시작 때 `processing` 상태는 `pending`으로 되돌려 다시 처리한다.
- 모델이 없으면 `failed` + 쉬운 이유("AI 모델이 아직 설치되지 않았습니다 …")를 저장하고, 기록 카드에 `AI로 다시 분류`·`직접 분류` 버튼을 보인다.

### D2. AI는 "제안", 규칙이 "검증" — 하이브리드 분류
작은 로컬 모델(0.5B~9B)은 한국어 메모에서 사실을 지어내거나 영역을 틀리기 쉽다. 그래서:
1. llama-server의 **JSON 스키마 제약 디코딩**(`response_format: json_schema`)으로 형식을 보장한다.
2. AI가 낸 모든 항목을 `classify::guard_llm`이 검증한다.
   - **원문 근거**: 문구의 글자 2-gram 중 85% 이상이 원문에 없으면 가장 비슷한 원문 절로 바꾸고, 근거가 없으면 버린다. 요약도 60% 미만이면 원문 첫 문장으로 대체한다.
   - **이름**: 원문에 없는 학생 이름은 버린다 (예: AI가 만든 "학생이름1").
   - **출결**: 결석·지각·조퇴·결과(교시 수식) 사실이 원문에 있을 때만 출결 영역. 부정("지각하지 않도록")과 서류 이름("결석계")은 사실이 아니다.
   - **봉사·동아리**: 원문에 그 낱말이 있을 때만.
   - **할 일**: 행동어 또는 "~까지" 기한이 있고, 과거 사실(~함, ~했음)이 아닐 때만. 학생 이름과 함께 적힌 "진로 상담"처럼 기한·의무 표현이 없으면 관찰 기록으로 본다.
   - **일정**: 날짜가 있어야 하고, "~까지"가 붙으면 일정이 아니라 기한(할 일)이다. "교과협의회"의 '협의'처럼 행사 이름 속 행동어는 행동으로 보지 않는다.
   - **여러 문장을 묶은 항목**은 문장별로 나누어 따로 검증한다.
   - **날짜·시각**은 AI가 아니라 `dates.rs`가 입력 시각 기준으로 계산한다. AI가 적은 날짜 표현은 원문에 있을 때만 참고한다.
3. AI가 놓친 것은 규칙으로 보완한다: 원문의 출결 사실, "~까지" 할 일, 날짜 있는 일정, 담당업무 핵심어.
4. 보정 내역은 `guard_notes`로 저장해 화면에 "안전 규칙이 고친 내용 n건"으로 보여 준다 (투명성).

규칙 분석은 AI가 없을 때 `직접 분류` 창에 미리 채워지는 제안으로도 쓰인다 → AI 없이도 클릭 한 번으로 정리 가능.

### D3. 파생 데이터의 정규화
- `notes`(원문)와 파생 데이터(`observations`, `tasks`, `events`, `duty_logs`, `note_students`)를 분리했다. 재분류는 해당 기록의 파생 데이터만 한 트랜잭션으로 교체한다.
- 학생 관찰(`observations`)은 **학생 × 영역 단위**다. 한 기록에서 A 학생은 출결, B 학생은 교과처럼 따로 연결된다. 학생이 없는 수업 관찰("학생들이…")은 `student_id` 없이 남기고 학생부 자료 화면에서 나중에 학생에게 연결한다.
- 동명이인: 학급(`classes`)·번호와 분리된 학생 id. 학급 없이 이름만 겹치면 자동으로 고르지 않고 "동명이인 확인 필요"로 남긴다.
- 학생부 초안(`record_drafts`)은 근거 관찰(`record_draft_sources`)을 함께 저장한다. 다른 학생의 관찰은 근거로 연결되지 않는다.
- 출처 표시: AI가 만든 항목 `ai`, 교사가 만들거나 확인해 저장한 항목 `manual`, 교사가 고친 AI 항목 `edited=1`.

### D4. 일정표의 단방향 반영
`events.layer`(annual/monthly/weekly/note) + 할 일·업무 날짜를 `schedule::layer_visible(layer, view)` 한 함수로 거른다.
연간 학사력 → 연·월·주 / 월간 계획 → 월·주 / 주간 계획 → 주 / 기록에서 찾은 일정·할 일·업무 → 월·주.
하위 계획이 연간 학사력으로 올라가는 경로 자체가 없다. 연간 보기는 학년도(3월 시작)로도 볼 수 있다.

### D5. 알림 대기열
- 할 일·일정·업무 기록마다 `remind_enabled / offsets(7·3·2·1·당일) / time / custom_at`을 저장하고, 바뀔 때마다 남은 예약을 지우고 다시 만든다(`reminders::rebuild`). 완료하면 남은 예약이 사라진다.
- 앱이 실행 중일 때 20초마다 확인해 Windows 알림을 보낸다. 앱이 꺼져 있던 동안 12시간 넘게 지난 알림은 "놓친 알림"으로 모아 한 번 알린다.
- Portable 앱은 설치 프로그램이 없어 알림 발신자(AppUserModelID)를 현재 사용자 레지스트리(HKCU)에 등록한다 (관리자 권한 불필요). 알림이 꺼져 있으면(사용자·그룹 정책) Windows가 알려 준 이유를 그대로 화면에 보여 준다.
- 한계를 숨기지 않기 위해 설정에 "창을 닫아도 알림 영역에서 계속 실행", "Windows 로그인 때 자동 시작(HKCU Run)"을 선택 기능으로 두었다.

### D6. llama.cpp 를 직접 빌드 (Visual C++ 런타임 의존 제거)
공식 Windows CPU 빌드는 `MSVCP140.dll`, `VCRUNTIME140(_1).dll`이 필요하다. 설치 권한이 없는 학교 PC에 VC++ 재배포 패키지가 없으면
AI 엔진이 `STATUS_DLL_NOT_FOUND`로 시작하지 못한다. 같은 태그(b11000) 소스를 llvm-mingw(clang 23)로 빌드하고
LLVM `libc++.dll`/`libunwind.dll`(Apache-2.0 WITH LLVM-exception, 재배포 가능)만 함께 넣었다.
- `GGML_BACKEND_DL=ON`, `GGML_CPU_ALL_VARIANTS=ON`: CPU 종류(SSE4.2~AVX-512/AMX)에 맞는 모듈을 실행 시 고른다.
- `GGML_OPENMP=OFF`(ggml 자체 스레드 풀), `LLAMA_OPENSSL=OFF`, 웹 UI 미포함.
- `scripts/prepare-runtime.ps1`이 필요한 DLL이 모두 있는지, VC++ 런타임 의존이 없는지 검사한다.

### D7. AI 엔진(sidecar) 수명주기와 보안
- 상태: Stopped → Starting → Ready → Stopping → Stopped / Failed(원인). 원인은 런타임 없음, 모델 없음, 모델 손상, 메모리 부족, 포트 충돌, CPU 미지원, 비정상 종료, 시간 초과로 구분해 쉬운 문장으로 보여 준다.
- `--host 127.0.0.1`, 실행마다 새 `--api-key`(24바이트 난수), `--offline`(인터넷 금지), `--no-webui`, `--no-slots`(요청 내용 노출 끄기), `--cache-ram 0`(8GB PC 메모리 보호), `--reasoning off`.
- 앱 안에서 Windows `GetExtendedTcpTable`로 엔진이 실제로 어느 주소에서 대기하는지 확인해 "이 PC 안에서만 접속할 수 있습니다"를 표시한다.
- Windows Job Object(`KILL_ON_JOB_CLOSE`)에 넣어 앱이 강제 종료돼도 엔진이 함께 끝난다. 정상 종료 때는 `RunEvent::Exit`에서 멈춘다.
- 필요할 때 켜고(lazy), 설정한 시간(기본 10분) 쓰지 않으면 내린다. 추론은 한 번에 하나.
- 엔진 출력은 메모리에만 잠시 두고(최근 120줄, 문제 해결 화면용) 파일로 남기지 않는다. 기술 로그에는 기록 번호·오류 종류·시간만 쓴다.

### D8. 모델 카탈로그와 다운로드
- 5단계(+대안): 초경량 Qwen2.5 0.5B / 기본 Qwen3.5 2B / 고급 Qwen3.5 4B / 최고급 Gemma 4 E4B / 울트라 Qwen3.5 9B (+ Qwen3 8B). 모두 Apache-2.0.
- 주소는 Hugging Face의 특정 커밋에 고정하고 크기·SHA-256·라이선스를 코드에 박아 둔다.
- `.part` 파일 + HTTP Range 이어받기, 일시정지, 자동 재시도(최대 5회), 저장 공간 확인, 크기·SHA-256 검증 후에만 이름을 바꿔 설치. 연결이 끊긴 뒤 이어받는 것을 로컬 HTTP 서버 시험으로 확인한다.
- **파일로 설치**: 한 교사가 받은 .gguf 를 USB로 다른 PC에 옮겨 설치(크기·SHA 검증 후 복사, 원본 보존). 학교 네트워크가 느리거나 막혀도 쓸 수 있다.
- 삭제: 엔진 중지 → 파일(.part 포함) 삭제 → 설치 기록·선택 해제 → 화면 갱신 이벤트.
- 기기 사양(CPU·RAM·DXGI GPU/VRAM·여유 공간)은 처음 한 번만 읽어 저장하고 `기기정보 다시 읽기`로만 갱신한다. 권장 모델은 RAM 기준.
- 다운로드는 Windows 시스템 프록시와 인증서 저장소(schannel)를 쓴다 (학교 SSL 검사 프록시 대응).

### D9. 데이터 위치와 Portable
- 사용자 데이터는 `%LOCALAPPDATA%\TeacherNotebookV2` (비교 기간 동안 기존 구현의 `TeacherNotebook` 폴더 보호를 위해 이름을 달리했다. `paths::DATA_DIR_NAME` 한 줄로 명세 경로로 바꿀 수 있다).
- 다른 프로그램이 만든 DB가 그 위치에 있으면 열지 않는다(`app_meta` 확인) — 잘못된 마이그레이션으로 기존 데이터를 망가뜨리지 않는다.
- WAL, `foreign_keys=ON`, busy timeout 5초, `PRAGMA user_version` 마이그레이션, 짧은 쓰기 트랜잭션, 하루 1회 `VACUUM INTO` 자동 백업(14개 보관).
- 모든 로컬 데이터 삭제: 확인 문구 입력 + Windows 확인 창(백엔드가 직접 띄움) → 표식 파일 → **다음 시작 때 DB를 열기 전에** 허용된 하위 경로만 지운다 (실행 중인 DB 강제 삭제 없음, 루트 밖 경로는 정규화 비교로 차단).
- WebView2 사용자 데이터도 사용자 데이터 폴더(`webview\`)에 둔다.
- EXE는 libunwind 를 정적으로 넣어(`+crt-static`) 시스템 DLL과 `WebView2Loader.dll`만 필요하다.

### D10. 화면 설계 원칙
- 모든 화면 위에 빠른 기록 입력창(Ctrl+Enter 저장, Ctrl+N 이동, 쓰다 만 글 자동 보관). 메뉴를 고르지 않고 적는다.
- 오늘 화면: 기한 지난 → 오늘 → 앞으로 2주 → 날짜 없는 할 일. 체크로 완료, 줄을 누르면 수정(날짜·원래 표현·알림).
- AI 결과 옆에 항상 수정 경로: 기록 카드의 `분류 수정`(항목 단위 편집), 학생 관찰의 연필, 할 일·일정·업무의 편집 창.
- 전문 용어 대신 쉬운 말: "127.0.0.1" → "이 PC 안에서만", "FTS" → "검색", "sidecar" → "AI 엔진".
- 학생부 자료는 "교사 검토용 초안" 배너, 근거 없는 표현 노란 표시, 글자 수·NEIS 방식 바이트(참고용), 복사 버튼. 자동 입력 기능은 없다.

## 3. 플랫폼 분리
Windows 전용 코드는 `#[cfg(windows)]`로 모았다: Job Object(runtime.rs), 수신 주소 확인(runtime.rs), 알림·자동 시작(notify.rs), DXGI GPU·저장 공간(device.rs, models.rs), 메시지 상자(lib.rs).
다른 OS에서는 같은 함수가 "아직 지원하지 않음"을 돌려주도록 대체 구현을 두었다 (macOS Apple Silicon 확장 대비).

## 4. 쓰지 않은 선택지
- Electron: 배포 크기·메모리 (명세 제외).
- AI에게 날짜 계산 맡기기: 작은 모델은 요일 계산을 자주 틀린다 → 규칙으로 계산.
- 공식 llama.cpp 바이너리 그대로 쓰기: VC++ 런타임 의존 (D6).
- 앱이 꺼져 있을 때도 알림 보장(Windows 작업 스케줄러 등록): 학교 PC 정책에서 막히는 경우가 많고 관리가 어렵다 → 선택형 자동 시작·트레이로 대신하고 한계를 설정 화면에 적었다.
- GPU 가속: 1차 대상(외장 GPU 없는 학교 PC)과 크기·의존성(VC++ 런타임)을 고려해 이번에는 넣지 않았다(사용자 결정).
