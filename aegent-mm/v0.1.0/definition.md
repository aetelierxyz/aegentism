# aegent-mm v0.1.0 — definition

Semi-formal definition of the market-making strategy run by `aegent-mm`. Facts about Binance are dated 2026-11-22.

## 1. Scope

- A market-making strategy is defined as a tuple of eight components (section 3).
- The venue binding covers Binance USDC-margined perpetual contracts (section 4).
- The first member of the class is a neutral arithmetic grid (section 5).
- The program level owns the wallet and the runs (section 6).
- One reference instance reproduces a grid created in the Binance app (section 7).
- Sections 3 to 6 are normative for `aegent-mm`. Section 8 compares them with Binance's Futures Grid. Section 9 gives the status of each fact.

## 2. Notation

| Symbol | Meaning |
|---|---|
| `τ` | tick size |
| `σ` | lot step |
| `ν` | minimum notional of an order |
| `n_max` | maximum resting orders per symbol |
| `f_m`, `f_t` | maker and taker fee rates |
| `mmr` | maintenance margin rate of the first bracket |
| `ℓ`, `ℓ_max` | leverage setting and its cap |
| `L`, `U` | lower and upper bound of the range |
| `N` | number of grids |
| `δ` | level spacing |
| `p_i` | price of level `i`, `i = 0..N` |
| `q` | quantity of every order |
| `h0`, `h` | empty level at start, empty level now |
| `r_i` | filled part of the resting order at level `i` |
| `pos` | signed position, positive when long |
| `K` | largest number of open lots |
| `M` | margin allocated to a run |
| `W` | wallet balance |
| `P_last`, `P_mark`, `b`, `a` | last price, mark price, best bid, best ask |
| `P0` | last price at the start of a run |
| `P_trig` | trigger price: `P_last` or `P_mark`, by parameter `trigger` |
| `Π(P)` | total profit of a run valued at price `P`; `Π_last`, `Π_mark` and `Π_trig` use `P_last`, `P_mark` and `P_trig` |
| `V`, `V0`, `d` | wallet value, its starting value, loss cap |
| `ρ` | risk ratio |
| `C`, `k`, `m` | completed fills, net levels moved, matched pairs |

## 3. Market-making strategy

```
MM = ( B, Θ, S, s0, Quote, Step, Value, Stop )
```

| Component | Signature | Meaning |
|---|---|---|
| `B` | — | venue binding: contract rules, fees, margin model, order types, price signals |
| `Θ` | — | parameters set by the operator |
| `S` | — | state kept between events |
| `s0` | `Θ × market → S` | initial state |
| `Quote` | `S → set of orders` | orders that must rest on the book in state `s` |
| `Step` | `S × event → S` | effect of one event |
| `Value` | `S × prices → ℝ` | profit of a run and value of the wallet |
| `Stop` | `S × prices → run or end(reason)` | exits |

Events:

```
Trade(i, x, px, fee, maker)    x units of the order at level i traded at price px
Price(P_last, P_mark, b, a)    new market prices
Funding(amount)                signed funding payment
Reject(i)                      the exchange refused, canceled or expired the order for level i
Backstop                       a backstop order of 6.1 traded
Command(stop, keep)            operator stop; keep = true leaves the position open
Tick                           at least once per second
```

Run:

```
Init → Run → Closing → Ended(reason)
reason ∈ { guard, backstop, failure, risk, sl, price, tp, command }
```

Requirements on every member:

| Id | Requirement |
|---|---|
| R1 | `Quote`, `Step`, `Value` and `Stop` are functions of the state and the event alone |
| R2 | after every `Step` the resting orders equal `Quote(s)` |
| R3 | `s0` holds resting orders on both sides of the market |
| R4 | the position is bounded by a function of `Θ` |

## 4. Venue binding — Binance USDC-margined perpetual

### 4.1 Contract rules

| Element | Symbol | SOLUSDC | Status |
|---|---|---|---|
| tick size | `τ` | 0.01 | reported, observed |
| lot step | `σ` | 0.01 SOL | reported, observed |
| minimum notional | `ν` | 5 USDC | reported |
| maximum resting orders per symbol | `n_max` | 200 | reported |
| maker fee | `f_m` | 0 on fills of 2026-08-13 and 2026-08-14 | observed for those dates |
| taker fee | `f_t` | 0.04% | unverified |
| maintenance margin rate, first bracket | `mmr` | 0.5% | derived |
| leverage cap | `ℓ_max` | 5x for a sub-account of a regular user | reported |
| price band | — | buy price ≤ mark × 1.05, sell price ≥ mark × 0.95 | documented rule, reported multipliers |
| funding interval | — | 8 h | reported default, not confirmed for SOLUSDC |

- `B` is read from the exchange at the start of every run: `exchangeInfo`, `commissionRate`, `leverageBracket`.
- `Θ` and `R` are checked against the values read (section 5.1). An inadmissible `Θ` prevents the start.
- Fee rates are recorded in the run's report. They never prevent a start.

### 4.2 Account

| Item | Value |
|---|---|
| position mode | one-way |
| margin mode | cross |
| asset mode | single-asset |
| funds | a sub-account that holds only this strategy's funds |

- The program verifies the modes and the leverage setting. It does not change account-wide modes.

### 4.3 Price signals

| Signal | Used for |
|---|---|
| last price `P_last` | start price `P0`, reported profit `Π_last`, trigger price when `trigger = last` |
| best bid `b`, best ask `a` | wallet value `V`, guard, daily checkpoint |
| mark price `P_mark` | funding, risk ratio `ρ`, liquidation distance, trigger price when `trigger = mark` |
| trade prints | fills in replay |

### 4.4 Orders

| Use | Order |
|---|---|
| grid order | `LIMIT`, `GTC` |
| closing order | `MARKET`, reduce-only |
| backstop order | `STOP_MARKET`, close-position, trigger on the mark price |

- A `GTC` order that crosses on arrival trades at once as taker.
- A `GTC` order expires one year after placement.
- Minimum notional applies to every new order.
- The remainder of a partly filled order keeps resting.

### 4.5 Fill reporting

| Channel | Fields used |
|---|---|
| `ORDER_TRADE_UPDATE` | `i` order id, `c` client order id, `x` execution type, `X` order status, `q` original quantity, `l` last filled quantity, `z` accumulated filled quantity, `L` last filled price, `ap` average price, `n` commission, `N` commission asset, `t` trade id, `T` trade time, `m` maker flag, `rp` realized profit |
| `TRADE_LITE` | `i`, `l`, `L`, `t`, `m`; no commission, no accumulated quantity |
| `GET /fapi/v1/order` | `status`, `origQty`, `executedQty`, `cumQuote`, `avgPrice` |
| `GET /fapi/v1/userTrades` | per trade: `orderId`, `price`, `qty`, `commission`, `commissionAsset`, `maker`, `realizedPnl`, `time` |

```
Trade(i, x, px, fee, maker)   from ORDER_TRADE_UPDATE with execution type TRADE:  x = l,  px = L,  fee = n or 0,  maker = m
r_i                           z of the order at level i
Fill(i)                       X = FILLED, equivalently z = q
Reject(i)                     X = REJECTED, EXPIRED or EXPIRED_IN_MATCH, or CANCELED without a cancel sent by the program
```

- The user data stream is the primary source. Events of one type on one connection arrive in order.
- The REST queries reconcile the state after a reconnection.
- A missing commission field means a commission of 0.
- The program maps every order id to its level.

## 5. Grid

### 5.1 Parameters

```
Θ = ( L, U, N, q, TP, SL, P_top, P_bot, trigger, close_tp_sl, close_risk )
R = ( M, ℓ )
```

| Symbol | Meaning | Admissible values |
|---|---|---|
| `L`, `U` | lower and upper bound of the range | `L < U`; `τ` divides `L` |
| `N` | number of grids, equal to the number of resting grid orders | `2 ≤ N ≤ n_max − 2`, two places kept for the backstop orders; `τ` divides `δ` |
| `q` | quantity of every order | `σ` divides `q`; `q·L ≥ ν` |
| `TP` | take-profit, given as an amount or as ROI of `M` | positive, or unset |
| `SL` | stop-loss, given as an amount or as ROI of `M` | negative, or unset |
| `P_top`, `P_bot` | price stops | unset, or `P_bot < P_trig < P_top` at start |
| `trigger` | price used by TP, SL and the price stops | `last` or `mark`; default `last` |
| `close_tp_sl` | close the position when the run ends by `tp`, `sl` or `price` | `on` or `off`; default `on` |
| `close_risk` | close the position when the run ends by `failure` or `risk` | `on` or `off`; default `on` |
| `M` | margin allocated to the run | `0 < M ≤ W` |
| `ℓ` | leverage setting on the symbol | `1 ≤ ℓ ≤ ℓ_max` |

- `q` is set directly. It is not derived from `M` and `ℓ`.
- A `TP` or `SL` given as ROI is converted at start: amount = ROI·`M`.
- Sizing preset, Binance's rule: `q_binance(M, ℓ) = 0.8·M·ℓ / Σ_{i≠h0} p_i`, expressed in multiples of `σ`.

### 5.2 Derived quantities

```
δ    = (U − L) / N
p_i  = L + i·δ                                    i = 0..N,  N + 1 levels
K    = max(h0, N − h0)
largest long position                           = q·h0
largest short position                          = q·(N − h0)
profit of one matched pair before fees          = q·δ
loss at a range bound after a one-way move      = q·δ·k(k − 1)/2 ,   k = h0 at L ,  k = N − h0 at U
```

### 5.3 State

```
s      = ( phase, h0, h, r, ledger )
phase  ∈ { Init, Run, Closing, Ended }
h0     ∈ { 1..N − 1 }        empty level at start
h      ∈ { 0..N }            empty level now: the level of the last completed fill
r_i    ∈ [0, q)              filled part of the resting order at level i; 0 where no order is partly filled
ledger                       every trade with level, side, quantity, price, fee, maker flag and time; every funding payment
pos    = q·(h0 − h) + Σ_{i<h} r_i − Σ_{i>h} r_i
```

### 5.4 Initial state

```
preconditions, all required before any order is sent
  Θ and R admissible under the B read at start
  account as in 4.2, leverage ℓ set on the symbol
  pos = 0 and no resting orders on the symbol
  1 ≤ h0 ≤ N − 1
  ρ(s0) ≥ 1                        equivalently  max( Σ_{i<h0} q·p_i , Σ_{i>h0} q·p_i ) / ℓ ≤ M
  D_low > 0 and D_high > 0         the ladder fits the remaining loss budget, 6.1
  P_bot < P_trig < P_top           for each price stop that is set

construction
  P0  = P_last at start
  h0  = argmin_i |p_i − P0|        a tie goes to the lower i
  send  buy  q @ p_i  for every i < h0
  send  sell q @ p_i  for every i > h0
  s0  = ( Init, h0, h0, r = 0, empty ledger )
  send the two backstop orders of 6.1
  phase → Run when all N orders and both backstop orders are acknowledged
  Reject during Init → Closing, reason failure
```

- Every start order sits at least `δ/2` away from `P0`, so none crosses at `P0`.
- Trades that arrive during `Init` are processed by `Step` as in `Run`.

### 5.5 Quote

```
Quote(s) = { buy (q − r_i) @ p_i : i < h }  ∪  { sell (q − r_i) @ p_i : i > h }
```

- No order rests at level `h`.
- `h = 0`: no buys rest. `h = N`: no sells rest.

### 5.6 Step

```
Step(s, Trade(i, x, px, fee, maker))
  add the trade to the ledger
  r_i ← r_i + x
  if r_i = q:   r_i ← 0 ;  Fill(i)

Fill(i)                               i = h − 1 for a buy ,  i = h + 1 for a sell
  g ← h
  h ← i
  send the order Quote(s) now requires at level g
      after a buy fill:    sell q @ p_g ,  g = i + 1
      after a sell fill:   buy  q @ p_g ,  g = i − 1

Step(s, Funding(amount))              add the payment to the ledger
Step(s, Reject(i))                    Closing, reason failure
Step(s, Backstop)                     Closing, reason backstop
Step(s, Command(stop, keep))          Closing, reason command
Step(s, Price(...)), Step(s, Tick)    state unchanged
after every Step                      Stop is evaluated
```

- Partial fill: the remainder keeps resting. No opposite order is sent before the order is complete.
- Crossing order: its trades are processed as any other `Trade`; the fee is the taker fee.
- A fill price can be better than the level price. The ledger keeps the actual price.

### 5.7 Invariants

| Id | Statement | Basis |
|---|---|---|
| I1 | after every `Step` the resting orders equal `Quote(s)` | R2 |
| I2 | a `Fill` occurs only at level `h − 1` or `h + 1` | the exchange fills better prices first |
| I3 | `pos = q·(h0 − h) + Σ_{i<h} r_i − Σ_{i>h} r_i` | I1, I2 |
| I4 | `−q·(N − h0) ≤ pos ≤ q·h0` | I3 and `r_i < q` |

### 5.8 Value

```
over the ledger of one run
  cash      = Σ_sells x·px − Σ_buys x·px
  fees      = Σ trade fees
  funding   = Σ funding payments

Π(P)        = cash + pos·P − fees + funding
Π_last      = Π(P_last)                           reported total profit
Π_mark      = Π(P_mark)
P_trig      = P_last if trigger = last ,  P_mark if trigger = mark
Π_trig      = Π(P_trig)                           used by TP and SL
matched     = Σ over pairs of  q·(sell px − buy px) − fees of the two orders
unmatched   = Π_last − matched − funding
ROI         = Π_last / M

wallet
  W         = wallet balance of the sub-account
  e         = average entry price of the open position
  V         = W + pos·(x − e) − f_t·|pos|·x ,     x = b if pos > 0 ,  x = a if pos < 0
  V         = W                                   if pos = 0
```

- A pair is one completed buy order and one completed sell order, paired first-in-last-out: a completed order that reduces the position is paired with the most recent unpaired completed order of the opposite side.
- Partial lots and unpaired lots count in `unmatched`.
- Fees come per trade from the exchange.

### 5.9 Stop

| Order | Reason | Condition | Position at the end |
|---|---|---|---|
| 1 | `guard` | `V < V0·(1 − d)`, section 6 | closed |
| 2 | `backstop` | `Backstop`, section 6.1 | closed by the exchange |
| 3 | `failure` | `Reject(i)`; no `Price` event for `T_data`; user data stream lost for `T_stream` | closed if `close_risk` is on |
| 4 | `risk` | `ρ < 1` | closed if `close_risk` is on |
| 5 | `sl` | `SL` set and `Π_trig ≤ SL` | closed if `close_tp_sl` is on |
| 6 | `price` | `P_top` set and `P_trig ≥ P_top`, or `P_bot` set and `P_trig ≤ P_bot` | closed if `close_tp_sl` is on |
| 7 | `tp` | `TP` set and `Π_trig ≥ TP` | closed if `close_tp_sl` is on |
| 8 | `command` | `Command(stop, keep)` | closed unless `keep` |

- Conditions are evaluated after every `Step`, so at least once per second.
- If several conditions hold at once, the first in the table gives the reason.
- A price outside the range is not an exit. With `h = 0` or `h = N` the run waits with its largest position.
- Resting orders are canceled at every exit.
- A position left open stays in the wallet and counts in `V`. No run starts while it exists (5.4).

```
ρ                = margin balance / occupied margin
margin balance   = M + Π_mark                         Π_mark is Π_last computed with P_mark
occupied margin  = max( |pos·P_mark + Σ_bids| , |pos·P_mark − Σ_asks| ) / ℓ
Σ_bids, Σ_asks   = notional of the resting buys, notional of the resting sells

Closing
  cancel every resting order of the symbol, backstop orders included
  if pos ≠ 0 and the reason closes the position:      send a reduce-only MARKET order for |pos|
  verify: no resting orders, and pos = 0 when the position was closed
  Ended(reason); the ledger is closed
```

### 5.10 Profit identity

```
C = number of completed fills in the run
k = |h − h0|
m = (C − k) / 2                            matched pairs

with every fill at its level price and no partial lot, before fees and funding:
  Π_last = q·δ·m − q·δ·k(k − 1)/2 + pos·(P_last − p_h)
         = (q·δ/2)·(C − k²) + pos·(P_last − p_h)
```

| Statement | Basis |
|---|---|
| at a level price, `Π_last > 0` exactly when `C > k²` | identity |
| for a driftless symmetric walk on the levels, the expected value of `C − k²` is 0 | `k² − C` is a martingale; simulation |
| profit before fees therefore requires mean reversion between levels | the two rows above |
| outside the range the loss grows linearly in price with the largest position | I3, I4 |

### 5.11 Cells

```
cell j ,  j = 0..N − 1
  state     buy:   a buy  q @ p_j     rests
            sell:  a sell q @ p_(j+1) rests
  start     buy for j < h0 ,  sell for j ≥ h0
  on fill   a filled buy re-arms the sell ;  a filled sell re-arms the buy
```

| Id | Statement |
|---|---|
| C1 | the resting orders of all cells equal `Quote(s)` |
| C2 | the number of cells in the buy state equals `h`, and they are the lowest cells |
| C3 | a cell's buy at `p_j` and its sell at `p_(j+1)`, in either order, form a matched pair of 5.8 |
| C4 | the fills of a cell depend on the tape and on that cell alone |

- Basis: I1 and I2. Checked by simulation on 3,000 random tapes with 36,565 fills: identical fill sequences and states.
- Stated for complete fills.

### 5.12 Replay

```
orders file, written from Θ before any fill is known: one row per cell
  symbol, cell, buy_px = p_j, sell_px = p_(j+1), qty = q, first_side, t_start_ms, t_end_ms

fill judgement, by the backtracing module (aetelier-tracer), row by row
  a resting order is filled by the first print of the opposite aggressor strictly through its price
  a print at the price is a touch; the order keeps resting
  after a fill the other side becomes active Δ later ,  Δ ∈ { 0, 250, 1000 } ms
  an order already through at activation is filled at once as taker at its price
  output: every order with activation time, fill time and price, and touches; price bars

accounting, by this program, on that output
  Value (5.8) and Stop (5.9) run over the fills and the bars
  the first exit condition met ends the run; later fills are discarded
  P0 and h0 are taken from the bars at t_start_ms
```

- This program holds no fill logic and reads no raw tape in replay.
- In replay an order is filled completely or not at all. Partial fills do not occur.
- Reason: the queue position at a level is unknown without order-book data.
- An exit only ends a run; it does not change earlier fills. Cutting the fill list at the exit time is exact up to the resolution of the bars.

## 6. Program

```
Program    = ( V0, d, T_dead, T_data, T_stream, runs )
wallet     = the sub-account of 4.2
V0         = V at the first live start; later runs do not change it
d          = loss cap ,  0 < d < 1
T_dead     = countdown of the dead-man switch
T_data     = longest tolerated silence of market data
T_stream   = longest tolerated loss of the user data stream
guard      : V < V0·(1 − d)   →   every resting order is canceled and the position is closed, with or without an active run
                                   an active run ends with reason guard
                                   no run starts until the operator resets the guard
start      : by operator command only, carrying Θ and R
             allowed when no run is active, the guard is not tripped, and the preconditions of 5.4 hold
checkpoint : every day at 00:00 UTC, record V and whether V ≥ V0
```

- Nothing restarts by itself after a run ends.
- Changing `Θ` during a run is `Command(stop, keep)` followed by a new start.

### 6.1 Exchange-side protections

They act when the program is dead. With the program alive the guard acts first.

```
dead-man switch
  a countdown of T_dead on the symbol, renewed by the program at least every T_dead / 3 while a run is active
  without renewal the exchange cancels every resting order of the symbol

backstop orders, sent in Init and canceled in Closing
  budget      B       = V − V0·(1 − d)                          at start
  long side   D_low   = ( B − q·δ·h0·(h0 − 1)/2 ) / ( q·h0 )
  short side  D_high  = ( B − q·δ·(N − h0)·(N − h0 − 1)/2 ) / ( q·(N − h0) )
  sell stop, close-position, trigger  L − D_low                 closes a long position
  buy  stop, close-position, trigger  U + D_high                closes a short position
  triggers are rounded to the tick toward the range
  a backstop trade  →  event Backstop  →  Closing, reason backstop
```

- Start precondition: `D_low > 0` and `D_high > 0`. The loss at each range bound after a one-way move is then smaller than the budget `B`.
- The distances ignore fees, funding and profit earned during the run.
- A position kept open after a run has no backstop order.

## 7. Reference instance

Configuration of a grid created in the Binance app on SOLUSDC.

| Parameter | Value |
|---|---|
| contract | SOLUSDC perpetual |
| `L`, `U`, `N` | 56, 84, 40 |
| `q` | 0.11 SOL |
| `TP`, `SL` | ROI 1% = +0.2 USDC, unset |
| `P_top`, `P_bot` | unset |
| `trigger` | `last` |
| `close_tp_sl`, `close_risk` | `on`, `on` |
| `M`, `ℓ` | 20 USDC, 20x |
| `P0` | 70.21 |

| Derived quantity | Value |
|---|---|
| `δ` | 0.70 |
| levels | 41, from 56.00 to 84.00 |
| `h0` | 20, price 70.00 |
| buys | 20, at 56.00 to 69.30 |
| sells | 20, at 70.70 to 84.00 |
| largest position | 2.2 SOL, long or short |
| profit of one matched pair | 0.077 USDC before fees |
| resting notional at start | 137.83 USDC of buys, 170.17 USDC of sells |
| `ρ` at start | 2.35 |
| loss at either range bound after a one-way move | 14.63 USDC, 73% of `M` |
| `ρ < 1` after a one-way move of | 18 levels, near 57.40 or 82.60, loss 11.78 USDC |
| `TP` | 2.6 matched pairs |
| `q_binance(20, 20)` | 0.8·20·20 / 2800 = 0.1143, shown as 0.11 |

| Displayed in the app | Recomputed |
|---|---|
| 40 order prices | the 41 levels without 70.00 |
| percent to fill of each order | 40 of 40, truncated to two decimals |
| quantity per order 0.11 | 0.1143 by Binance's rule |
| estimated liquidation price 53.83 long, 86.01 short | 53.83 and 86.01, with one side fully filled and `mmr` 0.5% |
| risk 2.3 | `ρ` = 2.35 |
| profit per grid 0.84% to 1.25% | `U/(U − δ) − 1` and `δ/L`, Binance's formula with maker fee 0 |

- With `ℓ` = 5 and `M` = 20 USDC the instance is not admissible: `ρ(s0)` = 0.59.
- With `ℓ` = 5 and `M` = 100 USDC the same ladder keeps `ρ ≥ 2.29` across the range and loses 14.6% of `M` at a bound.
- With `V0` = 100 USDC and `d` = 0.2 the backstop triggers of this ladder are 53.56 and 86.44.

## 8. Correspondence with Binance Futures Grid

Neutral mode, arithmetic spacing.

| Aspect | Binance Futures Grid | This definition | Status |
|---|---|---|---|
| levels | `L + i·(U − L)/N` | same | documented |
| orders | `N` orders on `N + 1` levels; the level nearest the start price stays empty | same | documented, observed |
| tie between two nearest levels | not stated | lower level | assumed |
| start near a bound | not stated | refused unless both sides get an order | rule of this definition |
| quantity | derived, `0.8·M·ℓ / Σp` | set directly; Binance's rule kept as a preset | differs by choice |
| order type | limit orders; post-only not stated | `LIMIT`, `GTC` | documented |
| opposite order after a fill | one level away from the fill | same | observed, documented by example |
| pairing | first-in-last-out | same | documented, observed |
| partial fill | counted as unmatched; timing of the opposite order not stated | counted as unmatched; opposite order on completion | assumed |
| total profit | realized + unrealized + funding | same, `Π_last` | documented |
| TP and SL | on total profit, last or mark price, entered as ROI or as an amount | same: `Π_trig`, parameter `trigger`, amount or ROI | documented |
| price stops | stop top price, stop bottom price | same, optional | documented |
| risk exit | grid expires when the risk ratio falls below 1 | same formula | documented; margin balance assumed |
| order failure | grid expires | run ends | documented |
| position at an exit | two toggles: on TP/SL trigger, on order failure or risk trigger | same: `close_tp_sl`, `close_risk`; the guard always closes | documented, observed |
| price outside the range | no new positions; trading resumes when the price returns | same | documented |
| funds | Trading Bots account, apart from Spot and Futures | dedicated sub-account | differs by venue feature |
| position and asset mode | one-way, single-asset | same | documented |
| fees | follow the futures fee schedule | read at start, reported | documented |
| restart | manual | manual | same |
| modify parameters | ends the grid and creates a new one | stop command, then a new start | documented |
| wallet guard | none | `V < V0·(1 − d)` | added here |
| exchange-side protections | not needed: the bot runs at the exchange | dead-man switch and two backstop orders, section 6.1 | added here |
| not covered | Long and Short modes, geometric spacing, trailing up and down, grid trigger, auto-add margin, more than 169 grids | — | out of scope |

## 9. Status of facts

| Status | Meaning |
|---|---|
| observed | seen in the Binance app: configuration, open orders and details of a SOLUSDC grid created on 2026-11-22; matched history of an older SOLUSDC grid with fills on 2026-08-13 and 2026-08-14 |
| documented | stated on a Binance page listed in section 10 |
| reported | read from Binance's public API on 2026-11-22 |
| derived | computed from observed or documented values |
| assumed | chosen here where no source decides |
| unverified | value from a secondary source |

Observed in the matched history of the older grid, 0.16 SOL per order, step 0.44:

| Observation | Value |
|---|---|
| pair | sell at 76.27, then buy at 75.83 |
| pair | sell at 75.83, then buy at 75.39 |
| matched profit of each pair | 0.0704 USDC = 0.16 × 0.44 |
| fee on each of the four legs | 0 |
| pairing order | the later sell was paired first |
| totals | 24 matched trades, 1.69 USDC |

Assumed:

| Assumption | Affects |
|---|---|
| the opposite order is sent when the filled order is complete, not on a partial fill | 5.6 |
| margin balance in `ρ` is `M` plus total profit at the mark price | 5.9 |
| one `ORDER_TRADE_UPDATE` is pushed per trade and carries the commission of that trade | 4.5 |
| a tie between two nearest levels goes to the lower level | 5.4 |
| the toggle for TP and SL also governs the price stops | 5.9 |
| close-position stop orders exist for this contract and are not canceled by the dead-man switch | 6.1 |
| the exchange fills resting orders at better prices first | I2 |

Unverified:

| Value | Note |
|---|---|
| taker fee 0.04% | read at start |
| maker fee today | 0 on the August 2026 fills; read at start |
| funding interval of SOLUSDC | 8 h is the reported default |

## 10. Sources

- Binance support, "What Is Futures Grid Trading?", updated 2026-11-17: https://www.binance.com/en/support/faq/what-is-futures-grid-trading-f4c453bab89648beb722aa26634120c3
- Binance support, "What Is Long/Short Grid Trading?", updated 2025-12-31: https://www.binance.com/en/support/faq/what-is-long-short-grid-trading-904e47602a3941b99e960a31e152a986
- Binance support, "Trading Bots Account: Migration Rules and Frequently Asked Questions", updated 2026-01-08: https://www.binance.com/en/support/faq/trading-bots-account-migration-rules-and-frequently-asked-questions-408ab63b852e47748a9554501669bce5
- Binance blog, "Demystifying the Futures Grid Bot", 2023-12-12: https://www.binance.com/en/blog/markets/demystifying-the-futures-grid-bot-5592639286007523221
- Binance developers, USDⓈ-M Futures user data streams: https://developers.binance.com/en/docs/products/derivatives-trading-usds-futures/user-data-streams
- Binance developers, USDⓈ-M Futures common definition: https://developers.binance.com/docs/derivatives/usds-margined-futures/common-definition
- Binance developers, USDⓈ-M Futures trade REST API: https://developers.binance.com/docs/derivatives/usds-margined-futures/trade/rest-api/Query-Order
- Binance developers, full documentation text: https://developers.binance.com/en/docs/llms-full.txt
