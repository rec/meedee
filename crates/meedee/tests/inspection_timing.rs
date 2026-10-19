use meedee::inspection_timing::Timing;

#[test]
fn clocks_and_loss_segment_observations_without_cross_source_regressions() {
    let mut t = Timing::default();
    t.observe("a", Some("one"), Some(10), 0, 128);
    t.observe("b", Some("two"), Some(-100), 1, 128);
    t.observe("a", Some("one"), Some(9), 2, 128);
    t.observe("a", Some("two"), Some(-200), 3, 128);
    t.boundary("a", 4, "gap", 128);
    t.observe("a", Some("two"), Some(-300), 5, 128);
    assert_eq!(t.regressions, 1);
    assert_eq!(t.segments.len(), 4);
    assert_eq!(t.segments[0].minimum, Some(9));
    assert_eq!(t.diagnostics[1].kind, "clock_change");
}
