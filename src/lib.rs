use std::future::Future;
use std::str::FromStr;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use ed25519_dalek::Signer;
use ed25519_dalek::Verifier;
use everscale_jrpc_client::{
    JrpcClient, JrpcClientOptions, SendOptions, SendStatus, TransportErrorAction,
};
use log::LevelFilter;
use nekoton::abi;
use nekoton::core::models::Expiration;
use nekoton::core::ton_wallet::{compute_address, ever_wallet, Gift, TransferAction, WalletType};
use nekoton::crypto::MnemonicType;
use pyo3::marker::Ungil;
use pyo3::prelude::*;
use tokio::sync::Mutex;
use ton_block::{GetRepresentationHash, MsgAddressInt};
use ton_types::SliceData;

mod utils;

#[pyclass]
pub struct TonSigner {
    signer: ed25519_dalek::Keypair,
    client: everscale_jrpc_client::JrpcClient,
    send_mutex: Mutex<()>,
    ctx: tokio::runtime::Runtime,
}

impl TonSigner {
    fn block_on<F>(&self, future: F) -> F::Output
    where
        F: Future + Send + Ungil,
        F::Output: Send + Ungil,
    {
        Python::with_gil(|py| py.allow_threads(move || self.ctx.block_on(future)))
    }
}

#[pymethods]
impl TonSigner {
    #[new]
    pub fn new(py: Python<'_>, phrase: &str, endpoint: &str) -> PyResult<Self> {
        let signer = nekoton::crypto::derive_from_phrase(phrase, MnemonicType::Labs(0))
            .context("Invalid seed")?;
        let runtime = tokio::runtime::Runtime::new().context("Failed to create runtime")?;

        let client = py.allow_threads(|| {
            runtime
                .block_on(async {
                    JrpcClient::new(
                        [endpoint.parse().context("Bad endpoint url")?],
                        JrpcClientOptions::default(),
                    )
                    .await
                })
                .context("Failed to create client")
        })?;

        Ok(Self {
            signer,
            ctx: runtime,
            client,
            send_mutex: Mutex::new(()),
        })
    }

    pub fn wallet_address(&self) -> PyResult<String> {
        let address = compute_address(&self.signer.public, WalletType::EverWallet, 0);
        Ok(address.to_string())
    }

    pub fn balance_of(&self, address: &str) -> PyResult<u64> {
        let address = MsgAddressInt::from_str(address).context("Bad address")?;
        let state = self
            .block_on(async { self.client.get_contract_state(&address).await })
            .context("Failed to get account state")?
            .context("Account does not exist")?
            .brief();

        Ok(state.balance)
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

        Ok(self.block_on(async {
            send_evers_inner(
                self,
                vec![Gift {
                    flags: 3,
                    amount,
                    bounce: false,
                    destination: to,
                    state_init: None,
                    body: None,
                }],
            )
            .await
        })?)
    }

    #[pyo3(text_signature = "($self, contract_address, attach_amount, abi, method, arguments)")]
    pub fn make_call_payload(
        &self,
        contract_address: &str,
        attach_amount: u64,
        abi: &str,
        method: &str,
        arguments: &str,
    ) -> PyResult<SendPayload> {
        let payload = call(
            contract_address,
            attach_amount,
            abi,
            method,
            arguments,
            false,
        )?;

        let payload: SendPayload = payload.try_into()?;

        Ok(payload)
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
        let payload = call(
            contract_address,
            attach_amount,
            abi,
            method,
            arguments,
            false,
        )?;

        let res = self
            .ctx
            .block_on(async { send_evers_inner(self, vec![payload]).await })?;

        Ok(res)
    }

    pub fn call_multi(&self, py: Python<'_>, payloads: PyObject) -> PyResult<String> {
        let payloads: Vec<SendPayload> = payloads.extract(py).context("Failed to map payloads")?;
        let payloads: Vec<Gift> = payloads
            .into_iter()
            .map(|payload| payload.try_into())
            .collect::<Result<Vec<_>>>()?;

        if payloads.len() > 3 || payloads.is_empty() {
            return Err(anyhow::anyhow!(
                "Invalid payloads count: {}. Expected 1..4",
                payloads.len()
            )
            .into());
        }

        let res = self
            .ctx
            .block_on(async { send_evers_inner(self, payloads).await })?;

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

async fn send_evers_inner(client: &TonSigner, gifts: Vec<Gift>) -> Result<String> {
    let from = compute_address(&client.signer.public, WalletType::EverWallet, 0);
    let state = client
        .client
        .get_contract_state(&from)
        .await?
        .map(|x| x.account)
        .unwrap_or_default();

    let tx = ever_wallet::prepare_transfer(
        &nekoton::utils::SimpleClock,
        &client.signer.public,
        &state,
        from,
        gifts,
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
        Ok(SendStatus::Confirmed(tx)) => tx_status(&tx),
        Ok(SendStatus::Expired) => anyhow::bail!("Message expired"),
        Err(err) => Err(err.context("internal error")),
    }
}

fn tx_status(tx: &ton_block::Transaction) -> Result<String> {
    if tx.read_description()?.is_aborted() {
        anyhow::bail!("Transaction aborted. Hash: {}", tx.hash()?.to_hex_string());
    } else {
        Ok(tx.hash()?.to_hex_string())
    }
}

fn call(
    contract_address: &str,
    attach_amount: u64,
    abi: &str,
    method: &str,
    arguments: &str,
    bounce: bool,
) -> Result<Gift> {
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

    let gift = Gift {
        flags: 3,
        bounce,
        destination: contract_address,
        amount: attach_amount,
        body: Some(body),
        state_init: None,
    };

    Ok(gift)
}

#[pyclass]
#[derive(FromPyObject)]
pub struct SendPayload {
    #[pyo3(get)]
    pub flags: u8,
    #[pyo3(get)]
    pub bounce: bool,
    #[pyo3(get)]
    pub destination: String,
    #[pyo3(get)]
    pub amount: u64,
    #[pyo3(get)]
    pub body: Option<String>,
}

impl TryFrom<SendPayload> for Gift {
    type Error = anyhow::Error;

    fn try_from(value: SendPayload) -> std::result::Result<Self, Self::Error> {
        let body = if let Some(body) = value.body {
            let body = base64::decode(body).context("Failed to decode body")?;
            let body = ton_types::deserialize_tree_of_cells(&mut body.as_slice())
                .context("Failed to deserialize body")?;
            let body = SliceData::from(body);
            Some(body)
        } else {
            None
        };

        Ok(Gift {
            flags: value.flags,
            bounce: value.bounce,
            destination: MsgAddressInt::from_str(&value.destination).context("Invalid address")?,
            amount: value.amount,
            body,
            state_init: None,
        })
    }
}

impl TryFrom<Gift> for SendPayload {
    type Error = anyhow::Error;

    fn try_from(value: Gift) -> std::result::Result<Self, Self::Error> {
        let body = if let Some(body) = value.body {
            let body = ton_types::serialize_toc(&body.into_cell())?;
            let body = base64::encode(body);
            Some(body)
        } else {
            None
        };

        Ok(SendPayload {
            flags: value.flags,
            bounce: value.bounce,
            destination: value.destination.to_string(),
            amount: value.amount,
            body,
        })
    }
}

/// A Python module implemented in Rust.
#[pymodule]
fn pyever_send(py: Python, m: &PyModule) -> PyResult<()> {
    pyo3_log::Logger::new(py, pyo3_log::Caching::LoggersAndLevels)?
        .filter(LevelFilter::Info)
        .install()
        .expect("Someone installed a logger before us :-(");
    log::info!("Initializing pyever_send");
    m.add_class::<TonSigner>()?;
    m.add_class::<SendPayload>()?;
    Ok(())
}
