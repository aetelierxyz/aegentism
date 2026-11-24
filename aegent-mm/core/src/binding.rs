use rust_decimal::Decimal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub symbol: String,
    pub tick_size: Decimal,
    pub lot_step: Decimal,
    pub min_notional: Decimal,
    pub max_resting_orders: u32,
    pub maker_fee: Decimal,
    pub taker_fee: Decimal,
    pub maintenance_margin_rate: Decimal,
    pub leverage_cap: u32,
    pub price_band_up: Decimal,
    pub price_band_down: Decimal,
    pub funding_interval_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindingError {
    TickSizeNotPositive,
    LotStepNotPositive,
    MinNotionalNegative,
    MaintenanceMarginRateOutOfRange,
}

impl Binding {
    pub fn check(&self) -> Result<(), Vec<BindingError>> {
        let mut found = Vec::new();
        if self.tick_size <= Decimal::ZERO {
            found.push(BindingError::TickSizeNotPositive);
        }
        if self.lot_step <= Decimal::ZERO {
            found.push(BindingError::LotStepNotPositive);
        }
        if self.min_notional < Decimal::ZERO {
            found.push(BindingError::MinNotionalNegative);
        }
        if self.maintenance_margin_rate < Decimal::ZERO
            || self.maintenance_margin_rate >= Decimal::ONE
        {
            found.push(BindingError::MaintenanceMarginRateOutOfRange);
        }
        if found.is_empty() { Ok(()) } else { Err(found) }
    }
}
