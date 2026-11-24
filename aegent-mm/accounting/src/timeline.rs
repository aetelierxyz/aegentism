use std::collections::VecDeque;

use rust_decimal::Decimal;

use crate::input::{Bar, Fill};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Price { time_ms: u64, price: Decimal },
    Fill { index: usize },
}

#[derive(Debug, Clone)]
pub struct Timeline<'a> {
    fills: &'a [Fill],
    bars: &'a [Bar],
    bar_ms: u64,
    empty_level: u32,
    bar: usize,
    fill: usize,
    pending: VecDeque<Anchor>,
}

impl<'a> Timeline<'a> {
    pub fn new(fills: &'a [Fill], bars: &'a [Bar], bar_ms: u64, start_level: u32) -> Timeline<'a> {
        Timeline {
            fills,
            bars,
            bar_ms,
            empty_level: start_level,
            bar: 0,
            fill: 0,
            pending: VecDeque::new(),
        }
    }

    fn take_fill(&mut self) {
        self.pending.push_back(Anchor::Fill { index: self.fill });
        self.empty_level = self.fills[self.fill].level;
        self.fill += 1;
    }

    fn fills_before(&self, limit_ms: u64) -> usize {
        self.fills[self.fill..]
            .iter()
            .take_while(|fill| fill.time_ms < limit_ms)
            .count()
    }

    fn load_bar(&mut self, bar: Bar) {
        let end_ms = bar.time_ms + self.bar_ms;
        for _ in 0..self.fills_before(bar.time_ms) {
            self.take_fill();
        }
        self.pending.push_back(Anchor::Price {
            time_ms: bar.time_ms,
            price: bar.open,
        });
        let inside = self.fills_before(end_ms);
        if inside == 0 {
            for price in bar.extremes_in_path_order() {
                self.pending.push_back(Anchor::Price {
                    time_ms: end_ms,
                    price,
                });
            }
        } else {
            let levels: Vec<u32> = std::iter::once(self.empty_level)
                .chain(
                    self.fills[self.fill..self.fill + inside]
                        .iter()
                        .map(|fill| fill.level),
                )
                .collect();
            let highest = levels.iter().copied().max().unwrap_or(self.empty_level);
            let lowest = levels.iter().copied().min().unwrap_or(self.empty_level);
            let high_segment = levels.iter().position(|level| *level == highest);
            let low_segment = levels.iter().position(|level| *level == lowest);
            for segment in 0..=inside {
                if segment > 0 {
                    self.take_fill();
                }
                let time_ms = self
                    .fills
                    .get(self.fill)
                    .filter(|_| segment < inside)
                    .map_or(end_ms, |fill| fill.time_ms);
                if high_segment == Some(segment) {
                    self.pending.push_back(Anchor::Price {
                        time_ms,
                        price: bar.high,
                    });
                }
                if low_segment == Some(segment) {
                    self.pending.push_back(Anchor::Price {
                        time_ms,
                        price: bar.low,
                    });
                }
            }
        }
        self.pending.push_back(Anchor::Price {
            time_ms: end_ms,
            price: bar.close,
        });
    }
}

impl Iterator for Timeline<'_> {
    type Item = Anchor;

    fn next(&mut self) -> Option<Anchor> {
        if let Some(anchor) = self.pending.pop_front() {
            return Some(anchor);
        }
        match self.bars.get(self.bar).copied() {
            Some(bar) => {
                self.bar += 1;
                self.load_bar(bar);
            }
            None if self.fill < self.fills.len() => self.take_fill(),
            None => return None,
        }
        self.pending.pop_front()
    }
}
