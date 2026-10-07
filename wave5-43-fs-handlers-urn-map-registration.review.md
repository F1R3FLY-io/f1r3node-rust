# result-collector spec timeout bump review (Wave 5 PR 5.43 fix, on branch `wave5-43-fs-handlers-urn-map-registration`)

**No changes needed.** Test-infrastructure fix for CI timeouts that cropped up after slice 5.36 made genesis-build slower.

Audited:

- Both affected specs swap `Duration::from_secs(10)` → `GENESIS_TEST_TIMEOUT` (60s, the shared constant already used by sibling specs like `pos_spec` / `make_mint_spec`). `use std::time::Duration;` correctly removed; `use crate::genesis::contracts::GENESIS_TEST_TIMEOUT;` added.
- Inline comments at both call sites explain the root cause (fs_generator addition in slice 5.36, ~800-line FsGenesis source pushing CI genesis-build past 10s) and the semantics-preservation argument.
- `failing_result_collector_spec` tests that failing assertions complete-and-fail — bound is just protective, not semantic. ✓
- `timeout_result_collector_spec` loads a `Nil` program and asserts `has_finished == false` — needs enough time for pipeline setup, 60s is enough. ✓
- No production code touched.
- Commit message cross-references PR #660's CI failure traces (`BugFoundError("Timeout of 10s expired in phase 'genesis-build' ..."`), so the root-cause narrative is anchored to real evidence.

Note on branch naming: branch is `wave5-43-fs-handlers-urn-map-registration` but the HEAD commit is the "Wave 5 PR 5.43 fix" (not slice 5.43 itself). The branch bundles the slice-5.43 commit (previously reviewed) + this test-timeout fix on top. The slice-5.43 commit's content is unchanged from the version I reviewed earlier.
