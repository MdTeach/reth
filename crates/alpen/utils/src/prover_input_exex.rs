use std::sync::Arc;

use reth_alpen_primitives::state_diff::{StateDiff, StateDiffOp};
use reth_exex::{ExExContext, ExExEvent, ExExNotification};
use reth_node_api::FullNodeComponents;
use reth_primitives::B256;
use tokio::sync::mpsc;

/// exex to extract and save prover input for each block
pub async fn prover_input_exex<Node: FullNodeComponents>(
    mut ctx: ExExContext<Node>,
    tx: mpsc::UnboundedSender<StateDiffOp>,
) -> eyre::Result<()> {
    while let Some(notification) = ctx.notifications.recv().await {
        match notification {
            ExExNotification::ChainCommitted { new } => {
                let tip_height = new.tip().number;
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
