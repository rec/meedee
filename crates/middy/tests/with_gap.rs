use middy::events::{with_gap, GapItem};
use std::{cell::Cell, time::Duration};

#[test]
fn inserts_a_gap_after_each_generic_item_including_the_last() {
    let gap = Duration::from_millis(100);
    assert_eq!(
        with_gap(["a", "b"], gap).collect::<Vec<_>>(),
        [
            GapItem::Item("a"),
            GapItem::Gap(gap),
            GapItem::Item("b"),
            GapItem::Gap(gap)
        ]
    );
    assert_eq!(
        with_gap([42], Duration::ZERO).collect::<Vec<_>>(),
        [GapItem::Item(42), GapItem::Gap(Duration::ZERO)]
    );
    assert_eq!(with_gap(std::iter::empty::<u8>(), gap).count(), 0);
}

#[test]
fn source_advances_only_when_the_next_item_is_requested() {
    let consumed = Cell::new(0);
    let source = (0..2).inspect(|_| consumed.set(consumed.get() + 1));
    let mut values = with_gap(source, Duration::from_millis(100));
    assert_eq!(consumed.get(), 0);
    assert_eq!(values.next(), Some(GapItem::Item(0)));
    assert_eq!(consumed.get(), 1);
    assert_eq!(
        values.next(),
        Some(GapItem::Gap(Duration::from_millis(100)))
    );
    assert_eq!(consumed.get(), 1);
    assert_eq!(values.next(), Some(GapItem::Item(1)));
    assert_eq!(consumed.get(), 2);
}
