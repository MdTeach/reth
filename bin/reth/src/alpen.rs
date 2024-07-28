#![allow(missing_docs)]

use reth_alpen_utils::state_diff_exex;
use reth_cli_util::sigsegv_handler;

// We use jemalloc for performance reasons.
#[cfg(all(feature = "jemalloc", unix))]
#[global_allocator]
static ALLOC: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

#[cfg(not(feature = "alpen"))]
compile_error!("Cannot build the `alpen-reth` binary with the `alpen` feature flag disabled. Did you mean to build `reth`?");

#[cfg(feature = "alpen")]
fn main() {
    use reth::cli::Cli;
    use reth_node_ethereum::EthereumNode;

    sigsegv_handler::install();

    // Enable backtraces unless a RUST_BACKTRACE value has already been explicitly provided.
    if std::env::var_os("RUST_BACKTRACE").is_none() {
        std::env::set_var("RUST_BACKTRACE", "1");
    }

    if let Err(err) = Cli::parse_args().run(|builder, _| async {
        let handle = builder
            .node(EthereumNode::default())
            .install_exex("test", |ctx| async {
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

                // TODO: sync to db
                ctx.task_executor().spawn(async move {
                    loop {
                        let _ = rx.recv().await;
                    }
                });

                Ok(state_diff_exex(ctx, tx))
            })
            .launch()
            .await?;
        handle.node_exit_future.await
    }) {
        eprintln!("Error: {err:?}");
        std::process::exit(1);
    }
}
