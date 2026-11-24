use aegent_mm_core::Side;
use aegent_mm_core::cells::Cells;
use aegent_mm_core::params::Instance;
use aegent_mm_core::start::nearest_level;
use aegent_mm_core::state::State;
use rust_decimal::Decimal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub symbol: String,
    pub cell: u32,
    pub buy_px: Decimal,
    pub sell_px: Decimal,
    pub qty: Decimal,
    pub first_side: Side,
    pub t_start_ms: u64,
    pub t_end_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrdersError {
    EmptyLevelAtBound,
    WindowEmpty,
    SymbolNotPlain,
}

fn plain(symbol: &str) -> bool {
    !symbol.is_empty()
        && symbol
            .chars()
            .all(|letter| letter.is_ascii_alphanumeric() || matches!(letter, '_' | '-' | '.'))
}

fn scaled(value: Decimal, step: Decimal) -> Decimal {
    let mut scaled = value;
    scaled.rescale(step.normalize().scale().max(value.normalize().scale()));
    scaled
}

pub fn rows(
    instance: &Instance,
    start_price: Decimal,
    t_start_ms: u64,
    t_end_ms: u64,
) -> Result<Vec<Row>, OrdersError> {
    let binding = instance.binding();
    if !plain(&binding.symbol) {
        return Err(OrdersError::SymbolNotPlain);
    }
    if t_end_ms <= t_start_ms {
        return Err(OrdersError::WindowEmpty);
    }
    let start_level = nearest_level(instance, start_price);
    if State::initial(instance, start_level).is_none() {
        return Err(OrdersError::EmptyLevelAtBound);
    }
    let qty = scaled(instance.quantity(), binding.lot_step);
    Ok(Cells::start(instance, start_level)
        .cells()
        .iter()
        .map(|cell| Row {
            symbol: binding.symbol.clone(),
            cell: cell.index(),
            buy_px: scaled(instance.level_price(cell.buy_level()), binding.tick_size),
            sell_px: scaled(instance.level_price(cell.sell_level()), binding.tick_size),
            qty,
            first_side: cell.state(),
            t_start_ms,
            t_end_ms,
        })
        .collect())
}
