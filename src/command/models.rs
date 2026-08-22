#[derive(Debug, Clone, Copy)]
pub enum CommandVersion {
    /// `h=303`
    V0,
    /// `h=16`
    V1,
    /// `h=0`
    V2,
}
