use meedee::{
    comparison::{compare, Options},
    inspection::Format,
};

#[test]
fn streaming_comparison_counts_differences_and_never_equates_invalid_prefixes() {
    let o = Options {
        detail_limit: 1,
        ..Options::default()
    };
    let r = compare(
        &b"\xf0\x01\xf7\xf0\x01\xf7"[..],
        &b"\xf0\x02\xf7"[..],
        Format::Syx,
        &o,
    )
    .unwrap();
    assert_eq!(r.differences, 2);
    assert_eq!(r.examples.len(), 1);
    assert_eq!(r.examples[0].record, Some(0));
    assert!(!r.equal);
    assert!(r.left_complete && r.right_complete);
    let r = compare(&b"\xf0\x01"[..], &b"\xf0\x01"[..], Format::Syx, &o).unwrap();
    assert!(!r.equal && !r.left_complete && !r.right_complete);
}
