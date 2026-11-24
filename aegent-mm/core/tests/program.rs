mod common;

use aegent_mm_core::Side;
use aegent_mm_core::program::{
    Checkpoint, DAY_MS, Guard, StartRefusal, checkpoint, next_checkpoint_ms, start_allowed,
};
use aegent_mm_core::stop::{ClosingAction, guard_actions};
use common::program;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

#[test]
fn the_guard_trips_below_the_floor_and_stays_tripped_until_reset() {
    let program = program();
    let mut guard = Guard::new();
    assert!(!guard.is_tripped());
    assert!(!guard.observe(&program, dec!(80)));
    assert!(!guard.observe(&program, dec!(120)));
    assert!(guard.observe(&program, dec!(79.99)));
    assert!(guard.is_tripped());
    assert!(guard.observe(&program, dec!(150)));
    assert!(guard.is_tripped());
    guard.reset();
    assert!(!guard.is_tripped());
    assert!(!guard.observe(&program, dec!(150)));
}

#[test]
fn the_floor_is_the_start_value_less_the_loss_cap() {
    let program = program();
    assert_eq!(program.floor(), dec!(80));
    assert!(program.guard_holds(dec!(79.999999)));
    assert!(!program.guard_holds(dec!(80)));
}

#[test]
fn a_start_needs_no_active_run_and_an_untripped_guard() {
    let program = program();
    let clear = Guard::new();
    let mut tripped = Guard::new();
    tripped.observe(&program, dec!(10));
    assert_eq!(start_allowed(false, &clear), Ok(()));
    assert_eq!(
        start_allowed(true, &clear),
        Err(vec![StartRefusal::RunActive])
    );
    assert_eq!(
        start_allowed(false, &tripped),
        Err(vec![StartRefusal::GuardTripped])
    );
    assert_eq!(
        start_allowed(true, &tripped),
        Err(vec![StartRefusal::RunActive, StartRefusal::GuardTripped])
    );
    tripped.reset();
    assert_eq!(start_allowed(false, &tripped), Ok(()));
}

#[test]
fn the_guard_cancels_every_order_and_closes_the_position_with_or_without_a_run() {
    assert_eq!(
        guard_actions(dec!(0.33)),
        vec![
            ClosingAction::CancelAllOrders,
            ClosingAction::ReducePosition {
                side: Side::Sell,
                quantity: dec!(0.33),
            },
            ClosingAction::VerifyNoRestingOrders,
            ClosingAction::VerifyFlat,
        ]
    );
    assert_eq!(
        guard_actions(dec!(-1.1)),
        vec![
            ClosingAction::CancelAllOrders,
            ClosingAction::ReducePosition {
                side: Side::Buy,
                quantity: dec!(1.1),
            },
            ClosingAction::VerifyNoRestingOrders,
            ClosingAction::VerifyFlat,
        ]
    );
    assert_eq!(
        guard_actions(Decimal::ZERO),
        vec![
            ClosingAction::CancelAllOrders,
            ClosingAction::VerifyNoRestingOrders,
            ClosingAction::VerifyFlat,
        ]
    );
}

#[test]
fn checkpoints_fall_on_every_midnight_utc() {
    let midnight = 1_795_478_400_000;
    assert_eq!(midnight % DAY_MS, 0);
    assert_eq!(next_checkpoint_ms(midnight - 1), midnight);
    assert_eq!(next_checkpoint_ms(midnight), midnight + DAY_MS);
    assert_eq!(next_checkpoint_ms(midnight + 1), midnight + DAY_MS);
    assert_eq!(next_checkpoint_ms(0), DAY_MS);
}

#[test]
fn a_checkpoint_records_the_wallet_value_and_whether_it_reaches_the_start_value() {
    let program = program();
    assert_eq!(
        checkpoint(&program, 86_400_000, dec!(100)),
        Checkpoint {
            time_ms: 86_400_000,
            wallet_value: dec!(100),
            at_or_above_start: true,
        }
    );
    assert!(checkpoint(&program, 86_400_000, dec!(103.2)).at_or_above_start);
    assert!(!checkpoint(&program, 86_400_000, dec!(99.99)).at_or_above_start);
}
