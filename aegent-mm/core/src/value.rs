use std::collections::BTreeMap;

use rust_decimal::Decimal;

use crate::event::Prices;
use crate::ledger::{Ledger, Side};
use crate::params::Instance;
use crate::state::State;

pub fn profit_at(instance: &Instance, state: &State, price: Decimal) -> Decimal {
    let ledger = state.ledger();
    ledger.cash() + state.position(instance.quantity()) * price - ledger.fees() + ledger.funding()
}

pub fn profit_last(instance: &Instance, state: &State, prices: &Prices) -> Decimal {
    profit_at(instance, state, prices.last)
}

pub fn profit_mark(instance: &Instance, state: &State, prices: &Prices) -> Decimal {
    profit_at(instance, state, prices.mark)
}

pub fn profit_trigger(instance: &Instance, state: &State, prices: &Prices) -> Decimal {
    profit_at(instance, state, prices.trigger(instance.theta().trigger))
}

pub fn roi(instance: &Instance, state: &State, prices: &Prices) -> Decimal {
    profit_last(instance, state, prices) / instance.margin()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedOrder {
    pub level: u32,
    pub side: Side,
    pub quantity: Decimal,
    pub notional: Decimal,
    pub fees: Decimal,
    pub completed_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    pub buy: CompletedOrder,
    pub sell: CompletedOrder,
}

impl Pair {
    pub fn profit(&self) -> Decimal {
        self.sell.notional - self.buy.notional - self.buy.fees - self.sell.fees
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pairing {
    pub pairs: Vec<Pair>,
    pub unpaired: Vec<CompletedOrder>,
    pub completed: u64,
}

impl Pairing {
    pub fn matched(&self) -> Decimal {
        self.pairs.iter().map(Pair::profit).sum()
    }
}

struct Accumulating {
    side: Side,
    quantity: Decimal,
    notional: Decimal,
    fees: Decimal,
}

pub fn pairing(ledger: &Ledger, quantity: Decimal) -> Pairing {
    let mut result = Pairing::default();
    let mut partial: BTreeMap<u32, Accumulating> = BTreeMap::new();
    for trade in ledger.trades() {
        let order = partial.entry(trade.level).or_insert(Accumulating {
            side: trade.side,
            quantity: Decimal::ZERO,
            notional: Decimal::ZERO,
            fees: Decimal::ZERO,
        });
        order.quantity += trade.quantity;
        order.notional += trade.quantity * trade.price;
        order.fees += trade.fee;
        if order.quantity < quantity {
            continue;
        }
        let completed = CompletedOrder {
            level: trade.level,
            side: order.side,
            quantity: order.quantity,
            notional: order.notional,
            fees: order.fees,
            completed_ms: trade.time_ms,
        };
        partial.remove(&trade.level);
        result.completed += 1;
        let reduces = result
            .unpaired
            .last()
            .is_some_and(|open| open.side != completed.side);
        if !reduces {
            result.unpaired.push(completed);
            continue;
        }
        if let Some(open) = result.unpaired.pop() {
            let pair = match completed.side {
                Side::Buy => Pair {
                    buy: completed,
                    sell: open,
                },
                Side::Sell => Pair {
                    buy: open,
                    sell: completed,
                },
            };
            result.pairs.push(pair);
        }
    }
    result
}

pub fn matched(instance: &Instance, state: &State) -> Decimal {
    pairing(state.ledger(), instance.quantity()).matched()
}

pub fn unmatched(instance: &Instance, state: &State, prices: &Prices) -> Decimal {
    profit_last(instance, state, prices) - matched(instance, state) - state.ledger().funding()
}

pub fn closing_price(position: Decimal, prices: &Prices) -> Decimal {
    if position < Decimal::ZERO {
        prices.ask
    } else {
        prices.bid
    }
}

pub fn wallet_value(
    wallet_balance: Decimal,
    entry_price: Decimal,
    position: Decimal,
    prices: &Prices,
    taker_fee: Decimal,
) -> Decimal {
    if position.is_zero() {
        return wallet_balance;
    }
    let exit_price = closing_price(position, prices);
    wallet_balance + position * (exit_price - entry_price) - taker_fee * position.abs() * exit_price
}

pub fn run_wallet_value(
    instance: &Instance,
    state: &State,
    balance_at_start: Decimal,
    prices: &Prices,
) -> Decimal {
    let position = state.position(instance.quantity());
    let exit_price = closing_price(position, prices);
    balance_at_start + profit_at(instance, state, exit_price)
        - instance.binding().taker_fee * position.abs() * exit_price
}
