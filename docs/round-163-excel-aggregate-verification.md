# 163차 엑셀 취합 검증 명세 상태

D01~D07 구현 검증용 PR #68. 아래는 T01~T50의 현재 테스트 연결 상태이며, CI 실행 결과는 별도로 확인한다.
"연결"은 해당 규격의 전부 또는 일부를 실제 테스트에서 다룬다는 뜻이며, 실행 성공이나 완전 커버리지를 뜻하지 않는다.

| ID | 현재 검증 또는 잔여 |
|---|---|
| T01 | actual_publish_rebuilds_from_db_after_damage_and_preserves_generation (취합 최초 생성; 개별 추출과 동시 검증 잔여) |
| T02 | actual_publish_rebuilds_from_db_after_damage_and_preserves_generation |
| T03 | ten_exports_accumulate_without_cross_origin_or_date |
| T04 | ten_exports_accumulate_without_cross_origin_or_date (동시 요청 검증 잔여) |
| T05 | 기존 save() 순번 동작; 취합 파일 존재 통합 검증 잔여 |
| T06 | serialized_reply_keeps_q_waybill_and_g_freight_as_text; 실제 헤더/열 형식 전체 비교 잔여 |
| T07 | duplicate_export_id_not_duplicate_identical_rows |
| T08 | transaction_rollback_and_generation_coalescing (예약 롤백; 실제 추출 DB 오류 주입 잔여) |
| T09 | duplicate_export_id_not_duplicate_identical_rows |
| T10-T12 | 기존 factory_reply_inbox 테스트군; 신규 취합 통합 검증 잔여 |
| T13 | process_kill_recovers_all_publication_boundaries |
| T14 | transaction_rollback_and_generation_coalescing (멀티 프로세스 경합 잔여) |
| T15-T17 | latest_reply_wins_without_approval_time_reversal (10회 실제 회신 적용 검증 잔여) |
| T18-T20 | 기존 공장 회신 업무 규칙 테스트군; 취합 제외 통합 검증 잔여 |
| T21-T22 | latest_reply_wins_without_approval_time_reversal; 실제 늦은 승인 회귀 검증 잔여 |
| T23 | legacy_correction_reads_original_and_missing_original_fails_closed |
| T24 | serialized_reply_keeps_q_waybill_and_g_freight_as_text |
| T25-T27 | actual_publish_retries_locked_file_without_losing_generation (10회 잠금 갱신 잔여) |
| T28-T29 | 권한/디스크 부족 장애 주입 잔여 |
| T30 | process_kill_recovers_all_publication_boundaries (기록 도중 종료; 쓰기 실패 주입 잔여) |
| T31 | windows_exclusive_handle_preserves_old_file_on_sharing_violation |
| T32-T33 | 경로 해제/복원 장애 시험 잔여 |
| T34-T37 | process_kill_recovers_all_publication_boundaries |
| T38-T39 | actual_publish_rebuilds_from_db_after_damage_and_preserves_generation |
| T40 | retry_delays_cap_and_new_work_resets_wait; 재실행 장기 대기 시험 잔여 |
| T41-T42 | ten_exports_accumulate_without_cross_origin_or_date; 실제 자정 저장 지연 잔여 |
| T43 | legacy_correction_reads_original_and_missing_original_fails_closed (과거 파일 불변 통합 검증 잔여) |
| T44 | legacy_date_uses_korea_midnight_and_leap_year |
| T45 | historical_legacy_reply_waits_for_configuration_then_keeps_original_binding |
| T46 | 운영/TEST 격리 실행 검증 잔여 |
| T47 | DB 메타데이터 기반 취합 구현; 대량 벤치마크 잔여 |
| T48 | UI 비차단 실측 잔여 |
| T49-T50 | 기존 업무 트랜잭션 및 회신 비용 경계; 취합 연계 실패 주입 검증 잔여 |

추가: legacy_original_is_verified_and_read_from_exact_row, stale_temp_cleanup_ignores_original_and_unknown_files, restore_reopens_generation_and_preserves_business_revision, missing_export_rows_never_generates_silent_empty_workbook.
과거 outputRows가 없는 추출은 불완전한 파일을 생성하지 않고 보류한다. 기존 회신 원본이 없거나 해시가 불일치하면 보류한다. 복원 전 이력에만 해당하는 취합 파일은 임의 삭제하지 않는다. 실제 Excel 잠금, 대량 데이터 및 Windows/Linux 종료 실기 결과는 별도로 검증한다.
