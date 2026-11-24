use rust_decimal::Decimal;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    pub fn opposite(self) -> Side {
        match self {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeRecord {
    pub time_ms: u64,
    pub level: u32,
    pub side: Side,
    pub quantity: Decimal,
    pub price: Decimal,
    pub fee: Decimal,
    pub maker: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FundingRecord {
    pub time_ms: u64,
    pub amount: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Trade(TradeRecord),
    Funding(FundingRecord),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ledger {
    entries: Vec<Entry>,
    cash: Decimal,
    fees: Decimal,
    funding: Decimal,
    bought: Decimal,
    sold: Decimal,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger::default()
    }

    pub fn record_trade(&mut self, trade: TradeRecord) {
        let notional = trade.quantity * trade.price;
        match trade.side {
            Side::Buy => {
                self.cash -= notional;
                self.bought += trade.quantity;
            }
            Side::Sell => {
                self.cash += notional;
                self.sold += trade.quantity;
            }
        }
        self.fees += trade.fee;
        self.entries.push(Entry::Trade(trade));
    }

    pub fn record_funding(&mut self, payment: FundingRecord) {
        self.funding += payment.amount;
        self.entries.push(Entry::Funding(payment));
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn trades(&self) -> impl Iterator<Item = &TradeRecord> {
        self.entries.iter().filter_map(|entry| match entry {
            Entry::Trade(trade) => Some(trade),
            Entry::Funding(_) => None,
        })
    }

    pub fn funding_payments(&self) -> impl Iterator<Item = &FundingRecord> {
        self.entries.iter().filter_map(|entry| match entry {
            Entry::Trade(_) => None,
            Entry::Funding(payment) => Some(payment),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn cash(&self) -> Decimal {
        self.cash
    }

    pub fn fees(&self) -> Decimal {
        self.fees
    }

    pub fn funding(&self) -> Decimal {
        self.funding
    }

    pub fn bought(&self) -> Decimal {
        self.bought
    }

    pub fn sold(&self) -> Decimal {
        self.sold
    }

    pub fn net_quantity(&self) -> Decimal {
        self.bought - self.sold
    }
}
