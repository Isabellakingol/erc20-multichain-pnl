/*
Build:
  cargo init --bin erc20-multichain-pnl
  cargo add reqwest tokio serde serde_json anyhow clap csv rayon
Run:
  cargo run -- --config config.json --baseline baseline.json --out pnl.csv
Config (config.json):
  {"chains":[{"name":"eth","rpc":"https://mainnet.infura.io/v3/KEY","multicall":"0x5BA1e12693Dc8F9c48aAD8770482f4739bEeD696"}],
   "wallets":["0x...","0x..."], "tokens":["0x...","0x..."]}
*/
use std::{fs::File, collections::HashMap};
use clap::Parser;
use serde::{Deserialize, Serialize};
use anyhow::Result;
use rayon::prelude::*;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long)] config: String,
    #[arg(long)] baseline: String,
    #[arg(long, default_value="pnl.csv")] out: String,
}

#[derive(Deserialize)]
struct Config { chains: Vec<Chain>, wallets: Vec<String>, tokens: Vec<String> }

#[derive(Deserialize)]
struct Chain { name: String, rpc: String, multicall: String }

#[derive(Serialize, Default, Clone)]
struct Snapshot { chain: String, wallet: String, token: String, balance: String }

#[derive(Serialize, Default)]
struct PnL { chain: String, wallet: String, token: String, qty: f64, base_qty: f64, diff: f64 }

const ERC20_BALANCE_OF: &str = "0x70a08231"; // function selector

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let cfg: Config = serde_json::from_reader(File::open(&args.config)?)?;
    let base: HashMap<String, f64> = serde_json::from_reader(File::open(&args.baseline)?)?;

    let mut rows: Vec<PnL> = Vec::new();

    cfg.chains.par_iter().for_each(|ch| {
        let client = reqwest::blocking::Client::new();
        for w in &cfg.wallets {
            for t in &cfg.tokens {
                let data = format!("{sel}{pad_wallet}{pad_slot}",
                    sel=ERC20_BALANCE_OF,
                    pad_wallet=format!("{:0>64}", w.trim_start_matches("0x")),
                    pad_slot="0".repeat(64));
                let body = serde_json::json!({
                    "jsonrpc":"2.0","id":1,"method":"eth_call",
                    "params":[{"to":t, "data":format!("0x{}", data)}, "latest"]
                });
                let resp = client.post(&ch.rpc).json(&body).send();
                if let Ok(r) = resp {
                    if let Ok(js) = r.json::<serde_json::Value>() {
                        if let Some(hex) = js.get("result").and_then(|v| v.as_str()) {
                            let wei = i128::from_str_radix(hex.trim_start_matches("0x"), 16).unwrap_or(0);
                            let qty = (wei as f64) / 1e18;
                            let key = format!("{}:{}:{}", ch.name, w, t);
                            let base_qty = *base.get(&key).unwrap_or(&0.0);
                            rows.push(PnL { chain: ch.name.clone(), wallet: w.clone(), token: t.clone(), qty, base_qty, diff: qty - base_qty });
                        }
                    }
                }
            }
        }
    });

    let mut wtr = csv::Writer::from_path(&args.out)?;
    for r in &rows { wtr.serialize(r)?; }
    wtr.flush()?;
    std::fs::write("pnl.json", serde_json::to_vec_pretty(&rows)?)?;
    println!("Wrote {}, pnl.json", &args.out);
    Ok(())
}
