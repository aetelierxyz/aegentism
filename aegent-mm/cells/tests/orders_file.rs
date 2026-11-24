use std::collections::BTreeSet;
use std::fs;

use aegent_mm_cells::format::{COLUMNS, header, line, render, write_to};
use aegent_mm_cells::{OrdersError, Row, rows, write_orders_file};
use aegent_mm_core::params::{Instance, Theta};
use aegent_mm_core::quote::quote;
use aegent_mm_core::state::State;
use aegent_mm_core::{Binding, Side, reference};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

const T_START: u64 = 1_795_478_400_000;
const T_END: u64 = 1_795_564_800_000;

fn admitted(binding: Binding, theta: Theta) -> Instance {
    Instance::admit(binding, theta, reference::resources(), dec!(100)).expect("admissible")
}

fn reference_instance() -> Instance {
    admitted(reference::app_binding(), reference::theta())
}

fn reference_rows() -> Vec<Row> {
    rows(
        &reference_instance(),
        reference::start_price(),
        T_START,
        T_END,
    )
    .expect("a start inside")
}

#[test]
fn the_columns_are_those_of_the_orders_file() {
    assert_eq!(
        COLUMNS,
        [
            "symbol",
            "cell",
            "buy_px",
            "sell_px",
            "qty",
            "first_side",
            "t_start_ms",
            "t_end_ms"
        ]
    );
    assert_eq!(
        header(),
        "symbol,cell,buy_px,sell_px,qty,first_side,t_start_ms,t_end_ms"
    );
}

#[test]
fn the_reference_instance_gives_one_row_per_cell() {
    let rows = reference_rows();
    assert_eq!(rows.len(), 40);
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(row.symbol, "SOLUSDC");
        assert_eq!(row.cell, index as u32);
        assert_eq!(
            row.buy_px,
            dec!(56) + dec!(0.7) * Decimal::from(index as u32)
        );
        assert_eq!(row.sell_px - row.buy_px, dec!(0.70));
        assert_eq!(row.qty, dec!(0.11));
        assert_eq!(row.t_start_ms, T_START);
        assert_eq!(row.t_end_ms, T_END);
    }
}

#[test]
fn cells_below_the_empty_level_start_as_buys_and_the_others_as_sells() {
    let rows = reference_rows();
    assert!(rows[..20].iter().all(|row| row.first_side == Side::Buy));
    assert!(rows[20..].iter().all(|row| row.first_side == Side::Sell));
    assert_eq!(rows[19].buy_px, dec!(69.30));
    assert_eq!(rows[19].sell_px, dec!(70.00));
    assert_eq!(rows[20].buy_px, dec!(70.00));
    assert_eq!(rows[20].sell_px, dec!(70.70));
}

#[test]
fn the_first_orders_of_all_rows_are_the_quote_of_the_initial_state() {
    let instance = reference_instance();
    let state = State::initial(&instance, 20).expect("start level inside the ladder");
    let quoted: BTreeSet<(Side, Decimal, Decimal)> = quote(&instance, &state)
        .into_iter()
        .map(|order| (order.side, order.price, order.quantity))
        .collect();
    let first: BTreeSet<(Side, Decimal, Decimal)> = reference_rows()
        .into_iter()
        .map(|row| match row.first_side {
            Side::Buy => (Side::Buy, row.buy_px, row.qty),
            Side::Sell => (Side::Sell, row.sell_px, row.qty),
        })
        .collect();
    assert_eq!(first.len(), 40);
    assert_eq!(first, quoted);
}

#[test]
fn lines_print_prices_at_the_tick_scale_and_quantities_at_the_lot_scale() {
    let rows = reference_rows();
    assert_eq!(
        line(&rows[0]),
        "SOLUSDC,0,56.00,56.70,0.11,buy,1795478400000,1795564800000"
    );
    assert_eq!(
        line(&rows[19]),
        "SOLUSDC,19,69.30,70.00,0.11,buy,1795478400000,1795564800000"
    );
    assert_eq!(
        line(&rows[20]),
        "SOLUSDC,20,70.00,70.70,0.11,sell,1795478400000,1795564800000"
    );
    assert_eq!(
        line(&rows[39]),
        "SOLUSDC,39,83.30,84.00,0.11,sell,1795478400000,1795564800000"
    );
}

#[test]
fn a_coarse_tick_and_a_whole_lot_print_without_padding() {
    let binding = Binding {
        symbol: "XYZ-PERP_1.0".to_string(),
        tick_size: dec!(0.5),
        lot_step: dec!(1),
        ..reference::app_binding()
    };
    let instance = admitted(binding, Theta::new(dec!(100), dec!(103), 3, dec!(2)));
    let rows = rows(&instance, dec!(101.2), 10, 20).expect("a start inside");
    assert_eq!(
        render(&rows),
        "symbol,cell,buy_px,sell_px,qty,first_side,t_start_ms,t_end_ms\n\
         XYZ-PERP_1.0,0,100.0,101.0,2,buy,10,20\n\
         XYZ-PERP_1.0,1,101.0,102.0,2,sell,10,20\n\
         XYZ-PERP_1.0,2,102.0,103.0,2,sell,10,20\n"
    );
}

#[test]
fn the_rendered_file_has_a_header_and_one_line_per_cell() {
    let rows = reference_rows();
    let text = render(&rows);
    let lines: Vec<&str> = text.lines().collect();
    assert!(text.ends_with('\n'));
    assert_eq!(lines.len(), 41);
    assert_eq!(lines[0], header());
    for (row, printed) in rows.iter().zip(&lines[1..]) {
        assert_eq!(&line(row), printed);
        assert_eq!(printed.split(',').count(), 8);
    }
    let mut buffer = Vec::new();
    write_to(&mut buffer, &rows).expect("writing to memory");
    assert_eq!(buffer, text.as_bytes());
}

#[test]
fn the_file_on_disk_holds_the_rendered_rows() {
    let rows = reference_rows();
    let directory = std::env::temp_dir().join(format!("aegent-mm-cells-{}", std::process::id()));
    fs::create_dir_all(&directory).expect("a temporary directory");
    let path = directory.join("orders.csv");
    write_orders_file(&path, &rows).expect("the file is written");
    let written = fs::read_to_string(&path).expect("the file is read");
    fs::remove_dir_all(&directory).expect("the directory is removed");
    assert_eq!(written, render(&rows));
}

#[test]
fn a_start_price_nearest_a_bound_gives_no_file() {
    let instance = reference_instance();
    for start_price in [dec!(56.35), dec!(40), dec!(83.66), dec!(90)] {
        assert_eq!(
            rows(&instance, start_price, T_START, T_END),
            Err(OrdersError::EmptyLevelAtBound)
        );
    }
    assert!(rows(&instance, dec!(56.36), T_START, T_END).is_ok());
    assert!(rows(&instance, dec!(83.65), T_START, T_END).is_ok());
}

#[test]
fn the_start_price_decides_the_first_sides() {
    let instance = reference_instance();
    let low = rows(&instance, dec!(56.36), T_START, T_END).expect("a start inside");
    let high = rows(&instance, dec!(83.65), T_START, T_END).expect("a start inside");
    let buys = |rows: &[Row]| {
        rows.iter()
            .filter(|row| row.first_side == Side::Buy)
            .count()
    };
    assert_eq!(buys(&low), 1);
    assert_eq!(buys(&high), 39);
    assert_eq!(buys(&reference_rows()), 20);
}

#[test]
fn the_window_must_end_after_it_starts() {
    let instance = reference_instance();
    for t_end in [T_START, T_START - 1, 0] {
        assert_eq!(
            rows(&instance, reference::start_price(), T_START, t_end),
            Err(OrdersError::WindowEmpty)
        );
    }
}

#[test]
fn a_symbol_that_would_break_a_line_is_refused() {
    for symbol in ["SOL,USDC", "SOL USDC", "SOL\nUSDC", "\"SOLUSDC\"", ""] {
        let binding = Binding {
            symbol: symbol.to_string(),
            ..reference::app_binding()
        };
        let instance = admitted(binding, reference::theta());
        assert_eq!(
            rows(&instance, reference::start_price(), T_START, T_END),
            Err(OrdersError::SymbolNotPlain)
        );
    }
}
