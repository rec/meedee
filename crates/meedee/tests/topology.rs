use meedee::topology::{Endpoint, Tracker};

fn endpoint(name: &str) -> Endpoint {
    Endpoint::new(
        "test".into(),
        "midi1".into(),
        "input".into(),
        1,
        name.into(),
    )
    .unwrap()
}

#[test]
fn complete_descriptors_and_duplicate_counts_define_observed_changes() {
    let mut tracker = Tracker::new(4, 4, 2).unwrap();
    let p = endpoint("keys");
    let first = tracker.observe(vec![p.clone(), p.clone()]).unwrap();
    assert_eq!(first.port_count, 2);
    assert_eq!(first.ports[0].count, 2);
    assert_eq!(first.last.unwrap().added, 0);
    assert!(first.history.is_empty());
    let next = tracker
        .observe(vec![p.clone(), endpoint("renamed")])
        .unwrap();
    let delta = next.last.unwrap();
    assert_eq!(delta.added, 1);
    assert_eq!(delta.removed, 1);
    assert_eq!(delta.changes.len(), 2);
    let unchanged = tracker.observe(vec![endpoint("renamed"), p]).unwrap();
    assert_eq!(unchanged.generation, 3);
    assert_eq!(unchanged.last.unwrap().added, 0);
    assert_eq!(unchanged.history.len(), 1);
}

#[test]
fn rejected_scans_preserve_the_complete_baseline_and_stop() {
    let mut tracker = Tracker::new(1, 1, 1).unwrap();
    tracker.observe(vec![endpoint("keys")]).unwrap();
    assert!(tracker
        .observe(vec![endpoint("keys"), endpoint("keys")])
        .is_err());
    let report = tracker.snapshot();
    assert!(report.stopped);
    assert_eq!(report.generation, 1);
    assert_eq!(report.port_count, 1);
    assert_eq!(report.ports[0].endpoint.name(), "keys");
    assert_eq!(report.last.unwrap().removed, 0);
    assert!(tracker.observe(vec![]).is_err());
}

#[test]
fn details_and_changed_scan_history_are_bounded_without_losing_totals() {
    let mut tracker = Tracker::new(2, 1, 1).unwrap();
    tracker.observe(vec![endpoint("a"), endpoint("b")]).unwrap();
    let report = tracker.observe(vec![endpoint("c"), endpoint("d")]).unwrap();
    let delta = report.last.unwrap();
    assert_eq!(delta.added, 2);
    assert_eq!(delta.removed, 2);
    assert_eq!(delta.changes.len(), 1);
    assert_eq!(delta.omitted_changes, 3);
    let report = tracker.observe(vec![]).unwrap();
    assert_eq!(report.history.len(), 1);
    assert_eq!(report.omitted_history, 1);
    assert_eq!(report.history[0].generation, 3);
}
