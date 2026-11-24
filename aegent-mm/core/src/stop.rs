use rust_decimal::Decimal;

use crate::event::Prices;
use crate::ledger::Side;
use crate::params::{Instance, Theta};
use crate::program::Program;
use crate::quote::resting_notional;
use crate::state::{Exit, Phase, PositionAtEnd, Reason, State};
use crate::value::{profit_at, profit_trigger};

pub const TABLE: [Reason; 8] = [
    Reason::Guard,
    Reason::Backstop,
    Reason::Failure,
    Reason::Risk,
    Reason::StopLoss,
    Reason::Price,
    Reason::TakeProfit,
    Reason::Command,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Signal {
    Reject,
    Backstop,
    Command { keep: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Liveness {
    pub now_ms: u64,
    pub last_price_ms: u64,
    pub stream_lost_since_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
pub struct StopInput<'a> {
    pub instance: &'a Instance,
    pub program: &'a Program,
    pub state: &'a State,
    pub prices: &'a Prices,
    pub wallet_value: Decimal,
    pub signal: Option<Signal>,
    pub liveness: Liveness,
}

pub fn exit_for(reason: Reason, theta: &Theta, keep: bool) -> Exit {
    let position = match reason {
        Reason::Guard => PositionAtEnd::Closed,
        Reason::Backstop => PositionAtEnd::ClosedByExchange,
        Reason::Failure | Reason::Risk => closed_if(theta.close_risk),
        Reason::StopLoss | Reason::Price | Reason::TakeProfit => closed_if(theta.close_tp_sl),
        Reason::Command => closed_if(!keep),
    };
    Exit { reason, position }
}

fn closed_if(closes: bool) -> PositionAtEnd {
    if closes {
        PositionAtEnd::Closed
    } else {
        PositionAtEnd::Kept
    }
}

pub fn exposure(instance: &Instance, state: &State, mark: Decimal) -> Decimal {
    let (bids, asks) = resting_notional(instance, state);
    let held = state.position(instance.quantity()) * mark;
    (held + bids).abs().max((held - asks).abs())
}

pub fn occupied_margin(instance: &Instance, state: &State, mark: Decimal) -> Decimal {
    exposure(instance, state, mark) / instance.leverage()
}

pub fn margin_balance(instance: &Instance, state: &State, mark: Decimal) -> Decimal {
    instance.margin() + profit_at(instance, state, mark)
}

pub fn risk_ratio(instance: &Instance, state: &State, mark: Decimal) -> Option<Decimal> {
    let exposure = exposure(instance, state, mark);
    if exposure.is_zero() {
        return None;
    }
    Some(margin_balance(instance, state, mark) * instance.leverage() / exposure)
}

pub fn risk_holds(instance: &Instance, state: &State, mark: Decimal) -> bool {
    margin_balance(instance, state, mark) * instance.leverage() < exposure(instance, state, mark)
}

pub fn data_silent(program: &Program, liveness: &Liveness) -> bool {
    liveness.now_ms.saturating_sub(liveness.last_price_ms) > program.data_silence_ms()
}

pub fn stream_lost(program: &Program, liveness: &Liveness) -> bool {
    liveness
        .stream_lost_since_ms
        .is_some_and(|since| liveness.now_ms.saturating_sub(since) > program.stream_loss_ms())
}

pub fn holds(input: &StopInput, reason: Reason) -> bool {
    let theta = input.instance.theta();
    match reason {
        Reason::Guard => input.program.guard_holds(input.wallet_value),
        Reason::Backstop => input.signal == Some(Signal::Backstop),
        Reason::Failure => {
            input.signal == Some(Signal::Reject)
                || data_silent(input.program, &input.liveness)
                || stream_lost(input.program, &input.liveness)
        }
        Reason::Risk => risk_holds(input.instance, input.state, input.prices.mark),
        Reason::StopLoss => input.instance.stop_loss().is_some_and(|stop_loss| {
            profit_trigger(input.instance, input.state, input.prices) <= stop_loss
        }),
        Reason::Price => {
            let trigger = input.prices.trigger(theta.trigger);
            theta.stop_top.is_some_and(|top| trigger >= top)
                || theta.stop_bottom.is_some_and(|bottom| trigger <= bottom)
        }
        Reason::TakeProfit => input.instance.take_profit().is_some_and(|take_profit| {
            profit_trigger(input.instance, input.state, input.prices) >= take_profit
        }),
        Reason::Command => matches!(input.signal, Some(Signal::Command { .. })),
    }
}

pub fn conditions(input: &StopInput) -> Vec<Reason> {
    TABLE
        .into_iter()
        .filter(|reason| holds(input, *reason))
        .collect()
}

pub fn evaluate(input: &StopInput) -> Option<Exit> {
    let keep = matches!(input.signal, Some(Signal::Command { keep: true }));
    TABLE
        .into_iter()
        .find(|reason| holds(input, *reason))
        .map(|reason| exit_for(reason, input.instance.theta(), keep))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClosingAction {
    CancelEveryRestingOrder,
    ReduceOnlyMarketOrder { side: Side, quantity: Decimal },
    VerifyNoRestingOrders,
    VerifyNoPosition,
}

pub fn closing_plan(position: Decimal, at_end: PositionAtEnd) -> Vec<ClosingAction> {
    let mut actions = vec![ClosingAction::CancelEveryRestingOrder];
    if at_end == PositionAtEnd::Closed && !position.is_zero() {
        let side = if position > Decimal::ZERO {
            Side::Sell
        } else {
            Side::Buy
        };
        actions.push(ClosingAction::ReduceOnlyMarketOrder {
            side,
            quantity: position.abs(),
        });
    }
    actions.push(ClosingAction::VerifyNoRestingOrders);
    if at_end != PositionAtEnd::Kept {
        actions.push(ClosingAction::VerifyNoPosition);
    }
    actions
}

pub fn closing_actions(instance: &Instance, state: &State) -> Vec<ClosingAction> {
    match state.phase() {
        Phase::Closing(exit) => closing_plan(state.position(instance.quantity()), exit.position),
        _ => Vec::new(),
    }
}

pub fn guard_actions(position: Decimal) -> Vec<ClosingAction> {
    closing_plan(position, PositionAtEnd::Closed)
}
