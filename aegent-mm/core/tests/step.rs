mod common;

use aegent_mm_core::ledger::{Entry, FundingRecord, TradeRecord};
use aegent_mm_core::params::{Instance, Theta};
use aegent_mm_core::quote::{Order, quote};
use aegent_mm_core::reference;
use aegent_mm_core::state::{Exit, Phase, PhaseError, PositionAtEnd, Reason, State};
use aegent_mm_core::step::{Fill, StepError, Stepped, step};
use aegent_mm_core::{Event, EventKind, Prices, Side};
use common::{admitted, app_instance, fill, price, running, trade};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

const START_LEVEL: u32 = 20;

fn event(kind: EventKind) -> Event {
    Event::at(7, kind)
}

fn closing(instance: &Instance, kind: EventKind) -> State {
    let mut state = running(instance, START_LEVEL);
    step(instance, &mut state, &event(kind)).expect("the event is accepted");
    state
}

#[test]
fn a_partial_fill_keeps_the_remainder_resting_and_sends_nothing() {
    let instance = app_instance();
    let mut state = running(&instance, START_LEVEL);
    let stepped = step(
        &instance,
        &mut state,
        &trade(1, 19, dec!(0.04), dec!(69.30)),
    );
    assert_eq!(stepped, Ok(Stepped::default()));
    assert_eq!(state.empty_level(), 20);
    assert_eq!(state.filled(19), dec!(0.04));
    assert_eq!(state.position(instance.quantity()), dec!(0.04));
    let orders = quote(&instance, &state);
    assert_eq!(orders.len(), 40);
    assert_eq!(
        orders[19],
        Order {
            level: 19,
            side: Side::Buy,
            quantity: dec!(0.07),
            price: dec!(69.30),
        }
    );
}

#[test]
fn a_completed_buy_moves_the_empty_level_down_and_sends_a_sell_one_level_up() {
    let instance = app_instance();
    let mut state = running(&instance, START_LEVEL);
    let stepped = step(
        &instance,
        &mut state,
        &trade(1, 19, dec!(0.11), dec!(69.30)),
    );
    assert_eq!(
        stepped,
        Ok(Stepped {
            fill: Some(Fill {
                level: 19,
                side: Side::Buy,
            }),
            send: Some(Order {
                level: 20,
                side: Side::Sell,
                quantity: dec!(0.11),
                price: dec!(70.00),
            }),
        })
    );
    assert_eq!(state.empty_level(), 19);
    assert_eq!(state.start_level(), 20);
    assert_eq!(state.filled(19), Decimal::ZERO);
    assert_eq!(state.position(instance.quantity()), dec!(0.11));
}

#[test]
fn a_completed_sell_moves_the_empty_level_up_and_sends_a_buy_one_level_down() {
    let instance = app_instance();
    let mut state = running(&instance, START_LEVEL);
    let stepped = step(
        &instance,
        &mut state,
        &trade(1, 21, dec!(0.11), dec!(70.70)),
    );
    assert_eq!(
        stepped,
        Ok(Stepped {
            fill: Some(Fill {
                level: 21,
                side: Side::Sell,
            }),
            send: Some(Order {
                level: 20,
                side: Side::Buy,
                quantity: dec!(0.11),
                price: dec!(70.00),
            }),
        })
    );
    assert_eq!(state.empty_level(), 21);
    assert_eq!(state.position(instance.quantity()), dec!(-0.11));
}

#[test]
fn partial_trades_add_up_to_a_fill() {
    let instance = app_instance();
    let mut state = running(&instance, START_LEVEL);
    for (time, quantity) in [(1, dec!(0.03)), (2, dec!(0.05))] {
        let stepped = step(
            &instance,
            &mut state,
            &trade(time, 21, quantity, dec!(70.70)),
        );
        assert_eq!(stepped, Ok(Stepped::default()));
    }
    assert_eq!(state.filled(21), dec!(0.08));
    assert_eq!(state.position(instance.quantity()), dec!(-0.08));
    let stepped = step(
        &instance,
        &mut state,
        &trade(3, 21, dec!(0.03), dec!(70.70)),
    )
    .expect("the last part completes the order");
    assert_eq!(
        stepped.fill,
        Some(Fill {
            level: 21,
            side: Side::Sell,
        })
    );
    assert_eq!(stepped.send.map(|order| order.level), Some(20));
    assert_eq!(state.filled(21), Decimal::ZERO);
    assert_eq!(state.empty_level(), 21);
    assert_eq!(state.ledger().len(), 3);
}

#[test]
fn the_ledger_keeps_the_actual_price_fee_maker_flag_and_time() {
    let instance = app_instance();
    let mut state = running(&instance, START_LEVEL);
    let taker = Event::at(
        1_700_000_000_123,
        EventKind::Trade {
            level: 19,
            quantity: dec!(0.11),
            price: dec!(69.21),
            fee: dec!(0.00304524),
            maker: false,
        },
    );
    step(&instance, &mut state, &taker).expect("admissible");
    assert_eq!(
        state.ledger().entries(),
        &[Entry::Trade(TradeRecord {
            time_ms: 1_700_000_000_123,
            level: 19,
            side: Side::Buy,
            quantity: dec!(0.11),
            price: dec!(69.21),
            fee: dec!(0.00304524),
            maker: false,
        })]
    );
    assert_eq!(state.ledger().cash(), dec!(-7.6131));
    assert_eq!(state.ledger().fees(), dec!(0.00304524));
    assert_eq!(state.ledger().net_quantity(), dec!(0.11));
}

#[test]
fn funding_is_added_to_the_ledger() {
    let instance = app_instance();
    let mut state = running(&instance, START_LEVEL);
    let before = quote(&instance, &state);
    for (time, amount) in [(10, dec!(-0.0123)), (20, dec!(0.0045))] {
        let stepped = step(
            &instance,
            &mut state,
            &Event::at(time, EventKind::Funding { amount }),
        );
        assert_eq!(stepped, Ok(Stepped::default()));
    }
    assert_eq!(
        state.ledger().entries(),
        &[
            Entry::Funding(FundingRecord {
                time_ms: 10,
                amount: dec!(-0.0123),
            }),
            Entry::Funding(FundingRecord {
                time_ms: 20,
                amount: dec!(0.0045),
            }),
        ]
    );
    assert_eq!(state.ledger().funding(), dec!(-0.0078));
    assert_eq!(state.ledger().funding_payments().count(), 2);
    assert_eq!(state.ledger().trades().count(), 0);
    assert_eq!(quote(&instance, &state), before);
    assert_eq!(state.phase(), Phase::Run);
}

#[test]
fn price_and_tick_leave_the_state_unchanged() {
    let instance = app_instance();
    let mut state = running(&instance, START_LEVEL);
    fill(&instance, &mut state, 1, 19);
    let before = state.clone();
    assert_eq!(
        step(&instance, &mut state, &price(2, dec!(12.34))),
        Ok(Stepped::default())
    );
    assert_eq!(
        step(&instance, &mut state, &Event::at(3, EventKind::Tick)),
        Ok(Stepped::default())
    );
    assert_eq!(state, before);
}

#[test]
fn inadmissible_trades_are_refused_and_change_nothing() {
    let instance = app_instance();
    let mut state = running(&instance, START_LEVEL);
    step(
        &instance,
        &mut state,
        &trade(1, 19, dec!(0.04), dec!(69.30)),
    )
    .expect("admissible");
    let before = state.clone();
    let cases = [
        (
            trade(2, 41, dec!(0.11), dec!(84.70)),
            StepError::UnknownLevel,
        ),
        (
            trade(2, 20, dec!(0.11), dec!(70.00)),
            StepError::NoOrderAtLevel,
        ),
        (
            trade(2, 21, dec!(0), dec!(70.70)),
            StepError::QuantityNotPositive,
        ),
        (
            trade(2, 21, dec!(-0.01), dec!(70.70)),
            StepError::QuantityNotPositive,
        ),
        (
            trade(2, 21, dec!(0.11), dec!(0)),
            StepError::PriceNotPositive,
        ),
        (
            trade(2, 19, dec!(0.07), dec!(69.31)),
            StepError::PriceWorseThanLevel,
        ),
        (
            trade(2, 21, dec!(0.11), dec!(70.69)),
            StepError::PriceWorseThanLevel,
        ),
        (trade(2, 21, dec!(0.12), dec!(70.70)), StepError::Overfill),
        (trade(2, 19, dec!(0.08), dec!(69.30)), StepError::Overfill),
        (
            trade(2, 18, dec!(0.11), dec!(68.60)),
            StepError::FillNotAdjacent,
        ),
        (
            trade(2, 22, dec!(0.11), dec!(71.40)),
            StepError::FillNotAdjacent,
        ),
        (
            trade(2, 0, dec!(0.11), dec!(56.00)),
            StepError::FillNotAdjacent,
        ),
        (
            trade(2, 40, dec!(0.11), dec!(84.00)),
            StepError::FillNotAdjacent,
        ),
    ];
    for (event, error) in cases {
        assert_eq!(step(&instance, &mut state, &event), Err(error));
        assert_eq!(state, before);
    }
}

#[test]
fn a_partial_trade_away_from_the_empty_level_is_recorded() {
    let instance = app_instance();
    let mut state = running(&instance, START_LEVEL);
    assert_eq!(
        step(
            &instance,
            &mut state,
            &trade(1, 17, dec!(0.05), dec!(67.90))
        ),
        Ok(Stepped::default())
    );
    assert_eq!(
        step(
            &instance,
            &mut state,
            &trade(2, 23, dec!(0.02), dec!(72.10))
        ),
        Ok(Stepped::default())
    );
    assert_eq!(state.filled(17), dec!(0.05));
    assert_eq!(state.filled(23), dec!(0.02));
    assert_eq!(state.empty_level(), 20);
    assert_eq!(state.position(instance.quantity()), dec!(0.03));
}

#[test]
fn the_position_counts_filled_parts_on_both_sides_of_the_empty_level() {
    let instance = app_instance();
    let mut state = running(&instance, START_LEVEL);
    step(
        &instance,
        &mut state,
        &trade(1, 19, dec!(0.04), dec!(69.30)),
    )
    .expect("admissible");
    fill(&instance, &mut state, 2, 21);
    fill(&instance, &mut state, 3, 22);
    step(
        &instance,
        &mut state,
        &trade(4, 23, dec!(0.06), dec!(72.10)),
    )
    .expect("admissible");
    assert_eq!(state.empty_level(), 22);
    assert_eq!(state.filled(19), dec!(0.04));
    assert_eq!(state.filled(23), dec!(0.06));
    assert_eq!(
        state.position(instance.quantity()),
        dec!(0.11) * dec!(-2) + dec!(0.04) - dec!(0.06)
    );
    assert_eq!(
        state.position(instance.quantity()),
        state.ledger().net_quantity()
    );
}

#[test]
fn trades_during_init_are_processed_as_in_run() {
    let instance = app_instance();
    let mut init = State::initial(&instance, START_LEVEL).expect("inside the ladder");
    let mut run = running(&instance, START_LEVEL);
    assert_eq!(init.phase(), Phase::Init);
    for (time, level) in [(1, 19), (2, 18), (3, 19), (4, 20), (5, 21)] {
        let in_init = fill(&instance, &mut init, time, level);
        let in_run = fill(&instance, &mut run, time, level);
        assert_eq!(in_init, in_run);
        assert!(in_init.send.is_some());
    }
    assert_eq!(init.phase(), Phase::Init);
    assert_eq!(init.ledger(), run.ledger());
    assert_eq!(quote(&instance, &init), quote(&instance, &run));
}

#[test]
fn a_reject_starts_closing_with_reason_failure() {
    let instance = app_instance();
    let state = closing(&instance, EventKind::Reject { level: 12 });
    assert_eq!(
        state.phase(),
        Phase::Closing(Exit {
            reason: Reason::Failure,
            position: PositionAtEnd::Closed,
        })
    );
}

#[test]
fn a_reject_during_init_starts_closing_with_reason_failure() {
    let instance = app_instance();
    let mut state = State::initial(&instance, START_LEVEL).expect("inside the ladder");
    step(
        &instance,
        &mut state,
        &event(EventKind::Reject { level: 3 }),
    )
    .expect("accepted");
    assert_eq!(state.exit().map(|exit| exit.reason), Some(Reason::Failure));
    assert_eq!(state.acknowledge_start(), Err(PhaseError::NotInInit));
}

#[test]
fn a_reject_keeps_the_position_when_close_risk_is_off() {
    let instance = admitted(
        Theta {
            close_risk: false,
            ..reference::theta()
        },
        reference::resources(),
    );
    let state = closing(&instance, EventKind::Reject { level: 12 });
    assert_eq!(
        state.exit(),
        Some(Exit {
            reason: Reason::Failure,
            position: PositionAtEnd::Kept,
        })
    );
}

#[test]
fn a_backstop_trade_starts_closing_with_the_position_closed_by_the_exchange() {
    let instance = app_instance();
    let state = closing(&instance, EventKind::Backstop);
    assert_eq!(
        state.phase(),
        Phase::Closing(Exit {
            reason: Reason::Backstop,
            position: PositionAtEnd::ClosedByExchange,
        })
    );
}

#[test]
fn a_stop_command_starts_closing_and_keeps_the_position_only_on_request() {
    let instance = app_instance();
    let closed = closing(&instance, EventKind::Command { keep: false });
    let kept = closing(&instance, EventKind::Command { keep: true });
    assert_eq!(
        closed.exit(),
        Some(Exit {
            reason: Reason::Command,
            position: PositionAtEnd::Closed,
        })
    );
    assert_eq!(
        kept.exit(),
        Some(Exit {
            reason: Reason::Command,
            position: PositionAtEnd::Kept,
        })
    );
}

#[test]
fn a_trade_during_closing_is_recorded_and_sends_nothing() {
    let instance = app_instance();
    let mut state = closing(&instance, EventKind::Command { keep: false });
    let stepped = fill(&instance, &mut state, 9, 19);
    assert_eq!(
        stepped,
        Stepped {
            fill: Some(Fill {
                level: 19,
                side: Side::Buy,
            }),
            send: None,
        }
    );
    assert_eq!(state.empty_level(), 19);
    assert_eq!(state.position(instance.quantity()), dec!(0.11));
    assert_eq!(state.ledger().len(), 1);
    step(
        &instance,
        &mut state,
        &Event::at(10, EventKind::Funding { amount: dec!(0.01) }),
    )
    .expect("accepted");
    assert_eq!(state.ledger().len(), 2);
}

#[test]
fn the_first_exit_stays_when_later_events_arrive_during_closing() {
    let instance = app_instance();
    let mut state = closing(&instance, EventKind::Command { keep: true });
    let first = state.exit();
    for kind in [
        EventKind::Reject { level: 5 },
        EventKind::Command { keep: false },
        EventKind::Tick,
        EventKind::Price(Prices::flat(dec!(1))),
    ] {
        step(&instance, &mut state, &event(kind)).expect("accepted");
        assert_eq!(state.exit(), first);
    }
}

#[test]
fn a_backstop_trade_during_closing_marks_the_position_as_closed_by_the_exchange() {
    let instance = app_instance();
    let mut state = closing(&instance, EventKind::Command { keep: true });
    step(&instance, &mut state, &event(EventKind::Backstop)).expect("accepted");
    assert_eq!(
        state.phase(),
        Phase::Closing(Exit {
            reason: Reason::Command,
            position: PositionAtEnd::ClosedByExchange,
        })
    );
}

#[test]
fn an_ended_run_accepts_no_event() {
    let instance = app_instance();
    let mut state = closing(&instance, EventKind::Command { keep: false });
    assert_eq!(state.confirm_closed(), Ok(()));
    assert_eq!(
        state.phase(),
        Phase::Ended(Exit {
            reason: Reason::Command,
            position: PositionAtEnd::Closed,
        })
    );
    assert_eq!(state.confirm_closed(), Err(PhaseError::NotInClosing));
    let before = state.clone();
    for kind in [
        EventKind::Trade {
            level: 19,
            quantity: dec!(0.11),
            price: dec!(69.30),
            fee: Decimal::ZERO,
            maker: true,
        },
        EventKind::Price(Prices::flat(dec!(70))),
        EventKind::Funding { amount: dec!(0.01) },
        EventKind::Reject { level: 1 },
        EventKind::Backstop,
        EventKind::Command { keep: true },
        EventKind::Tick,
    ] {
        assert_eq!(
            step(&instance, &mut state, &event(kind)),
            Err(StepError::RunEnded)
        );
        assert_eq!(state, before);
    }
}

#[test]
fn no_order_rests_at_the_empty_level() {
    let instance = app_instance();
    let mut state = running(&instance, START_LEVEL);
    for (time, level) in [(1, 21), (2, 22), (3, 21), (4, 20), (5, 19)] {
        fill(&instance, &mut state, time, level);
        let orders = quote(&instance, &state);
        assert_eq!(orders.len(), 40);
        assert!(
            orders
                .iter()
                .all(|order| order.level != state.empty_level())
        );
        assert!(orders.iter().all(|order| match order.side {
            Side::Buy => order.level < state.empty_level(),
            Side::Sell => order.level > state.empty_level(),
        }));
    }
}

#[test]
fn no_buys_rest_at_the_lowest_level_and_no_sells_at_the_highest() {
    let instance = app_instance();
    let mut low = running(&instance, START_LEVEL);
    for moved in 1..=20 {
        fill(&instance, &mut low, u64::from(moved), START_LEVEL - moved);
    }
    assert_eq!(low.empty_level(), 0);
    assert!(
        quote(&instance, &low)
            .iter()
            .all(|order| order.side == Side::Sell)
    );
    assert_eq!(quote(&instance, &low).len(), 40);
    assert_eq!(low.position(instance.quantity()), dec!(2.2));
    let mut high = running(&instance, START_LEVEL);
    for moved in 1..=20 {
        fill(&instance, &mut high, u64::from(moved), START_LEVEL + moved);
    }
    assert_eq!(high.empty_level(), 40);
    assert!(
        quote(&instance, &high)
            .iter()
            .all(|order| order.side == Side::Buy)
    );
    assert_eq!(high.position(instance.quantity()), dec!(-2.2));
    assert_eq!(
        step(
            &instance,
            &mut high,
            &trade(99, 41, dec!(0.11), dec!(84.70))
        ),
        Err(StepError::UnknownLevel)
    );
}

#[test]
fn quote_still_lists_orders_in_closing_although_closing_cancels_every_resting_order() {
    let instance = app_instance();
    let state = closing(&instance, EventKind::Command { keep: false });
    assert!(matches!(state.phase(), Phase::Closing(_)));
    assert_eq!(quote(&instance, &state).len(), 40);
}

#[test]
fn a_fill_price_may_be_better_than_the_level_price_but_not_worse() {
    let instance = app_instance();
    let mut state = running(&instance, START_LEVEL);
    assert!(
        step(
            &instance,
            &mut state,
            &trade(1, 19, dec!(0.05), dec!(69.30))
        )
        .is_ok()
    );
    assert!(
        step(
            &instance,
            &mut state,
            &trade(2, 19, dec!(0.02), dec!(60.00))
        )
        .is_ok()
    );
    assert!(
        step(
            &instance,
            &mut state,
            &trade(3, 21, dec!(0.05), dec!(70.70))
        )
        .is_ok()
    );
    assert!(
        step(
            &instance,
            &mut state,
            &trade(4, 21, dec!(0.02), dec!(75.00))
        )
        .is_ok()
    );
    let before = state.clone();
    assert_eq!(
        step(
            &instance,
            &mut state,
            &trade(5, 19, dec!(0.01), dec!(69.31))
        ),
        Err(StepError::PriceWorseThanLevel)
    );
    assert_eq!(
        step(
            &instance,
            &mut state,
            &trade(5, 21, dec!(0.01), dec!(70.69))
        ),
        Err(StepError::PriceWorseThanLevel)
    );
    assert_eq!(state, before);
}

#[test]
fn a_fill_two_levels_from_the_empty_level_is_refused_although_order_latency_can_produce_it() {
    let instance = app_instance();
    let mut state = running(&instance, START_LEVEL);
    let rearmed = fill(&instance, &mut state, 1, 21).send;
    assert_eq!(
        rearmed.map(|order| (order.level, order.side)),
        Some((20, Side::Buy))
    );
    assert_eq!(state.empty_level(), 21);
    let before = state.clone();
    let lower_buy_first = trade(2, 19, dec!(0.11), dec!(69.30));
    assert_eq!(
        step(&instance, &mut state, &lower_buy_first),
        Err(StepError::FillNotAdjacent)
    );
    assert_eq!(state, before);
    assert_eq!(state.ledger().trades().count(), 1);
    fill(&instance, &mut state, 3, 20);
    assert_eq!(state.empty_level(), 20);
    assert_eq!(state.position(instance.quantity()), Decimal::ZERO);
    assert_eq!(state.ledger().trades().count(), 2);
}

#[test]
fn a_part_filled_order_away_from_the_empty_level_cannot_be_completed_there() {
    let instance = app_instance();
    let mut state = running(&instance, START_LEVEL);
    step(
        &instance,
        &mut state,
        &trade(1, 18, dec!(0.05), dec!(68.60)),
    )
    .expect("recorded");
    assert_eq!(
        step(
            &instance,
            &mut state,
            &trade(2, 18, dec!(0.06), dec!(68.60))
        ),
        Err(StepError::FillNotAdjacent)
    );
    fill(&instance, &mut state, 3, 19);
    let completed = step(
        &instance,
        &mut state,
        &trade(4, 18, dec!(0.06), dec!(68.60)),
    )
    .expect("the order is adjacent now");
    assert_eq!(completed.fill.map(|fill| fill.level), Some(18));
}
