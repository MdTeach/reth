use std::{collections::HashMap, fs::File, io::Write, sync::Arc};

use alloy_rpc_types::EIP1186AccountProofResponse;
use eyre::eyre;
use reth_exex::{ExExContext, ExExEvent, ExExNotification};
use reth_node_api::FullNodeComponents;
use reth_primitives::{Address, TransactionSignedNoHash};
use reth_provider::{BlockReader, Chain, StateProviderFactory};
use reth_revm::db::BundleState;
use reth_rpc_types_compat::proof::from_primitive_account_proof;
use zkvm_primitives::{mpt::proofs_to_tries, ZKVMInput};

/// exex to extract and save prover input for each block
pub async fn prover_input_exex<Node: FullNodeComponents>(
    mut ctx: ExExContext<Node>,
) -> eyre::Result<()> {
    while let Some(notification) = ctx.notifications.recv().await {
        match notification {
            ExExNotification::ChainCommitted { new } => {
                let tip_height = new.tip().number;
                let input = extract_zkvm_input(&ctx, new)?;

                let mut file = File::create(format!("{}.bin", 1)).expect("Unable to open the file");
                let bin = bincode::serialize(&input).unwrap();
                let res = file.write_all(&bin).unwrap();

                ctx.events.send(ExExEvent::FinishedHeight(tip_height))?;
            }
            ExExNotification::ChainReorged { old, new } => {
                let tip_height = new.tip().number;
                ctx.events.send(ExExEvent::FinishedHeight(tip_height))?;
            }
            ExExNotification::ChainReverted { old } => {}
        }
    }

    Ok(())
}

fn extract_zkvm_input<Node: FullNodeComponents>(
    ctx: &ExExContext<Node>,
    new: Arc<Chain>,
) -> eyre::Result<ZKVMInput> {
    let (current_block_num, _) =
        new.blocks().first_key_value().ok_or(eyre!("Failed to get current block"))?;
    let previous_provider = ctx.provider().history_by_block_number(current_block_num - 1)?;
    let current_provider = ctx.provider().latest()?;

    let current_block = ctx
        .provider()
        .block_by_number(current_block_num.clone())?
        .ok_or(eyre!("Failed to get current block"))?;

    let current_block_txns = current_block
        .body
        .clone()
        .into_iter()
        .map(|tx| TransactionSignedNoHash::from(tx))
        .collect::<Vec<TransactionSignedNoHash>>();

    let prev_block = ctx
        .provider()
        .block_by_number(current_block_num - 1)?
        .ok_or(eyre!("Failed to get prev block"))?;
    let prev_state_root = prev_block.state_root;

    let previous_bundle_state = BundleState::default();
    let current_bundle_state = &new.execution_outcome().bundle;

    let mut parent_proofs: HashMap<Address, EIP1186AccountProofResponse> = HashMap::new();
    let mut current_proofs: HashMap<Address, EIP1186AccountProofResponse> = HashMap::new();

    // Accumulate account proof of account in previous block
    for (address, _) in current_bundle_state.state() {
        let proof = previous_provider.proof(&previous_bundle_state, address.clone(), &[])?;

        let proof = from_primitive_account_proof(proof);
        // TODO: fix this
        let proof_str = serde_json::to_string(&proof)?;
        let proof: EIP1186AccountProofResponse =
            serde_json::from_str::<EIP1186AccountProofResponse>(&proof_str)?;
        parent_proofs.insert(address.clone(), proof);
    }

    // Accumulate account proof of account in current block
    for (address, _) in current_bundle_state.state() {
        let proof = current_provider.proof(&current_bundle_state, address.clone(), &[])?;
        let proof = from_primitive_account_proof(proof);
        let proof_str = serde_json::to_string(&proof)?;
        let proof: EIP1186AccountProofResponse =
            serde_json::from_str::<EIP1186AccountProofResponse>(&proof_str)?;
        current_proofs.insert(address.clone(), proof);
    }

    let (state_trie, storage) =
        proofs_to_tries(prev_state_root.into(), parent_proofs.clone(), current_proofs.clone())
            .expect("Proof to tries infallable");

    let input = ZKVMInput {
        beneficiary: current_block.header.beneficiary,
        gas_limit: current_block.gas_limit.try_into()?,
        timestamp: current_block.header.timestamp.try_into()?,
        extra_data: current_block.header.extra_data,
        mix_hash: current_block.header.mix_hash,
        transactions: current_block_txns,
        withdrawals: Vec::new(),
        parent_state_trie: state_trie,
        parent_storage: storage,
        contracts: Default::default(),
        parent_header: prev_block.header,
        ancestor_headers: Default::default(),
    };

    Ok(input)
}
