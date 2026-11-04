// Shared scalar field inventory for Rust values and their native Python views.
// Functional layouts: M2-104-UM 1.1.2 sections 7.2/7.4, appendix F.
macro_rules! scalar_values {
    ($m:ident) => {$m! {
        Noop [0x00000000,0] {};
        JrClock [0x00100000,0] {ticks:u16 = 0..=65535 => [0,0,65535,0]};
        JrTimestamp [0x00200000,0] {ticks:u16 = 0..=65535 => [0,0,65535,0]};
        DeltaClockstampTicksPerQuarter [0x00300000,0] {ticks:u16 = 1..=65535 => [0,0,65535,0]};
        DeltaClockstamp [0x00400000,0] {ticks:u32 = 0..=1048575 => [0,0,1048575,0]};
        Midi2RegisteredPerNoteController [0x40000000,0] {group:u8 = 1..=16 => [0,24,15,-1],channel:u8 = 1..=16 => [0,16,15,-1],note:u8 = 0..=127 => [0,8,127,0],controller:u8 = 0..=255 => [0,0,255,0],value:u32 = 0..=4294967295 => [1,0,4294967295,0]};
        Midi2AssignablePerNoteController [0x40100000,0] {group:u8 = 1..=16 => [0,24,15,-1],channel:u8 = 1..=16 => [0,16,15,-1],note:u8 = 0..=127 => [0,8,127,0],controller:u8 = 0..=255 => [0,0,255,0],value:u32 = 0..=4294967295 => [1,0,4294967295,0]};
        Midi2PerNoteManagement [0x40f00000,0] {group:u8 = 1..=16 => [0,24,15,-1],channel:u8 = 1..=16 => [0,16,15,-1],note:u8 = 0..=127 => [0,8,127,0],flags:u8 = 0..=3 => [0,0,3,0]};
        Midi2RegisteredController [0x40200000,0] {group:u8 = 1..=16 => [0,24,15,-1],channel:u8 = 1..=16 => [0,16,15,-1],bank:u8 = 0..=127 => [0,8,127,0],index:u8 = 0..=127 => [0,0,127,0],value:u32 = 0..=4294967295 => [1,0,4294967295,0]};
        Midi2AssignableController [0x40300000,0] {group:u8 = 1..=16 => [0,24,15,-1],channel:u8 = 1..=16 => [0,16,15,-1],bank:u8 = 0..=127 => [0,8,127,0],index:u8 = 0..=127 => [0,0,127,0],value:u32 = 0..=4294967295 => [1,0,4294967295,0]};
        Midi2RelativeRegisteredController [0x40400000,0] {group:u8 = 1..=16 => [0,24,15,-1],channel:u8 = 1..=16 => [0,16,15,-1],bank:u8 = 0..=127 => [0,8,127,0],index:u8 = 0..=127 => [0,0,127,0],value:i32 = -2147483648..=2147483647 => [1,0,4294967295,0]};
        Midi2RelativeAssignableController [0x40500000,0] {group:u8 = 1..=16 => [0,24,15,-1],channel:u8 = 1..=16 => [0,16,15,-1],bank:u8 = 0..=127 => [0,8,127,0],index:u8 = 0..=127 => [0,0,127,0],value:i32 = -2147483648..=2147483647 => [1,0,4294967295,0]};
        Midi2PerNotePitchBend [0x40600000,0] {group:u8 = 1..=16 => [0,24,15,-1],channel:u8 = 1..=16 => [0,16,15,-1],note:u8 = 0..=127 => [0,8,127,0],value:i32 = -2147483648..=2147483647 => [1,0,4294967295,2147483648]};
    }};
}
