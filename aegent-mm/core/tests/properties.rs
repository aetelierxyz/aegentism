use std::collections::BTreeMap;

use aegent_mm_core::cells::{Cell, Cells, first_side, owner};
use aegent_mm_core::numeric::ceil_quotient;
use aegent_mm_core::params::{Instance, Resources, Theta};
use aegent_mm_core::quote::{Order, quote, resting_notional};
use aegent_mm_core::state::State;
use aegent_mm_core::step::{StepError, step};
use aegent_mm_core::value::{
    matched, pairing, profit_at, run_wallet_value, unmatched, wallet_value,
};
use aegent_mm_core::{Binding, Event, EventKind, Prices, Side, reference};
use proptest::prelude::*;
use proptest::sample::Index;
use proptest::test_runner::TestCaseError;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal_macros::dec;

#[derive(Debug, Clone)]
struct Setup {
    instance: Instance,
    start_level: u32,
}

#[derive(Debug, Clone)]
enum Aside {
    Nothing,
    Price(u16),
    Tick,
    Funding(i16),
}

#[derive(Debug, Clone)]
struct Move {
    up: bool,
    part: u16,
    improve: u8,
    fee_bps: u8,
    maker: bool,
    aside: Aside,
    valued_at: u16,
}

fn setups(max_grids: u32) -> impl Strategy<Value = Setup> {
    (
        prop_oneof![Just(1i64), Just(5i64), Just(10i64)],
        prop_oneof![Just(1i64), Just(10i64), Just(1000i64)],
        500i64..=50_000,
        1i64..=300,
        2u32..=max_grids,
        0i64..=400,
        any::<Index>(),
    )
        .prop_map(
            |(tick_cents, lot_mills, lower_ticks, spacing_ticks, grids, extra_lots, pick)| {
                let tick = Decimal::new(tick_cents, 2);
                let lot = Decimal::new(lot_mills, 3);
                let lower = tick * Decimal::from(lower_ticks);
                let spacing = tick * Decimal::from(spacing_ticks);
                let upper = lower + spacing * Decimal::from(grids);
                let binding = Binding {
                    tick_size: tick,
                    lot_step: lot,
                    ..reference::app_binding()
                };
                let least_lots = ceil_quotient(binding.min_notional, lower * lot);
                let quantity = lot * (least_lots + Decimal::from(extra_lots));
                let wallet = dec!(1000000);
                let instance = Instance::admit(
                    binding,
                    Theta::new(lower, upper, grids, quantity),
                    Resources {
                        margin: wallet,
                        leverage: 20,
                    },
                    wallet,
                )
                .expect("admissible by construction");
                let start_level = 1 + pick.index(grids as usize - 1) as u32;
                Setup {
                    instance,
                    start_level,
                }
            },
        )
}

fn asides(with_funding: bool) -> BoxedStrategy<Aside> {
    if with_funding {
        prop_oneof![
            4 => Just(Aside::Nothing),
            1 => any::<u16>().prop_map(Aside::Price),
            1 => Just(Aside::Tick),
            1 => any::<i16>().prop_map(Aside::Funding),
        ]
        .boxed()
    } else {
        prop_oneof![
            4 => Just(Aside::Nothing),
            1 => any::<u16>().prop_map(Aside::Price),
            1 => Just(Aside::Tick),
        ]
        .boxed()
    }
}

fn mixed_moves() -> impl Strategy<Value = Vec<Move>> {
    prop::collection::vec(
        (
            any::<bool>(),
            prop_oneof![2 => Just(0u16), 3 => any::<u16>()],
            0u8..=3,
            0u8..=10,
            any::<bool>(),
            asides(true),
            any::<u16>(),
        )
            .prop_map(
                |(up, part, improve, fee_bps, maker, aside, valued_at)| Move {
                    up,
                    part,
                    improve,
                    fee_bps,
                    maker,
                    aside,
                    valued_at,
                },
            ),
        1..160,
    )
}

fn complete_moves() -> impl Strategy<Value = Vec<Move>> {
    prop::collection::vec(
        (any::<bool>(), asides(false), any::<u16>()).prop_map(|(up, aside, valued_at)| Move {
            up,
            part: 0,
            improve: 0,
            fee_bps: 0,
            maker: true,
            aside,
            valued_at,
        }),
        1..160,
    )
}

fn book_of(orders: &[Order]) -> BTreeMap<u32, (Side, Decimal)> {
    orders
        .iter()
        .map(|order| (order.level, (order.side, order.quantity)))
        .collect()
}

struct CellPairs {
    pairs: Vec<(u64, u64)>,
    open: Vec<u64>,
}

struct Harness {
    instance: Instance,
    state: State,
    book: BTreeMap<u32, (Side, Decimal)>,
    cells: Cells,
    isolated: Vec<Cell>,
    completed: Vec<(u64, u32, Side)>,
    clock: u64,
    bought: Decimal,
    sold: Decimal,
    cash: Decimal,
    fees: Decimal,
    funding: Decimal,
    held: Decimal,
    entry: Decimal,
    realized: Decimal,
}

impl Harness {
    fn new(setup: &Setup) -> Harness {
        let instance = setup.instance.clone();
        let mut state =
            State::initial(&instance, setup.start_level).expect("start level inside the ladder");
        state.acknowledge_start().expect("state in Init");
        let book = book_of(&quote(&instance, &state));
        let cells = Cells::start(&instance, setup.start_level);
        let isolated = (0..instance.grids())
            .map(|index| Cell::start(index, setup.start_level))
            .collect();
        Harness {
            instance,
            state,
            book,
            cells,
            isolated,
            completed: Vec::new(),
            clock: 0,
            bought: Decimal::ZERO,
            sold: Decimal::ZERO,
            cash: Decimal::ZERO,
            fees: Decimal::ZERO,
            funding: Decimal::ZERO,
            held: Decimal::ZERO,
            entry: Decimal::ZERO,
            realized: Decimal::ZERO,
        }
    }

    fn quantity(&self) -> Decimal {
        self.instance.quantity()
    }

    fn tick(&self) -> Decimal {
        self.instance.binding().tick_size
    }

    fn some_price(&self, selector: u16) -> Decimal {
        let span = self.instance.upper() - self.instance.lower() + self.instance.spacing();
        let ticks = span / self.tick();
        let offset = Decimal::from(selector) % (ticks + Decimal::ONE);
        self.instance.lower() - self.instance.spacing() / Decimal::TWO + offset * self.tick()
    }

    fn aside(&mut self, aside: &Aside) -> Result<(), TestCaseError> {
        let kind = match aside {
            Aside::Nothing => return Ok(()),
            Aside::Price(selector) => EventKind::Price(Prices::flat(self.some_price(*selector))),
            Aside::Tick => EventKind::Tick,
            Aside::Funding(amount) => EventKind::Funding {
                amount: Decimal::new(i64::from(*amount), 4),
            },
        };
        self.clock += 1;
        let before = (self.state.empty_level(), quote(&self.instance, &self.state));
        let stepped = step(
            &self.instance,
            &mut self.state,
            &Event::at(self.clock, kind.clone()),
        );
        prop_assert!(stepped.is_ok());
        let stepped = stepped.expect("checked above");
        prop_assert_eq!(stepped.fill, None);
        prop_assert_eq!(stepped.send, None);
        prop_assert_eq!(
            (self.state.empty_level(), quote(&self.instance, &self.state)),
            before
        );
        if let EventKind::Funding { amount } = kind {
            self.funding += amount;
        }
        Ok(())
    }

    fn account(&mut self, side: Side, quantity: Decimal, price: Decimal) {
        let signed = match side {
            Side::Buy => quantity,
            Side::Sell => -quantity,
        };
        let adds = self.held.is_zero() || (self.held > Decimal::ZERO) == (signed > Decimal::ZERO);
        if adds {
            let size = self.held.abs() + quantity;
            self.entry = (self.entry * self.held.abs() + price * quantity) / size;
            self.held += signed;
            return;
        }
        let closed = quantity.min(self.held.abs());
        let direction = if self.held > Decimal::ZERO {
            Decimal::ONE
        } else {
            -Decimal::ONE
        };
        self.realized += (price - self.entry) * closed * direction;
        self.held += signed;
        if quantity > closed {
            self.entry = price;
        }
    }

    fn trade(&mut self, chosen: &Move) -> Result<(), TestCaseError> {
        let grids = self.instance.grids();
        let empty = self.state.empty_level();
        let up = (chosen.up && empty < grids) || empty == 0;
        let (level, side) = if up {
            (empty + 1, Side::Sell)
        } else {
            (empty - 1, Side::Buy)
        };
        let lot = self.instance.binding().lot_step;
        let remaining = self.quantity() - self.state.filled(level);
        let remaining_lots = (remaining / lot).to_u32().expect("a whole number of lots");
        let quantity = if chosen.part == 0 || remaining_lots < 2 {
            remaining
        } else {
            lot * Decimal::from(1 + u32::from(chosen.part - 1) % (remaining_lots - 1))
        };
        let better = self.tick() * Decimal::from(chosen.improve);
        let price = match side {
            Side::Buy => self.instance.level_price(level) - better,
            Side::Sell => self.instance.level_price(level) + better,
        };
        let fee = quantity * price * Decimal::new(i64::from(chosen.fee_bps), 4);
        self.clock += 1;
        let event = Event::at(
            self.clock,
            EventKind::Trade {
                level,
                quantity,
                price,
                fee,
                maker: chosen.maker,
            },
        );
        let cells_before = self.cells.clone();
        let stepped = step(&self.instance, &mut self.state, &event);
        prop_assert!(stepped.is_ok());
        let stepped = stepped.expect("checked above");
        let completes = quantity == remaining;
        prop_assert_eq!(stepped.fill.is_some(), completes);
        prop_assert_eq!(stepped.send.is_some(), completes);
        match side {
            Side::Buy => {
                self.bought += quantity;
                self.cash -= quantity * price;
            }
            Side::Sell => {
                self.sold += quantity;
                self.cash += quantity * price;
            }
        }
        self.fees += fee;
        self.account(side, quantity, price);
        let resting = self.book.get(&level).copied();
        prop_assert_eq!(resting, Some((side, remaining)));
        if completes {
            self.book.remove(&level);
            self.completed.push((self.clock, level, side));
            let fill = stepped.fill.expect("a completed order is a fill");
            prop_assert!(fill.level + 1 == empty || fill.level == empty + 1);
            prop_assert_eq!((fill.level, fill.side), (level, side));
            prop_assert_eq!(self.state.empty_level(), level);
        } else {
            self.book.insert(level, (side, remaining - quantity));
            prop_assert_eq!(self.state.empty_level(), empty);
        }
        if let Some(order) = &stepped.send {
            prop_assert_eq!(order.level, empty);
            prop_assert_eq!(order.side, side.opposite());
            prop_assert_eq!(order.quantity, self.quantity());
            prop_assert_eq!(order.price, self.instance.level_price(empty));
            prop_assert!(!self.book.contains_key(&order.level));
            self.book.insert(order.level, (order.side, order.quantity));
        }
        let cell = owner(&self.instance, level, side).expect("the order belongs to a cell");
        let rearmed = self.cells.trade(&self.instance, level, side, quantity);
        prop_assert_eq!(rearmed.as_ref(), Ok(&stepped.send));
        let alone = self.isolated[cell as usize].trade(&self.instance, side, quantity);
        prop_assert_eq!(alone.as_ref(), Ok(&stepped.send));
        for index in 0..grids {
            if index != cell {
                prop_assert_eq!(self.cells.cell(index), cells_before.cell(index));
            }
        }
        Ok(())
    }

    fn check(&self) -> Result<(), TestCaseError> {
        let grids = self.instance.grids();
        let quantity = self.quantity();
        let empty = self.state.empty_level();
        let start = self.state.start_level();
        let quoted = quote(&self.instance, &self.state);
        prop_assert_eq!(&self.book, &book_of(&quoted));
        prop_assert_eq!(quoted.len() as u32, grids);
        prop_assert_eq!(self.state.filled(empty), Decimal::ZERO);
        for level in 0..=grids {
            let filled = self.state.filled(level);
            prop_assert!(filled >= Decimal::ZERO && filled < quantity);
        }
        let position = self.state.position(quantity);
        prop_assert_eq!(position, self.bought - self.sold);
        prop_assert_eq!(position, self.state.ledger().net_quantity());
        prop_assert_eq!(position, self.held);
        prop_assert!(position <= quantity * Decimal::from(start));
        prop_assert!(position >= -quantity * Decimal::from(grids - start));
        let bids: Decimal = quoted
            .iter()
            .filter(|order| order.side == Side::Buy)
            .map(Order::notional)
            .sum();
        let asks: Decimal = quoted
            .iter()
            .filter(|order| order.side == Side::Sell)
            .map(Order::notional)
            .sum();
        prop_assert_eq!(resting_notional(&self.instance, &self.state), (bids, asks));
        prop_assert_eq!(&self.cells.resting(&self.instance), &quoted);
        prop_assert_eq!(self.cells.buying(), empty);
        for index in 0..grids {
            let cell = self.cells.cell(index).expect("one cell per grid");
            let expected = if index < empty { Side::Buy } else { Side::Sell };
            prop_assert_eq!(cell.state(), expected);
            prop_assert_eq!(Some(cell), self.isolated.get(index as usize));
        }
        prop_assert_eq!(self.state.ledger().cash(), self.cash);
        prop_assert_eq!(self.state.ledger().fees(), self.fees);
        prop_assert_eq!(self.state.ledger().funding(), self.funding);
        Ok(())
    }

    fn advance(&mut self, chosen: &Move) -> Result<(), TestCaseError> {
        self.aside(&chosen.aside)?;
        self.check()?;
        self.trade(chosen)?;
        self.check()
    }

    fn pairs_by_cell(&self) -> Result<CellPairs, TestCaseError> {
        let mut by_cell: BTreeMap<u32, Vec<(u64, Side)>> = BTreeMap::new();
        for (time_ms, level, side) in &self.completed {
            let cell = owner(&self.instance, *level, *side).expect("the order belongs to a cell");
            by_cell.entry(cell).or_default().push((*time_ms, *side));
        }
        let mut pairs = Vec::new();
        let mut open = Vec::new();
        for (cell, orders) in by_cell {
            let mut expected = first_side(cell, self.state.start_level());
            for (_, side) in &orders {
                prop_assert_eq!(*side, expected);
                expected = expected.opposite();
            }
            for couple in orders.chunks(2) {
                match couple {
                    [(first, Side::Buy), (second, _)] => pairs.push((*first, *second)),
                    [(first, Side::Sell), (second, _)] => pairs.push((*second, *first)),
                    [(last, _)] => open.push(*last),
                    _ => {}
                }
            }
        }
        pairs.sort_unstable();
        open.sort_unstable();
        Ok(CellPairs { pairs, open })
    }

    fn check_pairs(&self) -> Result<(), TestCaseError> {
        let result = pairing(self.state.ledger(), self.quantity());
        prop_assert_eq!(result.completed, self.completed.len() as u64);
        for pair in &result.pairs {
            prop_assert_eq!(pair.buy.side, Side::Buy);
            prop_assert_eq!(pair.sell.side, Side::Sell);
            prop_assert_eq!(pair.sell.level, pair.buy.level + 1);
            prop_assert_eq!(pair.buy.quantity, self.quantity());
            prop_assert_eq!(pair.sell.quantity, self.quantity());
        }
        let mut paired: Vec<(u64, u64)> = result
            .pairs
            .iter()
            .map(|pair| (pair.buy.completed_ms, pair.sell.completed_ms))
            .collect();
        paired.sort_unstable();
        let mut unpaired: Vec<u64> = result
            .unpaired
            .iter()
            .map(|order| order.completed_ms)
            .collect();
        unpaired.sort_unstable();
        let by_cell = self.pairs_by_cell()?;
        prop_assert_eq!(paired, by_cell.pairs);
        prop_assert_eq!(unpaired, by_cell.open);
        let moved = self.state.empty_level().abs_diff(self.state.start_level());
        prop_assert_eq!(result.unpaired.len() as u32, moved);
        Ok(())
    }
}

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn the_initial_state_rests_orders_on_both_sides(setup in setups(198)) {
        let state = State::initial(&setup.instance, setup.start_level)
            .expect("start level inside the ladder");
        let orders = quote(&setup.instance, &state);
        let buys = orders.iter().filter(|order| order.side == Side::Buy).count() as u32;
        let sells = orders.iter().filter(|order| order.side == Side::Sell).count() as u32;
        prop_assert_eq!(buys, setup.start_level);
        prop_assert_eq!(sells, setup.instance.grids() - setup.start_level);
        prop_assert!(buys >= 1 && sells >= 1);
        prop_assert!(orders.iter().all(|order| order.quantity == setup.instance.quantity()));
        prop_assert_eq!(state.position(setup.instance.quantity()), Decimal::ZERO);
        prop_assert_eq!(
            Cells::start(&setup.instance, setup.start_level).resting(&setup.instance),
            orders
        );
    }

    #[test]
    fn invariants_and_cell_properties_hold_on_tapes_with_partial_fills(
        setup in setups(24),
        tape in mixed_moves(),
    ) {
        let mut harness = Harness::new(&setup);
        harness.check()?;
        for chosen in &tape {
            harness.advance(chosen)?;
        }
        harness.check_pairs()?;
    }

    #[test]
    fn invariants_and_cell_properties_hold_on_tapes_of_complete_fills(
        setup in setups(24),
        tape in complete_moves(),
    ) {
        let mut harness = Harness::new(&setup);
        harness.check()?;
        for chosen in &tape {
            harness.advance(chosen)?;
            harness.check_pairs()?;
        }
    }

    #[test]
    fn invariants_hold_on_the_largest_ladders(
        setup in setups(198),
        tape in mixed_moves(),
    ) {
        let mut harness = Harness::new(&setup);
        for chosen in &tape {
            harness.advance(chosen)?;
        }
        harness.check_pairs()?;
    }

    #[test]
    fn the_profit_identity_holds_on_tapes_of_complete_fills_at_level_prices(
        setup in setups(24),
        tape in complete_moves(),
    ) {
        let mut harness = Harness::new(&setup);
        let instance = setup.instance.clone();
        let quantity = instance.quantity();
        let unit = instance.pair_profit();
        for chosen in &tape {
            harness.advance(chosen)?;
            let state = &harness.state;
            let completed = harness.completed.len() as i64;
            let moved = i64::from(state.empty_level().abs_diff(state.start_level()));
            prop_assert_eq!((completed - moved) % 2, 0);
            let pairs = (completed - moved) / 2;
            let last = harness.some_price(chosen.valued_at);
            let position = state.position(quantity);
            let carried = position * (last - instance.level_price(state.empty_level()));
            let drawdown = unit * Decimal::from(moved * (moved - 1) / 2);
            let profit = profit_at(&instance, state, last);
            prop_assert_eq!(profit, unit * Decimal::from(pairs) - drawdown + carried);
            prop_assert_eq!(
                profit,
                unit / Decimal::TWO * Decimal::from(completed - moved * moved) + carried
            );
            let result = pairing(state.ledger(), quantity);
            prop_assert_eq!(result.pairs.len() as i64, pairs);
            prop_assert_eq!(result.completed as i64, completed);
            prop_assert_eq!(result.unpaired.len() as i64, moved);
            prop_assert_eq!(result.matched(), unit * Decimal::from(pairs));
            prop_assert_eq!(matched(&instance, state), unit * Decimal::from(pairs));
            prop_assert_eq!(
                unmatched(&instance, state, &Prices::flat(last)),
                carried - drawdown
            );
            let at_level = profit_at(&instance, state, instance.level_price(state.empty_level()));
            prop_assert_eq!(at_level > Decimal::ZERO, completed > moved * moved);
            prop_assert_eq!(at_level.is_zero(), completed == moved * moved);
        }
    }

    #[test]
    fn profit_is_cash_plus_position_value_minus_fees_plus_funding_on_any_tape(
        setup in setups(24),
        tape in mixed_moves(),
    ) {
        let mut harness = Harness::new(&setup);
        let instance = setup.instance.clone();
        for chosen in &tape {
            harness.advance(chosen)?;
            let state = &harness.state;
            let last = harness.some_price(chosen.valued_at);
            let position = state.position(instance.quantity());
            let profit = profit_at(&instance, state, last);
            prop_assert_eq!(
                profit,
                harness.cash + position * last - harness.fees + harness.funding
            );
        }
    }

    #[test]
    fn the_run_wallet_value_agrees_with_the_account_formula(
        setup in setups(24),
        tape in mixed_moves(),
        spread in 0u8..=5,
    ) {
        let mut harness = Harness::new(&setup);
        let instance = setup.instance.clone();
        let taker_fee = instance.binding().taker_fee;
        let start_balance = dec!(1000000);
        let tolerance = dec!(0.000000000001);
        for chosen in &tape {
            harness.advance(chosen)?;
            let last = harness.some_price(chosen.valued_at);
            let half = harness.tick() * Decimal::from(spread);
            let prices = Prices {
                last,
                mark: last,
                bid: last - half,
                ask: last + half,
            };
            let balance = start_balance + harness.realized - harness.fees + harness.funding;
            let reported = wallet_value(balance, harness.entry, harness.held, &prices, taker_fee);
            let derived = run_wallet_value(&instance, &harness.state, start_balance, &prices);
            prop_assert!((reported - derived).abs() <= tolerance);
        }
    }

    #[test]
    fn a_trade_is_accepted_exactly_when_it_is_admissible(
        setup in setups(24),
        tape in mixed_moves(),
        level in 0u32..=26,
        lots in -2i64..=450,
        complete in any::<bool>(),
        offset_ticks in -3i64..=3,
        worthless in prop::bool::weighted(0.05),
    ) {
        let mut harness = Harness::new(&setup);
        for chosen in &tape {
            harness.advance(chosen)?;
        }
        let instance = setup.instance.clone();
        let lot = instance.binding().lot_step;
        let empty = harness.state.empty_level();
        let before = harness.state.clone();
        let quantity = if complete {
            instance.quantity() - before.filled(level)
        } else {
            lot * Decimal::from(lots)
        };
        let limit = instance.level_price(level.min(instance.grids()));
        let price = if worthless {
            Decimal::ZERO
        } else {
            limit + harness.tick() * Decimal::from(offset_ticks)
        };
        let worse = if level < empty {
            price > limit
        } else {
            price < limit
        };
        let event = Event::at(
            harness.clock + 1,
            EventKind::Trade {
                level,
                quantity,
                price,
                fee: Decimal::ZERO,
                maker: true,
            },
        );
        let outcome = step(&instance, &mut harness.state, &event);
        let expected = if level > instance.grids() {
            Err(StepError::UnknownLevel)
        } else if level == empty {
            Err(StepError::NoOrderAtLevel)
        } else if quantity <= Decimal::ZERO {
            Err(StepError::QuantityNotPositive)
        } else if price <= Decimal::ZERO {
            Err(StepError::PriceNotPositive)
        } else if worse {
            Err(StepError::PriceWorseThanLevel)
        } else if before.filled(level) + quantity > instance.quantity() {
            Err(StepError::Overfill)
        } else if before.filled(level) + quantity == instance.quantity()
            && level + 1 != empty
            && level != empty + 1
        {
            Err(StepError::FillNotAdjacent)
        } else {
            Ok(())
        };
        prop_assert_eq!(outcome.as_ref().map(|_| ()).map_err(|error| *error), expected);
        if outcome.is_err() {
            prop_assert_eq!(&harness.state, &before);
        } else {
            prop_assert_eq!(
                harness.state.position(instance.quantity()),
                harness.state.ledger().net_quantity()
            );
            let fill = outcome.expect("checked above").fill;
            let completes = before.filled(level) + quantity == instance.quantity();
            prop_assert_eq!(fill.is_some(), completes);
            if let Some(fill) = fill {
                prop_assert!(fill.level + 1 == empty || fill.level == empty + 1);
                prop_assert_eq!(harness.state.empty_level(), fill.level);
            }
        }
    }
}

#[test]
fn completed_fills_minus_squared_displacement_sum_to_zero_over_all_walks() {
    let instance = Instance::admit(
        reference::app_binding(),
        Theta::new(dec!(56), dec!(84), 40, dec!(0.11)),
        Resources {
            margin: dec!(100),
            leverage: 20,
        },
        dec!(100),
    )
    .expect("admissible");
    let steps = 12u32;
    let mut balance = 0i64;
    let mut profit = Decimal::ZERO;
    let mut winning = 0u32;
    let mut losing = 0u32;
    for path in 0u32..(1 << steps) {
        let mut state = State::initial(&instance, 20).expect("start level inside the ladder");
        for bit in 0..steps {
            let empty = state.empty_level();
            let level = if path >> bit & 1 == 1 {
                empty + 1
            } else {
                empty - 1
            };
            let event = Event::at(
                u64::from(bit),
                EventKind::Trade {
                    level,
                    quantity: instance.quantity(),
                    price: instance.level_price(level),
                    fee: Decimal::ZERO,
                    maker: true,
                },
            );
            step(&instance, &mut state, &event).expect("admissible complete fill");
        }
        let moved = i64::from(state.empty_level().abs_diff(20));
        balance += i64::from(steps) - moved * moved;
        let at_level = profit_at(&instance, &state, instance.level_price(state.empty_level()));
        assert_eq!(
            at_level,
            instance.pair_profit() / Decimal::TWO * Decimal::from(i64::from(steps) - moved * moved)
        );
        profit += at_level;
        if at_level > Decimal::ZERO {
            winning += 1;
        }
        if at_level < Decimal::ZERO {
            losing += 1;
        }
    }
    assert_eq!(balance, 0);
    assert_eq!(profit, Decimal::ZERO);
    assert!(winning > 0 && losing > 0);
}
