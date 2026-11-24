use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use crate::backstop::{Backstops, backstops, budget, high_slack, low_slack};
use crate::event::Prices;
use crate::numeric::floor_quotient;
use crate::params::Instance;
use crate::program::Program;
use crate::quote::{Order, quote};
use crate::state::State;
use crate::stop::{risk_ratio, risk_ratio_below_one};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Precondition {
    EmptyLevelAtBound,
    RiskRatioBelowOne,
    LowBackstopDistanceNotPositive,
    HighBackstopDistanceNotPositive,
    TriggerNotBelowStopTop,
    TriggerNotAboveStopBottom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Start {
    pub state: State,
    pub start_price: Decimal,
    pub orders: Vec<Order>,
    pub backstops: Backstops,
}

pub fn nearest_level(instance: &Instance, price: Decimal) -> u32 {
    let grids = instance.grids();
    let offset = price - instance.lower();
    if offset <= Decimal::ZERO {
        return 0;
    }
    let below = floor_quotient(offset, instance.spacing());
    if below >= Decimal::from(grids) {
        return grids;
    }
    let remainder = offset - below * instance.spacing();
    let nearest = if remainder * Decimal::TWO > instance.spacing() {
        below + Decimal::ONE
    } else {
        below
    };
    nearest.to_u32().unwrap_or(grids)
}

pub fn start_risk_ratio(instance: &Instance, start_level: u32, mark: Decimal) -> Option<Decimal> {
    State::initial(instance, start_level).and_then(|state| risk_ratio(instance, &state, mark))
}

pub fn start(
    instance: &Instance,
    program: &Program,
    prices: &Prices,
    wallet_value: Decimal,
) -> Result<Start, Vec<Precondition>> {
    let theta = instance.theta();
    let start_price = prices.last;
    let start_level = nearest_level(instance, start_price);
    let mut found = Vec::new();
    let state = State::initial(instance, start_level);
    let room = budget(program, wallet_value);
    let stops = backstops(instance, start_level, room);
    if state.is_none() {
        found.push(Precondition::EmptyLevelAtBound);
    }
    if state
        .as_ref()
        .is_some_and(|state| risk_ratio_below_one(instance, state, prices.mark))
    {
        found.push(Precondition::RiskRatioBelowOne);
    }
    if stops.is_some() && low_slack(instance, start_level, room) <= Decimal::ZERO {
        found.push(Precondition::LowBackstopDistanceNotPositive);
    }
    if stops.is_some() && high_slack(instance, start_level, room) <= Decimal::ZERO {
        found.push(Precondition::HighBackstopDistanceNotPositive);
    }
    let trigger = prices.trigger(theta.trigger);
    if theta.stop_top.is_some_and(|top| trigger >= top) {
        found.push(Precondition::TriggerNotBelowStopTop);
    }
    if theta.stop_bottom.is_some_and(|bottom| trigger <= bottom) {
        found.push(Precondition::TriggerNotAboveStopBottom);
    }
    match (state, stops) {
        (Some(state), Some(backstops)) if found.is_empty() => {
            let orders = quote(instance, &state);
            Ok(Start {
                state,
                start_price,
                orders,
                backstops,
            })
        }
        _ => Err(found),
    }
}
