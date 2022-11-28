from pyever_send import TonSigner
import json
import requests as req

seed = open("seed").read().strip()
signer = TonSigner(
    seed,
    "https://jrpc.everwallet.net/rpc")

# res = signer.send_evers(
#     "0:8e2586602513e99a55fa2be08561469c7ce51a7d5a25977558e77ef2bc9387b4",
#     1_000_000)

res = signer.check_signature(
    "0:8e2586602513e99a55fa2be08561469c7ce51a7d5a25977558e77ef2bc9387b4",
    # signature
    "3a4f5660ca692f87c76c3896f41dca00a2b8ec7291436aedb5dd8590fa8a5edd99447cf1acdd6e437beff7e9ce7cd482efa253e513e1fa1bbb84bda33935970a",
    # data hash
    "07123e1f482356c415f684407a3b8723e10b2cbbc0b8fcd6282c49d37c9c1abc")

assert res is True

payload = {
    "amount": 1,
    "recipient": "0:8e2586602513e99a55fa2be08561469c7ce51a7d5a25977558e77ef2bc9387b4",
    "deployWalletValue": 100_000_000,
    "remainingGasTo": "0:8e2586602513e99a55fa2be08561469c7ce51a7d5a25977558e77ef2bc9387b4",
    "notify": False,
    "payload": "te6ccgEBAQEAAgAAAA==",
}

payload = json.dumps(payload)

contract_address = "0:9aada4077f3304b20331cd3c50e93e6dfa8bae725bcc9a3200820653903087e9"
abi = \
    req.get(
        f"https://verify.everscan.io/info/address/{contract_address}").json()[
        "abi"]

abi = json.dumps(abi)
# [pyo3(text_signature = "($self, contract_address, attach_amount, abi, method, arguments)")]
res = signer.call(contract_address, 1_000_000_000, abi, "transfer", payload)
print(res)

dst = "0:e16ba67e5c201915de32d00429f01197c3b8217d409fa2ed03817e2d9f13312c"
mailer = "0:a06a244f2632aaff3573e2fa45283fc67e3ad8a11bcba62b060fe9b60c36a0c9"

mailer_abi = \
    req.get(f"https://verify.everscan.io/info/address/{mailer}").json()["abi"]
mailer_abi = json.dumps(mailer_abi)

payload = {
    "uniqueId": 148889,
    "recipient": dst,
    "key": "ZTU5YWYwNDIyNmNmNzVkZDUxY2U5Mjc1NDg3ZmQ1ZWYxMDQ4ZTFlYTk4ZGFjMGFiYzdjNjkwZDkyZjc0Njk0NQo=",
    "content": "ZTU5YWYwNDIyNmNmNzVkZDUxY2U5Mjc1NDg3ZmQ1ZWYxMDQ4ZTFlYTk4ZGFjMGFiYzdjNjkwZDkyZjc0Njk0NQo=",
}

payload = json.dumps(payload)

res = signer.call(mailer, 1_000_000_000, mailer_abi, "sendSmallMail", payload)
