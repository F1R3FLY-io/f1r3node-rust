# D1: Soak disk admission

```json
{
  "artifact": {
    "path": "scripts/run-merge-recovery-soak.sh",
    "commit": "bc1e1638068708f90f4b65574bc2b6b762bff73f",
    "id": "scripts-run-merge-recovery-soak-sh"
  },
  "claim_id": "CLAIM-SOAK-001",
  "scope": "D1 admission sub-obligation only",
  "status": "pending",
  "adapter": "direct-TLC-and-production-boundary-test",
  "evidence": {
    "kind": "historical-production-and-bounded-model-red-green",
    "ref": "#verification-manifest"
  },
  "waiver": null,
  "verified_at": null
}
```

## Local result

The historical driver at `0f5d2b7414786cd27b8a686ca636485c35a5edce` admitted one iteration after cleanup left 7,000 MiB free. The regression failed for that admission.

The historical floor-only configuration violated `AdmissionRequiresBand` with TLC exit 12. Its counterexample also admits at 7,000 MiB after zero reclamation.

Correction `ca85cfe3ecf73123b044b188b0c363fdbdff1e33` refused admission and recorded a failed run with zero iterations. The current driver has identical bytes and produced the same result.

The corrected model completed with 54 distinct states and no error. Coverage records both admission after sufficient reclamation and refusal publication.

No production correction was added during this cycle. The [model documentation](../../formal/tlaplus/soak_disk/README.md) defines the finite bounds and production mapping.

## Evidence limits

The production test uses a disposable, unprivileged container without host mounts or network access. External process fixtures replace disk samples, Docker operations, and workload execution.

The first fixture attempt failed on file permissions before the driver ran. That attempt is not RED evidence. Later retained runs include every required metric and summary helper.

This cycle does not establish a disk-growth bound, a guardian deadline, durable crash recovery, or finalization performance. It does not formally prove the Bash implementation.

`CLAIM-SOAK-001` remains proposed and unratified. `CLAIM-SOAK-GATE-001` and `CLAIM-FINALITY-002` remain pending.

## Remaining candidate obligations

- Confirm hosted execution of the new production regression and both disk configurations.
- Obtain maintainer review of the model-to-code correspondence and proposed mandatory scope.
- Complete G0 enforcement and acceptance identity.
- Complete D2, D3, and full-duration acceptance before discharging the resource claim.

The verification manifest below binds the tested sources and retained evidence. The claim inventory independently binds this record. Historical B1–B3 and repin manifests remain unchanged.

The record contains complete D1 TLC logs and exact excerpts from other retained logs. Raw files remain outside Cargo's target directory. The backup is local, not off-host.

The commit includes formatting-only changes in the metric and summary helpers. The isolated driver tests passed again with those exact bytes.

## Verification manifest

```json
{
  "schema_version": 1,
  "role": "historical-admission-red-green",
  "authority": "bounded-model-and-production-boundary-regression-only",
  "verified_at_utc": "2026-09-08T21:18:28Z",
  "candidate_base": "bc1e1638068708f90f4b65574bc2b6b762bff73f",
  "binding": "source-sha256",
  "source_inputs_sha256": {
    ".github/workflows/ci.yml": "c578fb6f1595378a729fe523dcc210ac18bdc281b9097ae86c1c0e99bc1bd925",
    ".github/workflows/slashing-tests.yml": "460be11512b1f01f79387d7a3dad6c60d3c45627b5790436f9db6e165eac69fb",
    "formal/tlaplus/carrier_index/CarrierIndex.tla": "5aae4327a4aca056d571d56d778dfe82684165fb4faa7123f480b8afd4d67575",
    "formal/tlaplus/carrier_index/MC_CarrierIndex.cfg": "4c5dc2805bb8ca65e1e4ff2fa9059efb30aef0b9caf71d7317d837d9c4249cd2",
    "formal/tlaplus/carrier_index/MC_CarrierIndex.tla": "2018d282687b9489c801c5f279be34fe680e896fb2a60b0c42936fa2f47db0cb",
    "formal/tlaplus/carrier_index/MC_CarrierIndex_dag_first_pre_fix.cfg": "ca704ce77b04c0aba4b0d6e3879ccc785b826dc166446b4ad634d6b0c480d9e1",
    "formal/tlaplus/carrier_index/MC_CarrierIndex_dag_first_pre_fix.tla": "186e6f96007d49f027d12c5a21e1e0c3c9e07f8fa1a948a05665b03842cc0f08",
    "formal/tlaplus/carrier_index/MC_CarrierIndex_read_failure_pre_fix.cfg": "2957030a60ce09a49d49ff2251cd100170e01e727c944d51aaab08e130b10dee",
    "formal/tlaplus/carrier_index/MC_CarrierIndex_read_failure_pre_fix.tla": "9316c773d6b2a7a6842382662fe737be9a25398b3fe559e89ebabc3221443a55",
    "formal/tlaplus/replay_liveness/MC_ReplayHotLoop.cfg": "cf88ab98868e7135b5c43a56e4310db087ecf9d5c2bff36359e0e7959920749b",
    "formal/tlaplus/replay_liveness/MC_ReplayHotLoop.tla": "36ce516043089ae4e672c2c0ec5fd89f3a115c2bd9c3606a5f30503de2c9d25f",
    "formal/tlaplus/replay_liveness/MC_ReplayHotLoop_quadratic_pre_fix.cfg": "e4d9b18ca321c2896a36e234be4f9cb31819cffa9cfcccdf06523dd79594a202",
    "formal/tlaplus/replay_liveness/MC_ReplayHotLoop_task_chain_pre_fix.cfg": "7631efee0837f04b4a1c5c2c59b6cbe2f72503c75bd6307be1e4f65f49a1ce91",
    "formal/tlaplus/replay_liveness/ReplayHotLoop.tla": "460651b3d5ac5b58d43fb89bdfeba2d3dd2cab71897de88e8185298e4f4cf2b4",
    "formal/tlaplus/soak_disk/MC_SoakDisk.cfg": "baa1c4153fc8c485d6d15ce267adb601f1585a4e24f0686ff07f3bec8b9894f4",
    "formal/tlaplus/soak_disk/MC_SoakDisk.tla": "3f1c1f5589f430a424efe8c8916f7b725b1bc33bd39e49959ffd3c63d11ab039",
    "formal/tlaplus/soak_disk/MC_SoakDisk_floor_only_pre_fix.cfg": "0ee62f80739ff4f03d47555b4cb4550738d909f6a247a66fd62b73ac30051132",
    "formal/tlaplus/soak_disk/MC_SoakDisk_floor_only_pre_fix.tla": "e15a4a65454687dffb48356319845bb0a16db5e53e6f05542784e655ca307890",
    "formal/tlaplus/soak_disk/SoakDisk.tla": "17219df7246a0e349fcb9072590a2576fa568f38755364c7f637f0227bede942",
    "scripts/bench/collect-soak-metrics.sh": "6a04b8aa2b3335012eba6b66793b784c28fbedfd5160844c27e0d8abf01e7554",
    "scripts/bench/soak-disk-test.Dockerfile": "85340de51da31c1ea8ded3c4a20a8e0cf8f85a26b0cfc13ab4b29768d5048aa9",
    "scripts/bench/soak-metrics.json": "d4a71e7996dc0b0a74fcfc339f5164121c246db7c2f4e27114b7ea80c2edfef7",
    "scripts/bench/test-run-merge-recovery-soak.sh": "2d0bc41e2945203ea2c75659f9a8f9ee6b6f6538f645945b6247322d2ba4c4b3",
    "scripts/bench/test-soak-disk-admission.sh": "c80222f3455d9f0f3374486a911d1c8a2310063479967fcd42a3f8f2b94dfe08",
    "scripts/bench/write-soak-summary.sh": "d1dbbceea1fd6e250f6f36289ed0cfae07b4c9e9e985afd3568f1d276e9bd4c0",
    "scripts/ci/check-tla-invariants.sh": "f347dab7818a92c1e46cace6dc13b08a1df4025a496558a7150ada86318978a3",
    "scripts/ci/test-check-tla-invariants.sh": "94e0375147320a29930f97497bc789bef4841924d5c8eced49a7630c6b5054a3",
    "scripts/ci/test-soak-pr-formal-gate.sh": "0f1099900d0b9538f2e7344f25fca35949b0d7ac877715ec33a09a490cd7b8af",
    "scripts/run-merge-recovery-soak.sh": "2107899197cc3b676dd01a458c3b67c79708445277d371f7e4ccb45f4cdd4abb"
  },
  "inventory_binding": "The inventory hashes this record. This record excludes itself and the inventory from its source hashes.",
  "raw_root": "/home/bf_spark/soak-evidence/f1r3node-rust/d1-20260908T184202Z",
  "raw_sha256": {
    "commit-review-20260908T211541Z/check-workflow-invariants.txt": "58aaa9d3aa8ec252a7e93ba60dea09172058b7c4e30e5a36e54a8b57657424ab",
    "commit-review-20260908T211541Z/classifier.txt": "bd544aabc03579e4807c2a7db3ed99477e769c379f5d3539b15584c32d8875cf",
    "commit-review-20260908T211541Z/composed-tla.txt": "303aeaaf2ed03f53ee886d031eca6731838ed20c5c55fc240a0581564243ed53",
    "commit-review-20260908T211541Z/composed-tlc/tlc-carrier_index-MC_CarrierIndex.log": "95b234ef4c79fd9f5d8929f48901ed9ec8909ea6efc500e3bb95218bce3bbf13",
    "commit-review-20260908T211541Z/composed-tlc/tlc-carrier_index-MC_CarrierIndex_dag_first_pre_fix.log": "0db96a32b6b9e9d83262d15c70cc4672306d00e89d5320b329bc7dcc10039d4f",
    "commit-review-20260908T211541Z/composed-tlc/tlc-carrier_index-MC_CarrierIndex_read_failure_pre_fix.log": "d4e5429d85632d67198375014179411ac69715946d2107ff6814e33c29737f2e",
    "commit-review-20260908T211541Z/composed-tlc/tlc-replay_liveness-MC_ReplayHotLoop.log": "22f1b422539a27c047c3e370f4ea00da05e4d17e3cc42ce7631145f227b486ef",
    "commit-review-20260908T211541Z/composed-tlc/tlc-soak_disk-MC_SoakDisk.log": "8b292f9234104ccaf1d49b2122ba58a22549f2a05d67d4467bd84f5783f2a49e",
    "commit-review-20260908T211541Z/composed-tlc/tlc-soak_disk-MC_SoakDisk_floor_only_pre_fix.log": "b06defe62671fe5a644d175e7d83cf586b2c2a032e02282c1963c04fd55f0932",
    "commit-review-20260908T211541Z/docker-version.json": "e594ea1605d49bd677d8dffce8296260f42a0e59752b6048bc1f9ab6abf12446",
    "commit-review-20260908T211541Z/driver-suite-container.json": "4c745f79a720ec2bce9c6990adc0eefc29b45c05c4446b384359bb81a188b44e",
    "commit-review-20260908T211541Z/driver-suite-finished.json": "e4b85289811bd3f09d3e4c99f546a26827f6716c7629008517a844bc5f578790",
    "commit-review-20260908T211541Z/driver-suite.txt": "bf05567217400498d2dff1088c82ea977a34a00cb0517f1e4e3fecbb8727575e",
    "commit-review-20260908T211541Z/formal-green-exit.txt": "9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa",
    "commit-review-20260908T211541Z/formal-green.txt": "bd511bcf96f685b5496d79d08e8d7e80439aab0796829078738a9e32e58919e2",
    "commit-review-20260908T211541Z/formal-red-exit.txt": "a1fb50e6c86fae1679ef3351296fd6713411a08cf8dd1790a4fd05fae8688164",
    "commit-review-20260908T211541Z/formal-red.txt": "e169c031a062dbb3eaa010cc37188e5f80f950e4cd4a2c397cc45d94acf93d51",
    "commit-review-20260908T211541Z/java-version.txt": "6d20c4776797c0ca5371780ff43f391a588816a9c8401db44ee1c7e575e151f6",
    "commit-review-20260908T211541Z/model/MC_SoakDisk.cfg": "baa1c4153fc8c485d6d15ce267adb601f1585a4e24f0686ff07f3bec8b9894f4",
    "commit-review-20260908T211541Z/model/MC_SoakDisk.tla": "3f1c1f5589f430a424efe8c8916f7b725b1bc33bd39e49959ffd3c63d11ab039",
    "commit-review-20260908T211541Z/model/MC_SoakDisk_floor_only_pre_fix.cfg": "0ee62f80739ff4f03d47555b4cb4550738d909f6a247a66fd62b73ac30051132",
    "commit-review-20260908T211541Z/model/MC_SoakDisk_floor_only_pre_fix.tla": "e15a4a65454687dffb48356319845bb0a16db5e53e6f05542784e655ca307890",
    "commit-review-20260908T211541Z/model/SoakDisk.tla": "17219df7246a0e349fcb9072590a2576fa568f38755364c7f637f0227bede942",
    "commit-review-20260908T211541Z/production-green-corrected-launch.txt": "4d2544e8d4aa6282a9b3996c71de2ab4c3249b952262fdf4450e30095f411928",
    "commit-review-20260908T211541Z/production-green-corrected/container-finished.json": "dc23ae8684652b3e833928deecfe36c0cc05982d4f8f23a1627f3b9089a0c49d",
    "commit-review-20260908T211541Z/production-green-corrected/container-inspect.json": "e2aed8cc52a6824b27ab3cea4a84ed81bc78f4052c610058a7df8d17b9cd0a19",
    "commit-review-20260908T211541Z/production-green-corrected/evidence/docker-commands.txt": "e0897deb7fb18e6b6eedd3f7c3429a7ecbedc48ca846d926b13826acf5ac5291",
    "commit-review-20260908T211541Z/production-green-corrected/evidence/driver-exit.txt": "4355a46b19d348dc2f57c046f8ef63d4538ebb936000f3c9ee954a27460dd865",
    "commit-review-20260908T211541Z/production-green-corrected/evidence/driver.log": "e9e9c5184213e3a08efc1d31d32b71b38251d09569ee0af90ab8c1b21d22bb93",
    "commit-review-20260908T211541Z/production-green-corrected/evidence/output/disk-floor-breach.txt": "c177749756e0c836f6ff819bd1d3d8af03c9333c0a306f340d0212164ecc41c9",
    "commit-review-20260908T211541Z/production-green-corrected/evidence/output/early-exit.txt": "405402f3ba10a3241e908cbff395d41b275c6c1a8e7a8913399cc613fd9c6241",
    "commit-review-20260908T211541Z/production-green-corrected/evidence/output/iterations.json": "37517e5f3dc66819f61f5a7bb8ace1921282415f10551d2defa5c3eb0985b570",
    "commit-review-20260908T211541Z/production-green-corrected/evidence/output/protection-breach.txt": "9b1b26190faf1e6ea68caad1e749833459f206a6a8bec3f66b28bd5a2e244233",
    "commit-review-20260908T211541Z/production-green-corrected/evidence/output/summary.json": "4cb849bc32d9a04b906152c2e3a6f8ae4488f0e2527c4b0f648f77006e5cfc8f",
    "commit-review-20260908T211541Z/production-green-corrected/evidence/output/summary.txt": "ba57b5da3e61f5141be31f0913d9969eb0ff455f630938eeba102372ad3edd82",
    "commit-review-20260908T211541Z/production-green-corrected/evidence/packages.txt": "318dbb0fdcf225941d2cb3c3bd5e7cd327cb6f6ea52a2a7c58ee78d00b459a46",
    "commit-review-20260908T211541Z/production-green-corrected/evidence/source-sha256.txt": "a5dcf4fa97aea2c9ee7dd7b4136e7b2a6e517ec9fb03b80b4c984c537f35020d",
    "commit-review-20260908T211541Z/production-green-corrected/image-inspect.json": "a60bb5d10b384aca542a50147c790b68485a4fc6a7c429f7e9e856b2174c6268",
    "commit-review-20260908T211541Z/production-green-corrected/result.txt": "563444d52e8de88cb6227f1aca5868ab4919b06aaf3f8f3af66b51fa18a67c10",
    "commit-review-20260908T211541Z/production-green-corrected/test-exit.txt": "9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa",
    "commit-review-20260908T211541Z/production-green-current-launch.txt": "f518c5e1be2d48a2da7185a7f236a4b2b8ccb162d17a802f87f0f80572b1abd3",
    "commit-review-20260908T211541Z/production-green-current/container-finished.json": "0952321973c1764f6b2ea11f42dc2331d104b2b1e6e8e8a72df9e4522402a46e",
    "commit-review-20260908T211541Z/production-green-current/container-inspect.json": "2603f47de2e466f46869995b235b260f5d7ac98a4ac5477d4771e44d909ceb07",
    "commit-review-20260908T211541Z/production-green-current/evidence/docker-commands.txt": "e0897deb7fb18e6b6eedd3f7c3429a7ecbedc48ca846d926b13826acf5ac5291",
    "commit-review-20260908T211541Z/production-green-current/evidence/driver-exit.txt": "4355a46b19d348dc2f57c046f8ef63d4538ebb936000f3c9ee954a27460dd865",
    "commit-review-20260908T211541Z/production-green-current/evidence/driver.log": "48648c60980eaa9d17e115ece3052f92534f57d7d27598af27a9fde6edb5c84a",
    "commit-review-20260908T211541Z/production-green-current/evidence/output/disk-floor-breach.txt": "c177749756e0c836f6ff819bd1d3d8af03c9333c0a306f340d0212164ecc41c9",
    "commit-review-20260908T211541Z/production-green-current/evidence/output/early-exit.txt": "405402f3ba10a3241e908cbff395d41b275c6c1a8e7a8913399cc613fd9c6241",
    "commit-review-20260908T211541Z/production-green-current/evidence/output/iterations.json": "37517e5f3dc66819f61f5a7bb8ace1921282415f10551d2defa5c3eb0985b570",
    "commit-review-20260908T211541Z/production-green-current/evidence/output/protection-breach.txt": "9b1b26190faf1e6ea68caad1e749833459f206a6a8bec3f66b28bd5a2e244233",
    "commit-review-20260908T211541Z/production-green-current/evidence/output/summary.json": "077528341b8e80b5ed319a38a1e6f0a55f2f84613dea0aa10ac6ac7f84fa71c1",
    "commit-review-20260908T211541Z/production-green-current/evidence/output/summary.txt": "16c7f8942fbf6be4d38dbaf0cd117a5a26f8e322c52a72d29b2c45c58309489e",
    "commit-review-20260908T211541Z/production-green-current/evidence/packages.txt": "318dbb0fdcf225941d2cb3c3bd5e7cd327cb6f6ea52a2a7c58ee78d00b459a46",
    "commit-review-20260908T211541Z/production-green-current/evidence/source-sha256.txt": "9e4c0a1ab60d1fc7bce50152a2dbaea22669e329a7441815819783d51d157489",
    "commit-review-20260908T211541Z/production-green-current/image-inspect.json": "a60bb5d10b384aca542a50147c790b68485a4fc6a7c429f7e9e856b2174c6268",
    "commit-review-20260908T211541Z/production-green-current/result.txt": "563444d52e8de88cb6227f1aca5868ab4919b06aaf3f8f3af66b51fa18a67c10",
    "commit-review-20260908T211541Z/production-green-current/test-exit.txt": "9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa",
    "commit-review-20260908T211541Z/production-red-launch.txt": "d96ae574c9128f14a874301de56cba4e31cdf85f5db8ed249bc69a4ad5b6024a",
    "commit-review-20260908T211541Z/production-red/container-finished.json": "cd047c28fea6c0a1354974aec6ace6fc5d934522c6745c0057821d00b275a91e",
    "commit-review-20260908T211541Z/production-red/container-inspect.json": "5ab4e2f256cd452ceaa78c702eb2b105263d56313ec634c7ba2185adb82bc5a6",
    "commit-review-20260908T211541Z/production-red/evidence/docker-commands.txt": "11689a5325f8bc5b186361746d1254818744cdb4ea46f73ce15c247d728cea65",
    "commit-review-20260908T211541Z/production-red/evidence/driver-exit.txt": "9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa",
    "commit-review-20260908T211541Z/production-red/evidence/driver.log": "4676af39301f6eb0b014186b1c94fc21f248d71068a56a070de3c6fa31690079",
    "commit-review-20260908T211541Z/production-red/evidence/output/finalize-requested": "53820ec30908026cec197a59afef8087699abbe1b0ce5dc21e47644187940b6b",
    "commit-review-20260908T211541Z/production-red/evidence/output/iteration-00001-docker/metrics.json": "bf9a0eefb8cf753447df40695002bc1d4168b488d3dad9a6ad27745a3916375f",
    "commit-review-20260908T211541Z/production-red/evidence/output/iteration-00001-docker/pytest.log": "993a71b8d4e180d9754cbb8a4c058044a66acd829f57201ff44f080de2a5625d",
    "commit-review-20260908T211541Z/production-red/evidence/output/iterations.json": "b1e60526c42b0b86fc77abc2fe8aeab1d044dc8899bdf4ffe2d460ca40724b19",
    "commit-review-20260908T211541Z/production-red/evidence/output/summary.json": "3eb114ec4dcfae6d4b4c00494fc93840552da94bedf8e4246bfb0cbfab6c2429",
    "commit-review-20260908T211541Z/production-red/evidence/output/summary.txt": "7a164e637984acff689eeae9d780e4c0e2d667e28f3bbc8357c87eabcc8f1f6b",
    "commit-review-20260908T211541Z/production-red/evidence/packages.txt": "318dbb0fdcf225941d2cb3c3bd5e7cd327cb6f6ea52a2a7c58ee78d00b459a46",
    "commit-review-20260908T211541Z/production-red/evidence/source-sha256.txt": "a17af52f00b075b6ba83c3b26db09886e5045f20b39aa5422e3bb1139e1038b3",
    "commit-review-20260908T211541Z/production-red/evidence/workload-started.txt": "0ade5f0c3b4e262396b082661a222c24ef273e7ff2e50c55c2e40d9c802de2e2",
    "commit-review-20260908T211541Z/production-red/image-inspect.json": "a60bb5d10b384aca542a50147c790b68485a4fc6a7c429f7e9e856b2174c6268",
    "commit-review-20260908T211541Z/production-red/result.txt": "ad9256fa1dee5ad05e31adcf56c5b00f70e25a0f82738e337491f3298875c6f6",
    "commit-review-20260908T211541Z/production-red/test-exit.txt": "4355a46b19d348dc2f57c046f8ef63d4538ebb936000f3c9ee954a27460dd865",
    "commit-review-20260908T211541Z/routing.txt": "5bf9a0d60c04a9d56eb1eb237609c5639bec78b1bb44e905e72def58e8a55bc7",
    "commit-review-20260908T211541Z/test-extend-issue24-metrics.txt": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    "commit-review-20260908T211541Z/test-release-gates.txt": "cfe6567acc34cb3ac6b04d1b1fc407615684a85d34e48f05d93777dece12ae2b",
    "commit-review-20260908T211541Z/test-release-workflows.txt": "a55ff24e1452a38de29ad829c85bbb561ac396d848ec6526a97b394f6e16a82f",
    "commit-review-20260908T211541Z/test-repin-system-integration.txt": "c163764ac6a691d0f60831fac3197a8392645d49ac35fc4353d818e37be01aaf",
    "commit-review-20260908T211541Z/verified-at.txt": "2b5681aab7600e026dd1a26d93b68be3070d1bd99a41358683a281d30c0cdc9d",
    "production-red-launch.txt": "88a9d99f356537cbcb0a60d1b7732f60aa3fe02301c4446f1064482a097aa117",
    "production-red-status.txt": "4355a46b19d348dc2f57c046f8ef63d4538ebb936000f3c9ee954a27460dd865",
    "production-red/image-build.log": "37fd10cc7821060a8972ef518aa381bbe901f28adeb3d9c13d1c2902bb37c5ff",
    "production-red/image-id.txt": "db15f81ed36987862fa43d997fb27238d88d2501d05f6ba2f89e48ef39729f8d"
  },
  "production": [
    {
      "case": "production-red",
      "revision_or_base": "0f5d2b7414786cd27b8a686ca636485c35a5edce",
      "binding": "git-revision-and-source-sha256",
      "test_exit": 1,
      "driver_exit": 0,
      "iterations": 1,
      "failures": 0,
      "source_sha256": {
        "scripts/run-merge-recovery-soak.sh": "8d6c071e85c57ed4d1d9a5666f3ef99d47aaae898383a1c80822f66977c83f68",
        "scripts/bench/write-soak-summary.sh": "373b612d2b3e09b84cb4d5d5943e7ae13fc21b0b1cb535dbd7a7f4c0678cb061",
        "scripts/bench/collect-soak-metrics.sh": "ad0a6d1f890b0bfeac25246f7291414d5eee2fd6c85e5335347d5fa32280cb12",
        "scripts/bench/soak-metrics.json": "d4a71e7996dc0b0a74fcfc339f5164121c246db7c2f4e27114b7ea80c2edfef7"
      },
      "fixture_image_id": "sha256:2ba4825186d56dc974b21c65f273f3cb3ae04356ad070075691a43520f0b5b8e",
      "container_id": "e4ea8797de8c598a3ac4eb976bd143d4e25b4710eedd2674a66ee6d21f397de3",
      "started_at": "2026-09-08T21:15:42.37873618Z",
      "finished_at": "2026-09-08T21:15:46.307805015Z",
      "isolation": {
        "mounts": [

        ],
        "network": "none",
        "user": "65534:65534",
        "privileged": false,
        "pid_mode": "",
        "cap_drop": [
          "ALL"
        ],
        "no_new_privileges": true,
        "memory_bytes": 268435456,
        "pids_limit": 128,
        "nano_cpus": 1000000000
      }
    },
    {
      "case": "production-green-corrected",
      "revision_or_base": "ca85cfe3ecf73123b044b188b0c363fdbdff1e33",
      "binding": "git-revision-and-source-sha256",
      "test_exit": 0,
      "driver_exit": 1,
      "iterations": 0,
      "failures": 1,
      "source_sha256": {
        "scripts/run-merge-recovery-soak.sh": "2107899197cc3b676dd01a458c3b67c79708445277d371f7e4ccb45f4cdd4abb",
        "scripts/bench/write-soak-summary.sh": "373b612d2b3e09b84cb4d5d5943e7ae13fc21b0b1cb535dbd7a7f4c0678cb061",
        "scripts/bench/collect-soak-metrics.sh": "ad0a6d1f890b0bfeac25246f7291414d5eee2fd6c85e5335347d5fa32280cb12",
        "scripts/bench/soak-metrics.json": "d4a71e7996dc0b0a74fcfc339f5164121c246db7c2f4e27114b7ea80c2edfef7"
      },
      "fixture_image_id": "sha256:2ba4825186d56dc974b21c65f273f3cb3ae04356ad070075691a43520f0b5b8e",
      "container_id": "83fdc7e00f9fa01a38af40f3ba622e10a9cde02bc694c03e2317167226ca5bac",
      "started_at": "2026-09-08T21:15:50.931881688Z",
      "finished_at": "2026-09-08T21:15:53.259643649Z",
      "isolation": {
        "mounts": [

        ],
        "network": "none",
        "user": "65534:65534",
        "privileged": false,
        "pid_mode": "",
        "cap_drop": [
          "ALL"
        ],
        "no_new_privileges": true,
        "memory_bytes": 268435456,
        "pids_limit": 128,
        "nano_cpus": 1000000000
      }
    },
    {
      "case": "production-green-current",
      "revision_or_base": "bc1e1638068708f90f4b65574bc2b6b762bff73f",
      "binding": "working-source-sha256",
      "test_exit": 0,
      "driver_exit": 1,
      "iterations": 0,
      "failures": 1,
      "source_sha256": {
        "scripts/run-merge-recovery-soak.sh": "2107899197cc3b676dd01a458c3b67c79708445277d371f7e4ccb45f4cdd4abb",
        "scripts/bench/write-soak-summary.sh": "d1dbbceea1fd6e250f6f36289ed0cfae07b4c9e9e985afd3568f1d276e9bd4c0",
        "scripts/bench/collect-soak-metrics.sh": "6a04b8aa2b3335012eba6b66793b784c28fbedfd5160844c27e0d8abf01e7554",
        "scripts/bench/soak-metrics.json": "d4a71e7996dc0b0a74fcfc339f5164121c246db7c2f4e27114b7ea80c2edfef7"
      },
      "fixture_image_id": "sha256:2ba4825186d56dc974b21c65f273f3cb3ae04356ad070075691a43520f0b5b8e",
      "container_id": "311c92c2ab26f5a4d57391a43f537386da268dc05fa309a60e3825eebfeb88dc",
      "started_at": "2026-09-08T21:15:54.350960993Z",
      "finished_at": "2026-09-08T21:15:56.328638464Z",
      "isolation": {
        "mounts": [

        ],
        "network": "none",
        "user": "65534:65534",
        "privileged": false,
        "pid_mode": "",
        "cap_drop": [
          "ALL"
        ],
        "no_new_privileges": true,
        "memory_bytes": 268435456,
        "pids_limit": 128,
        "nano_cpus": 1000000000
      }
    }
  ],
  "commands": {
    "historical_source_export": "git archive <revision> scripts/run-merge-recovery-soak.sh scripts/bench/write-soak-summary.sh scripts/bench/collect-soak-metrics.sh scripts/bench/soak-metrics.json | tar -xf - -C <empty-source-directory>",
    "production": "SOAK_DISK_TEST_IMAGE=<fixture-image-id> SOAK_DISK_TEST_SOURCE_SHA=<revision-or-base> bash scripts/bench/test-soak-disk-admission.sh <source-directory> <empty-evidence-directory>",
    "standalone_formal": "timeout --signal=TERM --kill-after=5 120 java -Xmx512m -Duser.timezone=UTC -XX:+UseParallelGC -jar <verified-tla2tools.jar> -workers 2 -metadir <state-directory> -coverage 1 -config <configuration>.cfg <configuration>.tla",
    "composed_formal": "JAVA_TOOL_OPTIONS=\"-Xmx512m -Duser.timezone=UTC\" RUN_EXHAUSTIVE_TLA=0 TLA_TOOLS_JAR=<verified-tla2tools.jar> bash scripts/ci/check-tla-invariants.sh --soak-pr"
  },
  "verifier": {
    "name": "TLC2 Version 2.19 of 08 August 2024 (rev: 5a47802)",
    "jar_sha256": "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88",
    "java_version": [
      "openjdk version \"1.8.0_502\"",
      "OpenJDK Runtime Environment (build 1.8.0_502-8u502-ga~us1-0ubuntu1~24.04-b07)",
      "OpenJDK 64-Bit Server VM (build 25.502-b07, mixed mode)"
    ],
    "workers": 2,
    "heap_limit_mib": 512,
    "configuration_timeout_seconds": 120,
    "standalone_kill_grace_seconds": 5,
    "composed_gate_kill_grace_seconds": 60
  },
  "formal": {
    "red": {
      "configuration": "MC_SoakDisk_floor_only_pre_fix",
      "exit": 12,
      "invariant": "AdmissionRequiresBand",
      "admission_sample_mib": 7000
    },
    "green": {
      "configuration": "MC_SoakDisk",
      "exit": 0,
      "generated_states": 66,
      "distinct_states": 54,
      "queued_states": 0
    },
    "bounds": {
      "floor_mib": 4096,
      "band_mib": 4096,
      "initial_free_mib": 7000,
      "free_samples_mib": [
        7000,
        8191,
        8192,
        8193
      ],
      "require_band_red": false,
      "require_band_green": true
    }
  },
  "regressions": {
    "classifier_cases": 21,
    "routing_scenarios": 6,
    "driver_scenarios": 3,
    "composed_positive_configurations": 3,
    "composed_exact_negative_controls": 3,
    "all_recorded_regressions_passed": true
  },
  "initial_fixture_attempt": {
    "path": "production-red-launch.txt",
    "result": "source-permission-error-before-driver-execution",
    "behavioral_red": false
  },
  "limitations": [
    "No actual node workload ran.",
    "The model assumes valid samples, completing commands, and no concurrent disk writes during the admission decision.",
    "The model-to-code mapping is not a mechanized Bash refinement proof.",
    "The fixture image is not an acceptance node image.",
    "The image digest fixes the tested image. Future builds can select different signed Debian package versions.",
    "Hosted D1 execution, required-check enforcement, and acceptance identity remain pending.",
    "Resource-claim ratification, emergency response, all-writer bounds, and full-duration acceptance remain pending.",
    "Raw retention is local, not off-host."
  ],
  "claims_discharged": [

  ]
}
```

## Retained output

The following blocks preserve complete log files. The package record uses a JSON string to preserve tabs and line endings. Paths refer to the raw root in the manifest.

### commit-review-20260908T211541Z/production-red/result.txt

```text
FAIL: Disk hygiene left 7000 MiB below 8192 MiB, but the driver admitted 1 iteration(s).
```

### commit-review-20260908T211541Z/production-green-corrected/result.txt

```text
PASS: Disk hygiene left 7000 MiB below 8192 MiB. No iteration started, and the driver recorded refusal.
```

### commit-review-20260908T211541Z/production-green-current/result.txt

```text
PASS: Disk hygiene left 7000 MiB below 8192 MiB. No iteration started, and the driver recorded refusal.
```

### commit-review-20260908T211541Z/production-green-current/evidence/packages.txt

```json
"bash\t5.2.15-2+b13\ncoreutils\t9.1-1\nfindutils\t4.9.0-4\njq\t1.6-2.1+deb12u2\nmawk\t1.3.4.20200120-3.1\nprocps\t2:4.0.2-3\n"
```

### commit-review-20260908T211541Z/formal-red.txt

```text
TLC2 Version 2.19 of 08 August 2024 (rev: 5a47802)
Running breadth-first search Model-Checking with fp 65 and seed -1648721945376795073 with 2 workers on 20 cores with 491MB heap and 64MB offheap memory (Linux 6.17.0-1032-nvidia aarch64, Private Build 1.8.0_502 x86_64, MSBDiskFPSet, DiskStateQueue).
Parsing file /home/bf_spark/soak-evidence/f1r3node-rust/d1-20260908T184202Z/commit-review-20260908T211541Z/model/MC_SoakDisk_floor_only_pre_fix.tla
Parsing file /home/bf_spark/soak-evidence/f1r3node-rust/d1-20260908T184202Z/commit-review-20260908T211541Z/model/SoakDisk.tla
Parsing file /tmp/Naturals.tla
Parsing file /tmp/TLC.tla
Parsing file /tmp/Sequences.tla
Parsing file /tmp/FiniteSets.tla
Semantic processing of module Naturals
Semantic processing of module Sequences
Semantic processing of module FiniteSets
Semantic processing of module TLC
Semantic processing of module SoakDisk
Semantic processing of module MC_SoakDisk_floor_only_pre_fix
Starting... (2026-09-08 21:15:48)
Implied-temporal checking--satisfiability problem has 1 branches.
Computing initial states...
Finished computing initial states: 1 distinct state generated at 2026-09-08 21:15:49.
Error: Invariant AdmissionRequiresBand is violated.
Error: The behavior up to this point is:
State 1: <Initial predicate>
/\ sample = 7000
/\ guardian = FALSE
/\ admissionSample = 0
/\ phase = "guard"
/\ free = 7000
/\ evidence = FALSE
/\ admitted = FALSE
/\ stopReason = "none"

State 2: <CheckGuardian line 29, col 5 to line 35, col 80 of module SoakDisk>
/\ sample = 7000
/\ guardian = FALSE
/\ admissionSample = 0
/\ phase = "probe"
/\ free = 7000
/\ evidence = FALSE
/\ admitted = FALSE
/\ stopReason = "none"

State 3: <ProbeBoundary line 38, col 5 to line 41, col 84 of module SoakDisk>
/\ sample = 7000
/\ guardian = FALSE
/\ admissionSample = 0
/\ phase = "hygiene"
/\ free = 7000
/\ evidence = FALSE
/\ admitted = FALSE
/\ stopReason = "none"

State 4: <Hygiene line 44, col 5 to line 47, col 86 of module SoakDisk>
/\ sample = 7000
/\ guardian = FALSE
/\ admissionSample = 0
/\ phase = "post-guard"
/\ free = 7000
/\ evidence = FALSE
/\ admitted = FALSE
/\ stopReason = "none"

State 5: <CheckGuardian line 29, col 5 to line 35, col 80 of module SoakDisk>
/\ sample = 7000
/\ guardian = FALSE
/\ admissionSample = 0
/\ phase = "post-probe"
/\ free = 7000
/\ evidence = FALSE
/\ admitted = FALSE
/\ stopReason = "none"

State 6: <ProbeAfterHygiene line 50, col 5 to line 53, col 84 of module SoakDisk>
/\ sample = 7000
/\ guardian = FALSE
/\ admissionSample = 0
/\ phase = "decide"
/\ free = 7000
/\ evidence = FALSE
/\ admitted = FALSE
/\ stopReason = "none"

State 7: <Decide line 56, col 5 to line 62, col 80 of module SoakDisk>
/\ sample = 7000
/\ guardian = FALSE
/\ admissionSample = 0
/\ phase = "admit"
/\ free = 7000
/\ evidence = FALSE
/\ admitted = FALSE
/\ stopReason = "none"

State 8: <Admit line 65, col 5 to line 69, col 65 of module SoakDisk>
/\ sample = 7000
/\ guardian = FALSE
/\ admissionSample = 7000
/\ phase = "running"
/\ free = 7000
/\ evidence = FALSE
/\ admitted = TRUE
/\ stopReason = "none"

The coverage statistics at 2026-09-08 21:15:49
<Init line 18, col 1 to line 18, col 4 of module SoakDisk>: 2:2
  line 19, col 5 to line 26, col 23 of module SoakDisk: 2
<CheckGuardian line 28, col 1 to line 28, col 13 of module SoakDisk>: 9:16
  line 29, col 8 to line 29, col 40 of module SoakDisk: 61
  |line 29, col 8 to line 29, col 12 of module SoakDisk: 45
  |line 29, col 18 to line 29, col 40 of module SoakDisk: 45
  line 30, col 11 to line 30, col 18 of module SoakDisk: 16
  line 31, col 16 to line 32, col 42 of module SoakDisk: 5
  line 33, col 19 to line 33, col 76 of module SoakDisk: 11
  line 34, col 19 to line 34, col 38 of module SoakDisk: 11
  line 35, col 8 to line 35, col 80 of module SoakDisk: 16
<ProbeBoundary line 37, col 1 to line 37, col 13 of module SoakDisk>: 1:3
  line 38, col 8 to line 38, col 22 of module SoakDisk: 46
  |line 38, col 8 to line 38, col 12 of module SoakDisk: 43
  line 39, col 8 to line 39, col 21 of module SoakDisk: 3
  line 40, col 8 to line 40, col 72 of module SoakDisk: 3
  line 41, col 8 to line 41, col 84 of module SoakDisk: 3
<Hygiene line 43, col 1 to line 43, col 7 of module SoakDisk>: 4:12
  line 44, col 8 to line 44, col 24 of module SoakDisk: 45
  |line 44, col 8 to line 44, col 12 of module SoakDisk: 42
  line 45, col 8 to line 45, col 56 of module SoakDisk: 12
  |line 45, col 18 to line 45, col 56 of module SoakDisk: 3:15
  ||line 45, col 43 to line 45, col 55 of module SoakDisk: 12
  ||line 45, col 29 to line 45, col 39 of module SoakDisk: 3
  line 46, col 8 to line 46, col 28 of module SoakDisk: 12
  line 47, col 8 to line 47, col 86 of module SoakDisk: 12
<ProbeAfterHygiene line 49, col 1 to line 49, col 17 of module SoakDisk>: 4:9
  line 50, col 8 to line 50, col 27 of module SoakDisk: 50
  |line 50, col 8 to line 50, col 12 of module SoakDisk: 41
  line 51, col 8 to line 51, col 21 of module SoakDisk: 9
  line 52, col 8 to line 52, col 24 of module SoakDisk: 9
  line 53, col 8 to line 53, col 84 of module SoakDisk: 9
<Decide line 55, col 1 to line 55, col 6 of module SoakDisk>: 5:6
  line 56, col 8 to line 56, col 23 of module SoakDisk: 46
  |line 56, col 8 to line 56, col 12 of module SoakDisk: 40
  line 57, col 11 to line 57, col 73 of module SoakDisk: 6
  line 58, col 19 to line 58, col 36 of module SoakDisk: 0
  line 59, col 19 to line 59, col 38 of module SoakDisk: 0
  line 60, col 19 to line 60, col 34 of module SoakDisk: 6
  line 61, col 19 to line 61, col 38 of module SoakDisk: 6
  line 62, col 8 to line 62, col 80 of module SoakDisk: 6
<Admit line 64, col 1 to line 64, col 5 of module SoakDisk>: 2:3
  line 65, col 8 to line 65, col 22 of module SoakDisk: 40
  |line 65, col 8 to line 65, col 12 of module SoakDisk: 39
  line 66, col 8 to line 66, col 23 of module SoakDisk: 1
  |line 66, col 20 to line 66, col 23 of module SoakDisk: 3
  line 67, col 8 to line 67, col 32 of module SoakDisk: 1
  |line 67, col 27 to line 67, col 32 of module SoakDisk: 3
  line 68, col 8 to line 68, col 25 of module SoakDisk: 1
  line 69, col 8 to line 69, col 65 of module SoakDisk: 1
<GuardianTrip line 71, col 1 to line 71, col 12 of module SoakDisk>: 15:19
  line 72, col 8 to line 72, col 50 of module SoakDisk: 55
  |line 72, col 8 to line 72, col 12 of module SoakDisk: 36
  |line 72, col 21 to line 72, col 50 of module SoakDisk: 36
  line 73, col 8 to line 73, col 16 of module SoakDisk: 50
  |line 73, col 9 to line 73, col 16 of module SoakDisk: 31
  line 74, col 8 to line 74, col 23 of module SoakDisk: 19
  line 75, col 8 to line 75, col 89 of module SoakDisk: 19
<PublishRefusal line 77, col 1 to line 77, col 14 of module SoakDisk>: 4:4
  line 78, col 8 to line 78, col 24 of module SoakDisk: 40
  |line 78, col 8 to line 78, col 12 of module SoakDisk: 36
  line 79, col 8 to line 79, col 23 of module SoakDisk: 4
  line 80, col 8 to line 80, col 22 of module SoakDisk: 4
  line 81, col 8 to line 81, col 82 of module SoakDisk: 4
<TypeOK line 88, col 1 to line 88, col 6 of module SoakDisk>
  line 89, col 5 to line 97, col 27 of module SoakDisk: 45
<AdmissionRequiresBand line 99, col 1 to line 99, col 21 of module SoakDisk>
  line 99, col 26 to line 99, col 74 of module SoakDisk: 45
  |line 99, col 26 to line 99, col 33 of module SoakDisk: 45
  |line 99, col 38 to line 99, col 74 of module SoakDisk: 2
<StopPreventsAdmission line 100, col 1 to line 100, col 21 of module SoakDisk>
  line 100, col 26 to line 100, col 57 of module SoakDisk: 43
  |line 100, col 26 to line 100, col 44 of module SoakDisk: 43
  |line 100, col 49 to line 100, col 57 of module SoakDisk: 8
<RefusalRecorded line 101, col 1 to line 101, col 15 of module SoakDisk>
  line 101, col 20 to line 101, col 81 of module SoakDisk: 43
  |line 101, col 20 to line 101, col 33 of module SoakDisk: 43
  |line 101, col 38 to line 101, col 81 of module SoakDisk: 4
End of statistics.
55 states generated, 45 distinct states found, 9 states left on queue.
The depth of the complete state graph search is 8.
The average outdegree of the complete state graph is 1 (minimum is 0, the maximum 5 and the 95th percentile is 2).
Finished in 02s at (2026-09-08 21:15:49)
```

### commit-review-20260908T211541Z/formal-green.txt

```text
TLC2 Version 2.19 of 08 August 2024 (rev: 5a47802)
Running breadth-first search Model-Checking with fp 1 and seed -2686144765318059403 with 2 workers on 20 cores with 491MB heap and 64MB offheap memory (Linux 6.17.0-1032-nvidia aarch64, Private Build 1.8.0_502 x86_64, MSBDiskFPSet, DiskStateQueue).
Parsing file /home/bf_spark/soak-evidence/f1r3node-rust/d1-20260908T184202Z/commit-review-20260908T211541Z/model/MC_SoakDisk.tla
Parsing file /home/bf_spark/soak-evidence/f1r3node-rust/d1-20260908T184202Z/commit-review-20260908T211541Z/model/SoakDisk.tla
Parsing file /tmp/Naturals.tla
Parsing file /tmp/TLC.tla
Parsing file /tmp/Sequences.tla
Parsing file /tmp/FiniteSets.tla
Semantic processing of module Naturals
Semantic processing of module Sequences
Semantic processing of module FiniteSets
Semantic processing of module TLC
Semantic processing of module SoakDisk
Semantic processing of module MC_SoakDisk
Starting... (2026-09-08 21:15:58)
Implied-temporal checking--satisfiability problem has 1 branches.
Computing initial states...
Finished computing initial states: 1 distinct state generated at 2026-09-08 21:15:59.
Progress(9) at 2026-09-08 21:15:59: 66 states generated, 54 distinct states found, 0 states left on queue.
Checking temporal properties for the complete state space with 54 total distinct states at (2026-09-08 21:15:59)
Finished checking temporal properties in 00s at 2026-09-08 21:15:59
Model checking completed. No error has been found.
  Estimates of the probability that TLC did not check all reachable states
  because two distinct states had the same fingerprint:
  calculated (optimistic):  val = 3.5E-17
The coverage statistics at 2026-09-08 21:15:59
<Init line 18, col 1 to line 18, col 4 of module SoakDisk>: 1:1
  line 19, col 5 to line 26, col 23 of module SoakDisk: 1
<CheckGuardian line 28, col 1 to line 28, col 13 of module SoakDisk>: 9:10
  line 29, col 8 to line 29, col 40 of module SoakDisk: 65
  |line 29, col 8 to line 29, col 12 of module SoakDisk: 55
  |line 29, col 18 to line 29, col 40 of module SoakDisk: 55
  line 30, col 11 to line 30, col 18 of module SoakDisk: 10
  line 31, col 16 to line 32, col 42 of module SoakDisk: 5
  line 33, col 19 to line 33, col 76 of module SoakDisk: 5
  line 34, col 19 to line 34, col 38 of module SoakDisk: 5
  line 35, col 8 to line 35, col 80 of module SoakDisk: 10
<ProbeBoundary line 37, col 1 to line 37, col 13 of module SoakDisk>: 2:3
  line 38, col 8 to line 38, col 22 of module SoakDisk: 58
  |line 38, col 8 to line 38, col 12 of module SoakDisk: 55
  line 39, col 8 to line 39, col 21 of module SoakDisk: 3
  line 40, col 8 to line 40, col 72 of module SoakDisk: 3
  line 41, col 8 to line 41, col 84 of module SoakDisk: 3
<Hygiene line 43, col 1 to line 43, col 7 of module SoakDisk>: 8:8
  line 44, col 8 to line 44, col 24 of module SoakDisk: 57
  |line 44, col 8 to line 44, col 12 of module SoakDisk: 55
  line 45, col 8 to line 45, col 56 of module SoakDisk: 8
  |line 45, col 18 to line 45, col 56 of module SoakDisk: 2:10
  ||line 45, col 43 to line 45, col 55 of module SoakDisk: 8
  ||line 45, col 29 to line 45, col 39 of module SoakDisk: 2
  line 46, col 8 to line 46, col 28 of module SoakDisk: 8
  line 47, col 8 to line 47, col 86 of module SoakDisk: 8
<ProbeAfterHygiene line 49, col 1 to line 49, col 17 of module SoakDisk>: 4:8
  line 50, col 8 to line 50, col 27 of module SoakDisk: 63
  |line 50, col 8 to line 50, col 12 of module SoakDisk: 55
  line 51, col 8 to line 51, col 21 of module SoakDisk: 8
  line 52, col 8 to line 52, col 24 of module SoakDisk: 8
  line 53, col 8 to line 53, col 84 of module SoakDisk: 8
<Decide line 55, col 1 to line 55, col 6 of module SoakDisk>: 6:8
  line 56, col 8 to line 56, col 23 of module SoakDisk: 63
  |line 56, col 8 to line 56, col 12 of module SoakDisk: 55
  line 57, col 11 to line 57, col 73 of module SoakDisk: 8
  line 58, col 16 to line 59, col 38 of module SoakDisk: 4
  line 60, col 19 to line 60, col 34 of module SoakDisk: 4
  line 61, col 19 to line 61, col 38 of module SoakDisk: 4
  line 62, col 8 to line 62, col 80 of module SoakDisk: 8
<Admit line 64, col 1 to line 64, col 5 of module SoakDisk>: 4:4
  line 65, col 8 to line 65, col 22 of module SoakDisk: 59
  |line 65, col 8 to line 65, col 12 of module SoakDisk: 55
  line 66, col 8 to line 66, col 23 of module SoakDisk: 4
  line 67, col 8 to line 67, col 32 of module SoakDisk: 4
  line 68, col 8 to line 68, col 25 of module SoakDisk: 4
  line 69, col 8 to line 69, col 65 of module SoakDisk: 4
<GuardianTrip line 71, col 1 to line 71, col 12 of module SoakDisk>: 12:18
  line 72, col 8 to line 72, col 50 of module SoakDisk: 73
  |line 72, col 8 to line 72, col 12 of module SoakDisk: 55
  |line 72, col 21 to line 72, col 50 of module SoakDisk: 55
  line 73, col 8 to line 73, col 16 of module SoakDisk: 53
  |line 73, col 9 to line 73, col 16 of module SoakDisk: 35
  line 74, col 8 to line 74, col 23 of module SoakDisk: 18
  line 75, col 8 to line 75, col 89 of module SoakDisk: 18
<PublishRefusal line 77, col 1 to line 77, col 14 of module SoakDisk>: 8:8
  line 78, col 8 to line 78, col 24 of module SoakDisk: 63
  |line 78, col 8 to line 78, col 12 of module SoakDisk: 55
  line 79, col 8 to line 79, col 23 of module SoakDisk: 8
  line 80, col 8 to line 80, col 22 of module SoakDisk: 8
  line 81, col 8 to line 81, col 82 of module SoakDisk: 8
<TypeOK line 88, col 1 to line 88, col 6 of module SoakDisk>
  line 89, col 5 to line 97, col 27 of module SoakDisk: 54
<AdmissionRequiresBand line 99, col 1 to line 99, col 21 of module SoakDisk>
  line 99, col 26 to line 99, col 74 of module SoakDisk: 54
  |line 99, col 26 to line 99, col 33 of module SoakDisk: 54
  |line 99, col 38 to line 99, col 74 of module SoakDisk: 4
<StopPreventsAdmission line 100, col 1 to line 100, col 21 of module SoakDisk>
  line 100, col 26 to line 100, col 57 of module SoakDisk: 54
  |line 100, col 26 to line 100, col 44 of module SoakDisk: 54
  |line 100, col 49 to line 100, col 57 of module SoakDisk: 16
<RefusalRecorded line 101, col 1 to line 101, col 15 of module SoakDisk>
  line 101, col 20 to line 101, col 81 of module SoakDisk: 54
  |line 101, col 20 to line 101, col 33 of module SoakDisk: 54
  |line 101, col 38 to line 101, col 81 of module SoakDisk: 8
End of statistics.
66 states generated, 54 distinct states found, 0 states left on queue.
The depth of the complete state graph search is 9.
The average outdegree of the complete state graph is 1 (minimum is 0, the maximum 4 and the 95th percentile is 2).
Finished in 02s at (2026-09-08 21:15:59)
```

### commit-review-20260908T211541Z/classifier.txt

```text
PASS: The formal gate accepts only the expected invariant violations.
```

### commit-review-20260908T211541Z/routing.txt

```text
PASS: Pull requests run the bounded formal baseline and preserve nightly verification.
```

### commit-review-20260908T211541Z/composed-tla.txt

```text
CHECK  replay_liveness/MC_ReplayHotLoop (started 21:16:21Z, cap 2m)
OK     replay_liveness/MC_ReplayHotLoop (3s)
CHECK  carrier_index/MC_CarrierIndex (started 21:16:24Z, cap 2m)
OK     carrier_index/MC_CarrierIndex (3s)
CHECK  soak_disk/MC_SoakDisk (started 21:16:27Z, cap 2m)
OK     soak_disk/MC_SoakDisk (3s)
CHECK  carrier_index/MC_CarrierIndex_dag_first_pre_fix (started 21:16:30Z, cap 2m)
EXPECTED-FAIL carrier_index/MC_CarrierIndex_dag_first_pre_fix (IndexCompleteForWindow, 3s)
CHECK  carrier_index/MC_CarrierIndex_read_failure_pre_fix (started 21:16:33Z, cap 2m)
EXPECTED-FAIL carrier_index/MC_CarrierIndex_read_failure_pre_fix (AbsenceProofSound, 4s)
CHECK  soak_disk/MC_SoakDisk_floor_only_pre_fix (started 21:16:37Z, cap 2m)
EXPECTED-FAIL soak_disk/MC_SoakDisk_floor_only_pre_fix (AdmissionRequiresBand, 2s)
All 3 post-fix TLA+ configurations clean.
All 3 negative controls violated their expected invariants.
```

### commit-review-20260908T211541Z/driver-suite.txt

```text
soak driver tests passed (fail-closed + deadline + disk-band paths)
```
