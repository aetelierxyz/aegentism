mod common;

use aegent_mm_core::backstop::{backstops, budget, high_slack, low_slack};
use aegent_mm_core::params::{Instance, Resources, Theta, Trigger};
use aegent_mm_core::program::Program;
use aegent_mm_core::reference;
use aegent_mm_core::run::Run;
use aegent_mm_core::start::{Precondition, Start, nearest_level, start, start_risk_ratio};
use aegent_mm_core::state::{Phase, PhaseError, State};
use aegent_mm_core::stop::risk_ratio_below_one;
use aegent_mm_core::{Prices, Side};
use common::{admitted, app_instance, program, wallet_balance};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn start_at(instance: &Instance, prices: Prices) -> Result<Start, Vec<Precondition>> {
    start(instance, &program(), &prices, wallet_balance())
}

fn refusal_at(instance: &Instance, price: Decimal) -> Vec<Precondition> {
    start_at(instance, Prices::flat(price)).expect_err("the start is refused")
}

#[test]
fn the_empty_level_is_the_level_nearest_the_start_price() {
    let instance = app_instance();
    assert_eq!(nearest_level(&instance, dec!(70.21)), 20);
    assert_eq!(nearest_level(&instance, dec!(70.00)), 20);
    assert_eq!(nearest_level(&instance, dec!(70.34)), 20);
    assert_eq!(nearest_level(&instance, dec!(70.36)), 21);
    assert_eq!(nearest_level(&instance, dec!(69.66)), 20);
    assert_eq!(nearest_level(&instance, dec!(69.64)), 19);
    assert_eq!(nearest_level(&instance, dec!(56.70)), 1);
}

#[test]
fn a_tie_between_two_nearest_levels_goes_to_the_lower_level() {
    let instance = app_instance();
    assert_eq!(nearest_level(&instance, dec!(70.35)), 20);
    assert_eq!(nearest_level(&instance, dec!(69.65)), 19);
    assert_eq!(nearest_level(&instance, dec!(56.35)), 0);
    assert_eq!(nearest_level(&instance, dec!(83.65)), 39);
}

#[test]
fn a_price_outside_the_range_maps_to_the_bound() {
    let instance = app_instance();
    assert_eq!(nearest_level(&instance, dec!(56.00)), 0);
    assert_eq!(nearest_level(&instance, dec!(12.00)), 0);
    assert_eq!(nearest_level(&instance, dec!(83.66)), 40);
    assert_eq!(nearest_level(&instance, dec!(84.00)), 40);
    assert_eq!(nearest_level(&instance, dec!(400.00)), 40);
}

#[test]
fn a_start_at_the_lowest_level_is_refused() {
    let instance = app_instance();
    assert_eq!(
        refusal_at(&instance, dec!(56.35)),
        vec![Precondition::EmptyLevelAtBound]
    );
    assert_eq!(
        refusal_at(&instance, dec!(40.00)),
        vec![Precondition::EmptyLevelAtBound]
    );
    assert_eq!(State::initial(&instance, 0), None);
}

#[test]
fn a_start_at_the_highest_level_is_refused() {
    let instance = app_instance();
    assert_eq!(
        refusal_at(&instance, dec!(83.66)),
        vec![Precondition::EmptyLevelAtBound]
    );
    assert_eq!(
        refusal_at(&instance, dec!(99.00)),
        vec![Precondition::EmptyLevelAtBound]
    );
    assert_eq!(State::initial(&instance, 40), None);
    assert_eq!(State::initial(&instance, 41), None);
}

#[test]
fn a_start_with_orders_on_both_sides_is_accepted() {
    let instance = admitted(
        Theta::new(dec!(56), dec!(57.4), 2, dec!(0.11)),
        reference::resources(),
    );
    let started = start_at(&instance, Prices::flat(dec!(56.70))).expect("preconditions hold");
    assert_eq!(started.state.start_level(), 1);
    assert_eq!(started.orders.len(), 2);
    assert_eq!(started.orders[0].side, Side::Buy);
    assert_eq!(started.orders[0].price, dec!(56.00));
    assert_eq!(started.orders[1].side, Side::Sell);
    assert_eq!(started.orders[1].price, dec!(57.40));
}

#[test]
fn a_risk_ratio_below_one_at_start_is_refused() {
    let exactly_one = admitted(
        reference::theta(),
        Resources {
            margin: dec!(8.5085),
            leverage: 20,
        },
    );
    let below_one = admitted(
        reference::theta(),
        Resources {
            margin: dec!(8.5084),
            leverage: 20,
        },
    );
    assert_eq!(
        start_risk_ratio(&exactly_one, 20, dec!(70.21)),
        Some(Decimal::ONE)
    );
    assert!(start_at(&exactly_one, Prices::flat(dec!(70.21))).is_ok());
    assert_eq!(
        refusal_at(&below_one, dec!(70.21)),
        vec![Precondition::RiskRatioBelowOne]
    );
}

#[test]
fn the_start_risk_condition_equals_the_larger_side_over_leverage_within_the_margin() {
    for (margin, leverage) in [
        (dec!(8.5084), 20),
        (dec!(8.5085), 20),
        (dec!(20), 20),
        (dec!(20), 5),
        (dec!(34.03), 5),
        (dec!(34.04), 5),
        (dec!(100), 1),
    ] {
        let instance = admitted(reference::theta(), Resources { margin, leverage });
        for start_level in 1..40 {
            let state = State::initial(&instance, start_level).expect("inside the ladder");
            let bids = instance.quantity() * instance.sum_of_prices_below(start_level);
            let asks = instance.quantity() * instance.sum_of_prices_above(start_level);
            let within = bids.max(asks) / Decimal::from(leverage) <= margin;
            assert_eq!(
                !risk_ratio_below_one(&instance, &state, dec!(70.21)),
                within
            );
        }
    }
}

#[test]
fn a_budget_that_does_not_exceed_the_loss_at_a_bound_is_refused() {
    let instance = app_instance();
    let program = program();
    let prices = Prices::flat(dec!(70.21));
    assert_eq!(budget(&program, dec!(94.63)), dec!(14.63));
    assert_eq!(low_slack(&instance, 20, dec!(14.63)), dec!(0));
    assert_eq!(high_slack(&instance, 20, dec!(14.63)), dec!(0));
    assert_eq!(
        start(&instance, &program, &prices, dec!(94.63)),
        Err(vec![
            Precondition::LowBackstopDistanceNotPositive,
            Precondition::HighBackstopDistanceNotPositive
        ])
    );
    assert!(start(&instance, &program, &prices, dec!(94.64)).is_ok());
    assert_eq!(
        start(&instance, &program, &prices, dec!(79)),
        Err(vec![
            Precondition::LowBackstopDistanceNotPositive,
            Precondition::HighBackstopDistanceNotPositive
        ])
    );
}

#[test]
fn only_the_long_side_can_lack_budget() {
    let instance = app_instance();
    assert_eq!(nearest_level(&instance, dec!(73.50)), 25);
    assert_eq!(instance.loss_at_lower_bound(25), dec!(23.1));
    assert_eq!(instance.loss_at_upper_bound(25), dec!(8.085));
    assert_eq!(
        refusal_at(&instance, dec!(73.50)),
        vec![Precondition::LowBackstopDistanceNotPositive]
    );
}

#[test]
fn only_the_short_side_can_lack_budget() {
    let instance = app_instance();
    assert_eq!(nearest_level(&instance, dec!(66.50)), 15);
    assert_eq!(instance.loss_at_lower_bound(15), dec!(8.085));
    assert_eq!(instance.loss_at_upper_bound(15), dec!(23.1));
    assert_eq!(
        refusal_at(&instance, dec!(66.50)),
        vec![Precondition::HighBackstopDistanceNotPositive]
    );
}

#[test]
fn backstop_distances_follow_the_budget_left_after_the_loss_at_each_bound() {
    let instance = app_instance();
    let stops = backstops(&instance, 25, dec!(30)).expect("inside the ladder");
    assert_eq!(stops.budget, dec!(30));
    assert_eq!(stops.low_distance, dec!(6.9) / dec!(2.75));
    assert_eq!(stops.high_distance, dec!(21.915) / dec!(1.65));
    assert_eq!(stops.sell_stop, dec!(53.50));
    assert_eq!(stops.buy_stop, dec!(97.28));
    assert_eq!(backstops(&instance, 0, dec!(30)), None);
    assert_eq!(backstops(&instance, 40, dec!(30)), None);
}

#[test]
fn backstop_triggers_are_rounded_to_the_tick_toward_the_range() {
    let instance = app_instance();
    let stops = backstops(&instance, 20, dec!(20)).expect("inside the ladder");
    let exact_low = dec!(56) - dec!(5.37) / dec!(2.2);
    let exact_high = dec!(84) + dec!(5.37) / dec!(2.2);
    assert!(exact_low < stops.sell_stop);
    assert!(stops.sell_stop - exact_low < dec!(0.01));
    assert!(exact_high > stops.buy_stop);
    assert!(exact_high - stops.buy_stop < dec!(0.01));
    let on_tick = backstops(&instance, 20, dec!(19.03)).expect("inside the ladder");
    assert_eq!(on_tick.low_distance, dec!(2));
    assert_eq!(on_tick.sell_stop, dec!(54.00));
    assert_eq!(on_tick.buy_stop, dec!(86.00));
}

#[test]
fn a_distance_below_one_tick_puts_the_backstop_triggers_on_the_range_bounds() {
    let instance = app_instance();
    let started = start(
        &instance,
        &program(),
        &Prices::flat(dec!(70.21)),
        dec!(94.64),
    )
    .expect("preconditions hold");
    let stops = started.backstops;
    assert_eq!(stops.budget, dec!(14.64));
    assert!(stops.low_distance > Decimal::ZERO && stops.low_distance < dec!(0.01));
    assert!(stops.high_distance > Decimal::ZERO && stops.high_distance < dec!(0.01));
    assert_eq!(stops.sell_stop, dec!(56.00));
    assert_eq!(stops.buy_stop, dec!(84.00));
}

#[test]
fn a_budget_wider_than_the_ladder_puts_the_sell_stop_below_zero() {
    let instance = admitted(
        Theta::new(dec!(56), dec!(57.4), 2, dec!(0.11)),
        reference::resources(),
    );
    let started = start_at(&instance, Prices::flat(dec!(56.70))).expect("preconditions hold");
    assert_eq!(started.backstops.budget, dec!(20));
    assert!(started.backstops.low_distance > instance.lower());
    assert_eq!(started.backstops.sell_stop, dec!(-125.81));
    assert_eq!(started.backstops.buy_stop, dec!(239.21));
}

#[test]
fn a_trigger_price_at_or_above_the_top_stop_is_refused() {
    let at_stop = admitted(
        Theta {
            stop_top: Some(dec!(70.21)),
            ..reference::theta()
        },
        reference::resources(),
    );
    let above_price = admitted(
        Theta {
            stop_top: Some(dec!(70.22)),
            ..reference::theta()
        },
        reference::resources(),
    );
    assert_eq!(
        refusal_at(&at_stop, dec!(70.21)),
        vec![Precondition::TriggerNotBelowStopTop]
    );
    assert!(start_at(&above_price, Prices::flat(dec!(70.21))).is_ok());
}

#[test]
fn a_trigger_price_at_or_below_the_bottom_stop_is_refused() {
    let at_stop = admitted(
        Theta {
            stop_bottom: Some(dec!(70.21)),
            ..reference::theta()
        },
        reference::resources(),
    );
    let below_price = admitted(
        Theta {
            stop_bottom: Some(dec!(70.20)),
            ..reference::theta()
        },
        reference::resources(),
    );
    assert_eq!(
        refusal_at(&at_stop, dec!(70.21)),
        vec![Precondition::TriggerNotAboveStopBottom]
    );
    assert!(start_at(&below_price, Prices::flat(dec!(70.21))).is_ok());
}

#[test]
fn the_price_stops_are_checked_against_the_configured_trigger_price() {
    let prices = Prices {
        last: dec!(70.21),
        mark: dec!(71.00),
        bid: dec!(70.20),
        ask: dec!(70.22),
    };
    let stops = Theta {
        stop_top: Some(dec!(70.50)),
        stop_bottom: Some(dec!(60.00)),
        ..reference::theta()
    };
    let on_last = admitted(
        Theta {
            trigger: Trigger::Last,
            ..stops.clone()
        },
        reference::resources(),
    );
    let on_mark = admitted(
        Theta {
            trigger: Trigger::Mark,
            ..stops
        },
        reference::resources(),
    );
    assert!(start_at(&on_last, prices).is_ok());
    assert_eq!(
        start_at(&on_mark, prices),
        Err(vec![Precondition::TriggerNotBelowStopTop])
    );
}

#[test]
fn every_failed_precondition_is_reported_together() {
    let instance = admitted(
        Theta {
            stop_top: Some(dec!(60)),
            stop_bottom: Some(dec!(58)),
            ..reference::theta()
        },
        Resources {
            margin: dec!(20),
            leverage: 5,
        },
    );
    let refused = start(
        &instance,
        &program(),
        &Prices::flat(dec!(73.50)),
        wallet_balance(),
    );
    assert_eq!(
        refused,
        Err(vec![
            Precondition::RiskRatioBelowOne,
            Precondition::LowBackstopDistanceNotPositive,
            Precondition::TriggerNotBelowStopTop,
        ])
    );
    assert_eq!(
        refusal_at(&instance, dec!(50)),
        vec![
            Precondition::EmptyLevelAtBound,
            Precondition::TriggerNotAboveStopBottom
        ]
    );
}

#[test]
fn the_initial_state_is_in_init_with_an_empty_ledger_and_no_filled_part() {
    let instance = app_instance();
    let started = start_at(&instance, Prices::flat(dec!(70.21))).expect("preconditions hold");
    let state = started.state;
    assert_eq!(state.phase(), Phase::Init);
    assert_eq!(state.start_level(), 20);
    assert_eq!(state.empty_level(), 20);
    assert!(state.ledger().is_empty());
    assert!((0..=40).all(|level| state.filled(level) == Decimal::ZERO));
    assert_eq!(state.position(instance.quantity()), Decimal::ZERO);
    assert_eq!(state.exit(), None);
}

#[test]
fn every_start_order_sits_at_least_half_a_spacing_from_the_start_price() {
    let instance = admitted(reference::theta(), common::roomy_resources());
    let program = Program::new(dec!(1000), dec!(0.5), 60_000, 5_000, 10_000).expect("valid");
    for cents in 5636..=8365 {
        let price = Decimal::new(cents, 2);
        let started = start(&instance, &program, &Prices::flat(price), dec!(1000))
            .expect("preconditions hold");
        assert_eq!(started.orders.len(), 40);
        assert!(
            started
                .orders
                .iter()
                .all(|order| (order.price - price).abs() >= instance.spacing() / Decimal::TWO)
        );
        assert!(started.orders.iter().all(|order| match order.side {
            Side::Buy => order.price < price,
            Side::Sell => order.price > price,
        }));
    }
}

#[test]
fn the_run_enters_run_when_the_start_is_acknowledged() {
    let mut run = Run::start(
        app_instance(),
        program(),
        Prices::flat(dec!(70.21)),
        1_000,
        wallet_balance(),
    )
    .expect("preconditions hold");
    assert_eq!(run.state().phase(), Phase::Init);
    assert_eq!(run.start_price(), dec!(70.21));
    assert_eq!(run.last_price_ms(), 1_000);
    assert_eq!(run.quote().len(), 40);
    assert_eq!(run.backstops().sell_stop, dec!(53.56));
    assert_eq!(run.backstops().buy_stop, dec!(86.44));
    assert_eq!(run.confirm_closed(), Err(PhaseError::NotInClosing));
    assert_eq!(run.acknowledge_start(), Ok(()));
    assert_eq!(run.state().phase(), Phase::Run);
    assert_eq!(run.acknowledge_start(), Err(PhaseError::NotInInit));
}

#[test]
fn a_refused_start_builds_no_run() {
    let refused = Run::start(
        app_instance(),
        program(),
        Prices::flat(dec!(84.00)),
        0,
        wallet_balance(),
    );
    assert_eq!(refused, Err(vec![Precondition::EmptyLevelAtBound]));
}

#[test]
fn the_start_price_is_the_last_price_and_not_the_mark_price() {
    let instance = app_instance();
    let prices = Prices {
        last: dec!(70.21),
        mark: dec!(71.20),
        bid: dec!(70.20),
        ask: dec!(70.22),
    };
    let started = start_at(&instance, prices).expect("preconditions hold");
    assert_eq!(nearest_level(&instance, dec!(71.20)), 22);
    assert_eq!(started.start_price, dec!(70.21));
    assert_eq!(started.state.start_level(), 20);
}

#[test]
fn sums_of_level_prices_are_empty_beyond_the_ladder() {
    let instance = app_instance();
    assert_eq!(instance.sum_of_prices_above(40), Decimal::ZERO);
    assert_eq!(instance.sum_of_prices_above(41), Decimal::ZERO);
    assert_eq!(instance.sum_of_prices_above(u32::MAX), Decimal::ZERO);
    assert_eq!(instance.sum_of_prices_below(0), Decimal::ZERO);
    assert_eq!(instance.sum_of_prices_below(41), dec!(2870));
    assert_eq!(instance.sum_of_prices_below(u32::MAX), dec!(2870));
}
