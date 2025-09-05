use clap::ValueEnum;

#[derive(Clone, Debug, ValueEnum, PartialEq)]
pub enum ExternalLrcProvider {
    Navidrome,
    NeteaseCloudMusic
}
