mod common;

use aegent_mm_core::params::{Instance, Resources, Target, Theta, Trigger};
use aegent_mm_core::reference;
use aegent_mm_core::run::{Applied, Observation, Run, Wallet};
use aegent_mm_core::state::{Exit, Phase, PositionAtEnd, Reason};
use aegent_mm_core::step::StepError;
use aegent_mm_core::stop::{ClosingAction, Liveness, Signal, StopInput, conditions, evaluate};
use aegent_mm_core::value::run_wallet_value;
use aegent_mm_core::{Event, EventKind, Prices, Side};
use common::{
    admitted, fill, plain_theta, price, program, roomy_resources, run_only, running, started_run,
    trade, wallet_balance,
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

const START_LEVEL: u32 = 20;

fn roomy(theta: Theta) -> Instance {
    admitted(theta, roomy_resources())
}

fn tight(theta: Theta) -> Instance {
    admitted(theta, reference::resources())
}

fn begin(instance: Instance) -> Run {
    started_run(instance, reference::start_price())
}

fn at_price(run: &mut Run, time_ms: u64, level_price: Decimal) -> Applied {
    run.apply(&price(time_ms, level_price), &run_only())
        .expect("a price event is always accepted")
}

fn filled(run: &mut Run, time_ms: u64, level: u32) -> Applied {
    let event = trade(
        time_ms,
        level,
        run.instance().quantity(),
        run.instance().level_price(level),
    );
    run.apply(&event, &run_only())
        .expect("admissible complete fill")
}

fn walk(run: &mut Run, levels: &[u32]) {
    for (index, level) in levels.iter().enumerate() {
        let time_ms = 10 * (index as u64 + 1);
        let level_price = run.instance().level_price(*level);
        assert_eq!(at_price(run, time_ms, level_price).exit, None);
        assert_eq!(filled(run, time_ms + 1, *level).exit, None);
    }
}

fn down(levels: u32) -> Vec<u32> {
    (1..=levels).map(|moved| START_LEVEL - moved).collect()
}

fn up(levels: u32) -> Vec<u32> {
    (1..=levels).map(|moved| START_LEVEL + moved).collect()
}

fn exit(reason: Reason, position: PositionAtEnd) -> Option<Exit> {
    Some(Exit { reason, position })
}

fn signalled(run: &mut Run, kind: EventKind) -> Applied {
    run.apply(&Event::at(1, kind), &run_only())
        .expect("accepted")
}

#[test]
fn the_guard_ends_the_run_when_the_wallet_value_falls_below_the_floor() {
    let mut run = begin(roomy(plain_theta()));
    walk(&mut run, &down(20));
    assert_eq!(run.position(), dec!(2.2));
    assert_eq!(at_price(&mut run, 1_000, dec!(53.60)).exit, None);
    assert_eq!(run.wallet_value(&run_only().wallet), dec!(80.042832));
    let applied = at_price(&mut run, 1_001, dec!(53.50));
    assert_eq!(run.wallet_value(&run_only().wallet), dec!(79.82292));
    assert_eq!(applied.exit, exit(Reason::Guard, PositionAtEnd::Closed));
    assert_eq!(
        run.state().phase(),
        Phase::Closing(Exit {
            reason: Reason::Guard,
            position: PositionAtEnd::Closed,
        })
    );
    assert_eq!(
        run.closing_actions(),
        vec![
            ClosingAction::CancelEveryRestingOrder,
            ClosingAction::ReduceOnlyMarketOrder {
                side: Side::Sell,
                quantity: dec!(2.2),
            },
            ClosingAction::VerifyNoRestingOrders,
            ClosingAction::VerifyNoPosition,
        ]
    );
}

#[test]
fn the_guard_closes_the_position_whatever_the_toggles_say() {
    let theta = Theta {
        close_tp_sl: false,
        close_risk: false,
        ..plain_theta()
    };
    let mut run = begin(roomy(theta));
    walk(&mut run, &up(20));
    assert_eq!(run.position(), dec!(-2.2));
    let applied = at_price(&mut run, 1_000, dec!(86.50));
    assert_eq!(applied.exit, exit(Reason::Guard, PositionAtEnd::Closed));
    assert_eq!(
        run.closing_actions()[1],
        ClosingAction::ReduceOnlyMarketOrder {
            side: Side::Buy,
            quantity: dec!(2.2),
        }
    );
}

#[test]
fn a_wallet_value_equal_to_the_floor_does_not_trip_the_guard() {
    let mut run = begin(roomy(plain_theta()));
    let reported = |balance: Decimal| Observation {
        wallet: Wallet::Reported {
            balance,
            entry_price: Decimal::ZERO,
        },
        stream_lost_since_ms: None,
    };
    let tick = Event::at(1, EventKind::Tick);
    assert_eq!(
        run.apply(&tick, &reported(dec!(80))).map(|a| a.exit),
        Ok(None)
    );
    assert_eq!(
        run.apply(&tick, &reported(dec!(79.99))).map(|a| a.exit),
        Ok(exit(Reason::Guard, PositionAtEnd::Closed))
    );
    assert_eq!(
        run.closing_actions(),
        vec![
            ClosingAction::CancelEveryRestingOrder,
            ClosingAction::VerifyNoRestingOrders,
            ClosingAction::VerifyNoPosition,
        ]
    );
}

#[test]
fn a_backstop_trade_ends_the_run_with_the_position_closed_by_the_exchange() {
    let mut run = begin(roomy(plain_theta()));
    walk(&mut run, &down(3));
    let applied = signalled(&mut run, EventKind::Backstop);
    assert_eq!(
        applied.exit,
        exit(Reason::Backstop, PositionAtEnd::ClosedByExchange)
    );
    assert_eq!(
        run.closing_actions(),
        vec![
            ClosingAction::CancelEveryRestingOrder,
            ClosingAction::VerifyNoRestingOrders,
            ClosingAction::VerifyNoPosition,
        ]
    );
}

#[test]
fn a_reject_ends_the_run_by_failure() {
    let mut run = begin(roomy(plain_theta()));
    walk(&mut run, &up(2));
    let applied = signalled(&mut run, EventKind::Reject { level: 30 });
    assert_eq!(applied.exit, exit(Reason::Failure, PositionAtEnd::Closed));
    assert_eq!(
        run.closing_actions(),
        vec![
            ClosingAction::CancelEveryRestingOrder,
            ClosingAction::ReduceOnlyMarketOrder {
                side: Side::Buy,
                quantity: dec!(0.22),
            },
            ClosingAction::VerifyNoRestingOrders,
            ClosingAction::VerifyNoPosition,
        ]
    );
}

#[test]
fn a_reject_during_init_ends_the_run_by_failure() {
    let mut run = Run::start(
        roomy(plain_theta()),
        program(),
        Prices::flat(reference::start_price()),
        0,
        wallet_balance(),
    )
    .expect("preconditions hold");
    assert_eq!(run.state().phase(), Phase::Init);
    let applied = signalled(&mut run, EventKind::Reject { level: 4 });
    assert_eq!(applied.exit, exit(Reason::Failure, PositionAtEnd::Closed));
    assert!(matches!(run.state().phase(), Phase::Closing(_)));
}

#[test]
fn a_failure_keeps_the_position_when_close_risk_is_off() {
    let theta = Theta {
        close_risk: false,
        ..plain_theta()
    };
    let mut run = begin(roomy(theta));
    walk(&mut run, &down(2));
    let applied = signalled(&mut run, EventKind::Reject { level: 30 });
    assert_eq!(applied.exit, exit(Reason::Failure, PositionAtEnd::Kept));
    assert_eq!(
        run.closing_actions(),
        vec![
            ClosingAction::CancelEveryRestingOrder,
            ClosingAction::VerifyNoRestingOrders,
        ]
    );
}

#[test]
fn market_data_silent_for_longer_than_tolerated_ends_the_run_by_failure() {
    let mut run = begin(roomy(plain_theta()));
    let tick = |time_ms: u64| Event::at(time_ms, EventKind::Tick);
    assert_eq!(
        run.apply(&tick(5_000), &run_only()).map(|a| a.exit),
        Ok(None)
    );
    assert_eq!(at_price(&mut run, 6_000, dec!(70.30)).exit, None);
    assert_eq!(run.last_price_ms(), 6_000);
    assert_eq!(
        run.apply(&tick(11_000), &run_only()).map(|a| a.exit),
        Ok(None)
    );
    assert_eq!(
        run.apply(&tick(11_001), &run_only()).map(|a| a.exit),
        Ok(exit(Reason::Failure, PositionAtEnd::Closed))
    );
}

#[test]
fn a_user_data_stream_lost_for_longer_than_tolerated_ends_the_run_by_failure() {
    let mut run = begin(roomy(plain_theta()));
    let lost = Observation {
        stream_lost_since_ms: Some(1_000),
        ..run_only()
    };
    let tick = |time_ms: u64| Event::at(time_ms, EventKind::Tick);
    assert_eq!(
        run.apply(&price(10_999, dec!(70.30)), &lost)
            .map(|a| a.exit),
        Ok(None)
    );
    assert_eq!(run.apply(&tick(11_000), &lost).map(|a| a.exit), Ok(None));
    assert_eq!(
        run.apply(&tick(11_001), &run_only()).map(|a| a.exit),
        Ok(None)
    );
    assert_eq!(
        run.apply(&tick(11_001), &lost).map(|a| a.exit),
        Ok(exit(Reason::Failure, PositionAtEnd::Closed))
    );
}

#[test]
fn a_risk_ratio_below_one_ends_the_run_after_eighteen_levels_down() {
    let mut run = begin(tight(plain_theta()));
    walk(&mut run, &down(17));
    let applied = at_price(&mut run, 1_000, dec!(57.40));
    assert_eq!(applied.exit, exit(Reason::Risk, PositionAtEnd::Closed));
    assert_eq!(
        run.closing_actions(),
        vec![
            ClosingAction::CancelEveryRestingOrder,
            ClosingAction::ReduceOnlyMarketOrder {
                side: Side::Sell,
                quantity: dec!(1.87),
            },
            ClosingAction::VerifyNoRestingOrders,
            ClosingAction::VerifyNoPosition,
        ]
    );
}

#[test]
fn a_risk_ratio_below_one_ends_the_run_after_eighteen_levels_up() {
    let mut run = begin(tight(plain_theta()));
    walk(&mut run, &up(17));
    let applied = at_price(&mut run, 1_000, dec!(82.60));
    assert_eq!(applied.exit, exit(Reason::Risk, PositionAtEnd::Closed));
    assert_eq!(
        run.closing_actions()[1],
        ClosingAction::ReduceOnlyMarketOrder {
            side: Side::Buy,
            quantity: dec!(1.87),
        }
    );
}

#[test]
fn a_risk_exit_keeps_the_position_when_close_risk_is_off() {
    let theta = Theta {
        close_risk: false,
        ..plain_theta()
    };
    let mut run = begin(tight(theta));
    walk(&mut run, &down(17));
    let applied = at_price(&mut run, 1_000, dec!(57.40));
    assert_eq!(applied.exit, exit(Reason::Risk, PositionAtEnd::Kept));
    assert_eq!(
        run.closing_actions(),
        vec![
            ClosingAction::CancelEveryRestingOrder,
            ClosingAction::VerifyNoRestingOrders,
        ]
    );
}

#[test]
fn the_stop_loss_ends_the_run_when_the_trigger_profit_reaches_it() {
    for (target, reached) in [
        (Target::Amount(dec!(-5)), true),
        (Target::Roi(dec!(-0.05)), true),
        (Target::Amount(dec!(-5.082)), true),
        (Target::Amount(dec!(-5.083)), false),
    ] {
        let theta = Theta {
            stop_loss: Some(target),
            ..plain_theta()
        };
        let mut run = begin(roomy(theta));
        walk(&mut run, &down(11));
        let applied = at_price(&mut run, 1_000, dec!(61.60));
        let expected = if reached {
            exit(Reason::StopLoss, PositionAtEnd::Closed)
        } else {
            None
        };
        assert_eq!(applied.exit, expected);
    }
}

#[test]
fn the_stop_loss_closes_the_position_only_when_close_tp_sl_is_on() {
    let theta = Theta {
        stop_loss: Some(Target::Amount(dec!(-5))),
        close_tp_sl: false,
        ..plain_theta()
    };
    let mut run = begin(roomy(theta.clone()));
    walk(&mut run, &down(11));
    let applied = at_price(&mut run, 1_000, dec!(61.60));
    assert_eq!(applied.exit, exit(Reason::StopLoss, PositionAtEnd::Kept));
    assert_eq!(
        run.closing_actions(),
        vec![
            ClosingAction::CancelEveryRestingOrder,
            ClosingAction::VerifyNoRestingOrders,
        ]
    );
    let mut closing = begin(roomy(Theta {
        close_tp_sl: true,
        ..theta
    }));
    walk(&mut closing, &down(11));
    at_price(&mut closing, 1_000, dec!(61.60));
    assert_eq!(
        closing.closing_actions()[1],
        ClosingAction::ReduceOnlyMarketOrder {
            side: Side::Sell,
            quantity: dec!(1.21),
        }
    );
}

#[test]
fn the_stop_loss_follows_the_configured_trigger_price() {
    let theta = Theta {
        stop_loss: Some(Target::Amount(dec!(-5))),
        trigger: Trigger::Mark,
        ..plain_theta()
    };
    let mut run = begin(roomy(theta));
    walk(&mut run, &down(11));
    let last_only = Prices {
        last: dec!(61.60),
        mark: dec!(62.30),
        bid: dec!(62.30),
        ask: dec!(62.30),
    };
    let mark_too = Prices {
        last: dec!(62.30),
        mark: dec!(61.60),
        bid: dec!(62.30),
        ask: dec!(62.30),
    };
    let applied = run
        .apply(&Event::at(1_000, EventKind::Price(last_only)), &run_only())
        .expect("accepted");
    assert_eq!(applied.exit, None);
    let applied = run
        .apply(&Event::at(1_001, EventKind::Price(mark_too)), &run_only())
        .expect("accepted");
    assert_eq!(applied.exit, exit(Reason::StopLoss, PositionAtEnd::Closed));
}

#[test]
fn a_price_stop_ends_the_run_when_the_trigger_price_reaches_it() {
    let theta = Theta {
        stop_top: Some(dec!(75.00)),
        stop_bottom: Some(dec!(65.00)),
        ..plain_theta()
    };
    for (level_price, reached) in [
        (dec!(74.99), false),
        (dec!(75.00), true),
        (dec!(75.01), true),
        (dec!(65.01), false),
        (dec!(65.00), true),
        (dec!(64.99), true),
    ] {
        let mut run = begin(roomy(theta.clone()));
        let applied = at_price(&mut run, 1, level_price);
        let expected = if reached {
            exit(Reason::Price, PositionAtEnd::Closed)
        } else {
            None
        };
        assert_eq!(applied.exit, expected);
    }
}

#[test]
fn a_price_stop_follows_the_configured_trigger_price_and_the_tp_sl_toggle() {
    let theta = Theta {
        stop_top: Some(dec!(75.00)),
        trigger: Trigger::Mark,
        close_tp_sl: false,
        ..plain_theta()
    };
    let mut run = begin(roomy(theta));
    walk(&mut run, &up(1));
    let last_above = Prices {
        last: dec!(75.50),
        mark: dec!(74.00),
        bid: dec!(71.00),
        ask: dec!(71.00),
    };
    let mark_above = Prices {
        last: dec!(71.00),
        mark: dec!(75.00),
        bid: dec!(71.00),
        ask: dec!(71.00),
    };
    let applied = run
        .apply(&Event::at(100, EventKind::Price(last_above)), &run_only())
        .expect("accepted");
    assert_eq!(applied.exit, None);
    let applied = run
        .apply(&Event::at(101, EventKind::Price(mark_above)), &run_only())
        .expect("accepted");
    assert_eq!(applied.exit, exit(Reason::Price, PositionAtEnd::Kept));
    assert_eq!(
        run.closing_actions(),
        vec![
            ClosingAction::CancelEveryRestingOrder,
            ClosingAction::VerifyNoRestingOrders,
        ]
    );
}

#[test]
fn the_take_profit_of_the_reference_instance_is_reached_within_three_round_trips() {
    let mut run = begin(tight(reference::theta()));
    for round in 0..2 {
        let time_ms = 100 * (round + 1);
        assert_eq!(at_price(&mut run, time_ms, dec!(70.70)).exit, None);
        assert_eq!(filled(&mut run, time_ms + 1, 21).exit, None);
        assert_eq!(at_price(&mut run, time_ms + 2, dec!(70.00)).exit, None);
        assert_eq!(filled(&mut run, time_ms + 3, 20).exit, None);
    }
    assert_eq!(at_price(&mut run, 300, dec!(70.70)).exit, None);
    assert_eq!(filled(&mut run, 301, 21).exit, None);
    let applied = at_price(&mut run, 302, dec!(70.00));
    assert_eq!(
        applied.exit,
        exit(Reason::TakeProfit, PositionAtEnd::Closed)
    );
    assert_eq!(
        run.closing_actions(),
        vec![
            ClosingAction::CancelEveryRestingOrder,
            ClosingAction::ReduceOnlyMarketOrder {
                side: Side::Buy,
                quantity: dec!(0.11),
            },
            ClosingAction::VerifyNoRestingOrders,
            ClosingAction::VerifyNoPosition,
        ]
    );
}

#[test]
fn the_take_profit_is_reached_at_equality_and_keeps_the_position_when_the_toggle_is_off() {
    let theta = Theta {
        take_profit: Some(Target::Amount(dec!(0.077))),
        close_tp_sl: false,
        ..reference::theta()
    };
    let mut run = begin(tight(theta));
    assert_eq!(at_price(&mut run, 1, dec!(70.70)).exit, None);
    assert_eq!(filled(&mut run, 2, 21).exit, None);
    assert_eq!(at_price(&mut run, 3, dec!(70.01)).exit, None);
    let applied = at_price(&mut run, 4, dec!(70.00));
    assert_eq!(applied.exit, exit(Reason::TakeProfit, PositionAtEnd::Kept));
}

#[test]
fn a_stop_command_ends_the_run_and_closes_the_position_unless_told_to_keep_it() {
    let mut closed = begin(roomy(plain_theta()));
    walk(&mut closed, &down(1));
    let applied = signalled(&mut closed, EventKind::Command { keep: false });
    assert_eq!(applied.exit, exit(Reason::Command, PositionAtEnd::Closed));
    assert_eq!(
        closed.closing_actions(),
        vec![
            ClosingAction::CancelEveryRestingOrder,
            ClosingAction::ReduceOnlyMarketOrder {
                side: Side::Sell,
                quantity: dec!(0.11),
            },
            ClosingAction::VerifyNoRestingOrders,
            ClosingAction::VerifyNoPosition,
        ]
    );
    let mut kept = begin(roomy(plain_theta()));
    walk(&mut kept, &down(1));
    let applied = signalled(&mut kept, EventKind::Command { keep: true });
    assert_eq!(applied.exit, exit(Reason::Command, PositionAtEnd::Kept));
    assert_eq!(
        kept.closing_actions(),
        vec![
            ClosingAction::CancelEveryRestingOrder,
            ClosingAction::VerifyNoRestingOrders,
        ]
    );
}

#[test]
fn the_first_condition_in_the_table_gives_the_reason() {
    let theta = Theta {
        stop_loss: Some(Target::Amount(dec!(-11))),
        stop_bottom: Some(dec!(57.50)),
        ..plain_theta()
    };
    let instance = tight(theta);
    let program = program();
    let mut state = running(&instance, START_LEVEL);
    for moved in 1..=17 {
        fill(&instance, &mut state, u64::from(moved), START_LEVEL - moved);
    }
    let prices = Prices::flat(dec!(57.40));
    let fresh = Liveness {
        now_ms: 100,
        last_price_ms: 100,
        stream_lost_since_ms: None,
    };
    let stale = Liveness {
        now_ms: 5_101,
        last_price_ms: 100,
        stream_lost_since_ms: None,
    };
    let sound = run_wallet_value(&instance, &state, wallet_balance(), &prices);
    assert!(sound > program.floor());
    let input = |wallet_value: Decimal, signal: Option<Signal>, liveness: Liveness| StopInput {
        instance: &instance,
        program: &program,
        state: &state,
        prices: &prices,
        wallet_value,
        signal,
        liveness,
    };
    let held = [Reason::Risk, Reason::StopLoss, Reason::Price];
    assert_eq!(conditions(&input(sound, None, fresh)), held);
    assert_eq!(
        evaluate(&input(sound, None, fresh)),
        exit(Reason::Risk, PositionAtEnd::Closed)
    );
    assert_eq!(
        conditions(&input(sound, Some(Signal::Command { keep: true }), fresh)),
        [
            Reason::Risk,
            Reason::StopLoss,
            Reason::Price,
            Reason::Command
        ]
    );
    assert_eq!(
        evaluate(&input(sound, Some(Signal::Command { keep: true }), fresh)),
        exit(Reason::Risk, PositionAtEnd::Closed)
    );
    assert_eq!(
        conditions(&input(sound, Some(Signal::Reject), fresh)),
        [
            Reason::Failure,
            Reason::Risk,
            Reason::StopLoss,
            Reason::Price
        ]
    );
    assert_eq!(
        conditions(&input(sound, None, stale)),
        [
            Reason::Failure,
            Reason::Risk,
            Reason::StopLoss,
            Reason::Price
        ]
    );
    assert_eq!(
        conditions(&input(sound, Some(Signal::Backstop), stale)),
        [
            Reason::Backstop,
            Reason::Failure,
            Reason::Risk,
            Reason::StopLoss,
            Reason::Price
        ]
    );
    assert_eq!(
        evaluate(&input(sound, Some(Signal::Backstop), stale)),
        exit(Reason::Backstop, PositionAtEnd::ClosedByExchange)
    );
    assert_eq!(
        conditions(&input(dec!(79.99), Some(Signal::Backstop), stale)),
        [
            Reason::Guard,
            Reason::Backstop,
            Reason::Failure,
            Reason::Risk,
            Reason::StopLoss,
            Reason::Price
        ]
    );
    assert_eq!(
        evaluate(&input(dec!(79.99), Some(Signal::Backstop), stale)),
        exit(Reason::Guard, PositionAtEnd::Closed)
    );
}

#[test]
fn a_price_stop_comes_before_the_take_profit() {
    let theta = Theta {
        stop_top: Some(dec!(72.00)),
        ..reference::theta()
    };
    let instance = tight(theta);
    let program = program();
    let mut state = running(&instance, START_LEVEL);
    fill(&instance, &mut state, 1, 19);
    fill(&instance, &mut state, 2, 18);
    let prices = Prices::flat(dec!(72.00));
    let input = StopInput {
        instance: &instance,
        program: &program,
        state: &state,
        prices: &prices,
        wallet_value: dec!(100),
        signal: None,
        liveness: Liveness {
            now_ms: 3,
            last_price_ms: 3,
            stream_lost_since_ms: None,
        },
    };
    assert_eq!(conditions(&input), [Reason::Price, Reason::TakeProfit]);
    assert_eq!(evaluate(&input), exit(Reason::Price, PositionAtEnd::Closed));
}

#[test]
fn one_price_event_meeting_three_conditions_ends_the_run_by_the_first() {
    let theta = Theta {
        stop_loss: Some(Target::Amount(dec!(-11))),
        stop_bottom: Some(dec!(57.50)),
        ..plain_theta()
    };
    let mut run = begin(tight(theta));
    walk(&mut run, &down(17));
    let applied = at_price(&mut run, 1_000, dec!(57.40));
    assert_eq!(applied.exit, exit(Reason::Risk, PositionAtEnd::Closed));
}

#[test]
fn the_guard_takes_the_reason_from_a_reject_that_arrives_at_the_same_time() {
    let mut run = begin(roomy(plain_theta()));
    let below_floor = Observation {
        wallet: Wallet::Reported {
            balance: dec!(79),
            entry_price: Decimal::ZERO,
        },
        stream_lost_since_ms: None,
    };
    let applied = run
        .apply(&Event::at(1, EventKind::Reject { level: 7 }), &below_floor)
        .expect("accepted");
    assert_eq!(applied.exit, exit(Reason::Guard, PositionAtEnd::Closed));
    assert_eq!(
        run.state().exit(),
        exit(Reason::Guard, PositionAtEnd::Closed)
    );
}

#[test]
fn an_exit_met_by_a_fill_sends_no_opposite_order() {
    let theta = Theta {
        stop_loss: Some(Target::Amount(dec!(-0.05))),
        ..plain_theta()
    };
    let mut run = begin(roomy(theta));
    assert_eq!(at_price(&mut run, 1, dec!(69.30)).exit, None);
    let costly = Event::at(
        2,
        EventKind::Trade {
            level: 19,
            quantity: dec!(0.11),
            price: dec!(69.30),
            fee: dec!(0.06),
            maker: false,
        },
    );
    let applied = run.apply(&costly, &run_only()).expect("admissible");
    assert!(applied.fill.is_some());
    assert_eq!(applied.send, None);
    assert_eq!(applied.exit, exit(Reason::StopLoss, PositionAtEnd::Closed));
}

#[test]
fn a_fill_without_an_exit_sends_the_opposite_order() {
    let mut run = begin(roomy(plain_theta()));
    assert_eq!(at_price(&mut run, 1, dec!(69.30)).exit, None);
    let applied = filled(&mut run, 2, 19);
    assert_eq!(applied.exit, None);
    assert_eq!(
        applied.send.map(|order| (order.level, order.side)),
        Some((20, Side::Sell))
    );
    assert_eq!(run.quote().len(), 40);
}

#[test]
fn a_price_outside_the_range_is_not_an_exit() {
    let mut run = begin(roomy(plain_theta()));
    walk(&mut run, &down(20));
    for (time_ms, level_price) in [
        (1_000, dec!(55.90)),
        (1_001, dec!(55.00)),
        (1_002, dec!(54.00)),
    ] {
        assert_eq!(at_price(&mut run, time_ms, level_price).exit, None);
    }
    assert_eq!(run.state().phase(), Phase::Run);
    assert_eq!(run.state().empty_level(), 0);
    assert_eq!(run.position(), dec!(2.2));
    assert!(run.quote().iter().all(|order| order.side == Side::Sell));
    assert_eq!(run.quote().len(), 40);
}

#[test]
fn closing_records_late_trades_and_ends_once_confirmed() {
    let mut run = begin(roomy(plain_theta()));
    walk(&mut run, &down(1));
    assert_eq!(run.closing_actions(), Vec::new());
    let applied = signalled(&mut run, EventKind::Command { keep: false });
    assert!(applied.exit.is_some());
    let late = filled(&mut run, 500, 18);
    assert!(late.fill.is_some());
    assert_eq!(late.send, None);
    assert_eq!(late.exit, None);
    assert_eq!(at_price(&mut run, 501, dec!(10.00)).exit, None);
    assert_eq!(run.prices().last, dec!(10.00));
    assert_eq!(
        run.closing_actions()[1],
        ClosingAction::ReduceOnlyMarketOrder {
            side: Side::Sell,
            quantity: dec!(0.22),
        }
    );
    assert_eq!(run.confirm_closed(), Ok(()));
    assert_eq!(
        run.state().phase(),
        Phase::Ended(Exit {
            reason: Reason::Command,
            position: PositionAtEnd::Closed,
        })
    );
    assert_eq!(run.closing_actions(), Vec::new());
    assert_eq!(
        run.apply(&price(600, dec!(70.00)), &run_only()),
        Err(StepError::RunEnded)
    );
    assert_eq!(run.state().ledger().len(), 2);
}

#[test]
fn a_refused_trade_leaves_the_run_untouched() {
    let mut run = begin(roomy(plain_theta()));
    let before = run.clone();
    let refused = run.apply(&trade(1, 17, dec!(0.11), dec!(67.90)), &run_only());
    assert_eq!(refused, Err(StepError::FillNotAdjacent));
    assert_eq!(run, before);
}

#[test]
fn resources_with_leverage_five_fit_the_whole_range() {
    let instance = admitted(
        plain_theta(),
        Resources {
            margin: dec!(100),
            leverage: 5,
        },
    );
    let mut down_run = begin(instance.clone());
    walk(&mut down_run, &down(20));
    assert_eq!(down_run.state().phase(), Phase::Run);
    let mut up_run = begin(instance);
    walk(&mut up_run, &up(20));
    assert_eq!(up_run.state().phase(), Phase::Run);
}

#[test]
fn the_risk_ratio_follows_the_mark_price() {
    let mut run = begin(tight(plain_theta()));
    walk(&mut run, &down(17));
    let last_lower = Prices {
        last: dec!(57.40),
        mark: dec!(58.10),
        bid: dec!(58.10),
        ask: dec!(58.10),
    };
    let mark_lower = Prices {
        last: dec!(58.10),
        mark: dec!(57.40),
        bid: dec!(58.10),
        ask: dec!(58.10),
    };
    let applied = run
        .apply(&Event::at(1_000, EventKind::Price(last_lower)), &run_only())
        .expect("accepted");
    assert_eq!(applied.exit, None);
    let applied = run
        .apply(&Event::at(1_001, EventKind::Price(mark_lower)), &run_only())
        .expect("accepted");
    assert_eq!(applied.exit, exit(Reason::Risk, PositionAtEnd::Closed));
}

#[test]
fn a_condition_met_during_closing_does_not_change_the_exit() {
    let theta = Theta {
        stop_bottom: Some(dec!(65.00)),
        ..plain_theta()
    };
    let mut run = begin(roomy(theta));
    walk(&mut run, &down(1));
    let applied = signalled(&mut run, EventKind::Command { keep: true });
    assert_eq!(applied.exit, exit(Reason::Command, PositionAtEnd::Kept));
    let later = at_price(&mut run, 500, dec!(60.00));
    assert_eq!(later.exit, None);
    let below_floor = Observation {
        wallet: Wallet::Reported {
            balance: dec!(10),
            entry_price: dec!(69.30),
        },
        stream_lost_since_ms: Some(0),
    };
    let tick = run
        .apply(&Event::at(900_000, EventKind::Tick), &below_floor)
        .expect("accepted");
    assert_eq!(tick.exit, None);
    assert_eq!(
        run.state().exit(),
        exit(Reason::Command, PositionAtEnd::Kept)
    );
}
