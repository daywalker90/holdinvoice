#!/usr/bin/python

import primitives_pb2 as primitives__pb2
import node_pb2_grpc as nodestub
import node_pb2 as noderpc
import hold_pb2_grpc as holdstub
import hold_pb2 as holdrpc
import grpc
from pyln.client import LightningRpc
import unittest
import secrets
import threading
import time
import os
from util import generate_random_label
from util import generate_random_number
from util import pay_with_thread

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


CLN_GRPC_HOST = "localhost:54344"
CLN_GRPC_HOLD_HOST = "localhost:54345"

os.environ["GRPC_SSL_CIPHER_SUITES"] = "HIGH+ECDSA"

# Create the SSL credentials object
creds = grpc.ssl_channel_credentials(
    root_certificates=server_cert,
    private_key=client_key,
    certificate_chain=client_cert,
)
# Create the gRPC channel using the SSL credentials
holdchannel = grpc.secure_channel(CLN_GRPC_HOLD_HOST, creds)

# Create the gRPC stub
holdstub = holdstub.HoldStub(holdchannel)

# Create the gRPC channel using the SSL credentials
channel = grpc.secure_channel(CLN_GRPC_HOST, creds)

# Create the gRPC stub
stub = nodestub.NodeStub(channel)


class TestStringMethods(unittest.TestCase):

    def test_node_grpc(self):
        request = noderpc.GetinfoRequest()
        result = stub.Getinfo(request)
        self.assertTrue(isinstance(result, noderpc.GetinfoResponse))
        self.assertIsNotNone(result.id)

    def test_valid_input(self):
        request = holdrpc.HoldInvoiceRequest(
            description="Valid invoice description",
            amount_msat=primitives__pb2.Amount(msat=1000000),
            label=generate_random_label()
        )
        result = holdstub.HoldInvoice(request)
        self.assertIsNotNone(result)
        self.assertTrue(isinstance(result, holdrpc.HoldInvoiceResponse))
        self.assertIsNotNone(result.payment_hash)

        request = holdrpc.HoldInvoiceRequest(
            description="",
            amount_msat=primitives__pb2.Amount(msat=1000000),
            label=generate_random_label()
        )
        result = holdstub.HoldInvoice(request)
        self.assertIsNotNone(result)
        self.assertTrue(isinstance(result, holdrpc.HoldInvoiceResponse))
        self.assertIsNotNone(result.payment_hash)

    def test_optional_fields(self):
        random_hex = secrets.token_hex(32)
        request = holdrpc.HoldInvoiceRequest(
            amount_msat=primitives__pb2.Amount(msat=2000000),
            description="Invoice with optional fields",
            label=generate_random_label(),
            expiry=3600,
            fallbacks=["bcrt1qcpw242j4xsjth7ueq9dgmrqtxjyutuvmraeryr",
                       "bcrt1qdwydlys0f8khnp87mx688vq4kskjyr68nrx58j"],
            preimage=bytes.fromhex(random_hex),
            cltv=144,
            deschashonly=True
        )
        result = holdstub.HoldInvoice(request)
        self.assertIsNotNone(result)
        self.assertTrue(isinstance(result, holdrpc.HoldInvoiceResponse))
        self.assertIsNotNone(result.payment_hash)

    def test_invalid_amount_msat(self):
        # 0 amount_msat
        request = holdrpc.HoldInvoiceRequest(
            description="Invalid amount",
            amount_msat=primitives__pb2.Amount(msat=0),
            label=generate_random_label()
        )
        with self.assertRaises(Exception) as result:
            holdstub.HoldInvoice(request)
        self.assertIsNotNone(result.exception)
        self.assertIn(
            "amount_msat|msatoshi: should be positive msat or ",
            result.exception.debug_error_string())

    def test_invalid_fallbacks(self):
        # Fallbacks not as a list of strings
        request = holdrpc.HoldInvoiceRequest(
            description="Invalid fallbacks",
            amount_msat=primitives__pb2.Amount(msat=800000),
            label=generate_random_label(),
            fallbacks="invalid_fallback"
        )
        with self.assertRaises(Exception) as result:
            holdstub.HoldInvoice(request)
        self.assertIsNotNone(result.exception)
        self.assertIn(
            "Fallback address not valid",
            result.exception.debug_error_string())

    def test_valid_hold_then_settle(self):
        request = holdrpc.HoldInvoiceRequest(
            description="Valid invoice description",
            amount_msat=primitives__pb2.Amount(msat=1_000_100_000),
            label=generate_random_label()
        )
        result = holdstub.HoldInvoice(request)
        self.assertIsNotNone(result)
        self.assertTrue(isinstance(result, holdrpc.HoldInvoiceResponse))
        self.assertIsNotNone(result.payment_hash)

        request_lookup = holdrpc.HoldInvoiceLookupRequest(
            payment_hash=result.payment_hash
        )
        result_lookup = holdstub.HoldInvoiceLookup(request_lookup)
        self.assertIsNotNone(result_lookup)
        self.assertTrue(isinstance(
            result_lookup, holdrpc.HoldInvoiceLookupResponse))
        self.assertIsNotNone(result_lookup.state)
        print(result_lookup)
        self.assertEqual(result_lookup.state,
                         holdrpc.HoldInvoiceLookupResponse.Holdstate.OPEN)
        self.assertIs(result_lookup.htlc_expiry, 0)

        # test that it won't settle if it's still open
        request_settle = holdrpc.HoldInvoiceSettleRequest(
            payment_hash=result.payment_hash
        )
        with self.assertRaises(Exception) as result_settle:
            holdstub.HoldInvoiceSettle(request_settle)
        self.assertIsNotNone(result_settle.exception)
        self.assertIn(
            "Holdinvoice is in wrong state: \\\'open\\\'\\",
            result_settle.exception.debug_error_string())

        threading.Thread(target=pay_with_thread, args=(
            rpc1, result.bolt11)).start()

        timeout = 10
        start_time = time.time()

        while time.time() - start_time < timeout:
            request_lookup = holdrpc.HoldInvoiceLookupRequest(
                payment_hash=result.payment_hash
            )
            result_lookup = holdstub.HoldInvoiceLookup(request_lookup)
            self.assertIsNotNone(result_lookup)
            self.assertTrue(isinstance(
                result_lookup, holdrpc.HoldInvoiceLookupResponse))

            if result_lookup.state == holdrpc.HoldInvoiceLookupResponse.Holdstate.ACCEPTED:
                break
            else:
                time.sleep(1)

        self.assertEqual(result_lookup.state,
                         holdrpc.HoldInvoiceLookupResponse.Holdstate.ACCEPTED)
        self.assertIsNot(result_lookup.htlc_expiry, 0)

        # test that it's actually holding the htlcs
        # and not letting them through
        doublecheck = rpc2.listinvoices(
            payment_hash=result.payment_hash.hex())["invoices"]
        self.assertEqual(doublecheck[0]["status"], "unpaid")

        request_settle = holdrpc.HoldInvoiceSettleRequest(
            payment_hash=result.payment_hash
        )
        result_settle = holdstub.HoldInvoiceSettle(request_settle)
        self.assertIsNotNone(result_settle)
        self.assertTrue(isinstance(
            result_settle, holdrpc.HoldInvoiceSettleResponse))
        self.assertEqual(result_settle.state,
                         holdrpc.HoldInvoiceSettleResponse.Holdstate.SETTLED)

        request_lookup = holdrpc.HoldInvoiceLookupRequest(
            payment_hash=result.payment_hash
        )
        result_lookup = holdstub.HoldInvoiceLookup(request_lookup)
        self.assertIsNotNone(result_lookup)
        self.assertTrue(isinstance(
            result_lookup, holdrpc.HoldInvoiceLookupResponse))
        self.assertEqual(result_lookup.state,
                         holdrpc.HoldInvoiceLookupResponse.Holdstate.SETTLED)
        self.assertIs(result_lookup.htlc_expiry, 0)

        # ask cln if the invoice is actually paid
        # should not be necessary because lookup does this aswell
        doublecheck = rpc2.listinvoices(
            payment_hash=result.payment_hash.hex())["invoices"]
        self.assertEqual(doublecheck[0]["status"], "paid")

        request_cancel_settled = holdrpc.HoldInvoiceCancelRequest(
            payment_hash=result.payment_hash
        )
        with self.assertRaises(Exception) as result_cancel_settled:
            holdstub.HoldInvoiceCancel(request_cancel_settled)
        self.assertIsNotNone(result_cancel_settled.exception)
        self.assertIn(
            "Holdinvoice is in wrong "
            "state: \\\'settled\\\'\\",
            result_cancel_settled.exception.debug_error_string())

    def test_valid_hold_then_cancel(self):
        request = holdrpc.HoldInvoiceRequest(
            description="Valid invoice description",
            amount_msat=primitives__pb2.Amount(msat=1_000_100_000),
            label=generate_random_label()
        )
        result = holdstub.HoldInvoice(request)
        self.assertIsNotNone(result)
        self.assertTrue(isinstance(result, holdrpc.HoldInvoiceResponse))
        self.assertIsNotNone(result.payment_hash)

        request_lookup = holdrpc.HoldInvoiceLookupRequest(
            payment_hash=result.payment_hash
        )
        result_lookup = holdstub.HoldInvoiceLookup(request_lookup)
        self.assertIsNotNone(result_lookup)
        self.assertTrue(isinstance(
            result_lookup, holdrpc.HoldInvoiceLookupResponse))
        self.assertIsNotNone(result_lookup.state)
        print(result_lookup)
        self.assertEqual(result_lookup.state,
                         holdrpc.HoldInvoiceLookupResponse.Holdstate.OPEN)
        self.assertIs(result_lookup.htlc_expiry, 0)

        # test that it won't settle if it's still open
        request_settle = holdrpc.HoldInvoiceSettleRequest(
            payment_hash=result.payment_hash
        )
        with self.assertRaises(Exception) as result_settle:
            holdstub.HoldInvoiceSettle(request_settle)
        self.assertIsNotNone(result_settle.exception)
        self.assertIn(
            "Holdinvoice is in wrong state: \\\'open\\\'\\",
            result_settle.exception.debug_error_string())

        threading.Thread(target=pay_with_thread, args=(
            rpc1, result.bolt11)).start()

        timeout = 10
        start_time = time.time()

        while time.time() - start_time < timeout:
            request_lookup = holdrpc.HoldInvoiceLookupRequest(
                payment_hash=result.payment_hash
            )
            result_lookup = holdstub.HoldInvoiceLookup(request_lookup)
            self.assertIsNotNone(result_lookup)
            self.assertTrue(isinstance(
                result_lookup, holdrpc.HoldInvoiceLookupResponse))

            if result_lookup.state == holdrpc.HoldInvoiceLookupResponse.Holdstate.ACCEPTED:
                break
            else:
                time.sleep(1)

        self.assertEqual(result_lookup.state,
                         holdrpc.HoldInvoiceLookupResponse.Holdstate.ACCEPTED)
        self.assertIsNot(result_lookup.htlc_expiry, 0)

        # test that it's actually holding the htlcs
        # and not letting them through
        doublecheck = rpc2.listinvoices(
            payment_hash=result.payment_hash.hex())["invoices"]
        self.assertEqual(doublecheck[0]["status"], "unpaid")

        request_settle = holdrpc.HoldInvoiceCancelRequest(
            payment_hash=result.payment_hash
        )
        result_settle = holdstub.HoldInvoiceCancel(request_settle)
        self.assertIsNotNone(result_settle)
        self.assertTrue(isinstance(
            result_settle, holdrpc.HoldInvoiceCancelResponse))
        self.assertEqual(result_settle.state,
                         holdrpc.HoldInvoiceCancelResponse.Holdstate.CANCELED)

        request_lookup = holdrpc.HoldInvoiceLookupRequest(
            payment_hash=result.payment_hash
        )
        result_lookup = holdstub.HoldInvoiceLookup(request_lookup)
        self.assertIsNotNone(result_lookup)
        self.assertTrue(isinstance(
            result_lookup, holdrpc.HoldInvoiceLookupResponse))
        self.assertEqual(result_lookup.state,
                         holdrpc.HoldInvoiceLookupResponse.Holdstate.CANCELED)
        self.assertIs(result_lookup.htlc_expiry, 0)

        # ask cln if the invoice is actually unpaid
        # should not be necessary because lookup does this aswell
        doublecheck = rpc2.listinvoices(
            payment_hash=result.payment_hash.hex())["invoices"]
        self.assertEqual(doublecheck[0]["status"], "unpaid")

        request_cancel_settled = holdrpc.HoldInvoiceSettleRequest(
            payment_hash=result.payment_hash
        )
        with self.assertRaises(Exception) as result_cancel_settled:
            holdstub.HoldInvoiceSettle(request_cancel_settled)
        self.assertIsNotNone(result_cancel_settled.exception)
        self.assertIn(
            "Holdinvoice is in wrong "
            "state: \\\'canceled\\\'\\",
            result_cancel_settled.exception.debug_error_string())


if __name__ == '__main__':
    unittest.main()
