use std::str::FromStr;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use ed25519_dalek::Signer;
use ed25519_dalek::Verifier;
use everscale_jrpc_client::{
    JrpcClient, JrpcClientOptions, SendOptions, SendStatus, TransportErrorAction,
};
use nekoton::abi;
use nekoton::core::models::Expiration;
use nekoton::core::ton_wallet::{wallet_v3, Gift, TransferAction, WalletType};
use nekoton::crypto::MnemonicType;
use pyo3::prelude::*;
use tokio::sync::Mutex;
use ton_block::MsgAddressInt;
use ton_types::SliceData;

mod utils;

#[pyclass]
pub struct TonSigner {
    signer: ed25519_dalek::Keypair,
    client: everscale_jrpc_client::JrpcClient,
    send_mutex: Mutex<()>,
    ctx: tokio::runtime::Runtime,
}

#[pymethods]
impl TonSigner {
    #[new]
    pub fn new(phrase: &str, endpoint: &str) -> PyResult<Self> {
        let signer = nekoton::crypto::derive_from_phrase(phrase, MnemonicType::Labs(0))?;
        let runtime = tokio::runtime::Runtime::new().context("Failed to create runtime")?;
        let client = runtime
            .block_on(async {
                JrpcClient::new(
                    [endpoint.parse().context("Bad endpoint url")?],
                    JrpcClientOptions::default(),
                )
                .await
            })
            .context("Failed to create client")?;

        Ok(Self {
            signer,
            ctx: runtime,
            client,
            send_mutex: Mutex::new(()),
        })
    }

    #[pyo3(text_signature = "($self, hash)")]
    pub fn sign(&self, hash: &str) -> PyResult<String> {
        if hash.len() != 64 {
            return Err(anyhow::anyhow!("hash should be 64 hex symbols in len").into());
        }

        let hash = hex::decode(hash).context("Failed decoding hex string")?;
        let signature = self.signer.sign(hash.as_slice());

        Ok(hex::encode(signature))
    }

    #[pyo3(text_signature = "($self, to, amount)")]
    pub fn send_evers(&self, to: &str, amount: u64) -> PyResult<String> {
        let to = ton_block::MsgAddressInt::from_str(to).context("Invalid address")?;

        Ok(self
            .ctx
            .block_on(async { send_evers_inner(self, to, amount, None).await })?)
    }

    #[pyo3(text_signature = "($self, contract_address, attach_amount, abi, method, arguments)")]
    pub fn call(
        &self,
        contract_address: &str,
        attach_amount: u64,
        abi: &str,
        method: &str,
        arguments: &str,
    ) -> PyResult<String> {
        let res = self.ctx.block_on(async {
            call(
                self,
                contract_address,
                attach_amount,
                abi,
                method,
                arguments,
            )
            .await
        })?;

        Ok(res)
    }

    #[pyo3(text_signature = "($self, address: str, signature: str, message: str)")]
    pub fn check_signature(&self, address: &str, signature: &str, message: &str) -> PyResult<bool> {
        let address = ton_block::MsgAddressInt::from_str(address).context("Invalid address")?;
        let signature = hex::decode(signature).context("Failed decoding signature hex string")?;
        let message = hex::decode(message).context("Failed decoding message string")?;
        if signature.len() != 64 {
            return Err(anyhow::anyhow!("signature should be 128 hex symbols in len").into());
        }

        let signature = ed25519_dalek::Signature::from_bytes(signature.as_slice())
            .context("Failed decoding signature")?;
        let res = self
            .ctx
            .block_on(async { check_signature(self, &address, &signature, &message).await })?;
        let res = match res {
            Some(res) => res,
            None => return Err(anyhow::anyhow!("{address} state is not found").into()),
        };

        Ok(res)
    }
}

async fn check_signature(
    client: &TonSigner,
    address: &MsgAddressInt,
    signature: &ed25519_dalek::Signature,
    message: &[u8],
) -> Result<Option<bool>> {
    let state = client.client.get_contract_state(address).await?;
    let state = match state {
        Some(state) => state,
        None => return Ok(None),
    };

    let pubkey = utils::extract_public_key(&state.account).context("Failed to get pubkey")?;

    Ok(Some(pubkey.verify(message, signature).is_ok()))
}

async fn send_evers_inner(
    client: &TonSigner,
    to: MsgAddressInt,
    amount: u64,
    body: Option<SliceData>,
) -> Result<String> {
    let from =
        nekoton::core::ton_wallet::compute_address(&client.signer.public, WalletType::WalletV3, 0);
    let state = client
        .client
        .get_contract_state(&from)
        .await?
        .map(|x| x.account)
        .unwrap_or_default();

    let seqno_offset = 0; //todo: tweak this

    let tx = wallet_v3::prepare_transfer(
        &nekoton::utils::SimpleClock,
        &client.signer.public,
        &state,
        seqno_offset,
        vec![Gift {
            flags: 3,
            bounce: false,
            destination: to.clone(),
            amount,
            body,
            state_init: None,
        }],
        Expiration::Timeout(60),
    )
    .unwrap();

    let message = match tx {
        TransferAction::DeployFirst => {
            unreachable!("Wallet3 doesn't need to be deployed")
        }
        TransferAction::Sign(m) => m,
    };

    let _guard = client.send_mutex.lock().await;
    let signature = client.signer.sign(message.hash()).to_bytes();
    let signed_message = message.sign(&signature).expect("invalid signature");

    let message = signed_message.message;
    let send_options = SendOptions {
        error_action: TransportErrorAction::Return,
        ttl: Duration::from_secs(60),
        poll_interval: Duration::from_secs(1),
    };
    let result = client.client.send_message(message, send_options).await;

    match result {
        Ok(SendStatus::Confirmed) => Ok("Confirmed".to_string()),
        Ok(SendStatus::Expired) => Ok("Pending".to_string()),
        Err(err) => Err(err),
    }
}

async fn call(
    client: &TonSigner,
    contract_address: &str,
    attach_amount: u64,
    abi: &str,
    method: &str,
    arguments: &str,
) -> Result<String> {
    let contract_address = MsgAddressInt::from_str(contract_address).context("Invalid address")?;
    let abi = ton_abi::Contract::load(abi).context("Invalid abi")?;
    let method = abi.function(method).context("Invalid method")?;
    let arguments = serde_json::from_str(arguments).context("Invalid arguments")?;

    let arguments =
        abi::parse_abi_tokens(&method.inputs, arguments).context("Failed to parse arguments")?;
    let input = method
        .encode_input(&Default::default(), &arguments, true, None, None)
        .context("Failed to encode input")?;
    let body = input
        .into_cell()
        .context("Failed to pack builder data to cell")?
        .into();

    send_evers_inner(client, contract_address, attach_amount, Some(body)).await
}

/// A Python module implemented in Rust.
#[pymodule]
fn pyever_send(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<TonSigner>()?;
    Ok(())
}
