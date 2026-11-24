use aegent_mm_accounting::{Bar, Fill, Replay, account};
use aegent_mm_cells::{Row, rows};
use aegent_mm_core::params::{Instance, Theta};
use aegent_mm_core::program::Program;
use aegent_mm_core::{Side, reference};
use rust_decimal_macros::dec;

const T0: u64 = 1_795_478_400_000;
const BAR_MS: u64 = 60_000;

fn instance() -> Instance {
    Instance::admit(
        reference::app_binding(),
        Theta {
            take_profit: None,
            ..reference::theta()
        },
        reference::resources(),
        dec!(100),
    )
    .expect("admissible")
}

fn fill_of(row: &Row, side: Side, time_ms: u64) -> Fill {
    let (level, price) = match side {
        Side::Buy => (row.cell, row.buy_px),
        Side::Sell => (row.cell + 1, row.sell_px),
    };
    Fill {
        time_ms,
        level,
        side,
        price,
        quantity: row.qty,
        maker: true,
    }
}

#[test]
fn the_two_orders_of_a_row_of_the_orders_file_form_a_matched_pair() {
    let instance = instance();
    let rows = rows(&instance, dec!(70.21), T0, T0 + 10 * BAR_MS).expect("a start inside");
    assert_eq!(rows[20].first_side, Side::Sell);
    assert_eq!(rows[19].first_side, Side::Buy);
    let fills = [
        fill_of(&rows[20], Side::Sell, T0 + 10_000),
        fill_of(&rows[20], Side::Buy, T0 + 20_000),
        fill_of(&rows[19], Side::Buy, T0 + 30_000),
        fill_of(&rows[19], Side::Sell, T0 + 40_000),
    ];
    let bars = [Bar {
        time_ms: T0,
        open: dec!(70.21),
        high: dec!(70.75),
        low: dec!(69.25),
        close: dec!(70.05),
    }];
    let replay = Replay {
        instance,
        program: Program::new(dec!(100), dec!(0.2), 60_000, 5_000, 10_000).expect("valid"),
        wallet_balance: dec!(100),
        start_ms: T0,
        bar_ms: BAR_MS,
    };
    let outcome = account(&replay, &fills, &bars).expect("a valid replay");
    assert_eq!(outcome.ended, None);
    assert_eq!(outcome.fills_applied, 4);
    let pairs: Vec<(u32, u32)> = outcome
        .pairing()
        .pairs
        .iter()
        .map(|pair| (pair.buy.level, pair.sell.level))
        .collect();
    assert_eq!(pairs, vec![(20, 21), (19, 20)]);
    assert_eq!(outcome.pairing().matched(), dec!(0.154));
    assert_eq!(outcome.run.state().start_level(), 20);
}
