use rust_decimal::Decimal;

use crate::ledger::Side;
use crate::params::Instance;
use crate::state::State;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    pub level: u32,
    pub side: Side,
    pub quantity: Decimal,
    pub price: Decimal,
}

impl Order {
    pub fn notional(&self) -> Decimal {
        self.quantity * self.price
    }
}

pub fn quote(instance: &Instance, state: &State) -> Vec<Order> {
    let empty = state.empty_level();
    (0..=instance.grids())
        .filter(|level| *level != empty)
        .map(|level| Order {
            level,
            side: if level < empty { Side::Buy } else { Side::Sell },
            quantity: instance.quantity() - state.filled(level),
            price: instance.level_price(level),
        })
        .collect()
}

pub fn resting_notional(instance: &Instance, state: &State) -> (Decimal, Decimal) {
    let mut bids = Decimal::ZERO;
    let mut asks = Decimal::ZERO;
    for order in quote(instance, state) {
        match order.side {
            Side::Buy => bids += order.notional(),
            Side::Sell => asks += order.notional(),
        }
    }
    (bids, asks)
}
