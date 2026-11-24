mod common;

use aegent_mm_core::binding::{Binding, BindingError};
use aegent_mm_core::params::{Inadmissible, Instance, Resources, Target, Theta, Trigger};
use aegent_mm_core::program::{Program, ProgramError};
use aegent_mm_core::reference;
use common::wallet_balance;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn refusal(binding: Binding, theta: Theta, resources: Resources) -> Vec<Inadmissible> {
    Instance::admit(binding, theta, resources, wallet_balance())
        .expect_err("the instance is inadmissible")
}

fn refusal_of_theta(theta: Theta) -> Vec<Inadmissible> {
    refusal(reference::app_binding(), theta, reference::resources())
}

fn refusal_of_resources(resources: Resources) -> Vec<Inadmissible> {
    refusal(reference::app_binding(), reference::theta(), resources)
}

fn refusal_of_binding(binding: Binding) -> Vec<Inadmissible> {
    refusal(binding, reference::theta(), reference::resources())
}

#[test]
fn the_reference_parameters_are_admissible() {
    let instance = Instance::admit(
        reference::app_binding(),
        reference::theta(),
        reference::resources(),
        wallet_balance(),
    )
    .expect("admissible");
    assert_eq!(instance.grids(), 40);
    assert_eq!(instance.quantity(), dec!(0.11));
    assert_eq!(instance.margin(), dec!(20));
    assert_eq!(instance.leverage(), dec!(20));
    assert_eq!(instance.theta().trigger, Trigger::Last);
    assert!(instance.theta().close_tp_sl);
    assert!(instance.theta().close_risk);
}

#[test]
fn defaults_are_trigger_last_and_both_closes_on() {
    let theta = Theta::new(dec!(56), dec!(84), 40, dec!(0.11));
    assert_eq!(theta.trigger, Trigger::Last);
    assert_eq!(Trigger::default(), Trigger::Last);
    assert!(theta.close_tp_sl);
    assert!(theta.close_risk);
    assert_eq!(theta.take_profit, None);
    assert_eq!(theta.stop_loss, None);
    assert_eq!(theta.stop_top, None);
    assert_eq!(theta.stop_bottom, None);
}

#[test]
fn lower_bound_must_be_below_upper_bound() {
    let equal = Theta {
        upper: dec!(56),
        ..reference::theta()
    };
    let inverted = Theta {
        upper: dec!(42),
        ..reference::theta()
    };
    assert_eq!(refusal_of_theta(equal), vec![Inadmissible::RangeEmpty]);
    assert_eq!(refusal_of_theta(inverted), vec![Inadmissible::RangeEmpty]);
}

#[test]
fn tick_must_divide_the_lower_bound() {
    let theta = Theta {
        lower: dec!(56.005),
        upper: dec!(84.005),
        ..reference::theta()
    };
    assert_eq!(refusal_of_theta(theta), vec![Inadmissible::LowerOffTick]);
}

#[test]
fn at_least_two_grids() {
    let one = Theta {
        grids: 1,
        ..reference::theta()
    };
    let none = Theta {
        grids: 0,
        ..reference::theta()
    };
    assert_eq!(refusal_of_theta(one), vec![Inadmissible::TooFewGrids]);
    assert_eq!(refusal_of_theta(none), vec![Inadmissible::TooFewGrids]);
}

#[test]
fn two_places_stay_free_for_the_backstop_orders() {
    let most = Theta {
        lower: dec!(50),
        upper: dec!(248),
        grids: 198,
        ..reference::theta()
    };
    let too_many = Theta {
        lower: dec!(50),
        upper: dec!(249),
        grids: 199,
        ..reference::theta()
    };
    assert!(
        Instance::admit(
            reference::app_binding(),
            most,
            reference::resources(),
            wallet_balance()
        )
        .is_ok()
    );
    assert_eq!(refusal_of_theta(too_many), vec![Inadmissible::TooManyGrids]);
}

#[test]
fn tick_must_divide_the_spacing() {
    let theta = Theta {
        grids: 3,
        ..reference::theta()
    };
    let half_tick = Theta {
        upper: dec!(84.20),
        ..reference::theta()
    };
    assert_eq!(refusal_of_theta(theta), vec![Inadmissible::SpacingOffTick]);
    assert_eq!(
        refusal_of_theta(half_tick),
        vec![Inadmissible::SpacingOffTick]
    );
}

#[test]
fn lot_step_must_divide_the_quantity() {
    let theta = Theta {
        quantity: dec!(0.115),
        ..reference::theta()
    };
    assert_eq!(
        refusal_of_theta(theta),
        vec![Inadmissible::QuantityOffLotStep]
    );
}

#[test]
fn the_lowest_order_must_reach_the_minimum_notional() {
    let below = Theta {
        quantity: dec!(0.08),
        ..reference::theta()
    };
    let lowest_admissible = Theta {
        quantity: dec!(0.09),
        ..reference::theta()
    };
    assert_eq!(dec!(0.08) * dec!(56), dec!(4.48));
    assert_eq!(dec!(0.09) * dec!(56), dec!(5.04));
    assert_eq!(
        refusal_of_theta(below),
        vec![Inadmissible::BelowMinNotional]
    );
    assert!(
        Instance::admit(
            reference::app_binding(),
            lowest_admissible,
            reference::resources(),
            wallet_balance()
        )
        .is_ok()
    );
}

#[test]
fn take_profit_must_be_positive() {
    for target in [
        Target::Amount(dec!(0)),
        Target::Amount(dec!(-0.2)),
        Target::Roi(dec!(0)),
        Target::Roi(dec!(-0.01)),
    ] {
        let theta = Theta {
            take_profit: Some(target),
            ..reference::theta()
        };
        assert_eq!(
            refusal_of_theta(theta),
            vec![Inadmissible::TakeProfitNotPositive]
        );
    }
}

#[test]
fn stop_loss_must_be_negative() {
    for target in [
        Target::Amount(dec!(0)),
        Target::Amount(dec!(3)),
        Target::Roi(dec!(0)),
        Target::Roi(dec!(0.1)),
    ] {
        let theta = Theta {
            stop_loss: Some(target),
            ..reference::theta()
        };
        assert_eq!(
            refusal_of_theta(theta),
            vec![Inadmissible::StopLossNotNegative]
        );
    }
}

#[test]
fn targets_given_as_roi_convert_with_the_margin() {
    let theta = Theta {
        take_profit: Some(Target::Roi(dec!(0.01))),
        stop_loss: Some(Target::Roi(dec!(-0.25))),
        ..reference::theta()
    };
    let instance = Instance::admit(
        reference::app_binding(),
        theta,
        reference::resources(),
        wallet_balance(),
    )
    .expect("admissible");
    assert_eq!(instance.take_profit(), Some(dec!(0.2)));
    assert_eq!(instance.stop_loss(), Some(dec!(-5)));
}

#[test]
fn targets_given_as_amounts_are_kept() {
    let theta = Theta {
        take_profit: Some(Target::Amount(dec!(1.5))),
        stop_loss: Some(Target::Amount(dec!(-4))),
        ..reference::theta()
    };
    let instance = Instance::admit(
        reference::app_binding(),
        theta,
        reference::resources(),
        wallet_balance(),
    )
    .expect("admissible");
    assert_eq!(instance.take_profit(), Some(dec!(1.5)));
    assert_eq!(instance.stop_loss(), Some(dec!(-4)));
}

#[test]
fn margin_must_be_positive() {
    for margin in [dec!(0), dec!(-20)] {
        let resources = Resources {
            margin,
            leverage: 20,
        };
        assert_eq!(
            refusal_of_resources(resources),
            vec![Inadmissible::MarginNotPositive]
        );
    }
}

#[test]
fn margin_must_not_exceed_the_wallet_balance() {
    let at_balance = Resources {
        margin: wallet_balance(),
        leverage: 20,
    };
    let above = Resources {
        margin: wallet_balance() + dec!(0.01),
        leverage: 20,
    };
    assert!(
        Instance::admit(
            reference::app_binding(),
            reference::theta(),
            at_balance,
            wallet_balance()
        )
        .is_ok()
    );
    assert_eq!(
        refusal_of_resources(above),
        vec![Inadmissible::MarginAboveWallet]
    );
}

#[test]
fn leverage_must_be_at_least_one() {
    let resources = Resources {
        margin: dec!(20),
        leverage: 0,
    };
    assert_eq!(
        refusal_of_resources(resources),
        vec![Inadmissible::LeverageBelowOne]
    );
}

#[test]
fn leverage_must_not_exceed_the_cap() {
    let resources = Resources {
        margin: dec!(20),
        leverage: 21,
    };
    assert_eq!(
        refusal_of_resources(resources),
        vec![Inadmissible::LeverageAboveCap]
    );
    assert_eq!(
        refusal(
            reference::binding(),
            reference::theta(),
            Resources {
                margin: dec!(20),
                leverage: 6,
            }
        ),
        vec![Inadmissible::LeverageAboveCap]
    );
}

#[test]
fn every_violation_is_reported_together() {
    let theta = Theta {
        lower: dec!(56.005),
        upper: dec!(84),
        grids: 1,
        quantity: dec!(0.015),
        take_profit: Some(Target::Amount(dec!(-1))),
        stop_loss: Some(Target::Amount(dec!(1))),
        ..reference::theta()
    };
    let resources = Resources {
        margin: dec!(500),
        leverage: 50,
    };
    assert_eq!(
        refusal(reference::app_binding(), theta, resources),
        vec![
            Inadmissible::LowerOffTick,
            Inadmissible::TooFewGrids,
            Inadmissible::SpacingOffTick,
            Inadmissible::QuantityOffLotStep,
            Inadmissible::BelowMinNotional,
            Inadmissible::TakeProfitNotPositive,
            Inadmissible::StopLossNotNegative,
            Inadmissible::MarginAboveWallet,
            Inadmissible::LeverageAboveCap,
        ]
    );
}

#[test]
fn a_negative_ladder_is_refused_although_its_lowest_notional_passes_the_minimum() {
    let theta = Theta {
        lower: dec!(-84),
        upper: dec!(-56),
        quantity: dec!(-0.11),
        ..plain()
    };
    assert!(theta.lower < theta.upper);
    assert!(theta.quantity * theta.lower >= reference::binding().min_notional);
    assert_eq!(
        refusal_of_theta(theta),
        vec![
            Inadmissible::LowerNotPositive,
            Inadmissible::QuantityNotPositive
        ]
    );
}

fn plain() -> Theta {
    Theta {
        take_profit: None,
        ..reference::theta()
    }
}

#[test]
fn a_binding_without_a_positive_tick_or_lot_step_is_refused() {
    let no_tick = Binding {
        tick_size: Decimal::ZERO,
        ..reference::app_binding()
    };
    let no_lot = Binding {
        lot_step: dec!(-0.01),
        ..reference::app_binding()
    };
    assert_eq!(
        refusal_of_binding(no_tick),
        vec![Inadmissible::Binding(BindingError::TickSizeNotPositive)]
    );
    assert_eq!(
        refusal_of_binding(no_lot),
        vec![Inadmissible::Binding(BindingError::LotStepNotPositive)]
    );
}

#[test]
fn a_binding_with_a_negative_minimum_notional_or_a_margin_rate_outside_the_unit_interval_is_refused()
 {
    let negative_notional = Binding {
        min_notional: dec!(-5),
        ..reference::app_binding()
    };
    let full_rate = Binding {
        maintenance_margin_rate: Decimal::ONE,
        ..reference::app_binding()
    };
    let negative_rate = Binding {
        maintenance_margin_rate: dec!(-0.005),
        ..reference::app_binding()
    };
    assert_eq!(
        refusal_of_binding(negative_notional),
        vec![Inadmissible::Binding(BindingError::MinNotionalNegative)]
    );
    assert_eq!(
        refusal_of_binding(full_rate),
        vec![Inadmissible::Binding(
            BindingError::MaintenanceMarginRateOutOfRange
        )]
    );
    assert_eq!(
        refusal_of_binding(negative_rate),
        vec![Inadmissible::Binding(
            BindingError::MaintenanceMarginRateOutOfRange
        )]
    );
}

#[test]
fn fee_rates_never_prevent_a_start() {
    let binding = Binding {
        maker_fee: dec!(0.5),
        taker_fee: dec!(-0.75),
        ..reference::app_binding()
    };
    assert!(
        Instance::admit(
            binding,
            reference::theta(),
            reference::resources(),
            wallet_balance()
        )
        .is_ok()
    );
}

#[test]
fn the_reference_binding_carries_the_contract_rules_of_solusdc() {
    let binding = reference::binding();
    assert_eq!(binding.symbol, "SOLUSDC");
    assert_eq!(binding.tick_size, dec!(0.01));
    assert_eq!(binding.lot_step, dec!(0.01));
    assert_eq!(binding.min_notional, dec!(5));
    assert_eq!(binding.max_resting_orders, 200);
    assert_eq!(binding.maker_fee, dec!(0));
    assert_eq!(binding.taker_fee, dec!(0.0004));
    assert_eq!(binding.maintenance_margin_rate, dec!(0.005));
    assert_eq!(binding.leverage_cap, 5);
    assert_eq!(binding.price_band_up, dec!(1.05));
    assert_eq!(binding.price_band_down, dec!(0.95));
    assert_eq!(binding.funding_interval_ms, 8 * 60 * 60 * 1000);
    assert_eq!(binding.check(), Ok(()));
}

#[test]
fn the_loss_cap_lies_strictly_between_zero_and_one() {
    for cap in [dec!(0), dec!(1), dec!(-0.2), dec!(1.5)] {
        assert_eq!(
            Program::new(dec!(100), cap, 60_000, 5_000, 10_000),
            Err(ProgramError::LossCapOutOfRange)
        );
    }
    let program = Program::new(dec!(100), dec!(0.2), 60_000, 5_000, 10_000).expect("valid");
    assert_eq!(program.floor(), dec!(80));
    assert_eq!(program.start_value(), dec!(100));
    assert_eq!(program.loss_cap(), dec!(0.2));
    assert_eq!(program.dead_man_ms(), 60_000);
    assert_eq!(program.dead_man_renewal_ms(), 20_000);
    assert_eq!(program.data_silence_ms(), 5_000);
    assert_eq!(program.stream_loss_ms(), 10_000);
}
