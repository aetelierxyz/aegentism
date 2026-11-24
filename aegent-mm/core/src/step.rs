use rust_decimal::Decimal;

use crate::event::{Event, EventKind};
use crate::ledger::{FundingRecord, Side, TradeRecord};
use crate::params::Instance;
use crate::quote::Order;
use crate::state::{Exit, Phase, PositionAtEnd, Reason, State};
use crate::stop::exit_for;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fill {
    pub level: u32,
    pub side: Side,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stepped {
    pub fill: Option<Fill>,
    pub send: Option<Order>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StepError {
    RunEnded,
    UnknownLevel,
    NoOrderAtLevel,
    QuantityNotPositive,
    PriceNotPositive,
    PriceWorseThanLevel,
    Overfill,
    FillNotAdjacent,
}

pub fn step(instance: &Instance, state: &mut State, event: &Event) -> Result<Stepped, StepError> {
    if matches!(state.phase(), Phase::Ended(_)) {
        return Err(StepError::RunEnded);
    }
    match &event.kind {
        EventKind::Trade {
            level,
            quantity,
            price,
            fee,
            maker,
        } => trade(
            instance,
            state,
            TradeInput {
                time_ms: event.time_ms,
                level: *level,
                quantity: *quantity,
                price: *price,
                fee: *fee,
                maker: *maker,
            },
        ),
        EventKind::Funding { amount } => {
            state.ledger_mut().record_funding(FundingRecord {
                time_ms: event.time_ms,
                amount: *amount,
            });
            Ok(Stepped::default())
        }
        EventKind::Reject { .. } => {
            leave(state, exit_for(Reason::Failure, instance.theta(), false));
            Ok(Stepped::default())
        }
        EventKind::Backstop => {
            match state.phase() {
                Phase::Closing(exit) => state.set_phase(Phase::Closing(Exit {
                    reason: exit.reason,
                    position: PositionAtEnd::ClosedByExchange,
                })),
                _ => leave(state, exit_for(Reason::Backstop, instance.theta(), false)),
            }
            Ok(Stepped::default())
        }
        EventKind::Command { keep } => {
            leave(state, exit_for(Reason::Command, instance.theta(), *keep));
            Ok(Stepped::default())
        }
        EventKind::Price(_) | EventKind::Tick => Ok(Stepped::default()),
    }
}

struct TradeInput {
    time_ms: u64,
    level: u32,
    quantity: Decimal,
    price: Decimal,
    fee: Decimal,
    maker: bool,
}

fn leave(state: &mut State, exit: Exit) {
    if state.is_live() {
        state.set_phase(Phase::Closing(exit));
    }
}

fn trade(instance: &Instance, state: &mut State, input: TradeInput) -> Result<Stepped, StepError> {
    let empty = state.empty_level();
    if input.level > instance.grids() {
        return Err(StepError::UnknownLevel);
    }
    if input.level == empty {
        return Err(StepError::NoOrderAtLevel);
    }
    if input.quantity <= Decimal::ZERO {
        return Err(StepError::QuantityNotPositive);
    }
    if input.price <= Decimal::ZERO {
        return Err(StepError::PriceNotPositive);
    }
    let side = if input.level < empty {
        Side::Buy
    } else {
        Side::Sell
    };
    let limit = instance.level_price(input.level);
    let worse = match side {
        Side::Buy => input.price > limit,
        Side::Sell => input.price < limit,
    };
    if worse {
        return Err(StepError::PriceWorseThanLevel);
    }
    let filled = state.filled(input.level) + input.quantity;
    if filled > instance.quantity() {
        return Err(StepError::Overfill);
    }
    let complete = filled == instance.quantity();
    let adjacent = input.level + 1 == empty || input.level == empty + 1;
    if complete && !adjacent {
        return Err(StepError::FillNotAdjacent);
    }
    state.ledger_mut().record_trade(TradeRecord {
        time_ms: input.time_ms,
        level: input.level,
        side,
        quantity: input.quantity,
        price: input.price,
        fee: input.fee,
        maker: input.maker,
    });
    if !complete {
        state.set_filled(input.level, filled);
        return Ok(Stepped::default());
    }
    state.set_filled(input.level, Decimal::ZERO);
    state.set_empty_level(input.level);
    let send = state.is_live().then(|| Order {
        level: empty,
        side: side.opposite(),
        quantity: instance.quantity(),
        price: instance.level_price(empty),
    });
    Ok(Stepped {
        fill: Some(Fill {
            level: input.level,
            side,
        }),
        send,
    })
}
