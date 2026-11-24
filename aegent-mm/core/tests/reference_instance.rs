mod common;

use aegent_mm_core::backstop::{backstops, budget};
use aegent_mm_core::estimates::{
    Ladder, binance_quantity, binance_quantity_in_lots, liquidation_price_long,
    liquidation_price_short, profit_per_grid,
};
use aegent_mm_core::params::{Inadmissible, Instance, Resources};
use aegent_mm_core::quote::{quote, resting_notional};
use aegent_mm_core::reference;
use aegent_mm_core::start::{Precondition, nearest_level, start, start_risk_ratio};
use aegent_mm_core::state::State;
use aegent_mm_core::stop::{risk_ratio, risk_ratio_below_one};
use aegent_mm_core::value::profit_at;
use aegent_mm_core::{Prices, Side};
use common::{admitted, app_instance, fill, program, running, wallet_balance};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

const START_LEVEL: u32 = 20;

fn one_way(instance: &Instance, levels: u32, down: bool) -> State {
    let mut state = running(instance, START_LEVEL);
    for moved in 1..=levels {
        let level = if down {
            START_LEVEL - moved
        } else {
            START_LEVEL + moved
        };
        fill(instance, &mut state, u64::from(moved), level);
    }
    state
}

fn ratio_after(instance: &Instance, levels: u32, down: bool) -> Decimal {
    let state = one_way(instance, levels, down);
    let mark = instance.level_price(state.empty_level());
    risk_ratio(instance, &state, mark).expect("orders rest on the book")
}

#[test]
fn the_app_grid_needs_a_leverage_cap_of_twenty() {
    let refused = Instance::admit(
        reference::binding(),
        reference::theta(),
        reference::resources(),
        wallet_balance(),
    );
    assert_eq!(refused, Err(vec![Inadmissible::LeverageAboveCap]));
    assert!(
        Instance::admit(
            reference::app_binding(),
            reference::theta(),
            reference::resources(),
            wallet_balance(),
        )
        .is_ok()
    );
}

#[test]
fn spacing_is_seventy_cents() {
    assert_eq!(app_instance().spacing(), dec!(0.70));
}

#[test]
fn forty_one_levels_span_the_range() {
    let instance = app_instance();
    let prices = instance.level_prices();
    assert_eq!(instance.level_count(), 41);
    assert_eq!(prices.len(), 41);
    assert_eq!(prices[0], dec!(56.00));
    assert_eq!(prices[40], dec!(84.00));
    assert!(
        prices
            .windows(2)
            .all(|pair| pair[1] - pair[0] == dec!(0.70))
    );
}

#[test]
fn the_empty_level_at_start_is_twenty_at_seventy() {
    let instance = app_instance();
    let level = nearest_level(&instance, reference::start_price());
    assert_eq!(reference::start_price(), dec!(70.21));
    assert_eq!(level, 20);
    assert_eq!(instance.level_price(level), dec!(70.00));
}

#[test]
fn twenty_buys_and_twenty_sells_rest_at_start() {
    let instance = app_instance();
    let state = State::initial(&instance, START_LEVEL).expect("start level inside the ladder");
    let orders = quote(&instance, &state);
    let buys: Vec<_> = orders
        .iter()
        .filter(|order| order.side == Side::Buy)
        .collect();
    let sells: Vec<_> = orders
        .iter()
        .filter(|order| order.side == Side::Sell)
        .collect();
    assert_eq!(buys.len(), 20);
    assert_eq!(sells.len(), 20);
    assert_eq!(buys.first().map(|order| order.price), Some(dec!(56.00)));
    assert_eq!(buys.last().map(|order| order.price), Some(dec!(69.30)));
    assert_eq!(sells.first().map(|order| order.price), Some(dec!(70.70)));
    assert_eq!(sells.last().map(|order| order.price), Some(dec!(84.00)));
    assert!(orders.iter().all(|order| order.quantity == dec!(0.11)));
}

#[test]
fn the_forty_order_prices_are_the_levels_without_seventy() {
    let instance = app_instance();
    let state = State::initial(&instance, START_LEVEL).expect("start level inside the ladder");
    let quoted: Vec<Decimal> = quote(&instance, &state)
        .iter()
        .map(|order| order.price)
        .collect();
    let expected: Vec<Decimal> = instance
        .level_prices()
        .into_iter()
        .filter(|price| *price != dec!(70.00))
        .collect();
    assert_eq!(quoted.len(), 40);
    assert_eq!(quoted, expected);
}

#[test]
fn the_largest_position_is_two_point_two_either_way() {
    let instance = app_instance();
    assert_eq!(instance.largest_long(START_LEVEL), dec!(2.2));
    assert_eq!(instance.largest_short(START_LEVEL), dec!(2.2));
    assert_eq!(instance.largest_open_lots(START_LEVEL), 20);
    let long = one_way(&instance, 20, true);
    let short = one_way(&instance, 20, false);
    assert_eq!(long.position(instance.quantity()), dec!(2.2));
    assert_eq!(short.position(instance.quantity()), dec!(-2.2));
}

#[test]
fn one_matched_pair_earns_seventy_seven_mills() {
    assert_eq!(app_instance().pair_profit(), dec!(0.077));
}

#[test]
fn resting_notional_at_start() {
    let instance = app_instance();
    let state = State::initial(&instance, START_LEVEL).expect("start level inside the ladder");
    let (bids, asks) = resting_notional(&instance, &state);
    assert_eq!(bids, dec!(137.83));
    assert_eq!(asks, dec!(170.17));
    assert_eq!(
        instance.quantity() * instance.sum_of_prices_below(START_LEVEL),
        dec!(137.83)
    );
    assert_eq!(
        instance.quantity() * instance.sum_of_prices_above(START_LEVEL),
        dec!(170.17)
    );
}

#[test]
fn risk_ratio_at_start_is_two_point_three_five() {
    let instance = app_instance();
    let ratio = start_risk_ratio(&instance, START_LEVEL, reference::start_price())
        .expect("orders rest on the book");
    assert_eq!(ratio.round_dp(2), dec!(2.35));
    assert_eq!(ratio.trunc_with_scale(1), dec!(2.3));
}

#[test]
fn a_one_way_move_to_either_bound_loses_fourteen_sixty_three() {
    let instance = app_instance();
    assert_eq!(instance.loss_at_lower_bound(START_LEVEL), dec!(14.63));
    assert_eq!(instance.loss_at_upper_bound(START_LEVEL), dec!(14.63));
    assert_eq!(
        (instance.loss_at_lower_bound(START_LEVEL) / instance.margin()).round_dp(2),
        dec!(0.73)
    );
    let at_lower = one_way(&instance, 20, true);
    let at_upper = one_way(&instance, 20, false);
    assert_eq!(profit_at(&instance, &at_lower, dec!(56.00)), dec!(-14.63));
    assert_eq!(profit_at(&instance, &at_upper, dec!(84.00)), dec!(-14.63));
}

#[test]
fn risk_ratio_falls_below_one_after_eighteen_levels() {
    let instance = app_instance();
    for down in [true, false] {
        for levels in 0..=17 {
            let state = one_way(&instance, levels, down);
            let mark = instance.level_price(state.empty_level());
            assert!(!risk_ratio_below_one(&instance, &state, mark));
            assert!(ratio_after(&instance, levels, down) >= Decimal::ONE);
        }
        let state = one_way(&instance, 18, down);
        let mark = instance.level_price(state.empty_level());
        assert!(risk_ratio_below_one(&instance, &state, mark));
        assert!(ratio_after(&instance, 18, down) < Decimal::ONE);
        assert_eq!(profit_at(&instance, &state, mark).round_dp(2), dec!(-11.78));
    }
    assert_eq!(instance.level_price(START_LEVEL - 18), dec!(57.40));
    assert_eq!(instance.level_price(START_LEVEL + 18), dec!(82.60));
    assert_eq!(instance.one_way_loss(18).round_dp(2), dec!(11.78));
}

#[test]
fn take_profit_of_one_percent_is_twenty_cents_or_two_point_six_pairs() {
    let instance = app_instance();
    let take_profit = instance.take_profit().expect("take-profit is set");
    assert_eq!(take_profit, dec!(0.2));
    assert_eq!(
        (take_profit / instance.pair_profit()).round_dp(1),
        dec!(2.6)
    );
    assert_eq!(instance.stop_loss(), None);
}

#[test]
fn the_binance_sizing_preset_gives_eleven_hundredths() {
    let instance = app_instance();
    let ladder = Ladder::of(&instance);
    let resources = reference::resources();
    let sum_without_empty =
        instance.sum_of_prices_below(START_LEVEL) + instance.sum_of_prices_above(START_LEVEL);
    assert_eq!(sum_without_empty, dec!(2800));
    let raw = binance_quantity(&ladder, START_LEVEL, &resources).expect("a ladder with orders");
    assert_eq!(raw, dec!(0.8) * dec!(20) * dec!(20) / dec!(2800));
    assert_eq!(raw.round_dp(4), dec!(0.1143));
    assert_eq!(
        binance_quantity_in_lots(&ladder, START_LEVEL, &resources, dec!(0.01)),
        Some(dec!(0.11))
    );
    assert_eq!(instance.quantity(), dec!(0.11));
}

#[test]
fn estimated_liquidation_prices_with_one_side_filled() {
    let instance = app_instance();
    let long = liquidation_price_long(&instance, START_LEVEL).expect("a long side");
    let short = liquidation_price_short(&instance, START_LEVEL).expect("a short side");
    assert_eq!(long.round_dp(2), dec!(53.83));
    assert_eq!(short.round_dp(2), dec!(86.01));
}

#[test]
fn profit_per_grid_runs_from_eighty_four_to_one_hundred_twenty_five_basis_points() {
    let (low, high) = profit_per_grid(&app_instance());
    assert_eq!(low, dec!(84) / dec!(83.3) - Decimal::ONE);
    assert_eq!((low * dec!(100)).round_dp(2), dec!(0.84));
    assert_eq!(high * dec!(100), dec!(1.25));
}

#[test]
fn leverage_five_with_margin_twenty_is_refused_at_a_ratio_of_fifty_nine_hundredths() {
    let instance = admitted(
        reference::theta(),
        Resources {
            margin: dec!(20),
            leverage: 5,
        },
    );
    let ratio = start_risk_ratio(&instance, START_LEVEL, reference::start_price())
        .expect("orders rest on the book");
    assert_eq!(ratio.round_dp(2), dec!(0.59));
    let refused = start(
        &instance,
        &program(),
        &Prices::flat(reference::start_price()),
        wallet_balance(),
    );
    assert_eq!(refused, Err(vec![Precondition::RiskRatioBelowOne]));
}

#[test]
fn leverage_five_with_margin_one_hundred_keeps_the_ratio_above_two_point_two_nine() {
    let instance = admitted(
        reference::theta(),
        Resources {
            margin: dec!(100),
            leverage: 5,
        },
    );
    let mut lowest = start_risk_ratio(&instance, START_LEVEL, reference::start_price())
        .expect("orders rest on the book");
    for down in [true, false] {
        for levels in 0..=20 {
            lowest = lowest.min(ratio_after(&instance, levels, down));
        }
    }
    assert!(lowest >= dec!(2.29));
    assert_eq!(lowest.round_dp(2), dec!(2.29));
    assert_eq!(lowest, ratio_after(&instance, 20, true));
    assert_eq!(
        (instance.loss_at_lower_bound(START_LEVEL) / instance.margin() * dec!(100)).round_dp(1),
        dec!(14.6)
    );
    assert_eq!(
        instance.loss_at_upper_bound(START_LEVEL),
        instance.loss_at_lower_bound(START_LEVEL)
    );
}

#[test]
fn backstop_triggers_for_a_start_value_of_one_hundred_and_a_cap_of_twenty_percent() {
    let instance = app_instance();
    let program = program();
    let budget = budget(&program, dec!(100));
    assert_eq!(budget, dec!(20));
    let stops = backstops(&instance, START_LEVEL, budget).expect("start level inside the ladder");
    assert_eq!(stops.sell_stop, dec!(53.56));
    assert_eq!(stops.buy_stop, dec!(86.44));
    assert_eq!(stops.low_distance, dec!(5.37) / dec!(2.2));
    assert_eq!(stops.high_distance, dec!(5.37) / dec!(2.2));
    let [sell, buy] = stops.orders();
    assert_eq!((sell.side, sell.trigger), (Side::Sell, dec!(53.56)));
    assert_eq!((buy.side, buy.trigger), (Side::Buy, dec!(86.44)));
}

#[test]
fn the_reference_start_passes_every_precondition_checked_without_an_account() {
    let instance = app_instance();
    let started = start(
        &instance,
        &program(),
        &Prices::flat(reference::start_price()),
        wallet_balance(),
    )
    .expect("start preconditions hold");
    assert_eq!(started.start_price, dec!(70.21));
    assert_eq!(started.state.start_level(), 20);
    assert_eq!(started.state.empty_level(), 20);
    assert_eq!(started.orders.len(), 40);
    assert!(started.state.ledger().is_empty());
    assert!(
        started
            .orders
            .iter()
            .all(|order| (order.price - dec!(70.21)).abs() >= instance.spacing() / dec!(2))
    );
}
