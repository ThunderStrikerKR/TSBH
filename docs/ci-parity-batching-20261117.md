# 36차 Rust·Node CI 중복 작업 제거

- 단일 frontend job은 npm ci 1회, Node 패키지 캐시 재사용, TypeScript/Vite 빌드·전체 Node 회귀검사를 유지한다.
- 기존 Windows distribution-script-syntax 전용 runner는 제거하고, PowerShell 구문 ParseFile 검증을 Ubuntu frontend job의 pwsh 스텝으로 옮겼다. Release/배포 작업은 실행하지 않는다.
- 이전 output-rust + integrated-rust-core는 rust-core-batch에서 단일 CARGO_TARGET_DIR를 공유해 두 크레이트를 검증한다.
- 기존 order-save/rules-comprehensive/history 3개 parity job은 rules-parity-batch에서 npm ci 1회, 공유 CARGO_TARGET_DIR로 총 5개 기존 명령을 유지한다.
- 기존 output/react-native-reception/factory-reply 3개 parity job은 output-reception-factory-parity-batch에서 npm ci 1회, 공유 CARGO_TARGET_DIR로 총 17개 기존 명령을 유지한다.
- 각 배치에 Cargo registry/git 캐시를 추가하고 Node는 setup-node npm 캐시를 사용한다. 서로 다른 GitHub runner 사이의 target 공유를 주장하지 않고, 같은 job 내부 Rust 빌드 재사용만 보장한다.
- 원래 24개 독립 테스트 명령을 모두 유지했는지 tests/ci-parity-batching-contract.test.mjs가 확인한다.
- final-rust-migration-gate는 새로운 10개 needs 결과를 검사한다. 일반 main/PR에는 네 가지 컴파일·비교 job 전부 성공이 필수이고, 수동 full에는 기존 Windows native/Release/WebView2 관련 지연 실행 게이트까지 성공해야 한다.
- 공유로 인해 개별 job의 실행 시간이 길어질 수 있다. 개선 판정은 전체 runner 분과 wall clock 실행 시간을 실제 Actions 기록과 비교해야 한다.
