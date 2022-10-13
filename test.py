from pyever_send import TonSigner

signer = TonSigner("words go here", "https://jrpc.everwallet.net/rpc")

res = signer.send_evers(
    "0:8e2586602513e99a55fa2be08561469c7ce51a7d5a25977558e77ef2bc9387b4",
    1_000_000)

res = signer.check_signature(
    "0:8e2586602513e99a55fa2be08561469c7ce51a7d5a25977558e77ef2bc9387b4",
    # signature
    "3a4f5660ca692f87c76c3896f41dca00a2b8ec7291436aedb5dd8590fa8a5edd99447cf1acdd6e437beff7e9ce7cd482efa253e513e1fa1bbb84bda33935970a",
    # data hash
    "07123e1f482356c415f684407a3b8723e10b2cbbc0b8fcd6282c49d37c9c1abc")

assert res is True
