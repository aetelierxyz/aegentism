mod common;

use aegent_mm_core::params::{Instance, Resources, Theta, Trigger};
use aegent_mm_core::reference;
use aegent_mm_core::state::State;
use aegent_mm_core::step::step;
use aegent_mm_core::value::{
    closing_price, matched, pairing, profit_at, profit_last, profit_mark, profit_trigger, roi,
    run_wallet_value, unmatched, wallet_value,
};
use aegent_mm_core::{Event, EventKind, Prices, Side};
use common::{admitted, app_instance, fill, running};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

const START_LEVEL: u32 = 20;

fn older_grid() -> Instance {
    admitted(
        Theta::new(dec!(73.19), dec!(77.59), 10, dec!(0.16)),
        reference::resources(),
    )
}

fn paid(time_ms: u64, level: u32, quantity: Decimal, price: Decimal, fee: Decimal) -> Event {
    Event::at(
        time_ms,
        EventKind::Trade {
            level,
            quantity,
            price,
            fee,
            maker: true,
        },
    )
}

fn funding(time_ms: u64, amount: Decimal) -> Event {
    Event::at(time_ms, EventKind::Funding { amount })
}

fn apply(instance: &Instance, state: &mut State, events: &[Event]) {
    for event in events {
        step(instance, state, event).expect("admissible event");
    }
}

fn quotes(last: Decimal, mark: Decimal) -> Prices {
    Prices {
        last,
        mark,
        bid: last,
        ask: last,
    }
}

#[test]
fn profit_is_cash_plus_position_value_minus_fees_plus_funding() {
    let instance = app_instance();
    let mut state = running(&instance, START_LEVEL);
    apply(
        &instance,
        &mut state,
        &[
            paid(1, 19, dec!(0.11), dec!(69.30), dec!(0.001)),
            funding(2, dec!(-0.02)),
        ],
    );
    assert_eq!(state.ledger().cash(), dec!(-7.623));
    assert_eq!(state.ledger().fees(), dec!(0.001));
    assert_eq!(state.ledger().funding(), dec!(-0.02));
    assert_eq!(profit_at(&instance, &state, dec!(70)), dec!(0.056));
    assert_eq!(profit_at(&instance, &state, dec!(69.30)), dec!(-0.021));
}

#[test]
fn the_trigger_price_selects_between_last_and_mark() {
    let prices = quotes(dec!(70.50), dec!(69.90));
    let on_last = admitted(
        Theta {
            trigger: Trigger::Last,
            ..reference::theta()
        },
        reference::resources(),
    );
    let on_mark = admitted(
        Theta {
            trigger: Trigger::Mark,
            ..reference::theta()
        },
        reference::resources(),
    );
    for instance in [&on_last, &on_mark] {
        let mut state = running(instance, START_LEVEL);
        fill(instance, &mut state, 1, 19);
        assert_eq!(profit_last(instance, &state, &prices), dec!(0.132));
        assert_eq!(profit_mark(instance, &state, &prices), dec!(0.066));
    }
    let mut state = running(&on_last, START_LEVEL);
    fill(&on_last, &mut state, 1, 19);
    assert_eq!(prices.trigger(Trigger::Last), dec!(70.50));
    assert_eq!(prices.trigger(Trigger::Mark), dec!(69.90));
    assert_eq!(profit_trigger(&on_last, &state, &prices), dec!(0.132));
    assert_eq!(profit_trigger(&on_mark, &state, &prices), dec!(0.066));
}

#[test]
fn roi_is_the_reported_profit_over_the_margin() {
    let instance = app_instance();
    let mut state = running(&instance, START_LEVEL);
    fill(&instance, &mut state, 1, 21);
    fill(&instance, &mut state, 2, 20);
    let prices = Prices::flat(dec!(70.00));
    assert_eq!(profit_last(&instance, &state, &prices), dec!(0.077));
    assert_eq!(roi(&instance, &state, &prices), dec!(0.00385));
}

#[test]
fn the_later_sell_is_paired_first_as_in_the_observed_history() {
    let instance = older_grid();
    assert_eq!(instance.spacing(), dec!(0.44));
    assert_eq!(instance.level_price(5), dec!(75.39));
    assert_eq!(instance.level_price(6), dec!(75.83));
    assert_eq!(instance.level_price(7), dec!(76.27));
    let mut state = running(&instance, 5);
    for (time, level) in [(1, 6), (2, 7), (3, 6), (4, 5)] {
        fill(&instance, &mut state, time, level);
    }
    let result = pairing(state.ledger(), instance.quantity());
    assert_eq!(result.completed, 4);
    assert!(result.unpaired.is_empty());
    assert_eq!(result.pairs.len(), 2);
    let first = &result.pairs[0];
    assert_eq!((first.sell.level, first.sell.completed_ms), (7, 2));
    assert_eq!((first.buy.level, first.buy.completed_ms), (6, 3));
    assert_eq!(first.sell.notional, dec!(0.16) * dec!(76.27));
    assert_eq!(first.buy.notional, dec!(0.16) * dec!(75.83));
    assert_eq!(first.profit(), dec!(0.0704));
    let second = &result.pairs[1];
    assert_eq!((second.sell.level, second.sell.completed_ms), (6, 1));
    assert_eq!((second.buy.level, second.buy.completed_ms), (5, 4));
    assert_eq!(second.profit(), dec!(0.0704));
    assert_eq!(result.matched(), dec!(0.1408));
    assert_eq!(matched(&instance, &state), dec!(0.1408));
}

#[test]
fn twenty_four_pairs_of_the_older_grid_total_one_sixty_nine() {
    let instance = older_grid();
    let mut state = running(&instance, 5);
    for round in 0..24 {
        fill(&instance, &mut state, 2 * round + 1, 6);
        fill(&instance, &mut state, 2 * round + 2, 5);
    }
    let result = pairing(state.ledger(), instance.quantity());
    assert_eq!(result.pairs.len(), 24);
    assert!(
        result
            .pairs
            .iter()
            .all(|pair| pair.profit() == dec!(0.0704))
    );
    assert_eq!(result.matched(), dec!(1.6896));
    assert_eq!(result.matched().round_dp(2), dec!(1.69));
}

#[test]
fn a_reducing_order_pairs_with_the_most_recent_unpaired_order_of_the_other_side() {
    let instance = app_instance();
    let mut state = running(&instance, START_LEVEL);
    for (time, level) in [(1, 19), (2, 18), (3, 17), (4, 18)] {
        fill(&instance, &mut state, time, level);
    }
    let result = pairing(state.ledger(), instance.quantity());
    assert_eq!(result.completed, 4);
    assert_eq!(result.pairs.len(), 1);
    assert_eq!(result.pairs[0].buy.level, 17);
    assert_eq!(result.pairs[0].sell.level, 18);
    assert_eq!(result.pairs[0].profit(), dec!(0.077));
    let open: Vec<(u32, Side)> = result
        .unpaired
        .iter()
        .map(|order| (order.level, order.side))
        .collect();
    assert_eq!(open, vec![(19, Side::Buy), (18, Side::Buy)]);
    assert_eq!(state.empty_level(), 18);
}

#[test]
fn pairs_form_on_the_short_side_too() {
    let instance = app_instance();
    let mut state = running(&instance, START_LEVEL);
    for (time, level) in [(1, 21), (2, 22), (3, 21), (4, 22), (5, 23)] {
        fill(&instance, &mut state, time, level);
    }
    let result = pairing(state.ledger(), instance.quantity());
    assert_eq!(result.completed, 5);
    assert_eq!(result.pairs.len(), 1);
    assert_eq!(
        (
            result.pairs[0].sell.level,
            result.pairs[0].sell.completed_ms
        ),
        (22, 2)
    );
    assert_eq!(
        (result.pairs[0].buy.level, result.pairs[0].buy.completed_ms),
        (21, 3)
    );
    let open: Vec<(u32, u64)> = result
        .unpaired
        .iter()
        .map(|order| (order.level, order.completed_ms))
        .collect();
    assert_eq!(open, vec![(21, 1), (22, 4), (23, 5)]);
}

#[test]
fn a_pair_pays_the_fees_of_its_two_orders() {
    let instance = app_instance();
    let mut state = running(&instance, START_LEVEL);
    apply(
        &instance,
        &mut state,
        &[
            paid(1, 19, dec!(0.11), dec!(69.30), dec!(0.003)),
            paid(2, 20, dec!(0.11), dec!(70.00), dec!(0.004)),
        ],
    );
    let result = pairing(state.ledger(), instance.quantity());
    assert_eq!(result.pairs.len(), 1);
    assert_eq!(result.pairs[0].buy.fees, dec!(0.003));
    assert_eq!(result.pairs[0].sell.fees, dec!(0.004));
    assert_eq!(result.pairs[0].profit(), dec!(0.07));
    assert_eq!(result.matched(), dec!(0.07));
}

#[test]
fn an_order_filled_in_parts_enters_its_pair_with_all_its_trades() {
    let instance = app_instance();
    let mut state = running(&instance, START_LEVEL);
    apply(
        &instance,
        &mut state,
        &[
            paid(1, 19, dec!(0.04), dec!(69.30), dec!(0.001)),
            paid(2, 19, dec!(0.07), dec!(69.20), dec!(0.002)),
            paid(3, 20, dec!(0.05), dec!(70.00), dec!(0.0015)),
            paid(4, 20, dec!(0.06), dec!(70.10), dec!(0.0025)),
        ],
    );
    let result = pairing(state.ledger(), instance.quantity());
    assert_eq!(result.completed, 2);
    assert_eq!(result.pairs.len(), 1);
    let pair = &result.pairs[0];
    assert_eq!(pair.buy.quantity, dec!(0.11));
    assert_eq!(pair.buy.notional, dec!(2.772) + dec!(4.844));
    assert_eq!(pair.buy.fees, dec!(0.003));
    assert_eq!(pair.buy.completed_ms, 2);
    assert_eq!(pair.sell.notional, dec!(3.5) + dec!(4.206));
    assert_eq!(pair.sell.fees, dec!(0.004));
    assert_eq!(pair.sell.completed_ms, 4);
    assert_eq!(pair.profit(), dec!(7.706) - dec!(7.616) - dec!(0.007));
}

#[test]
fn partial_lots_and_unpaired_lots_count_as_unmatched() {
    let instance = app_instance();
    let mut state = running(&instance, START_LEVEL);
    apply(
        &instance,
        &mut state,
        &[
            paid(1, 21, dec!(0.11), dec!(70.70), dec!(0.001)),
            paid(2, 20, dec!(0.11), dec!(70.00), dec!(0.001)),
            paid(3, 19, dec!(0.11), dec!(69.30), dec!(0.001)),
            paid(4, 18, dec!(0.05), dec!(68.60), dec!(0.0005)),
            funding(5, dec!(0.03)),
        ],
    );
    let prices = Prices::flat(dec!(69.00));
    let result = pairing(state.ledger(), instance.quantity());
    assert_eq!(result.pairs.len(), 1);
    assert_eq!(result.unpaired.len(), 1);
    assert_eq!(result.completed, 3);
    assert_eq!(matched(&instance, &state), dec!(0.075));
    let open_lot = dec!(0.11) * (dec!(69.00) - dec!(69.30)) - dec!(0.001);
    let partial_lot = dec!(0.05) * (dec!(69.00) - dec!(68.60)) - dec!(0.0005);
    assert_eq!(
        unmatched(&instance, &state, &prices),
        open_lot + partial_lot
    );
    assert_eq!(
        profit_last(&instance, &state, &prices),
        dec!(0.075) + open_lot + partial_lot + dec!(0.03)
    );
}

#[test]
fn a_flat_wallet_is_worth_its_balance() {
    let prices = quotes(dec!(70), dec!(71));
    assert_eq!(
        wallet_value(
            dec!(97.5),
            dec!(69.30),
            Decimal::ZERO,
            &prices,
            dec!(0.0004)
        ),
        dec!(97.5)
    );
}

#[test]
fn a_long_wallet_is_valued_at_the_bid_after_the_taker_fee() {
    let prices = Prices {
        last: dec!(68.01),
        mark: dec!(68.03),
        bid: dec!(68.00),
        ask: dec!(68.02),
    };
    assert_eq!(closing_price(dec!(0.22), &prices), dec!(68.00));
    assert_eq!(
        wallet_value(dec!(100), dec!(69.30), dec!(0.22), &prices, dec!(0.0004)),
        dec!(99.708016)
    );
}

#[test]
fn a_short_wallet_is_valued_at_the_ask_after_the_taker_fee() {
    let prices = Prices {
        last: dec!(71.99),
        mark: dec!(71.97),
        bid: dec!(71.98),
        ask: dec!(72.00),
    };
    assert_eq!(closing_price(dec!(-0.22), &prices), dec!(72.00));
    assert_eq!(
        wallet_value(dec!(100), dec!(70.70), dec!(-0.22), &prices, dec!(0.0004)),
        dec!(99.707664)
    );
}

#[test]
fn the_value_of_a_wallet_holding_one_run_follows_from_the_start_balance_and_the_ledger() {
    let instance = admitted(
        reference::theta(),
        Resources {
            margin: dec!(100),
            leverage: 20,
        },
    );
    let mut state = running(&instance, START_LEVEL);
    apply(
        &instance,
        &mut state,
        &[
            paid(1, 21, dec!(0.11), dec!(70.70), dec!(0.002)),
            paid(2, 20, dec!(0.11), dec!(70.00), dec!(0.002)),
            paid(3, 19, dec!(0.11), dec!(69.30), dec!(0.002)),
            paid(4, 18, dec!(0.11), dec!(68.60), dec!(0.002)),
            funding(5, dec!(-0.01)),
        ],
    );
    let prices = Prices {
        last: dec!(68.01),
        mark: dec!(68.03),
        bid: dec!(68.00),
        ask: dec!(68.02),
    };
    let realized = dec!(0.077);
    let balance = dec!(100) + realized - dec!(0.008) - dec!(0.01);
    let entry = dec!(68.95);
    let position = state.position(instance.quantity());
    assert_eq!(position, dec!(0.22));
    assert_eq!(
        run_wallet_value(&instance, &state, dec!(100), &prices),
        wallet_value(balance, entry, position, &prices, dec!(0.0004))
    );
    assert_eq!(
        run_wallet_value(&instance, &state, dec!(100), &prices),
        dec!(100) + profit_at(&instance, &state, dec!(68.00))
            - dec!(0.0004) * dec!(0.22) * dec!(68)
    );
}

#[test]
fn the_value_of_a_flat_run_wallet_is_the_start_balance_plus_the_realized_result() {
    let instance = app_instance();
    let mut state = running(&instance, START_LEVEL);
    apply(
        &instance,
        &mut state,
        &[
            paid(1, 19, dec!(0.11), dec!(69.30), dec!(0.003)),
            paid(2, 20, dec!(0.11), dec!(70.00), dec!(0.003)),
            funding(3, dec!(0.02)),
        ],
    );
    for price in [dec!(1), dec!(70), dec!(500)] {
        assert_eq!(
            run_wallet_value(&instance, &state, dec!(100), &Prices::flat(price)),
            dec!(100.091)
        );
    }
}
