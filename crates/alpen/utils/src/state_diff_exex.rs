use std::sync::Arc;

use reth_alpen_primitives::state_diff::{StateDiff, StateDiffOp};
use reth_exex::{ExExContext, ExExEvent, ExExNotification};
use reth_node_api::FullNodeComponents;
use reth_primitives::B256;
use reth_provider::StateProviderFactory;
use tokio::sync::mpsc;

// struct ProverInput {
//     pub parent_storage: MptNode,
// }

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
