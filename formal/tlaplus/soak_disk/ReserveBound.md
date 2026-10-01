# Reserve bound under a bounded response

This model supports gate D3 and the disk reserve argument.
It shows that the bounded emergency deadline holds the runner's operating reserve during the response.
An unbounded response lets the remaining writers exhaust the reserve.
Eventual completion alone does not hold the reserve, so the deadline bound is necessary.

The reserve inequality is `Reserve >= Required + Growth * Deadline + Burst + Margin`.
`Reserve` is the free space at the trigger, and `Required` is the operating reserve for the runner and the final upload.
`Growth` is the per-step growth of every remaining writer.
`Deadline` is the response bound in steps, and `Burst` is the one-time growth not represented by the sampled rate.
The model checks that the free space never falls below `Required` during the response.

The bounded configuration caps the writer growth at the deadline.
The free space then stays at or above the operating reserve, because the total growth is at most `Growth * Deadline + Burst`.
The unbounded configuration removes the deadline cap, so the growth continues until the reserve is gone.

## Correspondence

| Model element | Reserve argument term |
| --- | --- |
| `Reserve` | The free space at the guardian trigger, the floor |
| `Required` | R, the operating reserve for the runner and the final upload |
| `Growth` | G, the bounded growth rate of every remaining writer |
| `Deadline` | T, the composed emergency deadline in steps |
| `Burst` | J, the burst not represented by the sampled rate |
| `Grow` | The remaining writers consume space during the response |
| `OperatingReserveHeld` | The free space never falls below the operating reserve |

The bounded configuration passes with 15 distinct states.
The unbounded configuration violates `OperatingReserveHeld` with TLC exit 12.

## Limits

The model uses small concrete constants that satisfy the reserve inequality.
It represents growth as a constant per-step rate and one burst, not a measured distribution.
It does not represent the driver, the guardian, or the storage stall.
The margin term M is folded into the concrete reserve value and is not a separate variable.

The growth rate, the burst, and the operating reserve still need the D3 diagnostic run.
The gate runs this model and its unbounded control.
D2, D3, and claim discharge remain pending.
