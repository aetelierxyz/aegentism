use aegent_mm_core::backstop::{backstops, budget};
use aegent_mm_core::estimates::{Ladder, binance_quantity, binance_quantity_in_lots};
use aegent_mm_core::params::{Instance, Resources, Theta};
use aegent_mm_core::program::Program;
use aegent_mm_core::quote::quote;
use aegent_mm_core::start::{Precondition, nearest_level, start};
use aegent_mm_core::state::State;
use aegent_mm_core::steps::ceil_quotient;
use aegent_mm_core::{Binding, Prices, Side, reference};
use proptest::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

#[derive(Debug, Clone)]
struct Case {
    instance: Instance,
    wallet: Decimal,
}

fn cases() -> impl Strategy<Value = Case> {
    (
        prop_oneof![Just(1i64), Just(5i64), Just(10i64)],
        prop_oneof![Just(1i64), Just(10i64), Just(1000i64)],
        500i64..=50_000,
        1i64..=300,
        2u32..=60,
        0i64..=400,
        1u32..=20,
        1i64..=5_000_000,
    )
        .prop_map(
            |(
                tick_cents,
                lot_mills,
                lower_ticks,
                spacing_ticks,
                grids,
                extra_lots,
                leverage,
                margin_cents,
            )| {
                let tick = Decimal::new(tick_cents, 2);
                let lot = Decimal::new(lot_mills, 3);
                let lower = tick * Decimal::from(lower_ticks);
                let upper = lower + tick * Decimal::from(spacing_ticks) * Decimal::from(grids);
                let binding = Binding {
                    tick_size: tick,
                    lot_step: lot,
                    ..reference::app_binding()
                };
                let least_lots = ceil_quotient(binding.min_notional, lower * lot);
                let quantity = lot * (least_lots + Decimal::from(extra_lots));
                let margin = Decimal::new(margin_cents, 2);
                let instance = Instance::admit(
                    binding,
                    Theta::new(lower, upper, grids, quantity),
                    Resources { margin, leverage },
                    margin,
                )
                .expect("admissible by construction");
                Case {
                    instance,
                    wallet: margin,
                }
            },
        )
}

fn price_near(instance: &Instance, selector: u32, on_half_tick: bool) -> Decimal {
    let tick = instance.binding().tick_size;
    let span = instance.upper() - instance.lower() + instance.spacing() * Decimal::TWO;
    let offset = Decimal::from(selector) % (span / tick + Decimal::ONE);
    let half = if on_half_tick {
        tick / Decimal::TWO
    } else {
        Decimal::ZERO
    };
    instance.lower() - instance.spacing() + offset * tick + half
}

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: 512,
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn levels_are_evenly_spaced_on_the_tick_from_the_lower_to_the_upper_bound(case in cases()) {
        let instance = &case.instance;
        let prices = instance.level_prices();
        let tick = instance.binding().tick_size;
        prop_assert_eq!(prices.len() as u32, instance.grids() + 1);
        prop_assert_eq!(prices[0], instance.lower());
        prop_assert_eq!(prices[instance.grids() as usize], instance.upper());
        prop_assert_eq!(
            instance.spacing() * Decimal::from(instance.grids()),
            instance.upper() - instance.lower()
        );
        prop_assert!((instance.spacing() % tick).is_zero());
        prop_assert!(prices.windows(2).all(|pair| pair[1] - pair[0] == instance.spacing()));
        prop_assert!(prices.iter().all(|price| (*price % tick).is_zero()));
    }

    #[test]
    fn sums_of_level_prices_match_their_terms(case in cases(), pick in any::<u32>()) {
        let instance = &case.instance;
        let level = pick % (instance.grids() + 1);
        let prices = instance.level_prices();
        let below: Decimal = prices[..level as usize].iter().sum();
        let above: Decimal = prices[level as usize + 1..].iter().sum();
        prop_assert_eq!(instance.sum_of_prices_below(level), below);
        prop_assert_eq!(instance.sum_of_prices_above(level), above);
    }

    #[test]
    fn the_nearest_level_minimises_the_distance_and_ties_go_down(
        case in cases(),
        selector in any::<u32>(),
        on_half_tick in any::<bool>(),
    ) {
        let instance = &case.instance;
        let price = price_near(instance, selector, on_half_tick);
        let chosen = nearest_level(instance, price);
        let distance = |level: u32| (instance.level_price(level) - price).abs();
        for level in 0..=instance.grids() {
            prop_assert!(distance(chosen) <= distance(level));
            if distance(level) == distance(chosen) {
                prop_assert!(chosen <= level);
            }
        }
    }

    #[test]
    fn the_loss_after_a_one_way_move_is_the_sum_over_the_lots_bought_on_the_way(
        case in cases(),
        pick in any::<u32>(),
    ) {
        let instance = &case.instance;
        let start_level = 1 + pick % (instance.grids() - 1);
        let quantity = instance.quantity();
        let down: Decimal = (0..start_level)
            .map(|level| quantity * (instance.level_price(level) - instance.lower()))
            .sum();
        let up: Decimal = (start_level + 1..=instance.grids())
            .map(|level| quantity * (instance.upper() - instance.level_price(level)))
            .sum();
        prop_assert_eq!(instance.loss_at_lower_bound(start_level), down);
        prop_assert_eq!(instance.loss_at_upper_bound(start_level), up);
        prop_assert_eq!(
            instance.largest_long(start_level),
            quantity * Decimal::from(start_level)
        );
        prop_assert_eq!(
            instance.largest_short(start_level),
            quantity * Decimal::from(instance.grids() - start_level)
        );
        prop_assert_eq!(
            instance.largest_open_lots(start_level),
            start_level.max(instance.grids() - start_level)
        );
    }

    #[test]
    fn backstop_triggers_are_the_nearest_ticks_inside_the_exact_distances(
        case in cases(),
        pick in any::<u32>(),
        budget_cents in 1i64..=2_000_000,
    ) {
        let instance = &case.instance;
        let start_level = 1 + pick % (instance.grids() - 1);
        let tick = instance.binding().tick_size;
        let budget = Decimal::new(budget_cents, 2);
        let stops = backstops(instance, start_level, budget).expect("inside the ladder");
        let long = instance.largest_long(start_level);
        let short = instance.largest_short(start_level);
        let low_slack = budget - instance.loss_at_lower_bound(start_level);
        let high_slack = budget - instance.loss_at_upper_bound(start_level);
        prop_assert!((stops.sell_stop % tick).is_zero());
        prop_assert!((stops.buy_stop % tick).is_zero());
        prop_assert!((instance.lower() - stops.sell_stop) * long <= low_slack);
        prop_assert!((instance.lower() - stops.sell_stop + tick) * long > low_slack);
        prop_assert!((stops.buy_stop - instance.upper()) * short <= high_slack);
        prop_assert!((stops.buy_stop + tick - instance.upper()) * short > high_slack);
        prop_assert_eq!(stops.low_distance > Decimal::ZERO, low_slack > Decimal::ZERO);
        prop_assert_eq!(stops.high_distance > Decimal::ZERO, high_slack > Decimal::ZERO);
        prop_assert_eq!(low_slack >= tick * long, stops.sell_stop < instance.lower());
        prop_assert_eq!(high_slack >= tick * short, stops.buy_stop > instance.upper());
        prop_assert_eq!(low_slack >= Decimal::ZERO, stops.sell_stop <= instance.lower());
        prop_assert_eq!(high_slack >= Decimal::ZERO, stops.buy_stop >= instance.upper());
    }

    #[test]
    fn the_binance_preset_divides_four_fifths_of_the_leveraged_margin_by_the_order_prices(
        case in cases(),
        pick in any::<u32>(),
    ) {
        let instance = &case.instance;
        let start_level = 1 + pick % (instance.grids() - 1);
        let lot = instance.binding().lot_step;
        let ladder = Ladder::of(instance);
        let resources = *instance.resources();
        let prices: Decimal = instance
            .level_prices()
            .iter()
            .enumerate()
            .filter(|(level, _)| *level as u32 != start_level)
            .map(|(_, price)| *price)
            .sum();
        let funds = dec!(0.8) * instance.margin() * instance.leverage();
        let raw = binance_quantity(&ladder, start_level, &resources).expect("orders exist");
        let lots = binance_quantity_in_lots(&ladder, start_level, &resources, lot)
            .expect("orders exist");
        prop_assert!((raw - funds / prices).abs() <= dec!(0.00000000000000000001));
        prop_assert!((lots % lot).is_zero());
        prop_assert!(lots * prices <= funds);
        prop_assert!((lots + lot) * prices > funds);
    }

    #[test]
    fn a_start_is_accepted_exactly_when_every_precondition_holds(
        case in cases(),
        selector in any::<u32>(),
        start_cents in 1i64..=2_000_000,
        cap_percent in 1i64..=99,
        value_cents in 1i64..=2_000_000,
        top in prop::option::of(any::<u32>()),
        bottom in prop::option::of(any::<u32>()),
    ) {
        let theta = Theta {
            stop_top: top.map(|pick| price_near(&case.instance, pick, false)),
            stop_bottom: bottom.map(|pick| price_near(&case.instance, pick, false)),
            ..case.instance.theta().clone()
        };
        let instance = Instance::admit(
            case.instance.binding().clone(),
            theta,
            *case.instance.resources(),
            case.wallet,
        )
        .expect("admissible by construction");
        let program = Program::new(
            Decimal::new(start_cents, 2),
            Decimal::new(cap_percent, 2),
            60_000,
            5_000,
            10_000,
        )
        .expect("loss cap inside (0, 1)");
        let wallet_value = Decimal::new(value_cents, 2);
        let price = price_near(&instance, selector, false);
        let level = nearest_level(&instance, price);
        let grids = instance.grids();
        let quantity = instance.quantity();
        let mut expected = Vec::new();
        if level == 0 || level == grids {
            expected.push(Precondition::EmptyLevelAtBound);
        } else {
            let prices = instance.level_prices();
            let bids: Decimal = prices[..level as usize].iter().map(|p| quantity * *p).sum();
            let asks: Decimal = prices[level as usize + 1..].iter().map(|p| quantity * *p).sum();
            if bids.max(asks) > instance.margin() * instance.leverage() {
                expected.push(Precondition::RiskRatioBelowOne);
            }
            let room = budget(&program, wallet_value);
            if room <= instance.loss_at_lower_bound(level) {
                expected.push(Precondition::LowBackstopDistanceNotPositive);
            }
            if room <= instance.loss_at_upper_bound(level) {
                expected.push(Precondition::HighBackstopDistanceNotPositive);
            }
        }
        if instance.theta().stop_top.is_some_and(|stop| price >= stop) {
            expected.push(Precondition::TriggerNotBelowStopTop);
        }
        if instance.theta().stop_bottom.is_some_and(|stop| price <= stop) {
            expected.push(Precondition::TriggerNotAboveStopBottom);
        }
        let outcome = start(&instance, &program, &Prices::flat(price), wallet_value);
        match outcome {
            Err(found) => prop_assert_eq!(found, expected),
            Ok(started) => {
                prop_assert!(expected.is_empty());
                prop_assert_eq!(started.state.start_level(), level);
                prop_assert_eq!(started.start_price, price);
                let initial = State::initial(&instance, level).expect("inside the ladder");
                prop_assert_eq!(&started.orders, &quote(&instance, &initial));
                let on_their_sides = started.orders.iter().all(|order| match order.side {
                    Side::Buy => order.price < price,
                    Side::Sell => order.price > price,
                });
                let half_a_spacing_away = started
                    .orders
                    .iter()
                    .all(|order| (order.price - price).abs() * Decimal::TWO >= instance.spacing());
                prop_assert!(on_their_sides);
                prop_assert!(half_a_spacing_away);
            }
        }
    }
}
