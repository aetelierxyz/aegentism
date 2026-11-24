use std::io::{self, Write};

use aegent_mm_core::Side;

use crate::rows::Row;

pub const COLUMNS: [&str; 8] = [
    "symbol",
    "cell",
    "buy_px",
    "sell_px",
    "qty",
    "first_side",
    "t_start_ms",
    "t_end_ms",
];
pub const SEPARATOR: &str = ",";
pub const LINE_END: &str = "\n";
pub const BUY: &str = "buy";
pub const SELL: &str = "sell";

pub fn side_label(side: Side) -> &'static str {
    match side {
        Side::Buy => BUY,
        Side::Sell => SELL,
    }
}

pub fn header() -> String {
    COLUMNS.join(SEPARATOR)
}

pub fn line(row: &Row) -> String {
    [
        row.symbol.clone(),
        row.cell.to_string(),
        row.buy_px.to_string(),
        row.sell_px.to_string(),
        row.qty.to_string(),
        side_label(row.first_side).to_string(),
        row.t_start_ms.to_string(),
        row.t_end_ms.to_string(),
    ]
    .join(SEPARATOR)
}

pub fn render(rows: &[Row]) -> String {
    let mut text = header();
    text.push_str(LINE_END);
    for row in rows {
        text.push_str(&line(row));
        text.push_str(LINE_END);
    }
    text
}

pub fn write_to<W: Write>(writer: &mut W, rows: &[Row]) -> io::Result<()> {
    writer.write_all(render(rows).as_bytes())
}
