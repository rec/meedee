//! General MIDI profiles. Opening a port never selects a mode or bank.
use crate::messages::{AnyMessage, Command, ControlChange, Message, ProgramChange, ValueError};

#[path = "gm_extensions.rs"]
mod extensions;
pub use extensions::{
    chorus_parameter, decode_extended, reverb_parameter, ControllerDestination, ExtendedMessage,
    GlobalParameterControl, KeyBasedControl,
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BankProgramSelect {
    channel: u8,
    bank_msb: u8,
    bank_lsb: u8,
    program: u8,
}
impl BankProgramSelect {
    pub fn new(
        channel: i64,
        bank_msb: i64,
        bank_lsb: i64,
        program: i64,
    ) -> Result<Self, ValueError> {
        // Reuse the primitive validators; there is no implicit bank or mode.
        ControlChange::new(channel, 0, bank_msb)?;
        ControlChange::new(channel, 32, bank_lsb)?;
        ProgramChange::new(channel, program)?;
        Ok(Self {
            channel: channel as u8,
            bank_msb: bank_msb as u8,
            bank_lsb: bank_lsb as u8,
            program: program as u8,
        })
    }
    pub fn channel(&self) -> u8 {
        self.channel
    }
    pub fn bank_msb(&self) -> u8 {
        self.bank_msb
    }
    pub fn bank_lsb(&self) -> u8 {
        self.bank_lsb
    }
    pub fn program(&self) -> u8 {
        self.program
    }
}
impl Message for BankProgramSelect {}
impl Command for BankProgramSelect {
    fn expand(&self) -> Vec<AnyMessage> {
        vec![
            AnyMessage::ControlChange(
                ControlChange::new(self.channel.into(), 0, self.bank_msb.into())
                    .expect("validated bank"),
            ),
            AnyMessage::ControlChange(
                ControlChange::new(self.channel.into(), 32, self.bank_lsb.into())
                    .expect("validated bank"),
            ),
            AnyMessage::ProgramChange(
                ProgramChange::new(self.channel.into(), self.program.into())
                    .expect("validated program"),
            ),
        ]
    }
}

/// A GM1 program has no standardized bank address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Program {
    program: u8,
    name: &'static str,
    family: &'static str,
}
impl Program {
    pub fn program(&self) -> u8 {
        self.program
    }
    pub fn name(&self) -> &'static str {
        self.name
    }
    pub fn family(&self) -> &'static str {
        self.family
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BankProgram {
    program: u8,
    bank_lsb: u8,
    name: &'static str,
    family: &'static str,
}
impl BankProgram {
    pub fn program(&self) -> u8 {
        self.program
    }
    pub fn bank_msb(&self) -> u8 {
        121
    }
    pub fn bank_lsb(&self) -> u8 {
        self.bank_lsb
    }
    pub fn name(&self) -> &'static str {
        self.name
    }
    pub fn family(&self) -> &'static str {
        self.family
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DrumKit {
    program: u8,
    name: &'static str,
}
impl DrumKit {
    pub fn program(&self) -> u8 {
        self.program
    }
    pub fn name(&self) -> &'static str {
        self.name
    }
    pub fn bank_msb(&self) -> u8 {
        120
    }
    pub fn bank_lsb(&self) -> u8 {
        0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Percussion {
    note: u8,
    name: &'static str,
}
impl Percussion {
    pub fn note(&self) -> u8 {
        self.note
    }
    pub fn name(&self) -> &'static str {
        self.name
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProgramMatch {
    Gm1(Program),
    Gm2(BankProgram),
}
pub fn program(number: i64) -> Result<Program, ValueError> {
    ProgramChange::new(1, number)?;
    Ok(Program {
        program: number as u8,
        name: GM1_VOICES[number as usize - 1],
        family: FAMILIES[(number as usize - 1) / 8],
    })
}
pub fn programs() -> Vec<Program> {
    (1..=128)
        .map(|number| program(number).expect("GM1 range"))
        .collect()
}
pub fn gm2_programs() -> Vec<BankProgram> {
    GM2_VOICES
        .iter()
        .map(|&(program, bank_lsb, name)| BankProgram {
            program,
            bank_lsb,
            name,
            family: FAMILIES[(program as usize - 1) / 8],
        })
        .collect()
}
pub fn gm2_program(bank_lsb: i64, number: i64) -> Result<Option<BankProgram>, ValueError> {
    ControlChange::new(1, 32, bank_lsb)?;
    ProgramChange::new(1, number)?;
    Ok(GM2_VOICES
        .iter()
        .find(|&&(program, bank, _)| bank as i64 == bank_lsb && program as i64 == number)
        .map(|&(program, bank_lsb, name)| BankProgram {
            program,
            bank_lsb,
            name,
            family: FAMILIES[(program as usize - 1) / 8],
        }))
}
pub fn find_programs(name: &str) -> Vec<ProgramMatch> {
    let name = name.trim();
    programs()
        .into_iter()
        .filter(|entry| entry.name.eq_ignore_ascii_case(name))
        .map(ProgramMatch::Gm1)
        .chain(
            gm2_programs()
                .into_iter()
                .filter(|entry| entry.name.eq_ignore_ascii_case(name))
                .map(ProgramMatch::Gm2),
        )
        .collect()
}
pub fn drum_kits() -> Vec<DrumKit> {
    DRUM_KITS
        .iter()
        .map(|&(program, name)| DrumKit { program, name })
        .collect()
}
pub fn find_drum_kits(name: &str) -> Vec<DrumKit> {
    drum_kits()
        .into_iter()
        .filter(|entry| entry.name.eq_ignore_ascii_case(name.trim()))
        .collect()
}
pub fn percussion(note: i64) -> Result<Option<Percussion>, ValueError> {
    crate::messages::NoteOn::new(10, note, 0)?;
    Ok(if (35..=81).contains(&note) {
        Some(Percussion {
            note: note as u8,
            name: GM1_DRUMS[note as usize - 35],
        })
    } else {
        None
    })
}
/// Undefined/silent notes and unsupported kit programs return None. No fallback
/// instrument is invented, even where a GM2 sound generator should apply one.
pub fn gm2_percussion(program: i64, note: i64) -> Result<Option<Percussion>, ValueError> {
    ProgramChange::new(10, program)?;
    crate::messages::NoteOn::new(10, note, 0)?;
    if !DRUM_KITS.iter().any(|entry| entry.0 as i64 == program) {
        return Ok(None);
    }
    if let Some((_, _, name)) = GM2_DRUM_OVERRIDES
        .iter()
        .find(|&&(p, n, _)| p as i64 == program && n as i64 == note)
    {
        return Ok(name.map(|name| Percussion {
            note: note as u8,
            name,
        }));
    }
    Ok(if (27..=87).contains(&note) {
        Some(Percussion {
            note: note as u8,
            name: GM2_STANDARD[note as usize - 27],
        })
    } else {
        None
    })
}

macro_rules! modes {
    ($($name:ident => $code:literal),*) => {$(
        #[derive(Clone,Debug,PartialEq,Eq,Hash)]
        pub struct $name {device_id:u8}
        impl $name {
            pub fn new(device_id:i64)->Result<Self,ValueError> {
                if !(0..=127).contains(&device_id) {return Err(ValueError("device_id must be in 0..=127".into()));}
                Ok(Self {device_id:device_id as u8})
            }
            pub fn device_id(&self)->u8 {self.device_id}
        }
        impl Message for $name {}
        impl crate::messages::PrimitiveMessage for $name {
            fn encode(&self)->Vec<u8> {vec![240,126,self.device_id,9,$code,247]}
        }
    )*};
}
modes!(Gm1On=>1,GmOff=>2,Gm2On=>3);
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ModeMessage {
    Gm1On(Gm1On),
    GmOff(GmOff),
    Gm2On(Gm2On),
}
pub fn decode(message: &crate::messages::SysEx) -> Result<Option<ModeMessage>, ValueError> {
    let data = message.data();
    if data.len() < 3 || data[0] != 126 || data[2] != 9 {
        return Ok(None);
    }
    if data.len() < 4 {
        return Err(ValueError("truncated GM header".into()));
    }
    if !(1..=3).contains(&data[3]) {
        return Ok(None);
    }
    if data.len() != 4 {
        return Err(ValueError("incorrect GM mode message length".into()));
    }
    Ok(Some(match data[3] {
        1 => ModeMessage::Gm1On(Gm1On::new(data[1].into())?),
        2 => ModeMessage::GmOff(GmOff::new(data[1].into())?),
        _ => ModeMessage::Gm2On(Gm2On::new(data[1].into())?),
    }))
}

include!("gm_tables.rs");
const FAMILIES: [&str; 16] = [
    "Piano",
    "Chromatic Percussion",
    "Organ",
    "Guitar",
    "Bass",
    "Strings",
    "Ensemble",
    "Brass",
    "Reed",
    "Pipe",
    "Synth Lead",
    "Synth Pad",
    "Synth Effects",
    "Ethnic",
    "Percussive",
    "Sound Effects",
];
const DRUM_KITS: &[(u8, &str)] = &[
    (1, "STANDARD Set"),
    (9, "ROOM Set"),
    (17, "POWER Set"),
    (25, "ELECTRONIC Set"),
    (26, "ANALOG Set"),
    (33, "JAZZ Set"),
    (41, "BRUSH Set"),
    (49, "ORCHESTRA Set"),
    (57, "SFX Set"),
];
const GM1_VOICES: [&str; 128] = [
    "Acoustic Grand Piano",
    "Bright Acoustic Piano",
    "Electric Grand Piano",
    "Honky-tonk Piano",
    "Electric Piano 1",
    "Electric Piano 2",
    "Harpsichord",
    "Clavi",
    "Celesta",
    "Glockenspiel",
    "Music Box",
    "Vibraphone",
    "Marimba",
    "Xylophone",
    "Tubular Bells",
    "Dulcimer",
    "Drawbar Organ",
    "Percussive Organ",
    "Rock Organ",
    "Church Organ",
    "Reed Organ",
    "Accordion",
    "Harmonica",
    "Tango Accordion",
    "Acoustic Guitar (nylon)",
    "Acoustic Guitar (steel)",
    "Electric Guitar (jazz)",
    "Electric Guitar (clean)",
    "Electric Guitar (muted)",
    "Overdriven Guitar",
    "Distortion Guitar",
    "Guitar harmonics",
    "Acoustic Bass",
    "Electric Bass (finger)",
    "Electric Bass (pick)",
    "Fretless Bass",
    "Slap Bass 1",
    "Slap Bass 2",
    "Synth Bass 1",
    "Synth Bass 2",
    "Violin",
    "Viola",
    "Cello",
    "Contrabass",
    "Tremolo Strings",
    "Pizzicato Strings",
    "Orchestral Harp",
    "Timpani",
    "String Ensemble 1",
    "String Ensemble 2",
    "SynthStrings 1",
    "SynthStrings 2",
    "Choir Aahs",
    "Voice Oohs",
    "Synth Voice",
    "Orchestra Hit",
    "Trumpet",
    "Trombone",
    "Tuba",
    "Muted Trumpet",
    "French Horn",
    "Brass Section",
    "SynthBrass 1",
    "SynthBrass 2",
    "Soprano Sax",
    "Alto Sax",
    "Tenor Sax",
    "Baritone Sax",
    "Oboe",
    "English Horn",
    "Bassoon",
    "Clarinet",
    "Piccolo",
    "Flute",
    "Recorder",
    "Pan Flute",
    "Blown Bottle",
    "Shakuhachi",
    "Whistle",
    "Ocarina",
    "Lead 1 (square)",
    "Lead 2 (sawtooth)",
    "Lead 3 (calliope)",
    "Lead 4 (chiff)",
    "Lead 5 (charang)",
    "Lead 6 (voice)",
    "Lead 7 (fifths)",
    "Lead 8 (bass + lead)",
    "Pad 1 (new age)",
    "Pad 2 (warm)",
    "Pad 3 (polysynth)",
    "Pad 4 (choir)",
    "Pad 5 (bowed)",
    "Pad 6 (metallic)",
    "Pad 7 (halo)",
    "Pad 8 (sweep)",
    "FX 1 (rain)",
    "FX 2 (soundtrack)",
    "FX 3 (crystal)",
    "FX 4 (atmosphere)",
    "FX 5 (brightness)",
    "FX 6 (goblins)",
    "FX 7 (echoes)",
    "FX 8 (sci-fi)",
    "Sitar",
    "Banjo",
    "Shamisen",
    "Koto",
    "Kalimba",
    "Bag pipe",
    "Fiddle",
    "Shanai",
    "Tinkle Bell",
    "Agogo",
    "Steel Drums",
    "Woodblock",
    "Taiko Drum",
    "Melodic Tom",
    "Synth Drum",
    "Reverse Cymbal",
    "Guitar Fret Noise",
    "Breath Noise",
    "Seashore",
    "Bird Tweet",
    "Telephone Ring",
    "Helicopter",
    "Applause",
    "Gunshot",
];
const GM1_DRUMS: [&str; 47] = [
    "Acoustic Bass Drum",
    "Bass Drum 1",
    "Side Stick",
    "Acoustic Snare",
    "Hand Clap",
    "Electric Snare",
    "Low Floor Tom",
    "Closed Hi Hat",
    "High Floor Tom",
    "Pedal Hi-Hat",
    "Low Tom",
    "Open Hi-Hat",
    "Low-Mid Tom",
    "Hi Mid Tom",
    "Crash Cymbal 1",
    "High Tom",
    "Ride Cymbal 1",
    "Chinese Cymbal",
    "Ride Bell",
    "Tambourine",
    "Splash Cymbal",
    "Cowbell",
    "Crash Cymbal 2",
    "Vibraslap",
    "Ride Cymbal 2",
    "Hi Bongo",
    "Low Bongo",
    "Mute Hi Conga",
    "Open Hi Conga",
    "Low Conga",
    "High Timbale",
    "Low Timbale",
    "High Agogo",
    "Low Agogo",
    "Cabasa",
    "Maracas",
    "Short Whistle",
    "Long Whistle",
    "Short Guiro",
    "Long Guiro",
    "Claves",
    "Hi Wood Block",
    "Low Wood Block",
    "Mute Cuica",
    "Open Cuica",
    "Mute Triangle",
    "Open Triangle",
];
pub const GM1_DRUM_CHANNEL: u8 = 10;
pub const GM2_ON_SETTLING_NS: u64 = 100_000_000;
