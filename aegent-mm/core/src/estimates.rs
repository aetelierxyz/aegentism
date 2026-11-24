use rust_decimal::Decimal;

use crate::numeric::floor_quotient;
use crate::params::{Instance, Resources};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ladder {
    pub lower: Decimal,
    pub upper: Decimal,
    pub grids: u32,
}

impl Ladder {
    pub fn of(instance: &Instance) -> Ladder {
        Ladder {
            lower: instance.lower(),
            upper: instance.upper(),
            grids: instance.grids(),
        }
    }
}

fn binance_ratio(
    ladder: &Ladder,
    start_level: u32,
    resources: &Resources,
) -> Option<(Decimal, Decimal)> {
    let grids = Decimal::from(ladder.grids);
    let doubled = Decimal::TWO * grids;
    let numerator =
        Decimal::new(8, 1) * resources.margin * Decimal::from(resources.leverage) * doubled;
    let denominator = grids * (grids + Decimal::ONE) * (ladder.lower + ladder.upper)
        - doubled * ladder.lower
        - Decimal::TWO * Decimal::from(start_level) * (ladder.upper - ladder.lower);
    (denominator > Decimal::ZERO).then_some((numerator, denominator))
}

pub fn binance_quantity(
    ladder: &Ladder,
    start_level: u32,
    resources: &Resources,
) -> Option<Decimal> {
    binance_ratio(ladder, start_level, resources)
        .map(|(numerator, denominator)| numerator / denominator)
}

pub fn binance_quantity_in_lots(
    ladder: &Ladder,
    start_level: u32,
    resources: &Resources,
    lot_step: Decimal,
) -> Option<Decimal> {
    if lot_step <= Decimal::ZERO {
        return None;
    }
    binance_ratio(ladder, start_level, resources).map(|(numerator, denominator)| {
        floor_quotient(numerator, denominator * lot_step) * lot_step
    })
}

pub fn liquidation_price_long(instance: &Instance, start_level: u32) -> Option<Decimal> {
    let size = instance.largest_long(start_level);
    let keep = Decimal::ONE - instance.binding().maintenance_margin_rate;
    let cost = instance.quantity() * instance.sum_of_prices_below(start_level);
    (size > Decimal::ZERO).then(|| (cost - instance.margin()) / (size * keep))
}

pub fn liquidation_price_short(instance: &Instance, start_level: u32) -> Option<Decimal> {
    let size = instance.largest_short(start_level);
    let keep = Decimal::ONE + instance.binding().maintenance_margin_rate;
    let proceeds = instance.quantity() * instance.sum_of_prices_above(start_level);
    (size > Decimal::ZERO).then(|| (instance.margin() + proceeds) / (size * keep))
}

pub fn profit_per_grid(instance: &Instance) -> (Decimal, Decimal) {
    let spacing = instance.spacing();
    (
        spacing / (instance.upper() - spacing),
        spacing / instance.lower(),
    )
}
