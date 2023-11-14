#!/usr/bin/python

from pyln.client import LightningRpc
import time
import threading
import pickle
import os
import grpc
from util import generate_random_label, pay_with_thread

from proto import primitives_pb2 as primitives__pb2
from proto import node_pb2_grpc as nodestub
from proto import node_pb2 as noderpc


# number of invoices to create, pay, hold and then cancel
num_iterations = 500
# seconds to hold the invoices with inflight htlcs
delay_seconds = 120
# amount to be used in msat
amount_msat = 1_000_000

# need 2 nodes with sufficient liquidity on rpc1 side
# this is the node with holdinvoice
rpc2 = LightningRpc("/tmp/l2-regtest/regtest/lightning-rpc")
# this node pays the invoices
rpc1 = LightningRpc("/tmp/l1-regtest/regtest/lightning-rpc")

#######
# Works with CLN
#######

# Load the client's certificate and key
CLN_DIR = "/tmp/l2-regtest/regtest"
with open(os.path.join(CLN_DIR, "client.pem"), "rb") as f:
    client_cert = f.read()
with open(os.path.join(CLN_DIR, "client-key.pem"), "rb") as f:
    client_key = f.read()

# Load the server's certificate
with open(os.path.join(CLN_DIR, "server.pem"), "rb") as f:
    server_cert = f.read()


CLN_GRPC_HOST = "localhost:54345"

os.environ["GRPC_SSL_CIPHER_SUITES"] = "HIGH+ECDSA"

# Create the SSL credentials object
creds = grpc.ssl_channel_credentials(
    root_certificates=server_cert,
    private_key=client_key,
    certificate_chain=client_cert,
)
# Create the gRPC channel using the SSL credentials
channel = grpc.secure_channel(CLN_GRPC_HOST, creds)

# Create the gRPC stub
stub = nodestub.NodeStub(channel)


def lookup_stats(rpc, payment_hashes):
    state_counts = {'open': 0, 'settled': 0, 'canceled': 0, 'accepted': 0}
    for payment_hash in payment_hashes:
        try:
            request_lookup = noderpc.HoldInvoiceLookupRequest(
                payment_hash=result.payment_hash
            )
            invoice_info = stub.HoldInvoiceLookup(request_lookup)
            state = noderpc.HoldInvoiceLookupResponse.Holdstate.Name(
                invoice_info.state).lower()
            state_counts[state] = state_counts.get(state, 0) + 1
        except Exception as e:
            print(f"Error looking up payment hash {payment_hash}:", e)
    print(state_counts)


payment_hashes = []


for _ in range(num_iterations):
    label = generate_random_label()

    try:
        request = noderpc.HoldInvoiceRequest(
            amount_msat=primitives__pb2.Amount(msat=amount_msat),
            label=label,
            description="masstest",
            expiry=3600
        )
        result = stub.HoldInvoice(request)
        payment_hash = result.payment_hash
        payment_hashes.append(payment_hash)

        # Pay the invoice using a separate thread
        threading.Thread(target=pay_with_thread, args=(
            rpc1, result.bolt11)).start()
        time.sleep(0.2)
    except Exception as e:
        print("Error executing command:", e)

# Save payment hashes to disk incase something breaks
# and we want to do some manual cleanup
with open('payment_hashes.pkl', 'wb') as f:
    pickle.dump(payment_hashes, f)
    print("Saved payment hashes to disk.")

# wait a little more for payments to arrive
time.sleep(5)

lookup_stats(rpc2, payment_hashes)

print(f"Waiting for {delay_seconds} seconds...")

time.sleep(delay_seconds)

lookup_stats(rpc2, payment_hashes)

for payment_hash in payment_hashes:
    try:
        request = noderpc.HoldInvoiceCancelRequest(payment_hash=payment_hash)
        stub.HoldInvoiceCancel(request)
    except Exception as e:
        print(f"Error cancelling payment hash {payment_hash}:", e)

time.sleep(5)

lookup_stats(rpc2, payment_hashes)
