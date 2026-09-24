use anchor_lang::prelude::*;

#[error_code]
pub enum ErrorCode {
    #[msg("The mint metadata has already been initialized")]
    MetadataAlreadyInitialized,
}
