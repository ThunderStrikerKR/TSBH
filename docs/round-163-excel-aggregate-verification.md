# 163차 엑셀 취합 검증 명세 상태

D01~D07 구현 검증용 PR #68. 아래는 T01~T50의 현재 테스트 연결 상태이며, CI 실행 결과는 별도로 확인한다.
"연결"은 해당 규격의 전부 또는 일부를 실제 테스트에서 다룬다는 뜻이며, 실행 성공이나 완전 커버리지를 뜻하지 않는다.

| ID | 현재 검증 또는 잔여 |
|---|---|
| T01 | exported_xlsx_to_factory_inbox_and_settlement_roundtrip (개별 추출과 DB 예약 및 실제 취합) |
| T02 | actual_publish_rebuilds_from_db_after_damage_and_preserves_generation |
| T03 | ten_exports_accumulate_without_cross_origin_or_date |
| T04 | ten_exports_accumulate_without_cross_origin_or_date (동시 요청 검증 잔여) |
| T05 | aggregate_filename_never_consumes_individual_sequence_and_date_is_frozen |
| T06 | exported_xlsx_to_factory_inbox_and_settlement_roundtrip (16열 전체 비교), serialized_reply_keeps_q_waybill_and_g_freight_as_text |
| T07 | duplicate_export_id_not_duplicate_identical_rows |
| T08 | exported_xlsx_to_factory_inbox_and_settlement_roundtrip (실제 추출 트랜잭션에서 예약 실패 트리거) |
| T09 | duplicate_export_id_not_duplicate_identical_rows |
| T10-T12 | ten_real_replies_publish_only_latest_and_replays_do_not_change_customer_payment; same_filename_with_new_bytes_updates_and_invalid_reply_never_enters_aggregate |
| T13 | process_kill_recovers_all_publication_boundaries |
| T14 | independent_connections_cannot_publish_a_stale_generation (두 독립 DB 연결 경합 재현; parallel_processes_publish_one_complete_generation (4개 프로세스)) |
| T15-T17 | ten_real_replies_publish_only_latest_and_replays_do_not_change_customer_payment |
| T18-T20 | 기존 공장 회신 업무 규칙 테스트군, same_filename_with_new_bytes_updates_and_invalid_reply_never_enters_aggregate (수취인 불일치 취합 제외; 빈 송장 실제 접수, scoped_factory_reply_matches_full_result_and_checks_unrelated_waybills (다른 배송 송장 중복 제외)) |
| T21-T22 | ten_real_replies_publish_only_latest_and_replays_do_not_change_customer_payment, explicit_ambiguity_resolution_preserves_other_order_and_rejects_wrong_target (최신 회신 이후 늦은 승인 거부) |
| T23 | legacy_correction_reads_original_and_missing_original_fails_closed |
| T24 | serialized_reply_keeps_q_waybill_and_g_freight_as_text |
| T25-T27 | actual_publish_retries_locked_file_without_losing_generation, ten_locked_generations_coalesce_to_one_latest_workbook (10회 장애 주입) |
| T28-T29 | injected_io_failures_preserve_old_workbook_and_pending_revision (오류 주입; 실제 OS ACL/디스크 고갈 잔여) |
| T30 | process_kill_recovers_all_publication_boundaries, injected_io_failures_preserve_old_workbook_and_pending_revision |
| T31 | windows_exclusive_handle_preserves_old_file_on_sharing_violation |
| T32-T33 | missing_archive_root_recovers_without_redirecting_to_other_folder |
| T34-T37 | process_kill_recovers_all_publication_boundaries |
| T38-T39 | actual_publish_rebuilds_from_db_after_damage_and_preserves_generation |
| T40 | retry_delays_cap_and_new_work_resets_wait, durable_backoff_survives_connection_restart |
| T41-T42 | aggregate_filename_never_consumes_individual_sequence_and_date_is_frozen (고정 날짜 인자); 실제 시스템 시계 자정 전환 잔여 |
| T43 | legacy_correction_reads_original_and_missing_original_fails_closed, next_day_reply_does_not_modify_previous_daily_file |
| T44 | legacy_date_uses_korea_midnight_and_leap_year |
| T45 | historical_legacy_reply_waits_for_configuration_then_keeps_original_binding |
| T46 | isolated_archive_guard_rejects_other_workspace_before_creating_folders (경로 검사; 실제 앱 실행 프로필 통합 검증 잔여) |
| T47 | large_daily_workbook_roundtrip_without_individual_files (5,000행, 시간 로그; 실행 결과 대기) |
| T48 | worker_does_not_block_egui_frames_while_publication_waits (작업 스레드 대기 중 10개 egui 프레임 렌더링; 실제 전체 앱 입력/프레임 시간 측정 잔여) |
| T49-T50 | reply_commit_is_not_reverted_when_aggregate_path_is_unavailable, ten_real_replies_publish_only_latest_and_replays_do_not_change_customer_payment |

추가: legacy_original_is_verified_and_read_from_exact_row, stale_temp_cleanup_ignores_original_and_unknown_files, restore_reopens_generation_and_preserves_business_revision, missing_export_rows_never_generates_silent_empty_workbook.
과거 outputRows가 없는 추출은 불완전한 파일을 생성하지 않고 보류한다. 기존 회신 원본이 없거나 해시가 불일치하면 보류한다. 복원 전 이력에만 해당하는 취합 파일은 임의 삭제하지 않는다. 실제 Excel 잠금, 대량 데이터 및 Windows/Linux 종료 실기 결과는 별도로 검증한다.

추가 백업 검증: aggregate_jobs_survive_real_backup_and_restore_with_new_generation (실제 SQLite 백업/복원). 최신 커밋 CI 실행 전에는 통과로 표시하지 않는다.

동일 DB revision에서 과거 회신 경로 연결 집합이 달라지면 이전 작업의 파일 교체를 차단하도록 최종 갱신 검사를 보강했다. same_revision_legacy_binding_change_prevents_stale_replace로 해당 CAS 조건을 직접 시험한다.

운영 환경 실기 잔여: 실제 Excel 열림 상태, OS 권한 및 디스크 고갈, 전체 앱 UI 입력/프레임 시간, 실제 시계 자정 전환. 해시나 신뢰할 수 있는 원본 출력 행이 없는 과거 발송 이력은 임의 복원하지 않는다.

실제 자식 프로세스 4개가 동일 SQLite 취합 버전을 동시에 발행하는 검증 및 다음 날 회신의 전날 파일 분리 검증을 추가했다. 모두 최신 CI 확인 대기.

### Event-driven publication
The five-second aggregate polling loop is removed. One resident worker is started once; committed exports, accepted replies, approvals, archive configuration and restore wake it. Durable retry deadlines control retry wakeups. A 15-minute safety reconciliation remains for missed events, external changes and file integrity. The factory inbox retains its existing five-second scan and stability checks. Tests: next_due_ignores_completed_and_tracks_actual_retry; resident_worker_waits_for_events_and_durable_retry_without_idle_polling. Both tests await latest CI.
