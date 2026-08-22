use crate::command::{envelope::Envelope, models::CommandVersion};

pub trait Command {
    fn pack(&self, cv: CommandVersion) -> Vec<Envelope>;
}

pub struct PageStart {}

impl PageStart {}

impl Command for PageStart {
    fn pack(&self, cv: CommandVersion) -> Vec<Envelope> {
        todo!()
    }
}
