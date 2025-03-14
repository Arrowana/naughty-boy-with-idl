use anchor_lang::prelude::*;

declare_id!("ETV8B7LGMaZYMRwWYimjKpwcoE1AQHNfFwMDynstK4wR");

#[program]
pub mod memo {
    use super::*;

    pub fn memo(_ctx: Context<Memo>, memo: String) -> Result<()> {
        msg!("You: {}", memo);
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Memo {}
