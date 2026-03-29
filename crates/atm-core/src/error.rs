use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("bootstrap placeholder error")]
    Bootstrap,
}
