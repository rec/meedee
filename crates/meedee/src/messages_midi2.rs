include!("midi2_values.rs");
use crate::ump::UmpValue;

macro_rules! musical_values {
    ($($name:ident [$first:expr,$second:expr] {$($field:ident:$ty:ty = $lo:literal ..= $hi:literal => [$word:literal,$shift:literal,$mask:literal,$bias:literal]),*};)*) => {
        $(#[derive(Clone, Debug)]
        pub struct $name { metadata: Option<Box<Metadata>>, $($field: $ty),* }
        value_metadata!($name { $($field),* });
        impl $name {
            #[allow(clippy::too_many_arguments)]
            pub fn new($($field: i64),*) -> Result<Self, ValueError> {
                let value = Self { metadata: None, $($field: bounded($field, stringify!($field), $lo, $hi)? as $ty),* };
                crate::ump::validate_scalar(&value.encode_words()?)?;
                Ok(value)
            }
            $(pub fn $field(&self) -> $ty { self.$field })*
            pub fn version(&self) -> u8 { 2 }
            pub fn with_channel(mut self, channel: i64) -> Result<Self, ValueError> {
                self.channel = bounded(channel, "channel", 1, 16)? as u8;
                Ok(self)
            }
            pub fn with_group(mut self, group: i64) -> Result<Self, ValueError> {
                self.group = checked_group(group)?;
                Ok(self)
            }
        }
        impl PrimitiveMessage for $name {
            fn encode(&self) -> Result<Vec<u8>, ValueError> { Err(ValueError("MIDI 2 requires UMP encoding; convert explicitly for MIDI 1".into())) }
        }
        impl UmpValue for $name {
            fn encode_words(&self) -> Result<Vec<u32>, ValueError> {
                let mut words = vec![$first as u32, $second as u32];
                $(words[$word] |= ((self.$field as i64 + $bias) as u32) << $shift;)*
                Ok(words)
            }
        })*
        pub(crate) fn decode_musical(words: &[u32]) -> Result<Option<AnyMessage>, ValueError> {
            let key = words[0] & 0xf0f00000;
            $(if key == $first {
                $(let raw = (words[$word] >> $shift) & $mask;
                let $field = if stringify!($ty) == "i32" && ($bias as i64) == 0 { (raw as i32) as i64 } else { i64::from(raw) - $bias };)*
                let value = $name::new($($field),*)?;
                if value.encode_words()? != words { return Err(ValueError("nonzero reserved scalar fields".into())); }
                return Ok(Some(AnyMessage::$name(value)));
            })*
            Ok(None)
        }
    };
}
midi2_values!(musical_values);
