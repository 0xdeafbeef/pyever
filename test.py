from pyever_send import TonSigner

signer = TonSigner("words go here", "https://jrpc.everwallet.net/rpc")

res = signer.send_evers(
    "0:8e2586602513e99a55fa2be08561469c7ce51a7d5a25977558e77ef2bc9387b4",
    1_000_000)
