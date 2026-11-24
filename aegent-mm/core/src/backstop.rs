use rust_decimal::Decimal;

use crate::ledger::Side;
use crate::numeric::{ceil_quotient, floor_quotient};
use crate::params::Instance;
use crate::program::Program;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackstopOrder {
    pub side: Side,
    pub trigger: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backstops {
    pub budget: Decimal,
    pub low_distance: Decimal,
    pub high_distance: Decimal,
    pub sell_stop: Decimal,
    pub buy_stop: Decimal,
}

impl Backstops {
    pub fn orders(&self) -> [BackstopOrder; 2] {
        [
            BackstopOrder {
                side: Side::Sell,
                trigger: self.sell_stop,
            },
            BackstopOrder {
                side: Side::Buy,
                trigger: self.buy_stop,
            },
        ]
    }
}

pub fn budget(program: &Program, wallet_value: Decimal) -> Decimal {
    wallet_value - program.floor()
}

pub fn low_slack(instance: &Instance, start_level: u32, budget: Decimal) -> Decimal {
    budget - instance.loss_at_lower_bound(start_level)
}

pub fn high_slack(instance: &Instance, start_level: u32, budget: Decimal) -> Decimal {
    budget - instance.loss_at_upper_bound(start_level)
}

pub fn backstops(instance: &Instance, start_level: u32, budget: Decimal) -> Option<Backstops> {
    if start_level < 1 || start_level >= instance.grids() {
        return None;
    }
    let tick = instance.binding().tick_size;
    let long = instance.largest_long(start_level);
    let short = instance.largest_short(start_level);
    let low = low_slack(instance, start_level, budget);
    let high = high_slack(instance, start_level, budget);
    let sell_stop = ceil_quotient(instance.lower() * long - low, long * tick) * tick;
    let buy_stop = floor_quotient(instance.upper() * short + high, short * tick) * tick;
    Some(Backstops {
        budget,
        low_distance: low / long,
        high_distance: high / short,
        sell_stop,
        buy_stop,
    })
}
