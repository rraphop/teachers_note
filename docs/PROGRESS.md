# 진행 상황 (2026-09-29 새벽 중단 시점)

## 끝난 것
- 전체 소스: Rust 백엔드(기록·학생·학생부 자료·업무·할 일·일정·알림·검색·모델 관리·llama.cpp 런타임) + React/TS 화면 9개
- 단위 테스트 79개 통과 (`cargo test --lib`), TypeScript strict 검사·프로덕션 빌드 통과
- llama.cpp b11000 을 llvm-mingw 로 직접 빌드 → Visual C++ 런타임 없이 실행 (기존 구현은 MSVCP140 필요)
- Portable ZIP 19.1MB: self-test(내장 화면·런타임 상대 탐색·FTS5), SHA-256 무결성 재검산, 다른 폴더 이동 후 실행 확인
- 실제 EXE E2E
  - AI 없음: 23/23 (시나리오 B·C·D·E·F, 오늘 화면 순서, 학생 상세, 검색, 기기정보 캐시)
  - 0.5B 실제 다운로드(일시정지·이어받기·SHA 검증), 재분류, 127.0.0.1 바인딩, 모델 삭제(G): 22/23 → 발견한 문제 보정 완료
- 평가(규칙만): 조정용 세트 52/52, 검증용 세트 37/38 (출결 오분류 0건)

## 내일 할 일
1. 모델별 평가: `scripts/e2e/run-eval.mjs` — 0.5B(가져오기), Qwen3.5 2B·4B(다운로드), Qwen3-8B(기존 파일 복사, 시험 후 삭제)
2. 앱 종료·강제 종료 시 AI 엔진(llama-server) 정리 E2E
3. 기존 앱 비교 실행: `%LOCALAPPDATA%\TeacherNotebook` 백업 → 같은 입력 실행 → 복원
4. 검증 세트 H10("청소 당번표 다시 만들기") 행동어 보강 — 점수 보고 후 수정
5. 문서: 아키텍처 결정, 검증 보고서, 비교표, 알려진 제한사항, 실행·데이터 위치 설명
6. 최종 패키징과 소스 ZIP

## 다시 시작하는 방법
```powershell
. C:\Users\rraph\Documents\Claude\teacher-notebook-v2-tools\env.ps1
cd C:\Users\rraph\Documents\Claude\teacher-notebook-v2
powershell -ExecutionPolicy Bypass -File scripts\package-portable.ps1
```
