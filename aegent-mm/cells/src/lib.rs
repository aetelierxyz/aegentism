pub mod format;
pub mod rows;

pub use rows::{OrdersError, Row, rows};

use std::fs;
use std::io;
use std::path::Path;

pub fn write_orders_file(path: &Path, rows: &[Row]) -> io::Result<()> {
    fs::write(path, format::render(rows))
}
