use aegent_mm_core::Side;
use rust_decimal::Decimal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fill {
    pub time_ms: u64,
    pub level: u32,
    pub side: Side,
    pub price: Decimal,
    pub quantity: Decimal,
    pub maker: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bar {
    pub time_ms: u64,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
}

impl Bar {
    pub fn is_consistent(&self) -> bool {
        self.low > Decimal::ZERO
            && self.low <= self.open.min(self.close)
            && self.high >= self.open.max(self.close)
    }

    pub fn extremes_in_path_order(&self) -> [Decimal; 2] {
        if self.close >= self.open {
            [self.low, self.high]
        } else {
            [self.high, self.low]
        }
    }
}
