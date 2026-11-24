use aegent_mm_accounting::{Anchor, Bar, Fill, Timeline};
use aegent_mm_core::Side;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

const BAR_MS: u64 = 60;

fn bar(time_ms: u64, open: Decimal, high: Decimal, low: Decimal, close: Decimal) -> Bar {
    Bar {
        time_ms,
        open,
        high,
        low,
        close,
    }
}

fn fill(time_ms: u64) -> Fill {
    Fill {
        time_ms,
        level: 1,
        side: Side::Buy,
        price: dec!(10),
        quantity: dec!(1),
        maker: true,
    }
}

fn price(time_ms: u64, price: Decimal) -> Anchor {
    Anchor::Price { time_ms, price }
}

fn anchors(fills: &[Fill], bars: &[Bar]) -> Vec<Anchor> {
    Timeline::new(fills, bars, BAR_MS).collect()
}

#[test]
fn a_rising_bar_without_fills_gives_open_low_high_close() {
    let bars = [bar(0, dec!(10), dec!(12), dec!(9), dec!(11))];
    assert_eq!(
        anchors(&[], &bars),
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
fn a_bar_with_fills_gives_open_fills_close_and_no_extremes() {
    let bars = [bar(0, dec!(10), dec!(12), dec!(9), dec!(11))];
    let fills = [fill(10), fill(20)];
    assert_eq!(
        anchors(&fills, &bars),
        vec![
            price(0, dec!(10)),
            Anchor::Fill { index: 0 },
            Anchor::Fill { index: 1 },
            price(60, dec!(11)),
        ]
    );
}

#[test]
fn a_fill_at_the_time_of_a_bar_follows_its_open_and_one_at_its_end_belongs_to_the_next_bar() {
    let bars = [
        bar(0, dec!(10), dec!(10), dec!(10), dec!(10.5)),
        bar(60, dec!(10.6), dec!(10.6), dec!(10.6), dec!(10.7)),
    ];
    let fills = [fill(0), fill(60)];
    assert_eq!(
        anchors(&fills, &bars),
        vec![
            price(0, dec!(10)),
            Anchor::Fill { index: 0 },
            price(60, dec!(10.5)),
            price(60, dec!(10.6)),
            Anchor::Fill { index: 1 },
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
    let fills = [fill(100), fill(200), fill(400), fill(500)];
    assert_eq!(
        anchors(&fills, &bars),
        vec![
            price(0, dec!(10)),
            price(60, dec!(10)),
            price(60, dec!(10)),
            price(60, dec!(10)),
            Anchor::Fill { index: 0 },
            Anchor::Fill { index: 1 },
            price(300, dec!(11)),
            price(360, dec!(11)),
            price(360, dec!(11)),
            price(360, dec!(11)),
            Anchor::Fill { index: 2 },
            Anchor::Fill { index: 3 },
        ]
    );
}

#[test]
fn without_bars_the_fills_stand_alone() {
    let fills = [fill(5), fill(6)];
    assert_eq!(
        anchors(&fills, &[]),
        vec![Anchor::Fill { index: 0 }, Anchor::Fill { index: 1 }]
    );
    assert_eq!(anchors(&[], &[]), Vec::new());
}

#[test]
fn the_path_order_of_the_extremes_follows_the_direction_of_the_bar() {
    let rising = bar(0, dec!(10), dec!(12), dec!(9), dec!(11));
    let falling = bar(0, dec!(10), dec!(12), dec!(9), dec!(9.5));
    assert_eq!(rising.extremes_in_path_order(), [dec!(9), dec!(12)]);
    assert_eq!(falling.extremes_in_path_order(), [dec!(12), dec!(9)]);
    assert!(rising.is_consistent());
    assert!(!bar(0, dec!(10), dec!(9.9), dec!(9), dec!(9.5)).is_consistent());
    assert!(!bar(0, dec!(10), dec!(12), dec!(10.1), dec!(11)).is_consistent());
    assert!(!bar(0, dec!(10), dec!(12), dec!(0), dec!(11)).is_consistent());
}
