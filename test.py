from random import randint

from pyever_send import TonSigner
import json
import requests as req


def make_payload(uid: int) -> str:
    return json.dumps({
        "uniqueId": uid,
        "recipient": "0:e16ba67e5c201915de32d00429f01197c3b8217d409fa2ed03817e2d9f13312c",
        "key": "ZTU5YWYwNDIyNmNmNzVkZDUxY2U5Mjc1NDg3ZmQ1ZWYxMDQ4ZTFlYTk4ZGFjMGFiYzdjNjkwZDkyZjc0Njk0NQo=",
        "content": "ZTU5YWYwNDIyNmNmNzVkZDUxY2U5Mjc1NDg3ZmQ1ZWYxMDQ4ZTFlYTk4ZGFjMGFiYzdjNjkwZDkyZjc0Njk0NQo=",
    })


seed = open("seed").read().strip()

signer = TonSigner(
    seed,
    "https://jrpc.everwallet.net/rpc")

your_address = signer.wallet_address()
print("Your wallet address:", your_address)

print("Your wallet balance:", signer.balance_of(your_address) / 10 ** 9)
# you can just send evers:
res = signer.send_evers(
    "0:8e2586602513e99a55fa2be08561469c7ce51a7d5a25977558e77ef2bc9387b4",
    1_000_000)

# res will be hash or error
print("Tx hash:", res)

# you can check signature:
res = signer.check_signature(
    "0:8e2586602513e99a55fa2be08561469c7ce51a7d5a25977558e77ef2bc9387b4",
    # signature
    "3a4f5660ca692f87c76c3896f41dca00a2b8ec7291436aedb5dd8590fa8a5edd99447cf1acdd6e437beff7e9ce7cd482efa253e513e1fa1bbb84bda33935970a",
    # data hash
    "07123e1f482356c415f684407a3b8723e10b2cbbc0b8fcd6282c49d37c9c1abc")

assert res is True

mailer = "0:a06a244f2632aaff3573e2fa45283fc67e3ad8a11bcba62b060fe9b60c36a0c9"

mailer_abi = \
    req.get(f"https://verify.everscan.io/info/address/{mailer}").json()["abi"]
mailer_abi = json.dumps(mailer_abi)

res = signer.call(mailer, 1_000_000_000, mailer_abi, "sendSmallMail",
                  make_payload(1337))
print("tx hash:", res)
# or you can prepare payload and send up to 3 messages in one transaction

payloads = []

for _ in range(3):
    rand_id = randint(0, 2 ** 32)
    payload = make_payload(rand_id)
    payload = prepared_payload = signer.make_call_payload(mailer, 1_000_000_000,
                                                          mailer_abi,
                                                          "sendSmallMail",
                                                          payload)
    payloads.append(payload)

res = signer.call_multi(payloads)
print("tx hash:", res)
