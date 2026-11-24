use aegent_mm_accounting::{Anchor, Bar, Fill, Timeline};
use aegent_mm_core::Side;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

const BAR_MS: u64 = 60;
const START_LEVEL: u32 = 20;

fn bar(time_ms: u64, open: Decimal, high: Decimal, low: Decimal, close: Decimal) -> Bar {
    Bar {
        time_ms,
        open,
        high,
        low,
        close,
    }
}

fn wide(time_ms: u64) -> Bar {
    bar(time_ms, dec!(10), dec!(12), dec!(9), dec!(11))
}

fn fill(time_ms: u64, level: u32) -> Fill {
    Fill {
        time_ms,
        level,
        side: Side::Buy,
        price: dec!(10),
        quantity: dec!(1),
        maker: true,
    }
}

fn price(time_ms: u64, price: Decimal) -> Anchor {
    Anchor::Price { time_ms, price }
}

fn taken(index: usize) -> Anchor {
    Anchor::Fill { index }
}

fn anchors(fills: &[Fill], bars: &[Bar]) -> Vec<Anchor> {
    Timeline::new(fills, bars, BAR_MS, START_LEVEL).collect()
}

#[test]
fn a_rising_bar_without_fills_gives_open_low_high_close() {
    assert_eq!(
        anchors(&[], &[wide(0)]),
        vec![
            price(0, dec!(10)),
            price(60, dec!(9)),
            price(60, dec!(12)),
            price(60, dec!(11)),
        ]
    );
}

#[test]
fn a_falling_bar_without_fills_gives_open_high_low_close() {
    let bars = [bar(0, dec!(10), dec!(12), dec!(9), dec!(9.5))];
    assert_eq!(
        anchors(&[], &bars),
        vec![
            price(0, dec!(10)),
            price(60, dec!(12)),
            price(60, dec!(9)),
            price(60, dec!(9.5)),
        ]
    );
}

#[test]
fn a_flat_bar_counts_as_rising() {
    let bars = [bar(0, dec!(10), dec!(12), dec!(9), dec!(10))];
    assert_eq!(
        anchors(&[], &bars)[1..3],
        [price(60, dec!(9)), price(60, dec!(12))]
    );
}

#[test]
fn a_falling_run_of_fills_puts_the_high_before_them_and_the_low_after_them() {
    let fills = [fill(10, 19), fill(20, 18)];
    assert_eq!(
        anchors(&fills, &[wide(0)]),
        vec![
            price(0, dec!(10)),
            price(10, dec!(12)),
            taken(0),
            taken(1),
            price(60, dec!(9)),
            price(60, dec!(11)),
        ]
    );
}

#[test]
fn a_rising_run_of_fills_puts_the_low_before_them_and_the_high_after_them() {
    let fills = [fill(10, 21), fill(20, 22)];
    assert_eq!(
        anchors(&fills, &[wide(0)]),
        vec![
            price(0, dec!(10)),
            price(10, dec!(9)),
            taken(0),
            taken(1),
            price(60, dec!(12)),
            price(60, dec!(11)),
        ]
    );
}

#[test]
fn an_extreme_between_two_fills_carries_the_time_of_the_later_fill() {
    let up_then_down = [fill(10, 21), fill(30, 20)];
    let down_then_up = [fill(10, 19), fill(30, 20)];
    assert_eq!(
        anchors(&up_then_down, &[wide(0)]),
        vec![
            price(0, dec!(10)),
            price(10, dec!(9)),
            taken(0),
            price(30, dec!(12)),
            taken(1),
            price(60, dec!(11)),
        ]
    );
    assert_eq!(
        anchors(&down_then_up, &[wide(0)]),
        vec![
            price(0, dec!(10)),
            price(10, dec!(12)),
            taken(0),
            price(30, dec!(9)),
            taken(1),
            price(60, dec!(11)),
        ]
    );
}

#[test]
fn an_extreme_goes_to_the_first_state_of_the_bar_that_reached_it() {
    let fills = [fill(10, 19), fill(20, 20), fill(30, 19), fill(40, 20)];
    assert_eq!(
        anchors(&fills, &[wide(0)]),
        vec![
            price(0, dec!(10)),
            price(10, dec!(12)),
            taken(0),
            price(20, dec!(9)),
            taken(1),
            taken(2),
            taken(3),
            price(60, dec!(11)),
        ]
    );
}

#[test]
fn the_empty_level_carries_from_one_bar_to_the_next() {
    let fills = [fill(10, 19), fill(70, 18), fill(130, 19)];
    assert_eq!(
        anchors(&fills, &[wide(0), wide(60), wide(120)]),
        vec![
            price(0, dec!(10)),
            price(10, dec!(12)),
            taken(0),
            price(60, dec!(9)),
            price(60, dec!(11)),
            price(60, dec!(10)),
            price(70, dec!(12)),
            taken(1),
            price(120, dec!(9)),
            price(120, dec!(11)),
            price(120, dec!(10)),
            price(130, dec!(9)),
            taken(2),
            price(180, dec!(12)),
            price(180, dec!(11)),
        ]
    );
}

#[test]
fn a_fill_between_bars_moves_the_empty_level_of_the_next_bar() {
    let fills = [fill(100, 21), fill(310, 22)];
    assert_eq!(
        anchors(&fills, &[wide(0), wide(300)]),
        vec![
            price(0, dec!(10)),
            price(60, dec!(9)),
            price(60, dec!(12)),
            price(60, dec!(11)),
            taken(0),
            price(300, dec!(10)),
            price(310, dec!(9)),
            taken(1),
            price(360, dec!(12)),
            price(360, dec!(11)),
        ]
    );
}

#[test]
fn a_fill_at_the_time_of_a_bar_follows_its_open_and_one_at_its_end_belongs_to_the_next_bar() {
    let bars = [
        bar(0, dec!(10), dec!(10.5), dec!(10), dec!(10.5)),
        bar(60, dec!(10.6), dec!(10.7), dec!(10.6), dec!(10.7)),
    ];
    let fills = [fill(0, 21), fill(60, 22)];
    assert_eq!(
        anchors(&fills, &bars),
        vec![
            price(0, dec!(10)),
            price(0, dec!(10)),
            taken(0),
            price(60, dec!(10.5)),
            price(60, dec!(10.5)),
            price(60, dec!(10.6)),
            price(60, dec!(10.6)),
            taken(1),
            price(120, dec!(10.7)),
            price(120, dec!(10.7)),
        ]
    );
}

#[test]
fn fills_in_a_gap_and_after_the_last_bar_keep_their_order() {
    let bars = [
        bar(0, dec!(10), dec!(10), dec!(10), dec!(10)),
        bar(300, dec!(11), dec!(11), dec!(11), dec!(11)),
    ];
    let fills = [fill(100, 21), fill(200, 22), fill(400, 23), fill(500, 24)];
    assert_eq!(
        anchors(&fills, &bars),
        vec![
            price(0, dec!(10)),
            price(60, dec!(10)),
            price(60, dec!(10)),
            price(60, dec!(10)),
            taken(0),
            taken(1),
            price(300, dec!(11)),
            price(360, dec!(11)),
            price(360, dec!(11)),
            price(360, dec!(11)),
            taken(2),
            taken(3),
        ]
    );
}

#[test]
fn without_bars_the_fills_stand_alone() {
    let fills = [fill(5, 21), fill(6, 22)];
    assert_eq!(anchors(&fills, &[]), vec![taken(0), taken(1)]);
    assert_eq!(anchors(&[], &[]), Vec::new());
}

#[test]
fn every_bar_gives_its_open_both_extremes_and_its_close_whatever_its_fills() {
    let fills = [
        fill(5, 19),
        fill(6, 18),
        fill(7, 19),
        fill(65, 20),
        fill(200, 21),
    ];
    let bars = [wide(0), wide(60), wide(120), wide(180)];
    let produced = anchors(&fills, &bars);
    let prices = produced
        .iter()
        .filter(|anchor| matches!(anchor, Anchor::Price { .. }))
        .count();
    let taken: Vec<usize> = produced
        .iter()
        .filter_map(|anchor| match anchor {
            Anchor::Fill { index } => Some(*index),
            Anchor::Price { .. } => None,
        })
        .collect();
    assert_eq!(prices, 4 * bars.len());
    assert_eq!(taken, vec![0, 1, 2, 3, 4]);
    let times: Vec<u64> = produced
        .iter()
        .map(|anchor| match anchor {
            Anchor::Price { time_ms, .. } => *time_ms,
            Anchor::Fill { index } => fills[*index].time_ms,
        })
        .collect();
    assert!(times.windows(2).all(|pair| pair[0] <= pair[1]));
}

#[test]
fn the_path_order_of_the_extremes_follows_the_direction_of_the_bar() {
    let rising = wide(0);
    let falling = bar(0, dec!(10), dec!(12), dec!(9), dec!(9.5));
    assert_eq!(rising.extremes_in_path_order(), [dec!(9), dec!(12)]);
    assert_eq!(falling.extremes_in_path_order(), [dec!(12), dec!(9)]);
    assert!(rising.is_consistent());
    assert!(!bar(0, dec!(10), dec!(9.9), dec!(9), dec!(9.5)).is_consistent());
    assert!(!bar(0, dec!(10), dec!(12), dec!(10.1), dec!(11)).is_consistent());
    assert!(!bar(0, dec!(10), dec!(12), dec!(0), dec!(11)).is_consistent());
}
