//! General MIDI profiles. Opening a port never selects a mode or bank.
use crate::messages::{ControlChange, ProgramChange, ValueError};

#[path = "gm_extensions.rs"]
mod extensions;
pub use extensions::{
    chorus_parameter, reverb_parameter, ControllerDestination, GlobalParameterControl,
    KeyBasedControl,
};

/// A GM1 program has no standardized bank address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Program {
    program: u8,
    bank_lsb: Option<u8>,
    name: &'static str,
    family: &'static str,
}
impl Program {
    pub fn bank_msb(&self) -> Option<u8> {
        self.bank_lsb.map(|_| 121)
    }
    pub fn bank_lsb(&self) -> Option<u8> {
        self.bank_lsb
    }
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
pub fn program(number: i64, bank_lsb: Option<i64>) -> Result<Option<Program>, ValueError> {
    ProgramChange::new(number).and_then(|m| m.with_channel(1))?;
    if let Some(bank_lsb) = bank_lsb {
        ControlChange::new(32, bank_lsb)?;
        return Ok(GM2_VOICES
            .iter()
            .find(|&&(program, bank, _)| bank as i64 == bank_lsb && program as i64 == number)
            .map(|&(program, bank_lsb, name)| Program {
                program,
                bank_lsb: Some(bank_lsb),
                name,
                family: FAMILIES[(program as usize - 1) / 8],
            }));
    }
    Ok(Some(Program {
        program: number as u8,
        bank_lsb: None,
        name: GM1_VOICES[number as usize - 1],
        family: FAMILIES[(number as usize - 1) / 8],
    }))
}
pub fn programs(version: i64) -> Result<Vec<Program>, ValueError> {
    match version {
        1 => Ok((1..=128)
            .map(|number| {
                program(number, None)
                    .expect("GM1 range")
                    .expect("GM1 entry")
            })
            .collect()),
        2 => Ok(GM2_VOICES
            .iter()
            .map(|&(program, bank_lsb, name)| Program {
                program,
                bank_lsb: Some(bank_lsb),
                name,
                family: FAMILIES[(program as usize - 1) / 8],
            })
            .collect()),
        _ => Err(ValueError("version must be 1 or 2".into())),
    }
}
pub fn find_programs(name: &str) -> Vec<Program> {
    let name = name.trim();
    programs(1)
        .expect("GM1 catalogue")
        .into_iter()
        .chain(programs(2).expect("GM2 catalogue"))
        .filter(|entry| entry.name.eq_ignore_ascii_case(name))
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
pub fn percussion(note: i64, program: Option<i64>) -> Result<Option<Percussion>, ValueError> {
    crate::messages::NoteOn::new(note)
        .and_then(|m| m.with_velocity(0))
        .and_then(|m| m.with_channel(10))?;
    let Some(program) = program else {
        return Ok(if (35..=81).contains(&note) {
            Some(Percussion {
                note: note as u8,
                name: GM1_DRUMS[note as usize - 35],
            })
        } else {
            None
        });
    };
    ProgramChange::new(program).and_then(|m| m.with_channel(10))?;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    Gm1,
    Gm2,
    Off,
}
#[derive(Clone, Debug)]
pub struct GmMode {
    metadata: Option<Box<crate::messages::Metadata>>,
    mode: Mode,
    device_id: u8,
}
crate::messages::value_metadata!(GmMode { mode, device_id });
impl GmMode {
    pub fn new(mode: Mode, device_id: i64) -> Result<Self, ValueError> {
        if !(0..=127).contains(&device_id) {
            return Err(ValueError("device_id must be in 0..=127".into()));
        }
        Ok(Self {
            metadata: None,
            mode,
            device_id: device_id as u8,
        })
    }
    pub fn mode(&self) -> Mode {
        self.mode
    }
    pub fn device_id(&self) -> u8 {
        self.device_id
    }
}
impl crate::messages::PrimitiveMessage for GmMode {
    fn encode(&self) -> Result<Vec<u8>, ValueError> {
        Ok(vec![
            240,
            126,
            self.device_id,
            9,
            match self.mode {
                Mode::Gm1 => 1,
                Mode::Off => 2,
                Mode::Gm2 => 3,
            },
            247,
        ])
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum GmMessage {
    GmMode(GmMode),
    ControllerDestination(ControllerDestination),
    KeyBasedControl(KeyBasedControl),
    GlobalParameterControl(GlobalParameterControl),
}
/// Decode a supported GM mode or extended message, bounding parameter entries.
pub fn decode(
    message: &crate::messages::SysEx,
    max_entries: usize,
) -> Result<Option<GmMessage>, ValueError> {
    if max_entries == 0 {
        return Err(ValueError("limit must be positive".into()));
    }
    let data = message.data();
    if data.len() < 3 || data[0] != 126 || data[2] != 9 {
        return extensions::decode(message, max_entries);
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
    let mode = match data[3] {
        1 => Mode::Gm1,
        2 => Mode::Off,
        _ => Mode::Gm2,
    };
    Ok(Some(GmMessage::GmMode(GmMode::new(mode, data[1].into())?)))
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
