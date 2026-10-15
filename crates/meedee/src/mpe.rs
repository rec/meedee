//! MPE 1.1 zone configuration only. No voice allocator or receive-state inference.
use crate::messages::{AnyMessage, Command, Message, RpnSet, ValueError};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ZoneConfiguration {
    manager_channel: u8,
    members: u8,
}
impl ZoneConfiguration {
    pub fn new(manager_channel: i64, members: i64) -> Result<Self, ValueError> {
        if manager_channel != 1 && manager_channel != 16 {
            return Err(ValueError("manager_channel must be 1 or 16".into()));
        }
        if !(0..=15).contains(&members) {
            return Err(ValueError("members must be in 0..=15".into()));
        }
        Ok(Self {
            manager_channel: manager_channel as u8,
            members: members as u8,
        })
    }
    pub fn manager_channel(&self) -> u8 {
        self.manager_channel
    }
    pub fn members(&self) -> u8 {
        self.members
    }
}
impl Message for ZoneConfiguration {}
impl Command for ZoneConfiguration {
    fn expand(&self) -> Vec<AnyMessage> {
        RpnSet::new(self.manager_channel.into(), 6, self.members.into(), 7)
            .expect("validated MPE configuration fields")
            .expand()
    }
}
