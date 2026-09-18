# FlowLens

Windows용 Tauri 기반 개인 생산성 분석 앱입니다. 사용자의 작업 흐름을 방해하지 않으면서 **활성 앱, 앱 포커싱 시간, 키보드·마우스 활동량, 작업 세션 및 로컬 업무 패턴**을 기기 안에서 집계·분석합니다.

## 현재 구현

- Windows 전경 창을 200ms 간격으로 실제 추적하고, 창 핸들·PID·실행 파일명으로 앱별 포커싱 시간/전환 횟수를 집계
- 어두운 생산성 대시보드: 실제 활동 시간, 앱 활성화, 키보드·마우스 액션
- 시간대별 집중 흐름 그래프와 설명 가능한 패턴 분석 카드
- 가장 많이 사용한 앱의 활성 시간 분포(브라우저 URL은 수집하지 않음)
- 추적 일시정지/재개 UI 및 설정 페이지의 수집 데이터 전체 삭제
- Tauri Rust command: `set_tracking`, `get_tracking_state`, `get_embedding_analysis`, `set_embedding_enabled`, `clear_embedding_data`, `clear_all_data`
- Windows 활성 창 제목을 수집하는 Windows API 연동 (`GetForegroundWindow`, `GetWindowTextW`)
- Windows 앱 데이터 폴더의 `activity.json`에만 저장하며 임시 파일 교체로 기록
- 분당 활성 앱 수, 앱 전환 타임라인, 마우스 이동 거리·클릭 간격, 네트워크 인터페이스 총량 추적
- 리본 전용 `FlowMinute` 로컬 버킷: 관측·집중·유휴 초, 전환 수, 키보드·마우스 입력 집계만 기록하며 앱 이름·창 제목은 포함하지 않음
- `get_focus_experience_view` IPC: 5분 집중 흐름 버킷과 완료·진행 세션 카드용 읽기 모델을 로컬에서 생성
- P1 일일 리포트 Rust 백엔드: 사용자가 명시적으로 활성화할 때만 최대 365개의 집계형 `DailyReportRecord` 아카이브, 재진입 흐름 및 작업 흐름 변화 신호를 로컬 생성
- 일일 리포트는 단일 피로·생산성 점수를 만들지 않으며 후반 전환 변화, 재진입 흐름, 긴 연속 활동, 상호작용 리듬 변화의 수치 근거만 반환
- 일일 리포트·태그·선택형 작업 흐름 만족도는 개별 삭제 명령을 제공하며, 화면 연결은 후속 UI 릴리스에서 제공
- 추가 행동 지표와 로컬 임베딩 분석 설계: `BEHAVIOR_ANALYTICS.md`
- 유휴 시간, 작업 세션, 재개 지연, 컨텍스트 전환 밀도, 입력·클릭 리듬, 앱 시간 분포를 `DailyFeatureVector`로 로컬 계산
- 알림/앱별 네트워크의 별도 권한·배포 경로: `ADVANCED_COLLECTION.md`
- 명시적 사용자 활성화 후 24차원 로컬 임베딩, 개인 기준선, 유사 업무일 및 설명 가능한 인사이트 계산
- 임베딩 알고리즘·보관·삭제 범위: `LOCAL_EMBEDDING.md`
- 제목 없는 창·제목 변경·짧은 활성화 누락을 줄이는 앱 식별 방식과 검증 절차: `TRACKING_RELIABILITY.md`

## 개인정보 보호 원칙

FlowLens는 화면 캡처, 키 입력 내용, 문서 본문, 브라우저 URL을 기록하지 않습니다. 수집 대상은 활성 창 제목/활성 시간과 입력 이벤트 개수 같은 집계값이며 Windows 앱 데이터 폴더의 `activity.json`에만 저장됩니다. 임베딩 분석은 기본 비활성이고, 사용자가 직접 켤 때에만 수치형 특징 24개를 이 기기 안에서 비교합니다. 설정에서 분석 이력만 삭제하거나 전체 삭제를 실행할 수 있습니다.

## 실행

```bash
npm install
npm run tauri dev
```

Windows 배포 빌드:

```bash
npm run tauri build
```

> 현재 브라우저 `npm run dev`에서는 데모 데이터로 UI를 확인할 수 있습니다. Tauri 런타임에서는 Rust 명령이 연결됩니다.
