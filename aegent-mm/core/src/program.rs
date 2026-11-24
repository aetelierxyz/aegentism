use rust_decimal::Decimal;

pub const DAY_MS: u64 = 86_400_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Program {
    start_value: Decimal,
    loss_cap: Decimal,
    dead_man_ms: u64,
    data_silence_ms: u64,
    stream_loss_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProgramError {
    LossCapOutOfRange,
}

impl Program {
    pub fn new(
        start_value: Decimal,
        loss_cap: Decimal,
        dead_man_ms: u64,
        data_silence_ms: u64,
        stream_loss_ms: u64,
    ) -> Result<Program, ProgramError> {
        if loss_cap <= Decimal::ZERO || loss_cap >= Decimal::ONE {
            return Err(ProgramError::LossCapOutOfRange);
        }
        Ok(Program {
            start_value,
            loss_cap,
            dead_man_ms,
            data_silence_ms,
            stream_loss_ms,
        })
    }

    pub fn start_value(&self) -> Decimal {
        self.start_value
    }

    pub fn loss_cap(&self) -> Decimal {
        self.loss_cap
    }

    pub fn dead_man_ms(&self) -> u64 {
        self.dead_man_ms
    }

    pub fn data_silence_ms(&self) -> u64 {
        self.data_silence_ms
    }

    pub fn stream_loss_ms(&self) -> u64 {
        self.stream_loss_ms
    }

    pub fn floor(&self) -> Decimal {
        self.start_value * (Decimal::ONE - self.loss_cap)
    }

    pub fn guard_holds(&self, wallet_value: Decimal) -> bool {
        wallet_value < self.floor()
    }

    pub fn dead_man_renewal_ms(&self) -> u64 {
        self.dead_man_ms / 3
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Guard {
    tripped: bool,
}

impl Guard {
    pub fn new() -> Guard {
        Guard::default()
    }

    pub fn is_tripped(&self) -> bool {
        self.tripped
    }

    pub fn observe(&mut self, program: &Program, wallet_value: Decimal) -> bool {
        if program.guard_holds(wallet_value) {
            self.tripped = true;
        }
        self.tripped
    }

    pub fn reset(&mut self) {
        self.tripped = false;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StartRefusal {
    RunActive,
    GuardTripped,
}

pub fn start_allowed(run_active: bool, guard: &Guard) -> Result<(), Vec<StartRefusal>> {
    let mut found = Vec::new();
    if run_active {
        found.push(StartRefusal::RunActive);
    }
    if guard.is_tripped() {
        found.push(StartRefusal::GuardTripped);
    }
    if found.is_empty() { Ok(()) } else { Err(found) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checkpoint {
    pub time_ms: u64,
    pub wallet_value: Decimal,
    pub at_or_above_start: bool,
}

pub fn checkpoint(program: &Program, time_ms: u64, wallet_value: Decimal) -> Checkpoint {
    Checkpoint {
        time_ms,
        wallet_value,
        at_or_above_start: wallet_value >= program.start_value(),
    }
}

pub fn next_checkpoint_ms(now_ms: u64) -> u64 {
    (now_ms / DAY_MS + 1) * DAY_MS
}
