# Disk Settings Admission Correspondence

## Failure and correction

B24 supplies three invalid disk configurations to the production driver. Each baseline execution admits an iteration instead of rejecting its configuration.

The first case supplies floor `9223372036854775808`. The second supplies that value as the hygiene band.

The third uses floor `4096` and band `9223372036854771712`. Both values fit individually, but their sum exceeds signed 64-bit arithmetic.

The correction validates both decimal strings before arithmetic. It removes leading zeros and uses a lexical comparison under C collation for equal-length values.

Both values must fit `0` through `9223372036854775807`. The band must not exceed the maximum minus the floor, which prevents addition overflow.

All three corrected executions return configuration error 2 before workload admission. They do not produce a soak summary because configuration validation precedes the run.

Two maximum-value cases pass on both baseline and corrected source. A maximum floor with band zero preserves disk refusal.

A zero floor with maximum band preserves the explicit protection opt-out. These cases provide characterization coverage, not additional repair cycles.

## Model

The model uses the three exact invalid configurations and the valid default configuration. Decimal strings preserve every digit without exceeding the model checker's native integer range.

Explicit column steps add the padded decimal values. Lexical digit comparison checks the individual values and their sum against the signed maximum.

The negative control bypasses range enforcement. It requires exit 12 on `AdmissionRequiresValidDiskSettings`.

The corrected model has 88 distinct states. It separates arithmetic steps from configuration acceptance and refusal.

The first recursive model found the required counterexample, but its positive run reached the 120-second verification limit. That limit is not behavioral RED.

The explicit column model replaces that arithmetic implementation. Fresh negative and positive runs pass their expected results. The original source and logs remain retained.

## Limits

The model covers four configurations, not every possible environment string. The production maximum-value cases are outside that finite model domain.

The driver assumes signed 64-bit Bash arithmetic. This cycle does not verify every malformed sample, filesystem location, configuration field, or later scheduling race.

Configuration refusal does not establish durable evidence publication or a complete emergency deadline. Hosted checks, maintainer review, D2, and acceptance remain pending.
