// Native MIDI 2 musical fields. Shared with the Python binding generator.
macro_rules! midi2_values {
    ($m:ident) => {$m! {
        PerNoteManagement [0x40f00000,0] {group:u8 = 1..=16 => [0,24,15,-1],channel:u8 = 1..=16 => [0,16,15,-1],note:u8 = 0..=127 => [0,8,127,0],flags:u8 = 0..=3 => [0,0,3,0]};
        PerNotePitchBend [0x40600000,0] {group:u8 = 1..=16 => [0,24,15,-1],channel:u8 = 1..=16 => [0,16,15,-1],note:u8 = 0..=127 => [0,8,127,0],value:i32 = -2147483648..=2147483647 => [1,0,4294967295,2147483648]};
    }};
}

macro_rules! controller_values {
    ($m:ident) => {$m! {
        PerNoteController [0x40000000,0] {group:u8 = 1..=16 => [0,24,15,-1],channel:u8 = 1..=16 => [0,16,15,-1],note:u8 = 0..=127 => [0,8,127,0],controller:u8 = 0..=255 => [0,0,255,0],value:u32 = 0..=4294967295 => [1,0,4294967295,0]};
        ParameterController [0x40200000,0] {group:u8 = 1..=16 => [0,24,15,-1],channel:u8 = 1..=16 => [0,16,15,-1],bank:u8 = 0..=127 => [0,8,127,0],index:u8 = 0..=127 => [0,0,127,0],value:u32 = 0..=4294967295 => [1,0,4294967295,0]};
        RelativeParameterController [0x40400000,0] {group:u8 = 1..=16 => [0,24,15,-1],channel:u8 = 1..=16 => [0,16,15,-1],bank:u8 = 0..=127 => [0,8,127,0],index:u8 = 0..=127 => [0,0,127,0],value:i32 = -2147483648..=2147483647 => [1,0,4294967295,0]};
    }};
}
