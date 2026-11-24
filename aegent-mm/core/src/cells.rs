use rust_decimal::Decimal;

use crate::ledger::Side;
use crate::params::Instance;
use crate::quote::Order;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CellError {
    UnknownCell,
    NoOrderOnSide,
    QuantityNotPositive,
    Overfill,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    index: u32,
    state: Side,
    filled: Decimal,
}

impl Cell {
    pub fn start(index: u32, start_level: u32) -> Cell {
        Cell {
            index,
            state: first_side(index, start_level),
            filled: Decimal::ZERO,
        }
    }

    pub fn index(&self) -> u32 {
        self.index
    }

    pub fn state(&self) -> Side {
        self.state
    }

    pub fn filled(&self) -> Decimal {
        self.filled
    }

    pub fn buy_level(&self) -> u32 {
        self.index
    }

    pub fn sell_level(&self) -> u32 {
        self.index + 1
    }

    pub fn resting(&self, instance: &Instance) -> Order {
        let level = match self.state {
            Side::Buy => self.buy_level(),
            Side::Sell => self.sell_level(),
        };
        Order {
            level,
            side: self.state,
            quantity: instance.quantity() - self.filled,
            price: instance.level_price(level),
        }
    }

    pub fn trade(
        &mut self,
        instance: &Instance,
        side: Side,
        quantity: Decimal,
    ) -> Result<Option<Order>, CellError> {
        if side != self.state {
            return Err(CellError::NoOrderOnSide);
        }
        if quantity <= Decimal::ZERO {
            return Err(CellError::QuantityNotPositive);
        }
        let filled = self.filled + quantity;
        if filled > instance.quantity() {
            return Err(CellError::Overfill);
        }
        if filled < instance.quantity() {
            self.filled = filled;
            return Ok(None);
        }
        self.filled = Decimal::ZERO;
        self.state = self.state.opposite();
        Ok(Some(self.resting(instance)))
    }
}

pub fn first_side(index: u32, start_level: u32) -> Side {
    if index < start_level {
        Side::Buy
    } else {
        Side::Sell
    }
}

pub fn owner(instance: &Instance, level: u32, side: Side) -> Option<u32> {
    match side {
        Side::Buy if level < instance.grids() => Some(level),
        Side::Sell if level >= 1 && level <= instance.grids() => Some(level - 1),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cells {
    cells: Vec<Cell>,
}

impl Cells {
    pub fn start(instance: &Instance, start_level: u32) -> Cells {
        Cells {
            cells: (0..instance.grids())
                .map(|index| Cell::start(index, start_level))
                .collect(),
        }
    }

    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    pub fn cell(&self, index: u32) -> Option<&Cell> {
        self.cells.get(index as usize)
    }

    pub fn buying(&self) -> u32 {
        self.cells
            .iter()
            .filter(|cell| cell.state == Side::Buy)
            .count() as u32
    }

    pub fn resting(&self, instance: &Instance) -> Vec<Order> {
        let mut orders: Vec<Order> = self
            .cells
            .iter()
            .map(|cell| cell.resting(instance))
            .collect();
        orders.sort_by_key(|order| (order.level, order.side));
        orders
    }

    pub fn trade(
        &mut self,
        instance: &Instance,
        level: u32,
        side: Side,
        quantity: Decimal,
    ) -> Result<Option<Order>, CellError> {
        let index = owner(instance, level, side).ok_or(CellError::UnknownCell)?;
        let cell = self
            .cells
            .get_mut(index as usize)
            .ok_or(CellError::UnknownCell)?;
        cell.trade(instance, side, quantity)
    }
}
