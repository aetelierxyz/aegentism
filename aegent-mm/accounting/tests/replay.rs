use aegent_mm_accounting::{Bar, Ended, Fill, Replay, ReplayError, account};
use aegent_mm_core::ledger::TradeRecord;
use aegent_mm_core::params::{Instance, Resources, Target, Theta};
use aegent_mm_core::program::Program;
use aegent_mm_core::start::Precondition;
use aegent_mm_core::state::{Exit, Phase, PositionAtEnd, Reason};
use aegent_mm_core::step::StepError;
use aegent_mm_core::{Binding, Side, reference};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

const T0: u64 = 1_795_478_400_000;
const BAR_MS: u64 = 60_000;

fn admitted(binding: Binding, theta: Theta, margin: Decimal) -> Instance {
    Instance::admit(
        binding,
        theta,
        Resources {
            margin,
            leverage: 20,
        },
        dec!(100),
    )
    .expect("admissible")
}

fn plain() -> Theta {
    Theta {
        take_profit: None,
        ..reference::theta()
    }
}

fn tight(theta: Theta) -> Instance {
    admitted(reference::app_binding(), theta, dec!(20))
}

fn roomy(theta: Theta) -> Instance {
    admitted(reference::app_binding(), theta, dec!(100))
}

fn replay(instance: Instance) -> Replay {
    Replay {
        instance,
        program: Program::new(dec!(100), dec!(0.2), 60_000, 5_000, 10_000).expect("valid"),
        wallet_balance: dec!(100),
        start_ms: T0,
        bar_ms: BAR_MS,
    }
}

fn bar(index: u64, open: Decimal, high: Decimal, low: Decimal, close: Decimal) -> Bar {
    Bar {
        time_ms: T0 + index * BAR_MS,
        open,
        high,
        low,
        close,
    }
}

fn still(index: u64, price: Decimal) -> Bar {
    bar(index, price, price, price, price)
}

fn fill(offset_ms: u64, level: u32, side: Side) -> Fill {
    Fill {
        time_ms: T0 + offset_ms,
        level,
        side,
        price: dec!(56) + dec!(0.7) * Decimal::from(level),
        quantity: dec!(0.11),
        maker: true,
    }
}

fn buys_down_to(lowest: u32) -> Vec<Fill> {
    (lowest..20)
        .rev()
        .enumerate()
        .map(|(index, level)| fill(1_000 * (index as u64 + 1), level, Side::Buy))
        .collect()
}

fn ended(reason: Reason, position: PositionAtEnd, time_ms: u64) -> Option<Ended> {
    Some(Ended {
        exit: Exit { reason, position },
        time_ms,
    })
}

#[test]
fn a_quiet_market_leaves_the_run_running_with_an_empty_ledger() {
    let bars = [
        still(0, dec!(70.21)),
        still(1, dec!(70.21)),
        bar(2, dec!(70.21), dec!(70.60), dec!(70.05), dec!(70.30)),
    ];
    let outcome = account(&replay(tight(reference::theta())), &[], &bars).expect("a valid replay");
    assert_eq!(outcome.ended, None);
    assert_eq!(outcome.exit(), None);
    assert_eq!(outcome.exit_ms(), None);
    assert!(outcome.ledger().is_empty());
    assert_eq!(outcome.fills_applied, 0);
    assert_eq!(outcome.fills_discarded, 0);
    assert_eq!(outcome.run.state().phase(), Phase::Run);
    assert_eq!(outcome.run.prices().last, dec!(70.30));
    assert_eq!(outcome.profit_last(), Decimal::ZERO);
    assert_eq!(outcome.wallet_value(), dec!(100));
}

#[test]
fn the_start_price_and_the_empty_level_come_from_the_bar_at_the_start_time() {
    let earlier = Bar {
        time_ms: T0 - BAR_MS,
        open: dec!(60),
        high: dec!(60),
        low: dec!(60),
        close: dec!(60),
    };
    let bars = [
        earlier,
        bar(0, dec!(70.21), dec!(70.40), dec!(70.10), dec!(70.35)),
    ];
    let outcome = account(&replay(tight(reference::theta())), &[], &bars).expect("a valid replay");
    assert_eq!(outcome.run.start_price(), dec!(70.21));
    assert_eq!(outcome.run.state().start_level(), 20);
    let higher = [bar(0, dec!(70.90), dec!(70.95), dec!(70.80), dec!(70.85))];
    let outcome =
        account(&replay(tight(reference::theta())), &[], &higher).expect("a valid replay");
    assert_eq!(outcome.run.start_price(), dec!(70.90));
    assert_eq!(outcome.run.state().start_level(), 21);
}

#[test]
fn a_replay_without_a_bar_at_the_start_time_is_refused() {
    let bars = [Bar {
        time_ms: T0 + 1,
        open: dec!(70.21),
        high: dec!(70.21),
        low: dec!(70.21),
        close: dec!(70.21),
    }];
    assert_eq!(
        account(&replay(tight(reference::theta())), &[], &bars),
        Err(ReplayError::NoBarAtStart)
    );
    assert_eq!(
        account(&replay(tight(reference::theta())), &[], &[]),
        Err(ReplayError::NoBarAtStart)
    );
}

#[test]
fn a_start_that_fails_a_precondition_is_refused() {
    let at_bound = [still(0, dec!(84.00))];
    let lopsided = [still(0, dec!(73.50))];
    assert_eq!(
        account(&replay(tight(reference::theta())), &[], &at_bound),
        Err(ReplayError::Refused(vec![Precondition::EmptyLevelAtBound]))
    );
    assert_eq!(
        account(&replay(tight(reference::theta())), &[], &lopsided),
        Err(ReplayError::Refused(vec![
            Precondition::LowBackstopDistanceNotPositive
        ]))
    );
}

#[test]
fn three_round_trips_reach_the_take_profit_of_the_reference_instance() {
    let fills = [
        fill(10_000, 21, Side::Sell),
        fill(20_000, 20, Side::Buy),
        fill(70_000, 21, Side::Sell),
        fill(80_000, 20, Side::Buy),
        fill(130_000, 21, Side::Sell),
        fill(140_000, 20, Side::Buy),
        fill(150_000, 21, Side::Sell),
    ];
    let bars = [
        bar(0, dec!(70.21), dec!(70.75), dec!(69.95), dec!(70.10)),
        bar(1, dec!(70.10), dec!(70.75), dec!(69.95), dec!(70.05)),
        bar(2, dec!(70.05), dec!(70.75), dec!(69.95), dec!(70.72)),
        still(3, dec!(70.72)),
    ];
    let outcome =
        account(&replay(tight(reference::theta())), &fills, &bars).expect("a valid replay");
    assert_eq!(
        outcome.ended,
        ended(Reason::TakeProfit, PositionAtEnd::Closed, T0 + 140_000)
    );
    assert_eq!(outcome.fills_applied, 6);
    assert_eq!(outcome.fills_discarded, 1);
    assert_eq!(outcome.ledger().trades().count(), 6);
    assert_eq!(outcome.run.position(), Decimal::ZERO);
    assert_eq!(outcome.pairing().pairs.len(), 3);
    assert_eq!(outcome.pairing().matched(), dec!(0.231));
    assert_eq!(outcome.profit_last(), dec!(0.231));
    assert_eq!(outcome.wallet_value(), dec!(100.231));
    assert_eq!(
        outcome.run.state().phase(),
        Phase::Ended(Exit {
            reason: Reason::TakeProfit,
            position: PositionAtEnd::Closed,
        })
    );
}

#[test]
fn the_ledger_holds_the_fills_as_given() {
    let fills = [fill(10_000, 21, Side::Sell), fill(20_000, 20, Side::Buy)];
    let bars = [bar(0, dec!(70.21), dec!(70.75), dec!(69.95), dec!(70.10))];
    let outcome =
        account(&replay(tight(reference::theta())), &fills, &bars).expect("a valid replay");
    let trades: Vec<TradeRecord> = outcome.ledger().trades().cloned().collect();
    assert_eq!(
        trades,
        vec![
            TradeRecord {
                time_ms: T0 + 10_000,
                level: 21,
                side: Side::Sell,
                quantity: dec!(0.11),
                price: dec!(70.70),
                fee: Decimal::ZERO,
                maker: true,
            },
            TradeRecord {
                time_ms: T0 + 20_000,
                level: 20,
                side: Side::Buy,
                quantity: dec!(0.11),
                price: dec!(70.00),
                fee: Decimal::ZERO,
                maker: true,
            },
        ]
    );
    assert_eq!(outcome.ended, None);
    assert_eq!(outcome.profit_last(), dec!(0.077));
}

#[test]
fn a_one_way_fall_ends_by_risk_at_the_end_of_the_bar_whose_low_breaks_the_ratio() {
    let mut fills = buys_down_to(3);
    fills.push(fill(125_000, 2, Side::Buy));
    let bars = [
        bar(0, dec!(70.21), dec!(70.25), dec!(58.05), dec!(58.10)),
        bar(1, dec!(58.00), dec!(58.05), dec!(57.40), dec!(57.60)),
        bar(2, dec!(57.60), dec!(57.60), dec!(57.30), dec!(57.35)),
    ];
    let outcome = account(&replay(tight(plain())), &fills, &bars).expect("a valid replay");
    assert_eq!(
        outcome.ended,
        ended(Reason::Risk, PositionAtEnd::Closed, T0 + 2 * BAR_MS)
    );
    assert_eq!(outcome.fills_applied, 17);
    assert_eq!(outcome.fills_discarded, 1);
    assert_eq!(outcome.run.position(), dec!(1.87));
    assert_eq!(outcome.run.prices().last, dec!(57.40));
    assert_eq!(outcome.profit_last(), dec!(-11.781));
}

#[test]
fn a_fall_below_the_range_ends_by_guard() {
    let fills = buys_down_to(0);
    let bars = [
        bar(0, dec!(70.21), dec!(70.21), dec!(55.90), dec!(55.95)),
        bar(1, dec!(55.95), dec!(56.00), dec!(53.50), dec!(53.55)),
    ];
    let outcome = account(&replay(roomy(plain())), &fills, &bars).expect("a valid replay");
    assert_eq!(
        outcome.ended,
        ended(Reason::Guard, PositionAtEnd::Closed, T0 + 2 * BAR_MS)
    );
    assert_eq!(outcome.fills_applied, 20);
    assert_eq!(outcome.run.position(), dec!(2.2));
    assert_eq!(outcome.wallet_value(), dec!(79.82292));
}

#[test]
fn a_price_below_the_range_without_a_met_condition_is_not_an_exit() {
    let fills = buys_down_to(0);
    let bars = [
        bar(0, dec!(70.21), dec!(70.21), dec!(55.90), dec!(55.95)),
        bar(1, dec!(55.95), dec!(56.00), dec!(53.60), dec!(53.65)),
    ];
    let outcome = account(&replay(roomy(plain())), &fills, &bars).expect("a valid replay");
    assert_eq!(outcome.ended, None);
    assert_eq!(outcome.run.state().empty_level(), 0);
    assert_eq!(outcome.run.position(), dec!(2.2));
}

#[test]
fn the_extremes_of_a_bar_with_fills_are_not_valued_with_the_position_after_them() {
    let fills = [
        fill(5_000, 19, Side::Buy),
        fill(10_000, 18, Side::Buy),
        fill(15_000, 17, Side::Buy),
    ];
    let bars = [
        bar(0, dec!(70.21), dec!(70.60), dec!(67.80), dec!(67.85)),
        still(1, dec!(67.85)),
    ];
    let outcome =
        account(&replay(tight(reference::theta())), &fills, &bars).expect("a valid replay");
    assert_eq!(outcome.ended, None);
    assert_eq!(outcome.fills_applied, 3);
    assert_eq!(outcome.profit_last(), dec!(-0.2475));
}

#[test]
fn the_extremes_of_a_bar_without_fills_are_followed_in_the_order_of_its_direction() {
    let stops = Theta {
        stop_top: Some(dec!(70.50)),
        stop_bottom: Some(dec!(69.90)),
        ..plain()
    };
    let rising = [bar(0, dec!(70.21), dec!(70.55), dec!(69.85), dec!(70.30))];
    let falling = [bar(0, dec!(70.21), dec!(70.55), dec!(69.85), dec!(70.10))];
    let up = account(&replay(roomy(stops.clone())), &[], &rising).expect("a valid replay");
    let down = account(&replay(roomy(stops)), &[], &falling).expect("a valid replay");
    assert_eq!(up.exit().map(|exit| exit.reason), Some(Reason::Price));
    assert_eq!(up.run.prices().last, dec!(69.85));
    assert_eq!(down.exit().map(|exit| exit.reason), Some(Reason::Price));
    assert_eq!(down.run.prices().last, dec!(70.55));
    assert_eq!(up.exit_ms(), Some(T0 + BAR_MS));
    assert_eq!(down.exit_ms(), Some(T0 + BAR_MS));
}

#[test]
fn an_exit_at_the_open_of_a_bar_carries_the_time_of_the_bar() {
    let theta = Theta {
        stop_bottom: Some(dec!(69.00)),
        ..plain()
    };
    let fills = [fill(61_000, 19, Side::Buy)];
    let bars = [
        still(0, dec!(70.21)),
        bar(1, dec!(68.90), dec!(68.95), dec!(68.80), dec!(68.85)),
    ];
    let outcome = account(&replay(roomy(theta)), &fills, &bars).expect("a valid replay");
    assert_eq!(
        outcome.ended,
        ended(Reason::Price, PositionAtEnd::Closed, T0 + BAR_MS)
    );
    assert_eq!(outcome.fills_applied, 0);
    assert_eq!(outcome.fills_discarded, 1);
    assert!(outcome.ledger().is_empty());
}

#[test]
fn an_exit_at_a_fill_carries_the_time_of_the_fill_and_keeps_that_fill() {
    let theta = Theta {
        stop_bottom: Some(dec!(69.30)),
        ..plain()
    };
    let fills = [fill(30_000, 19, Side::Buy), fill(40_000, 18, Side::Buy)];
    let bars = [bar(0, dec!(70.21), dec!(70.21), dec!(68.50), dec!(68.55))];
    let outcome = account(&replay(roomy(theta)), &fills, &bars).expect("a valid replay");
    assert_eq!(
        outcome.ended,
        ended(Reason::Price, PositionAtEnd::Closed, T0 + 30_000)
    );
    assert_eq!(outcome.fills_applied, 1);
    assert_eq!(outcome.fills_discarded, 1);
    assert_eq!(outcome.run.position(), dec!(0.11));
    assert_eq!(outcome.ledger().trades().count(), 1);
}

#[test]
fn an_exit_at_the_close_of_a_bar_carries_the_end_of_the_bar() {
    let theta = Theta {
        stop_bottom: Some(dec!(69.00)),
        ..plain()
    };
    let fills = [fill(30_000, 19, Side::Buy), fill(90_000, 18, Side::Buy)];
    let bars = [
        bar(0, dec!(70.21), dec!(70.21), dec!(68.90), dec!(68.95)),
        still(1, dec!(68.60)),
    ];
    let outcome = account(&replay(roomy(theta)), &fills, &bars).expect("a valid replay");
    assert_eq!(
        outcome.ended,
        ended(Reason::Price, PositionAtEnd::Closed, T0 + BAR_MS)
    );
    assert_eq!(outcome.fills_applied, 1);
    assert_eq!(outcome.fills_discarded, 1);
}

#[test]
fn the_stop_loss_ends_the_run_and_keeps_the_position_when_the_toggle_is_off() {
    let theta = Theta {
        stop_loss: Some(Target::Amount(dec!(-5))),
        close_tp_sl: false,
        ..plain()
    };
    let fills = buys_down_to(8);
    let bars = [bar(0, dec!(70.21), dec!(70.21), dec!(61.55), dec!(61.59))];
    let outcome = account(&replay(roomy(theta)), &fills, &bars).expect("a valid replay");
    assert_eq!(
        outcome.ended,
        ended(Reason::StopLoss, PositionAtEnd::Kept, T0 + 12_000)
    );
    assert_eq!(outcome.fills_applied, 12);
    assert_eq!(outcome.profit_last(), dec!(-5.082));
}

#[test]
fn fees_follow_the_maker_and_taker_rates_of_the_binding() {
    let binding = Binding {
        maker_fee: dec!(0.0002),
        taker_fee: dec!(0.0004),
        ..reference::app_binding()
    };
    let instance = admitted(binding, plain(), dec!(20));
    let taker = Fill {
        maker: false,
        ..fill(20_000, 20, Side::Buy)
    };
    let fills = [fill(10_000, 21, Side::Sell), taker];
    let bars = [bar(0, dec!(70.21), dec!(70.75), dec!(69.95), dec!(70.00))];
    let outcome = account(&replay(instance), &fills, &bars).expect("a valid replay");
    let fees: Vec<Decimal> = outcome.ledger().trades().map(|trade| trade.fee).collect();
    assert_eq!(fees, vec![dec!(0.0015554), dec!(0.00308)]);
    assert_eq!(outcome.ledger().fees(), dec!(0.0046354));
    assert_eq!(outcome.pairing().matched(), dec!(0.0723646));
    assert_eq!(outcome.profit_last(), dec!(0.0723646));
    assert_eq!(outcome.wallet_value(), dec!(100.0723646));
}

#[test]
fn fills_between_bars_and_after_the_last_bar_are_accounted() {
    let fills = [
        fill(2 * BAR_MS + 5_000, 21, Side::Sell),
        fill(7 * BAR_MS, 20, Side::Buy),
    ];
    let bars = [still(0, dec!(70.21)), still(5, dec!(70.69))];
    let outcome = account(&replay(tight(plain())), &fills, &bars).expect("a valid replay");
    assert_eq!(outcome.ended, None);
    assert_eq!(outcome.fills_applied, 2);
    assert_eq!(outcome.pairing().pairs.len(), 1);
    assert_eq!(outcome.run.prices().last, dec!(70.00));
}

#[test]
fn a_fill_on_the_wrong_side_of_the_empty_level_is_refused() {
    let fills = [fill(1_000, 19, Side::Sell)];
    let bars = [still(0, dec!(70.21))];
    assert_eq!(
        account(&replay(tight(plain())), &fills, &bars),
        Err(ReplayError::FillSideMismatch { fill: 0 })
    );
}

#[test]
fn a_fill_the_machine_cannot_take_is_refused_with_its_index() {
    let bars = [still(0, dec!(70.21))];
    let skipping = [fill(1_000, 19, Side::Buy), fill(2_000, 17, Side::Buy)];
    let unknown = [fill(1_000, 41, Side::Sell)];
    let worthless = [Fill {
        price: dec!(-69.30),
        ..fill(1_000, 19, Side::Buy)
    }];
    assert_eq!(
        account(&replay(tight(plain())), &skipping, &bars),
        Err(ReplayError::Step {
            fill: 1,
            error: StepError::FillNotAdjacent,
        })
    );
    assert_eq!(
        account(&replay(tight(plain())), &unknown, &bars),
        Err(ReplayError::Step {
            fill: 0,
            error: StepError::UnknownLevel,
        })
    );
    assert_eq!(
        account(&replay(tight(plain())), &worthless, &bars),
        Err(ReplayError::Step {
            fill: 0,
            error: StepError::PriceNotPositive,
        })
    );
}

#[test]
fn a_fill_of_less_or_more_than_the_order_quantity_is_refused() {
    let bars = [still(0, dec!(70.21))];
    for quantity in [dec!(0.05), dec!(0.12)] {
        let fills = [
            fill(1_000, 19, Side::Buy),
            Fill {
                quantity,
                ..fill(2_000, 18, Side::Buy)
            },
        ];
        assert_eq!(
            account(&replay(tight(plain())), &fills, &bars),
            Err(ReplayError::FillNotComplete { fill: 1 })
        );
    }
}

#[test]
fn malformed_inputs_are_refused() {
    let good = [still(0, dec!(70.21)), still(1, dec!(70.21))];
    let unordered_fills = [fill(2_000, 19, Side::Buy), fill(1_000, 18, Side::Buy)];
    let early_fill = [Fill {
        time_ms: T0 - 1,
        ..fill(0, 19, Side::Buy)
    }];
    let overlapping = [
        still(0, dec!(70.21)),
        Bar {
            time_ms: T0 + BAR_MS - 1,
            ..still(1, dec!(70.21))
        },
    ];
    let inverted = [bar(0, dec!(70.21), dec!(70.10), dec!(70.00), dec!(70.05))];
    let worthless = [bar(0, dec!(70.21), dec!(70.30), dec!(0), dec!(70.05))];
    let plain_replay = replay(tight(plain()));
    assert_eq!(
        account(&plain_replay, &unordered_fills, &good),
        Err(ReplayError::FillsNotAscending { fill: 1 })
    );
    assert_eq!(
        account(&plain_replay, &early_fill, &good),
        Err(ReplayError::FillBeforeStart { fill: 0 })
    );
    assert_eq!(
        account(&plain_replay, &[], &overlapping),
        Err(ReplayError::BarsNotAscending { bar: 1 })
    );
    assert_eq!(
        account(&plain_replay, &[], &inverted),
        Err(ReplayError::BarInconsistent { bar: 0 })
    );
    assert_eq!(
        account(&plain_replay, &[], &worthless),
        Err(ReplayError::BarInconsistent { bar: 0 })
    );
    let no_interval = Replay {
        bar_ms: 0,
        ..plain_replay
    };
    assert_eq!(
        account(&no_interval, &[], &good),
        Err(ReplayError::BarIntervalZero)
    );
}
