# FX-19-format: the refusal event leaves the record format

Base: `patch/FX-8b` at 914f2c47b, after FX-20 and FX-19. This is step 3 of the merge order of 2026-11-05. The memo is `memos/FX-19-record-format.md`.

## What changed

- **Proto.** `RefusalEventProto` is removed. `EventProto` has `reserved 4;` and `reserved "refusal";`. Tag 4 is not reused.
- **Model.** `Event::Refusal` and `RefusalEvent` are removed from `casper_message.rs`. An event with tag 4 now decodes to no instance, and `Event::from_proto` refuses it as a malformed event.
- **Converter.** `to_rspace_event` returns dev's type again (`RspaceEvent`, not `Option`), because every event now converts. Its five callers use `map` again, as on dev.
- **Other uses.** The refusal arms are removed from `replay_cache.rs` (normalization and heap size), from `footprint.rs` and from the event print in `runtime_manager_test.rs`.
- FX-19 had already removed `stops.rs`, `accept_metered`, the membrane's refusal machinery and the refusal events in the logs.

## Tests

- `models::casper_message::event_tests::an_event_with_the_reserved_refusal_tag_does_not_decode`.

## Decisions

### D1. Dev's `to_rspace_event` signature returns

- **Decision:** restore dev's `event_converter.rs` and the `map` calls.
- **Alternatives:** keep `Option` and `filter_map`.
- **Reason:** the rule is to prefer dev's code. The `Option` existed only to skip refusal events.
- **Reversible:** yes.

## Results

- `models --lib event_tests`: 1 passed.
- `casper --lib` (replay cache, event, footprint, `v6::`): 146 passed.
- `casper --test mod v6::`: 123 passed, 9 ignored. `multi_node::v6`: 11 passed. `util::rholang::runtime_manager_test`: 39 passed.
