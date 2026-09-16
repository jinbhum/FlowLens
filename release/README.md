# FlowLens Windows Release

이 폴더에는 GitHub Actions의 Windows 러너에서 생성한 FlowLens 설치파일이 들어갑니다.

- `.msi`: Windows 설치 패키지
- `.exe`: Tauri 번들에서 생성된 실행 파일 또는 설치 실행 파일

로컬 Linux 환경에서는 Windows 네이티브 바이너리를 직접 빌드할 수 없으므로, 저장소의 **Actions → Build Windows Release → Run workflow**를 실행하면 `FlowLens-Windows` 아티팩트가 생성됩니다. `v0.1.0` 같은 태그를 푸시하면 GitHub Release에도 자동으로 첨부됩니다.
