use rust_decimal::Decimal;

use crate::backstop::Backstops;
use crate::event::{Event, EventKind, Prices};
use crate::params::Instance;
use crate::program::Program;
use crate::quote::{Order, quote};
use crate::start::{Precondition, start};
use crate::state::{Exit, Phase, PhaseError, State};
use crate::step::{Fill, StepError, step};
use crate::stop::{ClosingAction, Liveness, Signal, StopInput, closing_actions, evaluate};
use crate::value::{run_wallet_value, wallet_value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wallet {
    Reported {
        balance: Decimal,
        entry_price: Decimal,
    },
    RunOnly {
        balance_at_start: Decimal,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observation {
    pub wallet: Wallet,
    pub stream_lost_since_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    pub fill: Option<Fill>,
    pub send: Option<Order>,
    pub exit: Option<Exit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    instance: Instance,
    program: Program,
    state: State,
    prices: Prices,
    last_price_ms: u64,
    start_price: Decimal,
    backstops: Backstops,
}

impl Run {
    pub fn start(
        instance: Instance,
        program: Program,
        prices: Prices,
        time_ms: u64,
        wallet_value: Decimal,
    ) -> Result<Run, Vec<Precondition>> {
        let started = start(&instance, &program, &prices, wallet_value)?;
        Ok(Run {
            instance,
            program,
            state: started.state,
            prices,
            last_price_ms: time_ms,
            start_price: started.start_price,
            backstops: started.backstops,
        })
    }

    pub fn instance(&self) -> &Instance {
        &self.instance
    }

    pub fn program(&self) -> &Program {
        &self.program
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn prices(&self) -> &Prices {
        &self.prices
    }

    pub fn last_price_ms(&self) -> u64 {
        self.last_price_ms
    }

    pub fn start_price(&self) -> Decimal {
        self.start_price
    }

    pub fn backstops(&self) -> &Backstops {
        &self.backstops
    }

    pub fn position(&self) -> Decimal {
        self.state.position(self.instance.quantity())
    }

    pub fn quote(&self) -> Vec<Order> {
        quote(&self.instance, &self.state)
    }

    pub fn wallet_value(&self, wallet: &Wallet) -> Decimal {
        match wallet {
            Wallet::Reported {
                balance,
                entry_price,
            } => wallet_value(
                *balance,
                *entry_price,
                self.position(),
                &self.prices,
                self.instance.binding().taker_fee,
            ),
            Wallet::RunOnly { balance_at_start } => {
                run_wallet_value(&self.instance, &self.state, *balance_at_start, &self.prices)
            }
        }
    }

    pub fn acknowledge_start(&mut self) -> Result<(), PhaseError> {
        self.state.acknowledge_start()
    }

    pub fn confirm_closed(&mut self) -> Result<(), PhaseError> {
        self.state.confirm_closed()
    }

    pub fn closing_actions(&self) -> Vec<ClosingAction> {
        closing_actions(&self.instance, &self.state)
    }

    pub fn apply(
        &mut self,
        event: &Event,
        observation: &Observation,
    ) -> Result<Applied, StepError> {
        let was_live = self.state.is_live();
        let stepped = step(&self.instance, &mut self.state, event)?;
        if let EventKind::Price(prices) = &event.kind {
            self.prices = *prices;
            self.last_price_ms = event.time_ms;
        }
        if !was_live {
            return Ok(Applied {
                fill: stepped.fill,
                send: None,
                exit: None,
            });
        }
        let signal = match &event.kind {
            EventKind::Reject { .. } => Some(Signal::Reject),
            EventKind::Backstop => Some(Signal::Backstop),
            EventKind::Command { keep } => Some(Signal::Command { keep: *keep }),
            _ => None,
        };
        let exit = evaluate(&StopInput {
            instance: &self.instance,
            program: &self.program,
            state: &self.state,
            prices: &self.prices,
            wallet_value: self.wallet_value(&observation.wallet),
            signal,
            liveness: Liveness {
                now_ms: event.time_ms,
                last_price_ms: self.last_price_ms,
                stream_lost_since_ms: observation.stream_lost_since_ms,
            },
        });
        match exit {
            Some(exit) => {
                self.state.set_phase(Phase::Closing(exit));
                Ok(Applied {
                    fill: stepped.fill,
                    send: None,
                    exit: Some(exit),
                })
            }
            None => Ok(Applied {
                fill: stepped.fill,
                send: stepped.send,
                exit: None,
            }),
        }
    }
}
