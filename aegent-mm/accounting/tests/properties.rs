use aegent_mm_accounting::{Bar, Fill, Outcome, Replay, account};
use aegent_mm_core::ledger::TradeRecord;
use aegent_mm_core::params::{Instance, Resources, Target, Theta};
use aegent_mm_core::program::Program;
use aegent_mm_core::state::{Phase, Reason};
use aegent_mm_core::{Side, reference};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

const T0: u64 = 1_795_478_400_000;
const BAR_MS: u64 = 60_000;
const START_LEVEL: u32 = 20;
const GRIDS: u32 = 40;

#[derive(Debug, Clone)]
struct Tape {
    fills: Vec<Fill>,
    bars: Vec<Bar>,
}

fn level_price(level: u32) -> Decimal {
    dec!(56) + dec!(0.7) * Decimal::from(level)
}

fn tape_from(moves: &[(u8, u32)], drift: u8, wiggles: &[(u8, u8)], quiet_bars: u64) -> Tape {
    let tick = dec!(0.01);
    let mut level = START_LEVEL;
    let mut time_ms = T0;
    let mut fills = Vec::new();
    let mut prints = Vec::new();
    for (roll, pause_ms) in moves {
        time_ms += u64::from(*pause_ms);
        let rises = (*roll < drift && level < GRIDS) || level == 0;
        let (next, side, through) = if rises {
            (level + 1, Side::Sell, tick)
        } else {
            (level - 1, Side::Buy, -tick)
        };
        fills.push(Fill {
            time_ms,
            level: next,
            side,
            price: level_price(next),
            quantity: dec!(0.11),
            maker: true,
        });
        prints.push((time_ms, level_price(next) + through, next));
        level = next;
    }
    let bar_count = (time_ms - T0) / BAR_MS + 1 + quiet_bars;
    let mut bars = Vec::new();
    let mut cursor = 0;
    let mut last = dec!(70.21);
    let mut empty = START_LEVEL;
    for index in 0..bar_count {
        let start_ms = T0 + index * BAR_MS;
        let open = last;
        let mut high = open;
        let mut low = open;
        let mut traded = false;
        while cursor < prints.len() && prints[cursor].0 < start_ms + BAR_MS {
            let (_, print, after) = prints[cursor];
            high = high.max(print);
            low = low.min(print);
            last = print;
            empty = after;
            traded = true;
            cursor += 1;
        }
        if !traded {
            let (rise, fall) = wiggles[index as usize % wiggles.len()];
            let outside = Decimal::from(8);
            high = if empty < GRIDS {
                (open + tick * Decimal::from(rise)).min(level_price(empty + 1))
            } else {
                open + tick * Decimal::from(rise) * outside
            };
            low = if empty > 0 {
                (open - tick * Decimal::from(fall)).max(level_price(empty - 1))
            } else {
                open - tick * Decimal::from(fall) * outside
            };
        }
        bars.push(Bar {
            time_ms: start_ms,
            open,
            high,
            low,
            close: last,
        });
    }
    Tape { fills, bars }
}

fn tapes() -> impl Strategy<Value = Tape> {
    (
        prop::collection::vec((any::<u8>(), 1u32..=90_000), 0..80),
        prop_oneof![2 => Just(128u8), 1 => Just(20u8), 1 => Just(236u8)],
        prop::collection::vec((0u8..=60, 0u8..=60), 1..8),
        0u64..=3,
    )
        .prop_map(|(moves, drift, wiggles, quiet_bars)| {
            tape_from(&moves, drift, &wiggles, quiet_bars)
        })
}

fn thetas() -> impl Strategy<Value = Theta> {
    (
        prop::option::of(1u32..=30),
        prop::option::of(1u32..=40),
        prop::option::of(7100i64..=8400),
        prop::option::of(5600i64..=6950),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(take_profit, stop_loss, top, bottom, close_tp_sl, close_risk)| Theta {
                take_profit: take_profit
                    .map(|steps| Target::Amount(dec!(0.05) * Decimal::from(steps))),
                stop_loss: stop_loss
                    .map(|steps| Target::Amount(dec!(-0.25) * Decimal::from(steps))),
                stop_top: top.map(|cents| Decimal::new(cents, 2)),
                stop_bottom: bottom.map(|cents| Decimal::new(cents, 2)),
                close_tp_sl,
                close_risk,
                ..reference::theta()
            },
        )
}

fn replay(theta: Theta, margin: Decimal, loss_cap: Decimal) -> Replay {
    let instance = Instance::admit(
        reference::app_binding(),
        theta,
        Resources {
            margin,
            leverage: 20,
        },
        dec!(100),
    )
    .expect("admissible");
    Replay {
        instance,
        program: Program::new(dec!(100), loss_cap, 60_000, 5_000, 10_000).expect("valid"),
        wallet_balance: dec!(100),
        start_ms: T0,
        bar_ms: BAR_MS,
    }
}

fn recorded(outcome: &Outcome) -> Vec<TradeRecord> {
    outcome.ledger().trades().cloned().collect()
}

fn check_ledger(outcome: &Outcome, tape: &Tape) -> Result<(), TestCaseError> {
    prop_assert_eq!(
        outcome.fills_applied + outcome.fills_discarded,
        tape.fills.len()
    );
    let expected: Vec<TradeRecord> = tape.fills[..outcome.fills_applied]
        .iter()
        .map(|fill| TradeRecord {
            time_ms: fill.time_ms,
            level: fill.level,
            side: fill.side,
            quantity: fill.quantity,
            price: fill.price,
            fee: Decimal::ZERO,
            maker: fill.maker,
        })
        .collect();
    prop_assert_eq!(recorded(outcome), expected);
    prop_assert_eq!(outcome.run.position(), outcome.ledger().net_quantity());
    Ok(())
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
    fn an_exit_cuts_the_fill_list_and_changes_no_earlier_fill(
        tape in tapes(),
        theta in thetas(),
        roomy in any::<bool>(),
    ) {
        let margin = if roomy { dec!(100) } else { dec!(20) };
        let replay = replay(theta, margin, dec!(0.2));
        let outcome = account(&replay, &tape.fills, &tape.bars);
        prop_assert!(outcome.is_ok());
        let outcome = outcome.expect("checked above");
        check_ledger(&outcome, &tape)?;
        match outcome.ended {
            None => {
                prop_assert_eq!(outcome.fills_discarded, 0);
                prop_assert_eq!(outcome.run.state().phase(), Phase::Run);
            }
            Some(ended) => {
                prop_assert_eq!(outcome.run.state().phase(), Phase::Ended(ended.exit));
                prop_assert!(ended.time_ms >= T0);
                let (kept, cut) = tape.fills.split_at(outcome.fills_applied);
                prop_assert!(kept.iter().all(|fill| fill.time_ms <= ended.time_ms));
                prop_assert!(cut.iter().all(|fill| fill.time_ms >= ended.time_ms));
            }
        }
        let shortened = account(&replay, &tape.fills[..outcome.fills_applied], &tape.bars);
        prop_assert!(shortened.is_ok());
        let shortened = shortened.expect("checked above");
        prop_assert_eq!(&shortened.run, &outcome.run);
        prop_assert_eq!(shortened.ended, outcome.ended);
        prop_assert_eq!(shortened.fills_applied, outcome.fills_applied);
        prop_assert_eq!(shortened.fills_discarded, 0);
    }

    #[test]
    fn the_reason_of_an_exit_holds_at_the_exit(
        tape in tapes(),
        theta in thetas(),
        roomy in any::<bool>(),
    ) {
        let margin = if roomy { dec!(100) } else { dec!(20) };
        let replay = replay(theta, margin, dec!(0.2));
        let outcome = account(&replay, &tape.fills, &tape.bars).expect("a valid replay");
        let instance = &replay.instance;
        let last = outcome.run.prices().last;
        if let Some(ended) = outcome.ended {
            match ended.exit.reason {
                Reason::TakeProfit => {
                    let target = instance.take_profit().expect("take-profit is set");
                    prop_assert!(outcome.profit_last() >= target);
                }
                Reason::StopLoss => {
                    let target = instance.stop_loss().expect("stop-loss is set");
                    prop_assert!(outcome.profit_last() <= target);
                }
                Reason::Price => {
                    let above = instance.theta().stop_top.is_some_and(|top| last >= top);
                    let below = instance
                        .theta()
                        .stop_bottom
                        .is_some_and(|bottom| last <= bottom);
                    prop_assert!(above || below);
                }
                Reason::Guard => {
                    prop_assert!(outcome.wallet_value() < replay.program.floor());
                }
                Reason::Risk => {}
                Reason::Backstop | Reason::Failure | Reason::Command => {
                    prop_assert!(false, "no such exit arises in replay");
                }
            }
        }
    }

    #[test]
    fn a_run_without_exit_conditions_accounts_every_fill_and_meets_the_profit_identity(
        tape in tapes(),
    ) {
        let theta = Theta {
            take_profit: None,
            ..reference::theta()
        };
        let replay = replay(theta, dec!(100), dec!(0.9));
        let outcome = account(&replay, &tape.fills, &tape.bars).expect("a valid replay");
        check_ledger(&outcome, &tape)?;
        prop_assert_eq!(outcome.ended, None);
        prop_assert_eq!(outcome.fills_applied, tape.fills.len());
        let state = outcome.run.state();
        let completed = tape.fills.len() as i64;
        let moved = i64::from(state.empty_level().abs_diff(START_LEVEL));
        let unit = replay.instance.pair_profit();
        let last = outcome.run.prices().last;
        let carried = outcome.run.position() * (last - level_price(state.empty_level()));
        prop_assert_eq!(
            outcome.profit_last(),
            unit / Decimal::TWO * Decimal::from(completed - moved * moved) + carried
        );
        prop_assert_eq!(outcome.pairing().pairs.len() as i64, (completed - moved) / 2);
        prop_assert_eq!(
            outcome.wallet_value(),
            dec!(100) + outcome.profit_last()
                - dec!(0.0004) * outcome.run.position().abs() * last
        );
    }
}
