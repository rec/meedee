// Shared scalar field inventory for Rust values and their native Python views.
// Functional layouts: M2-104-UM 1.1.2 sections 7.2/7.4, appendix F.
macro_rules! scalar_values {
    ($m:ident) => {$m! {
        Noop [0x00000000,0] {};
        JrClock [0x00100000,0] {ticks:u16 = 0..=65535 => [0,0,65535,0]};
        JrTimestamp [0x00200000,0] {ticks:u16 = 0..=65535 => [0,0,65535,0]};
        DeltaClockstampTicksPerQuarter [0x00300000,0] {ticks:u16 = 1..=65535 => [0,0,65535,0]};
        DeltaClockstamp [0x00400000,0] {ticks:u32 = 0..=1048575 => [0,0,1048575,0]};
    }};
}
