use rust_decimal::Decimal;

use crate::input::{Bar, Fill};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Price { time_ms: u64, price: Decimal },
    Fill { index: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Open,
    Fills,
    FirstExtreme,
    SecondExtreme,
    Close,
}

#[derive(Debug, Clone)]
pub struct Timeline<'a> {
    fills: &'a [Fill],
    bars: &'a [Bar],
    bar_ms: u64,
    bar: usize,
    fill: usize,
    stage: Stage,
    traded: bool,
}

impl<'a> Timeline<'a> {
    pub fn new(fills: &'a [Fill], bars: &'a [Bar], bar_ms: u64) -> Timeline<'a> {
        Timeline {
            fills,
            bars,
            bar_ms,
            bar: 0,
            fill: 0,
            stage: Stage::Open,
            traded: false,
        }
    }

    fn fill_before(&mut self, limit_ms: Option<u64>) -> Option<Anchor> {
        let fill = self.fills.get(self.fill)?;
        if limit_ms.is_some_and(|limit| fill.time_ms >= limit) {
            return None;
        }
        let index = self.fill;
        self.fill += 1;
        Some(Anchor::Fill { index })
    }
}

impl Iterator for Timeline<'_> {
    type Item = Anchor;

    fn next(&mut self) -> Option<Anchor> {
        loop {
            let Some(bar) = self.bars.get(self.bar).copied() else {
                return self.fill_before(None);
            };
            let end_ms = bar.time_ms + self.bar_ms;
            match self.stage {
                Stage::Open => {
                    if let Some(anchor) = self.fill_before(Some(bar.time_ms)) {
                        return Some(anchor);
                    }
                    self.stage = Stage::Fills;
                    self.traded = false;
                    return Some(Anchor::Price {
                        time_ms: bar.time_ms,
                        price: bar.open,
                    });
                }
                Stage::Fills => {
                    if let Some(anchor) = self.fill_before(Some(end_ms)) {
                        self.traded = true;
                        return Some(anchor);
                    }
                    self.stage = if self.traded {
                        Stage::Close
                    } else {
                        Stage::FirstExtreme
                    };
                }
                Stage::FirstExtreme => {
                    self.stage = Stage::SecondExtreme;
                    return Some(Anchor::Price {
                        time_ms: end_ms,
                        price: bar.extremes_in_path_order()[0],
                    });
                }
                Stage::SecondExtreme => {
                    self.stage = Stage::Close;
                    return Some(Anchor::Price {
                        time_ms: end_ms,
                        price: bar.extremes_in_path_order()[1],
                    });
                }
                Stage::Close => {
                    self.stage = Stage::Open;
                    self.bar += 1;
                    return Some(Anchor::Price {
                        time_ms: end_ms,
                        price: bar.close,
                    });
                }
            }
        }
    }
}
