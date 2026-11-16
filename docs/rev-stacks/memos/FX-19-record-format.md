# FX-19 and the record format (lane A2 to lane C)

Base: `patch/FX-20`, from `patch/FX-8b` at b2bc4a75e. Lane C's memo: `docs/rev-stacks/memos/FX-19-declared-bound.md` on `patch/FX-19` at 527665315. Names: V1…Vn validators, u1…um deployers, d1…di deployments, B1…Bj blocks. W is an installer's wallet.

**Purpose:** agree with lane C how the refusal event leaves the record format when FX-19 lands. Lane A2 owns the format. Lane C owns the membrane.

## 1. What leaves the format

| Item | Location | Change |
|---|---|---|
| `RefusalEventProto` | `models/src/main/protobuf/CasperMessage.proto` | Remove the message. Keep `refusal = 4` in the `EventProto` oneof as `reserved 4;` and `reserved "refusal";`. Do not reuse tag 4. |
| `Event::Refusal`, `RefusalEvent` | `models/src/rust/casper/protocol/casper_message.rs` | Remove. A decoder that finds tag 4 refuses the event, as for an unknown event. |
| `to_event`, `refusals_in`, `refusals_of`, `system_refusals`, `with_refusals` | `casper/src/rust/v6/stops.rs` | Remove. `settled_work` and the other settlement helpers stay. |
| `replay_with_refusals` and the `logged` refusals in `replay_held` | `casper/src/rust/v6/executor.rs` | Remove. Replay checks the firings from the logged COMMs, as lane C's memo section 4 gives. |
| The close's funding meter and `accept_metered` on the close | `executor.rs` (`close_gate`), `surface.rs` | Lane C removes the meter and `accept_metered`. Lane A2 removes the call sites in `execute_prior` and `replay_after`. |
| Refusal filters | `event_converter.rs`, `replay_cache.rs`, `footprint.rs` | Remove the `Event::Refusal` arms. |

The block record (`BlockRecordProto`) has no refusal field of its own. The refusals ride in the user logs (`RecordEntryProto` field 13) and in the close's system-deploy event list. After the change those logs hold only produce, consume and COMM events.

## 2. What stays in the format

- The per-wallet totals each run draws, in the settlement (`SettlementProto`).
- The κ field (`BlockRecordProto` field 20, FX-20). κ is the close's own PoS work. It does not include the work under sealed firings that the close causes, which W pays under FX-19's bound.
- The void reasons. Tag 1, (a), stays reserved for B6.

## 3. Order of the patches

1. FX-20 lands first. It keeps today's gating of continuations that the close fires against their installer (Decision 120, interim), so it keeps the close's funding meter.
2. FX-19 (lane C) removes the membrane parts and adds the declared bound.
3. Lane A2 adds a format patch on top of FX-19 (working name FX-19-format), with the changes in section 1. Lane A2 starts it when lane C names the FX-19 commit. Size: about half a day with tests.

Alternative: lane C makes the format change inside FX-19, and lane A2 reviews it. This alternative saves one patch, but two lanes then edit the record format at the same time. Lane A2 recommends the order above.

## 4. Requests to lane C

1. Confirm the list in section 1, or name what is missing.
2. Confirm that the replay of a sealed firing needs no new record field. Lane C's memo section 4 says that replay checks the logged COMMs only. If FX-19 needs a field (for example, the bound b per firing), name it, and lane A2 adds it in the format patch. The next free `BlockRecordProto` number is 21.
3. Confirm the order in section 3.

## 5. Decision 88 labels

- *Known issue:* until FX-19 lands, the refusal event stays in the format and the interim gating stays. Reason: Decision 120 keeps the interim gating until FX-19's bound replaces it. No record is incorrect in the meantime.
