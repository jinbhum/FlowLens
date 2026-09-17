# FlowLens

Windows용 Tauri 기반 개인 생산성 분석 앱입니다. 사용자의 작업 흐름을 방해하지 않으면서 **활성 앱, 앱 포커싱 시간, 키보드·마우스 활동량, 웹사이트 사용 패턴**을 로컬에서 집계하고 대시보드로 보여주는 것을 목표로 합니다.

## 현재 구현

- Windows 활성 창을 1초 간격으로 실제 추적하고 앱별 포커싱 시간/전환 횟수를 집계
- 어두운 생산성 대시보드: 실제 활동 시간, 앱 활성화, 키보드·마우스 액션
- 시간대별 집중 흐름 그래프와 패턴 기반 추천 카드
- 가장 많이 사용한 앱과 자주 방문한 웹사이트 목록
- 추적 일시정지/재개 UI 및 설정 페이지의 수집 데이터 전체 삭제
- Tauri Rust command: `set_tracking`, `get_tracking_state`, `capture_snapshot`, `clear_all_data`
- Windows 활성 창 제목을 수집하는 Windows API 연동 (`GetForegroundWindow`, `GetWindowTextW`)
- Windows 앱 데이터 폴더의 `activity.json`에만 저장하며 임시 파일 교체로 기록

## 개인정보 보호 원칙

FlowLens는 화면 캡처, 키 입력 내용, 문서 본문, 브라우저 URL을 기록하지 않습니다. 수집 대상은 활성 창 제목/활성 시간과 입력 이벤트 개수 같은 집계값이며 Windows 앱 데이터 폴더의 `activity.json`에만 저장됩니다. 설정 페이지에서 전체 삭제를 실행하면 이 파일과 메모리 상태가 함께 삭제됩니다.

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
