# 88차 — 상태 변화에 따른 레이아웃 할당 정적 검사

기존 `egui-async-layout-contract-20261123.md`의 불변 계약을 보강한다. 별도 디자인 토큰을 추가하지 않는다.

- `node scripts/check-egui-state-layout.mjs`: egui-app/src 전체 후보 인벤토리(검토용).
- `node scripts/check-egui-state-layout.mjs --base <40자리 SHA> --require-base`: 기준 커밋 대비 신규 후보 또는 중복 증가를 실패 처리. 기준을 읽을 수 없으면 실패.
- 검사 대상: 상태 관련 if/match 안의 직접 UI 할당, 여백, 구분선, 버튼, 공통 액션 및 고정 슬롯 자체의 조건부 호출. 단락 평가로 조건식 안에서 실행되는 버튼도 검사한다.
- 공통 fixed_row/fixed_status_slot/fixed_result_viewport의 무조건 할당된 내부와 별도 context menu/hover UI는 국소 콘텐츠 변경으로 처리한다. 일반 ScrollArea 전체를 무조건 면제하지 않는다.
- 기존 후보는 인벤토리에 포함하되 새 후보의 CI 차단과 구분한다. 파일 전체 또는 특정 화면 전체를 면제하지 않는다. 신규 후보는 원칙에 맞춰 수정하고 필요시 별도 실제 Rect 테스트를 추가한다.
- 문자열·주석·cfg(test) 모듈을 구분하는 토큰 기반 휴리스틱이다. Rust AST 전체 의미 분석이 아니며 이름에 드러나지 않는 간접 상태/헬퍼 호출 등을 모두 잡는다고 주장하지 않는다.
- 0.5pt Rect 테스트 및 Windows 실제 시각 검수는 독립적으로 유지한다. 정적 검사 통과만으로 실제 화면 합격으로 처리하지 않는다.
