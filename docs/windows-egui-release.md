# BANDO HUB v0.8.0 Windows 배포

> 보안 개선 브랜치: [필수 보안 게이트](security-v0.8.0-release-gate.md)를 우선 적용한다. 기존 자동 OCR 다운로드는 비활성화되었으며 독립적으로 승인된 OCR 패키지, 잠금 파일 및 보안 검사 결과 없이는 배포할 수 없다. 아래의 기존 자동 준비 설명보다 보안 게이트를 우선한다.

## 빌드 컴퓨터
Windows x64, Rust MSVC x64 toolchain, Visual Studio C++ build tools가 필요하다. 저장소 최신 main을 받은 뒤 루트의 PACKAGE_WINDOWS_EGUI.cmd를 실행한다. 최초 OCR 패키지 준비나 Cargo 의존성 다운로드에는 인터넷이 필요할 수 있다. 기존 OCR 번들을 재사용할 수 있으며 압축 해제본의 OCR 실추론 검증이 실패하면 새 ZIP을 공개하지 않는다.

## 결과
release/BANDO-HUB-v0.8.0-windows-x64.zip

- BANDO-HUB.exe: native egui Release
- RUN_BANDO_HUB.cmd: 운영 실행 진입점
- ocr/: CPython, PaddleOCR, 한국어 모델
- README.txt: 실행 안내

버전의 원본은 egui-app/Cargo.toml이다. 배포 ZIP에는 운영/개발 DB, 사용자 설정, 발주 원문, 고객 자료를 넣지 않는다. 패키징은 앱을 실행하거나 업무 DB를 직접 열지 않는다.

## 배포 컴퓨터
1. 기존 프로그램 종료 후 운영 DB와 기존 실행 파일을 백업한다.
2. ZIP 전체를 별도 폴더에 압축 해제한다.
3. RUN_BANDO_HUB.cmd로 실행한다. EXE만 직접 실행하면 일부 기존 쓰기 권한 환경변수가 설정되지 않는다.
4. 기존 %APPDATA%\com.bandostand.bandohub.tauri\bando-hub-v001.sqlite의 호환 스키마가 필요하다. 신규 운영 DB 초기화는 이 패키지 범위가 아니다.
5. 설정 버전, 접수, 상품/거래처 수정, 엑셀 송장 출력 및 회신, OCR, 주문 내역 및 일일 결산을 점검한다. 기존 DB에 자동 색상 정리 경로가 있으므로 업그레이드 전 백업은 필수다.

빌드와 OCR 검증 성공 여부는 Windows에서 실제 패키징 실행 후 판정한다. 운영 배포 승인 및 UI 실화면 검증은 별도이며 이 문서는 합격 증거가 아니다.
