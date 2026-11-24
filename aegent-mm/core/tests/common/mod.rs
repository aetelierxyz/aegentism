#![allow(dead_code)]

use aegent_mm_core::params::{Instance, Resources, Theta};
use aegent_mm_core::program::Program;
use aegent_mm_core::reference;
use aegent_mm_core::run::{Observation, Run, Wallet};
use aegent_mm_core::state::State;
use aegent_mm_core::step::{Stepped, step};
use aegent_mm_core::{Event, EventKind, Prices};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

pub fn wallet_balance() -> Decimal {
    dec!(100)
}

pub fn app_instance() -> Instance {
    admitted(reference::theta(), reference::resources())
}

pub fn admitted(theta: Theta, resources: Resources) -> Instance {
    Instance::admit(reference::app_binding(), theta, resources, wallet_balance())
        .expect("admissible under the binding of the app grid")
}

pub fn plain_theta() -> Theta {
    Theta {
        take_profit: None,
        ..reference::theta()
    }
}

pub fn roomy_resources() -> Resources {
    Resources {
        margin: dec!(100),
        leverage: 20,
    }
}

pub fn program() -> Program {
    Program::new(dec!(100), dec!(0.2), 60_000, 5_000, 10_000).expect("loss cap inside (0, 1)")
}

pub fn trade(time_ms: u64, level: u32, quantity: Decimal, price: Decimal) -> Event {
    Event::at(
        time_ms,
        EventKind::Trade {
            level,
            quantity,
            price,
            fee: Decimal::ZERO,
            maker: true,
        },
    )
}

pub fn price(time_ms: u64, price: Decimal) -> Event {
    Event::at(time_ms, EventKind::Price(Prices::flat(price)))
}

pub fn running(instance: &Instance, start_level: u32) -> State {
    let mut state = State::initial(instance, start_level).expect("start level inside the ladder");
    state.acknowledge_start().expect("state in Init");
    state
}

pub fn fill(instance: &Instance, state: &mut State, time_ms: u64, level: u32) -> Stepped {
    let event = trade(
        time_ms,
        level,
        instance.quantity(),
        instance.level_price(level),
    );
    step(instance, state, &event).expect("admissible complete fill")
}

pub fn run_only() -> Observation {
    Observation {
        wallet: Wallet::RunOnly {
            balance_at_start: wallet_balance(),
        },
        stream_lost_since_ms: None,
    }
}

pub fn started_run(instance: Instance, start_price: Decimal) -> Run {
    let mut run = Run::start(
        instance,
        program(),
        Prices::flat(start_price),
        0,
        wallet_balance(),
    )
    .expect("start preconditions hold");
    run.acknowledge_start().expect("run in Init");
    run
}
