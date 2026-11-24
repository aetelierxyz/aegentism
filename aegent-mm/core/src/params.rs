use rust_decimal::Decimal;

use crate::binding::{Binding, BindingError};
use crate::steps::{divides, triangular};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Trigger {
    #[default]
    Last,
    Mark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Amount(Decimal),
    Roi(Decimal),
}

impl Target {
    pub fn given(&self) -> Decimal {
        match self {
            Target::Amount(amount) => *amount,
            Target::Roi(roi) => *roi,
        }
    }

    pub fn amount(&self, margin: Decimal) -> Decimal {
        match self {
            Target::Amount(amount) => *amount,
            Target::Roi(roi) => *roi * margin,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theta {
    pub lower: Decimal,
    pub upper: Decimal,
    pub grids: u32,
    pub quantity: Decimal,
    pub take_profit: Option<Target>,
    pub stop_loss: Option<Target>,
    pub stop_top: Option<Decimal>,
    pub stop_bottom: Option<Decimal>,
    pub trigger: Trigger,
    pub close_tp_sl: bool,
    pub close_risk: bool,
}

impl Theta {
    pub fn new(lower: Decimal, upper: Decimal, grids: u32, quantity: Decimal) -> Theta {
        Theta {
            lower,
            upper,
            grids,
            quantity,
            take_profit: None,
            stop_loss: None,
            stop_top: None,
            stop_bottom: None,
            trigger: Trigger::Last,
            close_tp_sl: true,
            close_risk: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resources {
    pub margin: Decimal,
    pub leverage: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Inadmissible {
    Binding(BindingError),
    LowerNotPositive,
    QuantityNotPositive,
    RangeEmpty,
    LowerOffTick,
    TooFewGrids,
    TooManyGrids,
    SpacingOffTick,
    QuantityOffLotStep,
    BelowMinNotional,
    TakeProfitNotPositive,
    StopLossNotNegative,
    MarginNotPositive,
    MarginAboveWallet,
    LeverageBelowOne,
    LeverageAboveCap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    binding: Binding,
    theta: Theta,
    resources: Resources,
    spacing: Decimal,
    take_profit: Option<Decimal>,
    stop_loss: Option<Decimal>,
}

impl Instance {
    pub fn admit(
        binding: Binding,
        theta: Theta,
        resources: Resources,
        wallet_balance: Decimal,
    ) -> Result<Instance, Vec<Inadmissible>> {
        if let Err(errors) = binding.check() {
            return Err(errors.into_iter().map(Inadmissible::Binding).collect());
        }
        let mut found = Vec::new();
        let width = theta.upper - theta.lower;
        if theta.lower <= Decimal::ZERO {
            found.push(Inadmissible::LowerNotPositive);
        }
        if theta.quantity <= Decimal::ZERO {
            found.push(Inadmissible::QuantityNotPositive);
        }
        if width <= Decimal::ZERO {
            found.push(Inadmissible::RangeEmpty);
        }
        if !divides(binding.tick_size, theta.lower) {
            found.push(Inadmissible::LowerOffTick);
        }
        if theta.grids < 2 {
            found.push(Inadmissible::TooFewGrids);
        }
        if theta.grids.saturating_add(2) > binding.max_resting_orders {
            found.push(Inadmissible::TooManyGrids);
        }
        if theta.grids > 0 && !divides(binding.tick_size * Decimal::from(theta.grids), width) {
            found.push(Inadmissible::SpacingOffTick);
        }
        if !divides(binding.lot_step, theta.quantity) {
            found.push(Inadmissible::QuantityOffLotStep);
        }
        if theta.quantity * theta.lower < binding.min_notional {
            found.push(Inadmissible::BelowMinNotional);
        }
        if theta
            .take_profit
            .is_some_and(|target| target.given() <= Decimal::ZERO)
        {
            found.push(Inadmissible::TakeProfitNotPositive);
        }
        if theta
            .stop_loss
            .is_some_and(|target| target.given() >= Decimal::ZERO)
        {
            found.push(Inadmissible::StopLossNotNegative);
        }
        if resources.margin <= Decimal::ZERO {
            found.push(Inadmissible::MarginNotPositive);
        }
        if resources.margin > wallet_balance {
            found.push(Inadmissible::MarginAboveWallet);
        }
        if resources.leverage < 1 {
            found.push(Inadmissible::LeverageBelowOne);
        }
        if resources.leverage > binding.leverage_cap {
            found.push(Inadmissible::LeverageAboveCap);
        }
        if !found.is_empty() {
            return Err(found);
        }
        let spacing = width / Decimal::from(theta.grids);
        let take_profit = theta
            .take_profit
            .map(|target| target.amount(resources.margin));
        let stop_loss = theta
            .stop_loss
            .map(|target| target.amount(resources.margin));
        Ok(Instance {
            binding,
            theta,
            resources,
            spacing,
            take_profit,
            stop_loss,
        })
    }

    pub fn binding(&self) -> &Binding {
        &self.binding
    }

    pub fn theta(&self) -> &Theta {
        &self.theta
    }

    pub fn resources(&self) -> &Resources {
        &self.resources
    }

    pub fn lower(&self) -> Decimal {
        self.theta.lower
    }

    pub fn upper(&self) -> Decimal {
        self.theta.upper
    }

    pub fn grids(&self) -> u32 {
        self.theta.grids
    }

    pub fn level_count(&self) -> u32 {
        self.theta.grids + 1
    }

    pub fn quantity(&self) -> Decimal {
        self.theta.quantity
    }

    pub fn margin(&self) -> Decimal {
        self.resources.margin
    }

    pub fn leverage(&self) -> Decimal {
        Decimal::from(self.resources.leverage)
    }

    pub fn spacing(&self) -> Decimal {
        self.spacing
    }

    pub fn take_profit(&self) -> Option<Decimal> {
        self.take_profit
    }

    pub fn stop_loss(&self) -> Option<Decimal> {
        self.stop_loss
    }

    pub fn level_price(&self, level: u32) -> Decimal {
        self.theta.lower + Decimal::from(level) * self.spacing
    }

    pub fn level_prices(&self) -> Vec<Decimal> {
        (0..=self.theta.grids)
            .map(|level| self.level_price(level))
            .collect()
    }

    pub fn sum_of_prices_below(&self, level: u32) -> Decimal {
        Decimal::from(level) * self.theta.lower + self.spacing * triangular(level)
    }

    pub fn sum_of_prices_above(&self, level: u32) -> Decimal {
        let count = self.theta.grids.saturating_sub(level);
        let indices = triangular(self.theta.grids + 1) - triangular(level + 1);
        Decimal::from(count) * self.theta.lower + self.spacing * indices
    }

    pub fn largest_open_lots(&self, start_level: u32) -> u32 {
        start_level.max(self.theta.grids.saturating_sub(start_level))
    }

    pub fn largest_long(&self, start_level: u32) -> Decimal {
        self.theta.quantity * Decimal::from(start_level)
    }

    pub fn largest_short(&self, start_level: u32) -> Decimal {
        self.theta.quantity * Decimal::from(self.theta.grids.saturating_sub(start_level))
    }

    pub fn pair_profit(&self) -> Decimal {
        self.theta.quantity * self.spacing
    }

    pub fn one_way_loss(&self, levels: u32) -> Decimal {
        self.pair_profit() * triangular(levels)
    }

    pub fn loss_at_lower_bound(&self, start_level: u32) -> Decimal {
        self.one_way_loss(start_level)
    }

    pub fn loss_at_upper_bound(&self, start_level: u32) -> Decimal {
        self.one_way_loss(self.theta.grids.saturating_sub(start_level))
    }
}
