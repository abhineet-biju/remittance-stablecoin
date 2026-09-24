use anchor_lang::{
    prelude::*,
    solana_program::program::invoke,
    system_program::{self, CreateAccount},
};
use anchor_spl::token_2022::{
    self,
    spl_token_2022::{
        extension::{default_account_state, metadata_pointer, transfer_fee, ExtensionType},
        instruction::initialize_mint_close_authority,
        state::{AccountState, Mint},
    },
    InitializeMint2, Token2022,
};

#[derive(Accounts)]
pub struct InitializeMint<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(mut)]
    pub mint: Signer<'info>,

    /// Controls issuance, freezing, fees, the metadata pointer, and mint closure.
    pub authority: Signer<'info>,

    pub token_program: Program<'info, Token2022>,
    pub system_program: Program<'info, System>,
}

impl<'info> InitializeMint<'info> {
    /// The maximum fee is expressed in raw token units.
    pub fn handler(
        &mut self,
        decimals: u8,
        transfer_fee_basis_points: u16,
        maximum_fee: u64,
    ) -> Result<()> {
        let space = ExtensionType::try_calculate_account_len::<Mint>(&[
            ExtensionType::TransferFeeConfig,
            ExtensionType::MetadataPointer,
            ExtensionType::DefaultAccountState,
            ExtensionType::MintCloseAuthority,
        ])?;

        system_program::create_account(
            CpiContext::new(
                self.system_program.key(),
                CreateAccount {
                    from: self.payer.to_account_info(),
                    to: self.mint.to_account_info(),
                },
            ),
            Rent::get()?.minimum_balance(space),
            space as u64,
            &self.token_program.key(),
        )?;

        // Initialize all four extensions before the base mint.
        let initialize_transfer_fee = transfer_fee::instruction::initialize_transfer_fee_config(
            &self.token_program.key(),
            &self.mint.key(),
            Some(&self.authority.key()),
            Some(&self.authority.key()),
            transfer_fee_basis_points,
            maximum_fee,
        )?;
        invoke(
            &initialize_transfer_fee,
            &[
                self.mint.to_account_info(),
                self.token_program.to_account_info(),
            ],
        )?;

        // The metadata payload will be initialized separately on this mint.
        let initialize_metadata_pointer = metadata_pointer::instruction::initialize(
            &self.token_program.key(),
            &self.mint.key(),
            Some(self.authority.key()),
            Some(self.mint.key()),
        )?;
        invoke(
            &initialize_metadata_pointer,
            &[
                self.mint.to_account_info(),
                self.token_program.to_account_info(),
            ],
        )?;

        let initialize_default_state =
            default_account_state::instruction::initialize_default_account_state(
                &self.token_program.key(),
                &self.mint.key(),
                &AccountState::Frozen,
            )?;
        invoke(
            &initialize_default_state,
            &[
                self.mint.to_account_info(),
                self.token_program.to_account_info(),
            ],
        )?;

        let initialize_close_authority = initialize_mint_close_authority(
            &self.token_program.key(),
            &self.mint.key(),
            Some(&self.authority.key()),
        )?;
        invoke(
            &initialize_close_authority,
            &[
                self.mint.to_account_info(),
                self.token_program.to_account_info(),
            ],
        )?;

        token_2022::initialize_mint2(
            CpiContext::new(
                self.token_program.key(),
                InitializeMint2 {
                    mint: self.mint.to_account_info(),
                },
            ),
            decimals,
            &self.authority.key(),
            Some(&self.authority.key()),
        )
    }
}
