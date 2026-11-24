# aegent-mm v0.1.0 — instruction

Staged specification and runbook for the agent that builds and operates `aegent-mm`. The strategy itself is defined in [definition.md](definition.md). This file says how it is built, tested, run and changed.

## 1. Objective

```
wallet     the sub-account that holds only aegent-mm funds, instance aegent-mm-00
V          liquidation value of the wallet in USDC, definition 5.8
V0         V at the first live start; 100 USDC on the first test
target     V ≥ V0 at every daily checkpoint, 00:00 UTC
loss cap   d = 0.20 on the first test: the guard closes everything when V < 80 USDC
```

- The design does not guarantee the target.
- The guard limits the loss. Gaps, fees and funding can carry the realized loss past the cap.

## 2. Roles and limits

| Actor | Does | Never does |
|---|---|---|
| agent | builds, tests, backtraces, prepares runs, reports; stays outside the order path | handles keys; starts a live run; commits, pushes or opens pull requests; changes account-wide settings; downloads data without a go-ahead |
| operator | holds the keys; approves `Θ`, software changes and model changes; starts and stops live runs; commits | — |
| program | runs the deterministic state machine of the definition | takes a decision the definition does not contain |

## 3. Venue and account

| Item | Value |
|---|---|
| venue | Binance USDⓈ-M futures, USDC-margined perpetual contracts |
| contract | SOLUSDC only |
| account | verified, futures enabled; funds in a dedicated sub-account |
| modes | one-way position mode, cross margin, single-asset mode |
| leverage | the smallest setting under which the start preconditions of definition 5.4 hold |
| API key | trade-only, no withdrawal right, restricted to the server's address; set by the operator in the server's environment |

## 4. Repositories

| Repository | Holds |
|---|---|
| `aegentism` | the definition, this instruction, the program: all strategy logic |
| `aetelier-sdk` | venue connectivity, neutral: futures market data, history converter, signed private client |
| `aetelier-tracer` | backtracing, neutral: judging of maker orders, the re-arming maker pair order source, price bars |
| `aetelier-sentinel` | monitoring, neutral: probes the program's status |

- A capability the platform lacks is added there as a neutral feature. It is not built here.

## 5. Prerequisites in the platform

| Id | Capability | Repository | Needed for |
|---|---|---|---|
| P1 | futures market data: order book, trades, best bid and ask, mark price and funding, contract rules | `aetelier-sdk` | collection, live prices |
| P2 | converter from Binance's public trade history to the collector's trades parquet | `aetelier-sdk` | long backtraces |
| P3 | signed private client: orders, cancels, user data stream, account, fees, brackets, dead-man switch, stop orders | `aetelier-sdk` | live runs, gates 3 and 4 |
| P4 | re-arming maker pair order source, with price bars in its output | `aetelier-tracer` | backtraces, gate 2 |

- Each is neutral work in its own repository, agreed with the operator before code is written.
- A stage that needs a missing capability waits for it.

## 6. Program

A Rust workspace in this repository.

| Part | Responsibility | Touches |
|---|---|---|
| core | `Θ`, `R`, binding checks, `s0`, `Quote`, `Step`, `Value`, `Stop`, cells: exactly the definition | nothing |
| cells file | writes the orders file of definition 5.12 from `Θ` | a file |
| accounting | runs `Value` and `Stop` over tracer's fills and bars; produces the run ledger and the exit | tracer's output |
| sweep | builds candidates, calls tracer, collects reports | tracer |
| runner | live: reads the binding, verifies account and preconditions, runs `Init`, `Run` and `Closing`; renews the dead-man switch; keeps the backstop orders; reconciles after a reconnection | the SDK |
| status and log | read-only JSON status; JSON-lines event log | HTTP, a file |

- One code path: live operation and replay accounting use the same core.
- No fill logic and no raw tape reading in this repository.
- Credentials come only from the environment and are redacted in every output.
- Fee rates are read at start and recorded. They never block a start.

## 7. Data

| Source | Use | Rule |
|---|---|---|
| Binance public history, SOLUSDC `aggTrades`, from 2024-01-04 | long backtraces | converted by P2; downloaded only after the operator's go-ahead |
| Binance public `fundingRate` files | funding in replay | same |
| own collection through P1: trades, best bid and ask, mark price | comparison of replay with live runs | starts when P1 works |

- In replay `b = a =` the last print.

## 8. Backtrace and choice of Θ

```
candidates   a small lattice: range width around the start price, level spacing δ, TP on or off
sizing       q = the largest lot whose range-bound loss stays within the loss budget V − V0·(1 − d)
replay       each candidate from many start dates:  cells file → tracer → accounting
split        candidates are chosen on one period and confirmed on a later one
report       per candidate, as distributions:
               share of runs with V ≥ V0 at every daily checkpoint
               median and 5th percentile of final Π_last
               share of runs ended by guard, by risk, by tp
decision     the operator approves one Θ, or none
```

- A backtrace measures the past. It promises nothing about a live run.

## 9. Gates before the first live run

| Gate | Passes when | Needs |
|---|---|---|
| 1 properties | invariants I1 to I4, cell properties C1 to C4 and the profit identity hold on generated tapes; the reference instance reproduces definition section 7 | the core |
| 2 parity | the replay of a real Binance SOLUSDC grid over its real period reproduces the app's matched pairs: same count, same levels; each difference explained | P4; the grid's history from the operator |
| 3 demo plumbing | on Binance's demo environment: start, a fill and its opposite order, stop by command, stop by guard, a backstop trade, expiry of the dead-man switch, reconnection with reconciliation | P3; demo keys set by the operator |
| 4 dry-run | on the real sub-account: binding read, account and preconditions verified, the order list equals `Quote(s0)` plus the two backstop orders, nothing sent | P3; keys set by the operator |

- Gates 2 and 3 also settle the assumptions of definition section 9 that they can observe: the opposite order on a partial fill, fee rates, stop-order mechanics, the behaviour of the dead-man switch.
- A failed gate stops the sequence. The fix follows the change classes of section 12.

## 10. Live run

| Step | Who | What |
|---|---|---|
| prepare | agent | the approved `Θ` and `R`; the dry-run output of gate 4 for them |
| start | operator | the start command on the server, carrying `Θ` and `R` |
| run | program | definition sections 5 and 6 |
| stop | operator, or an exit of the definition | `Command(stop, keep)`, or the exit's own closing |
| after | — | nothing restarts by itself |

- Host: the home server, as a container. The development machine builds, tests, backtraces and runs gate 4.
- The agent prepares server commands as single commands. The operator runs them.
- Exchange-side protections follow definition 6.1. `T_dead`, `T_data` and `T_stream` are set with `Θ` and shown in the dry-run.

By-hand procedure when the server is unreachable, by the operator, in the Binance app, on the sub-account:

1. Cancel all open orders on SOLUSDC, stop orders included.
2. Close the SOLUSDC position at market.
3. Check that no open order and no position remain.

## 11. Monitoring

| Status field | Content |
|---|---|
| `state` | `idle`, `init`, `run`, `closing`, `ended` or `attention` |
| `reason` | exit reason of the last run |
| `h`, `pos` | empty level, position |
| `pi_last`, `v`, `v0`, `floor` | `Π_last`, `V`, `V0`, `V0·(1 − d)` |
| `rho` | risk ratio |
| `orders_resting`, `orders_expected` | resting orders, size of `Quote(s)` plus backstop orders |
| `age_data_ms`, `age_stream_ms`, `age_deadman_ms` | time since the last price, the last user-stream message, the last dead-man renewal |
| `checkpoint` | result of the last daily checkpoint |

| `attention` when |
|---|
| `V` is within a set distance of the floor |
| the resting orders differ from `Quote(s)` |
| a run ended by `guard`, `backstop`, `failure` or `risk` |

- The status is served read-only on the server's internal network. `aetelier-sentinel` probes it with an `http` target judged on a JSON field.
- The event log holds one JSON line per fill, per exit and per daily checkpoint.

## 12. Reviews and changes

A review is made when the operator asks for one.

| Part of a review | Content |
|---|---|
| parity | live ledger against tracer's replay of the same days: fills, prices, fees; each difference classified |
| result | the run's outcome placed in the backtrace distribution of its `Θ` |
| incidents | exits by `guard`, `backstop`, `failure` or `risk`; attention states; invariant breaks |
| proposals | a few at most, each with evidence, expected effect and risk; "no change" is a valid outcome |

| Change class | What changes | Evidence | Approval |
|---|---|---|---|
| parameters | `Θ` of the next run | a sweep report of section 8 | the operator approves one `Θ` per run, then starts it |
| software | code here or in a platform repository | the change in the working tree; the gates it touches re-run | the operator reviews and commits |
| model | the definition | a measured reason from live runs or backtraces | the operator approves the amendment of `definition.md` first; software follows |

- Reviews and proposals are written outside this repository, at a place named by the operator.
- Nothing reaches live funds without the operator's approval.

## 13. Records

| Record | Content |
|---|---|
| run record | `Θ`, `R`, the binding as read, fee rates, start and end time, exit reason |
| backtrace record | sha256 of the cells file and of each tape, tracer trace id, tracer version, definition version |
| event log | fills, exits, daily checkpoints |

- This version makes no attestation. The records are kept so that one can be added later.
