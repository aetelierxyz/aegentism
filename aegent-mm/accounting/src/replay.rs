use aegent_mm_core::ledger::Ledger;
use aegent_mm_core::params::Instance;
use aegent_mm_core::program::Program;
use aegent_mm_core::run::{Observation, Run, Wallet};
use aegent_mm_core::start::Precondition;
use aegent_mm_core::state::{Exit, PhaseError};
use aegent_mm_core::step::StepError;
use aegent_mm_core::value::{Pairing, pairing, profit_last};
use aegent_mm_core::{Event, EventKind, Prices, Side};
use rust_decimal::Decimal;

use crate::input::{Bar, Fill};
use crate::timeline::{Anchor, Timeline};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replay {
    pub instance: Instance,
    pub program: Program,
    pub wallet_balance: Decimal,
    pub start_ms: u64,
    pub bar_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ended {
    pub exit: Exit,
    pub time_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub run: Run,
    pub wallet: Wallet,
    pub ended: Option<Ended>,
    pub fills_applied: usize,
    pub fills_discarded: usize,
}

impl Outcome {
    pub fn ledger(&self) -> &Ledger {
        self.run.state().ledger()
    }

    pub fn exit(&self) -> Option<Exit> {
        self.ended.map(|ended| ended.exit)
    }

    pub fn exit_ms(&self) -> Option<u64> {
        self.ended.map(|ended| ended.time_ms)
    }

    pub fn profit_last(&self) -> Decimal {
        profit_last(self.run.instance(), self.run.state(), self.run.prices())
    }

    pub fn wallet_value(&self) -> Decimal {
        self.run.wallet_value(&self.wallet)
    }

    pub fn pairing(&self) -> Pairing {
        pairing(self.ledger(), self.run.instance().quantity())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayError {
    BarIntervalZero,
    BarsNotAscending { bar: usize },
    BarInconsistent { bar: usize },
    FillsNotAscending { fill: usize },
    FillBeforeStart { fill: usize },
    NoBarAtStart,
    MarginAboveWallet,
    Refused(Vec<Precondition>),
    FillSideMismatch { fill: usize },
    FillNotComplete { fill: usize },
    Step { fill: usize, error: StepError },
    Engine(StepError),
    Phase(PhaseError),
}

fn check_bars(bars: &[Bar], bar_ms: u64) -> Result<(), ReplayError> {
    if bar_ms == 0 {
        return Err(ReplayError::BarIntervalZero);
    }
    for (index, bar) in bars.iter().enumerate() {
        if !bar.is_consistent() {
            return Err(ReplayError::BarInconsistent { bar: index });
        }
        if index > 0 && bars[index - 1].time_ms.saturating_add(bar_ms) > bar.time_ms {
            return Err(ReplayError::BarsNotAscending { bar: index });
        }
    }
    Ok(())
}

fn check_fills(fills: &[Fill], start_ms: u64) -> Result<(), ReplayError> {
    for (index, fill) in fills.iter().enumerate() {
        if fill.time_ms < start_ms {
            return Err(ReplayError::FillBeforeStart { fill: index });
        }
        if index > 0 && fills[index - 1].time_ms > fill.time_ms {
            return Err(ReplayError::FillsNotAscending { fill: index });
        }
    }
    Ok(())
}

struct Session {
    run: Run,
    observation: Observation,
    ended: Option<Ended>,
    applied: usize,
}

impl Session {
    fn note(&mut self, exit: Option<Exit>, time_ms: u64) {
        if self.ended.is_none() {
            self.ended = exit.map(|exit| Ended { exit, time_ms });
        }
    }

    fn price(&mut self, time_ms: u64, price: Decimal) -> Result<(), ReplayError> {
        let event = Event::at(time_ms, EventKind::Price(Prices::flat(price)));
        let applied = self
            .run
            .apply(&event, &self.observation)
            .map_err(ReplayError::Engine)?;
        self.note(applied.exit, time_ms);
        Ok(())
    }

    fn fill(&mut self, index: usize, fill: &Fill) -> Result<(), ReplayError> {
        self.price(fill.time_ms, fill.price)?;
        let empty = self.run.state().empty_level();
        let resting = if fill.level < empty {
            Side::Buy
        } else {
            Side::Sell
        };
        if fill.level != empty && fill.side != resting {
            return Err(ReplayError::FillSideMismatch { fill: index });
        }
        if fill.quantity != self.run.instance().quantity() {
            return Err(ReplayError::FillNotComplete { fill: index });
        }
        let binding = self.run.instance().binding();
        let rate = if fill.maker {
            binding.maker_fee
        } else {
            binding.taker_fee
        };
        let event = Event::at(
            fill.time_ms,
            EventKind::Trade {
                level: fill.level,
                quantity: fill.quantity,
                price: fill.price,
                fee: fill.quantity * fill.price * rate,
                maker: fill.maker,
            },
        );
        let applied = self
            .run
            .apply(&event, &self.observation)
            .map_err(|error| ReplayError::Step { fill: index, error })?;
        self.applied += 1;
        self.note(applied.exit, fill.time_ms);
        Ok(())
    }
}

pub fn account(replay: &Replay, fills: &[Fill], bars: &[Bar]) -> Result<Outcome, ReplayError> {
    check_bars(bars, replay.bar_ms)?;
    check_fills(fills, replay.start_ms)?;
    if replay.instance.margin() > replay.wallet_balance {
        return Err(ReplayError::MarginAboveWallet);
    }
    let first = bars
        .iter()
        .position(|bar| bar.time_ms == replay.start_ms)
        .ok_or(ReplayError::NoBarAtStart)?;
    let mut run = Run::start(
        replay.instance.clone(),
        replay.program,
        Prices::flat(bars[first].open),
        replay.start_ms,
        replay.wallet_balance,
    )
    .map_err(ReplayError::Refused)?;
    run.acknowledge_start().map_err(ReplayError::Phase)?;
    let wallet = Wallet::RunOnly {
        balance_at_start: replay.wallet_balance,
    };
    let mut session = Session {
        run,
        observation: Observation {
            wallet,
            stream_lost_since_ms: None,
        },
        ended: None,
        applied: 0,
    };
    for anchor in Timeline::new(fills, &bars[first..], replay.bar_ms) {
        match anchor {
            Anchor::Price { time_ms, price } => session.price(time_ms, price)?,
            Anchor::Fill { index } => session.fill(index, &fills[index])?,
        }
        if session.ended.is_some() {
            break;
        }
    }
    if session.ended.is_some() {
        session.run.confirm_closed().map_err(ReplayError::Phase)?;
    }
    Ok(Outcome {
        run: session.run,
        wallet,
        ended: session.ended,
        fills_applied: session.applied,
        fills_discarded: fills.len() - session.applied,
    })
}
