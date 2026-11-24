use rust_decimal::Decimal;

use crate::ledger::Ledger;
use crate::params::Instance;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Reason {
    Guard,
    Backstop,
    Failure,
    Risk,
    StopLoss,
    Price,
    TakeProfit,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PositionAtEnd {
    Closed,
    ClosedByExchange,
    Kept,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Exit {
    pub reason: Reason,
    pub position: PositionAtEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Init,
    Run,
    Closing(Exit),
    Ended(Exit),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PhaseError {
    NotInInit,
    NotInClosing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    phase: Phase,
    start_level: u32,
    empty_level: u32,
    filled: Vec<Decimal>,
    ledger: Ledger,
}

impl State {
    pub fn initial(instance: &Instance, start_level: u32) -> Option<State> {
        if start_level < 1 || start_level >= instance.grids() {
            return None;
        }
        Some(State {
            phase: Phase::Init,
            start_level,
            empty_level: start_level,
            filled: vec![Decimal::ZERO; instance.level_count() as usize],
            ledger: Ledger::new(),
        })
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn start_level(&self) -> u32 {
        self.start_level
    }

    pub fn empty_level(&self) -> u32 {
        self.empty_level
    }

    pub fn filled(&self, level: u32) -> Decimal {
        self.filled
            .get(level as usize)
            .copied()
            .unwrap_or(Decimal::ZERO)
    }

    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    pub fn is_live(&self) -> bool {
        matches!(self.phase, Phase::Init | Phase::Run)
    }

    pub fn exit(&self) -> Option<Exit> {
        match self.phase {
            Phase::Init | Phase::Run => None,
            Phase::Closing(exit) | Phase::Ended(exit) => Some(exit),
        }
    }

    pub fn position(&self, quantity: Decimal) -> Decimal {
        let lots = i64::from(self.start_level) - i64::from(self.empty_level);
        let empty = self.empty_level as usize;
        let below: Decimal = self.filled.iter().take(empty).sum();
        let above: Decimal = self.filled.iter().skip(empty + 1).sum();
        quantity * Decimal::from(lots) + below - above
    }

    pub fn acknowledge_start(&mut self) -> Result<(), PhaseError> {
        match self.phase {
            Phase::Init => {
                self.phase = Phase::Run;
                Ok(())
            }
            _ => Err(PhaseError::NotInInit),
        }
    }

    pub fn confirm_closed(&mut self) -> Result<(), PhaseError> {
        match self.phase {
            Phase::Closing(exit) => {
                self.phase = Phase::Ended(exit);
                Ok(())
            }
            _ => Err(PhaseError::NotInClosing),
        }
    }

    pub(crate) fn ledger_mut(&mut self) -> &mut Ledger {
        &mut self.ledger
    }

    pub(crate) fn set_phase(&mut self, phase: Phase) {
        self.phase = phase;
    }

    pub(crate) fn set_filled(&mut self, level: u32, filled: Decimal) {
        self.filled[level as usize] = filled;
    }

    pub(crate) fn set_empty_level(&mut self, level: u32) {
        self.empty_level = level;
    }
}
