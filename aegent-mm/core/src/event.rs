use rust_decimal::Decimal;

use crate::params::Trigger;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prices {
    pub last: Decimal,
    pub mark: Decimal,
    pub bid: Decimal,
    pub ask: Decimal,
}

impl Prices {
    pub fn flat(price: Decimal) -> Prices {
        Prices {
            last: price,
            mark: price,
            bid: price,
            ask: price,
        }
    }

    pub fn trigger(&self, trigger: Trigger) -> Decimal {
        match trigger {
            Trigger::Last => self.last,
            Trigger::Mark => self.mark,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    Trade {
        level: u32,
        quantity: Decimal,
        price: Decimal,
        fee: Decimal,
        maker: bool,
    },
    Price(Prices),
    Funding {
        amount: Decimal,
    },
    Reject {
        level: u32,
    },
    Backstop,
    Command {
        keep: bool,
    },
    Tick,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub time_ms: u64,
    pub kind: EventKind,
}

impl Event {
    pub fn at(time_ms: u64, kind: EventKind) -> Event {
        Event { time_ms, kind }
    }
}
