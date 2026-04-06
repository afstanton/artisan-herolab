use thiserror::Error;

#[derive(Debug, Error)]
pub enum HerolabError {
    #[error("parse error: {0}")]
    Parse(String),
    #[error("unparse error: {0}")]
    Unparse(String),
}
