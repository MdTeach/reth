use std::{fs::File, io::Write, sync::Arc};

// use hashbrown::HashMap;
use reth_alpen_primitives::state_diff::{StateDiff, StateDiffOp};
use reth_exex::{ExExContext, ExExEvent, ExExNotification};
use reth_node_api::FullNodeComponents;
use reth_primitives::{Address, TransactionSignedNoHash, B256};
use reth_provider::{BlockReader, StateProviderFactory};
use reth_revm::db::BundleState;
use reth_rpc_types::EIP1186AccountProofResponse;
use reth_rpc_types_compat::proof::from_primitive_account_proof;
use serde_json::to_string;
use std::collections::HashMap;
use tokio::sync::mpsc;
use zkvm_primitives::{mpt::proofs_to_tries, SP1RethInput};

/// exex to extract and save state diffs for each block
pub async fn state_diff_exex<Node: FullNodeComponents>(
    mut ctx: ExExContext<Node>,
    tx: mpsc::UnboundedSender<StateDiffOp>,
) -> eyre::Result<()> {
    while let Some(notification) = ctx.notifications.recv().await {
        match notification {
            ExExNotification::ChainCommitted { new } => {
                let tip_height = new.tip().number;

                println!("*\n*\n**This is a good place to find a city**\n*\n*");
                //
                //

                let (current_block_num, _) = new.blocks().first_key_value().unwrap();

                let previous_provider =
                    ctx.provider().history_by_block_number(current_block_num - 1).unwrap();
                let current_provider = ctx.provider().latest().unwrap();
                let current_block =
                    ctx.provider().block_by_number(current_block_num.clone()).unwrap().unwrap();
                let current_block_txns = current_block
                    .body
                    .clone()
                    .into_iter()
                    .map(|tx| TransactionSignedNoHash::from(tx))
                    .collect::<Vec<TransactionSignedNoHash>>();

                let prev_block =
                    ctx.provider().block_by_number(current_block_num - 1).unwrap().unwrap();
                let prev_state_root = prev_block.state_root;

                let previous_bundle_state = BundleState::default();
                let current_bundle_state = &new.execution_outcome().bundle;

                // let p1 = prev_prov.proof(&bs, addrs, &[]).unwrap();
                // let p2 = prov.proof(&state, addrs, &[]).unwrap();
                let mut parent_proofs: HashMap<Address, EIP1186AccountProofResponse> =
                    HashMap::new();
                let mut current_proofs: HashMap<Address, EIP1186AccountProofResponse> =
                    HashMap::new();

                for (address, _) in current_bundle_state.state() {
                    let proof = previous_provider
                        .proof(&previous_bundle_state, address.clone(), &[])
                        .unwrap();
                    let proof = from_primitive_account_proof(proof);
                    parent_proofs.insert(address.clone(), proof);
                }

                for (address, _) in current_bundle_state.state() {
                    let proof = current_provider
                        .proof(&current_bundle_state, address.clone(), &[])
                        .unwrap();
                    let proof = from_primitive_account_proof(proof);
                    current_proofs.insert(address.clone(), proof);
                }

                // TODO: continue from here:
                let (state_trie, storage) = proofs_to_tries(
                    prev_state_root.into(),
                    parent_proofs.clone(),
                    current_proofs.clone(),
                )
                .unwrap();

                let input = SP1RethInput {
                    beneficiary: current_block.header.beneficiary,
                    gas_limit: current_block.gas_limit.try_into().unwrap(),
                    timestamp: current_block.header.timestamp.try_into().unwrap(),
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

                println!("input generation done now saving the file...");
                let json_str = to_string(&input).unwrap();
                // let mut file = File::create("output.json").unwrap();
                // file.write_all(json_str.as_bytes()).unwrap();
                let mut file = File::create(format!("{}.bin", current_block_num))
                    .expect("Unable to open the file");
                bincode::serialize_into(&mut file, &input).expect("Unable to serialize the input");
                file.write_all(json_str.as_bytes()).unwrap();
                println!("done saving the file :D");

                // handle the state diffs
                let new_diffs = extract_new_diffs(new);
                for (block_hash, state_diff) in new_diffs {
                    tx.send(StateDiffOp::Put(block_hash, state_diff))?;
                }

                ctx.events.send(ExExEvent::FinishedHeight(tip_height))?;
            }
            ExExNotification::ChainReorged { old, new } => {
                let tip_height = new.tip().number;

                let new_diffs = extract_new_diffs(new);
                let old_block_hashes = extract_old_block_hashes(old);

                for block_hash in old_block_hashes {
                    tx.send(StateDiffOp::Del(block_hash))?;
                }
                for (block_hash, state_diff) in new_diffs {
                    tx.send(StateDiffOp::Put(block_hash, state_diff))?;
                }

                ctx.events.send(ExExEvent::FinishedHeight(tip_height))?;
            }
            ExExNotification::ChainReverted { old } => {
                let old_block_hashes = extract_old_block_hashes(old);

                for block_hash in old_block_hashes {
                    tx.send(StateDiffOp::Del(block_hash))?;
                }
            }
        }
    }

    Ok(())
}

fn extract_new_diffs(new: Arc<reth_execution_types::Chain>) -> Vec<(B256, StateDiff)> {
    let blocks = new.blocks();
    new.range()
        .filter_map(|block_number| {
            blocks
                .get(&block_number)
                .map(|block| block.hash())
                .zip(new.execution_outcome_at_block(block_number))
        })
        .map(|(block_hash, execution_outcome)| (block_hash, execution_outcome.bundle.into()))
        .collect()
}

fn extract_old_block_hashes(old: Arc<reth_execution_types::Chain>) -> Vec<B256> {
    let blocks = old.blocks();
    old.range()
        .filter_map(|block_number| blocks.get(&block_number).map(|block| block.hash()))
        .collect()
}
