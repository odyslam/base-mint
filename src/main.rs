use clap::{command, Parser};
use futures_util::StreamExt;
use alloy::{sol, sol_types::*};
use alloy::providers::{Provider, ProviderBuilder, WsConnect};
use op_alloy_network::{Optimism, TransactionBuilder};
use alloy::rpc::types::TransactionRequest;
use sol_data::Bytes;
use eyre::Result;

sol!(
    #[allow(missing_docs)]
    #[derive(Debug, PartialEq, Eq)]
    function mint() external view;
    #[allow(missing_docs)]
    #[derive(Debug, PartialEq, Eq)]
    function decimals() public returns(uint8);
);

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// JSON RPC URL for the provider. It only supports OP-Stack based networks.
    #[arg(short, long, env = "RPC_URL")]
    rpc_url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Create the provider.
    let args = Cli::parse();
    let ws = WsConnect::new(args.rpc_url);
    let provider = ProviderBuilder::new().network::<Optimism>().on_ws(ws).await?;

    // Subscribe to new blocks.
    let mut sub = provider.subscribe_blocks().await?.into_stream();

    let mint = mintCall::new(());
    let decimals = decimalsCall::new(());

    println!("Awaiting blocks...");

    // Take the stream and print the block number upon receiving a new block.
    let handle = tokio::spawn(async move {
        loop {
            match sub.next().await {
                Some(block) => {
                    let block_id = block.header.hash.to_string();
                    println!("┌─────────────────────────────────────────────────────────────────────────");
                    println!("│ Block: {}", block.header.number);
                    match provider.get_block_receipts(block.header.hash.into()).await {
                        Ok(Some(receipts)) => {
                            let new_contracts = receipts.iter().filter_map(|r| r.inner.contract_address).collect::<Vec<alloy::primitives::Address>>();
                            for contract in new_contracts {
                                println!("├─ Contract Deployed: {:?}", contract);
                                let mint_call_tx: TransactionRequest = <TransactionRequest as TransactionBuilder<Optimism>>::with_call::<mintCall>(TransactionRequest::default().to(contract), &mint);
                                let mint_decimals_tx: TransactionRequest = <TransactionRequest as TransactionBuilder<Optimism>>::with_call::<decimalsCall>(TransactionRequest::default().to(contract), &decimals);
                                match provider.call(&mint_call_tx).await {
                                    Ok(res) => {
                                        let decimals_res = provider.call(&mint_decimals_tx).await;
                                        let code = provider.get_code_at(contract).await.unwrap();
                                        // fixme false positives that return 0x
                                        if decimals_res.is_ok() && code != alloy::primitives::Bytes::default(){
                                            println!("│  ├─ Mint() call result: {:?}", res);
                                            println!("│  ├─ Decimals() call result: {:?}", decimals_res.unwrap());
                                            println!("│  └─ Code: {:?}", code);
                                        }
                                    },
                                    Err(_) => continue,
                                }
                            }
                        },
                        Ok(None) => println!("│  No receipts found for block"),
                        Err(e) => println!("│  Error fetching block receipts: {:?}", e),
                    }
                    println!("└─────────────────────────────────────────────────────────────────────────");
                },
                None => {
                    eprintln!("Block stream ended unexpectedly");
                    break;
                },
            }
            // Wait for 1 second before the next iteration
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });

    handle.await?;

    Ok(())
}