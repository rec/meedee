use meedee::metrics::{Delivery, Histogram};

#[test]
fn fixed_bins_keep_exact_aggregates_including_values_wider_than_u64_totals() {
    let mut h = Histogram::default();
    assert_eq!(h.minimum_ns(), None);
    for n in [
        0,
        1,
        1000,
        1001,
        1_000_000_000,
        1_000_000_001,
        u64::MAX,
        u64::MAX,
    ] {
        h.observe(n).unwrap();
    }
    assert_eq!(h.count(), 8);
    assert_eq!(h.minimum_ns(), Some(0));
    assert_eq!(h.maximum_ns(), Some(u64::MAX));
    assert_eq!(h.sum_ns(), 2 * u128::from(u64::MAX) + 2_000_002_003);
    assert_eq!(h.buckets(), &[1, 2, 1, 0, 0, 0, 0, 1, 3]);
}
#[test]
fn delivery_counts_include_window_idle_time_but_intervals_begin_at_first_event() {
    let mut d = Delivery::new(10);
    assert_eq!(d.snapshot(10).unwrap().events, 0);
    assert!(d.observe(9).is_err());
    d.observe(20).unwrap();
    d.observe(20).unwrap();
    d.observe(35).unwrap();
    let saved = d.snapshot(50).unwrap();
    assert_eq!(saved.elapsed_ns, 40);
    assert_eq!(saved.events, 3);
    assert_eq!(saved.intervals.count(), 2);
    assert_eq!(saved.intervals.sum_ns(), 15);
    assert!(d.snapshot(34).is_err());
    assert!(d.observe(34).is_err());
    d.observe(40).unwrap();
    assert_eq!(saved.events, 3);
    assert_eq!(d.snapshot(50).unwrap().events, 4);
}
