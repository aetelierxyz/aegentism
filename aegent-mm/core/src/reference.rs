use rust_decimal::Decimal;

use crate::binding::Binding;
use crate::params::{Resources, Target, Theta, Trigger};

pub const SYMBOL: &str = "SOLUSDC";
pub const APP_LEVERAGE: u32 = 20;

pub fn binding() -> Binding {
    Binding {
        symbol: SYMBOL.to_string(),
        tick_size: Decimal::new(1, 2),
        lot_step: Decimal::new(1, 2),
        min_notional: Decimal::new(5, 0),
        max_resting_orders: 200,
        maker_fee: Decimal::ZERO,
        taker_fee: Decimal::new(4, 4),
        maintenance_margin_rate: Decimal::new(5, 3),
        leverage_cap: 5,
        price_band_up: Decimal::new(105, 2),
        price_band_down: Decimal::new(95, 2),
        funding_interval_ms: 28_800_000,
    }
}

pub fn app_binding() -> Binding {
    Binding {
        leverage_cap: APP_LEVERAGE,
        ..binding()
    }
}

pub fn theta() -> Theta {
    Theta {
        lower: Decimal::new(56, 0),
        upper: Decimal::new(84, 0),
        grids: 40,
        quantity: Decimal::new(11, 2),
        take_profit: Some(Target::Roi(Decimal::new(1, 2))),
        stop_loss: None,
        stop_top: None,
        stop_bottom: None,
        trigger: Trigger::Last,
        close_tp_sl: true,
        close_risk: true,
    }
}

pub fn resources() -> Resources {
    Resources {
        margin: Decimal::new(20, 0),
        leverage: APP_LEVERAGE,
    }
}

pub fn start_price() -> Decimal {
    Decimal::new(7021, 2)
}
