#!/usr/bin/python

import logging
import os
import secrets
import threading
import time

import grpc
import hold_pb2 as holdrpc
import hold_pb2_grpc as holdstub
import pytest
from grpc._channel import _InactiveRpcError
from pyln.testing.fixtures import *  # noqa: F403
from util import (
    find_unused_port,
    generate_random_label,
    get_plugin,  # noqa: F401
    pay_with_thread,
)


def test_inputs(node_factory, bitcoind, get_plugin):  # noqa: F811
    LOGGER = logging.getLogger(__name__)
    port = find_unused_port()
    l1, l2 = node_factory.get_nodes(
        2, opts=[{}, {"important-plugin": get_plugin, "grpc-hold-port": port}]
    )
    l1.rpc.connect(l2.info["id"], "localhost", l2.port)
    cl1, _ = l1.fundchannel(l2, 1_000_000)
    cl2, _ = l1.fundchannel(l2, 1_000_000)
    cl3, _ = l1.fundchannel(l2, 1_000_000, announce_channel=False)

    bitcoind.generate_block(6)

    l1.wait_channel_active(cl1)
    l1.wait_channel_active(cl2)

    l2info = l2.rpc.getinfo()

    CLN_DIR = l2info["lightning-dir"]
    LOGGER.info(l2info["lightning-dir"])

    with open(os.path.join(CLN_DIR, "client.pem"), "rb") as f:
        client_cert = f.read()
    with open(os.path.join(CLN_DIR, "client-key.pem"), "rb") as f:
        client_key = f.read()

    # Load the server's certificate
    with open(os.path.join(CLN_DIR, "server.pem"), "rb") as f:
        server_cert = f.read()

    CLN_GRPC_HOLD_HOST = f"localhost:{port}"

    os.environ["GRPC_SSL_CIPHER_SUITES"] = "HIGH+ECDSA"

    # Create the SSL credentials object
    creds = grpc.ssl_channel_credentials(
        root_certificates=server_cert,
        private_key=client_key,
        certificate_chain=client_cert,
    )
    # Create the gRPC channel using the SSL credentials
    holdchannel = grpc.secure_channel(
        CLN_GRPC_HOLD_HOST,
        creds,
        options=(("grpc.ssl_target_name_override", "cln"),),
    )

    # Create the gRPC stub
    hold_stub = holdstub.HoldStub(holdchannel)

    request = holdrpc.HoldInvoiceRequest(
        description="Valid invoice description",
        amount_msat=holdrpc.Amount(msat=1000000),
        label=generate_random_label(),
        cltv=144,
    )
    result = hold_stub.HoldInvoice(request)
    assert result is not None
    assert isinstance(result, holdrpc.HoldInvoiceResponse) is True
    assert result.payment_hash is not None

    request = holdrpc.HoldInvoiceRequest(
        description="",
        amount_msat=holdrpc.Amount(msat=1000000),
        label=generate_random_label(),
        cltv=144,
    )
    result = hold_stub.HoldInvoice(request)
    assert result is not None
    assert isinstance(result, holdrpc.HoldInvoiceResponse) is True
    assert result.payment_hash is not None

    random_hex = secrets.token_hex(32)
    request = holdrpc.HoldInvoiceRequest(
        amount_msat=holdrpc.Amount(msat=2000000),
        description="Invoice with optional fields",
        label=generate_random_label(),
        expiry=3600,
        fallbacks=[
            "bcrt1qcpw242j4xsjth7ueq9dgmrqtxjyutuvmraeryr",
            "bcrt1qdwydlys0f8khnp87mx688vq4kskjyr68nrx58j",
        ],
        preimage=bytes.fromhex(random_hex),
        cltv=144,
        deschashonly=True,
    )
    result = hold_stub.HoldInvoice(request)
    assert result is not None
    assert isinstance(result, holdrpc.HoldInvoiceResponse) is True
    assert result.payment_hash is not None

    # 0 amount_msat
    request = holdrpc.HoldInvoiceRequest(
        description="Invalid amount",
        amount_msat=holdrpc.Amount(msat=0),
        label=generate_random_label(),
        cltv=144,
    )
    with pytest.raises(
        _InactiveRpcError,
        match=r"amount_msat|msatoshi: should be positive msat or ",
    ):
        hold_stub.HoldInvoice(request)

    # Fallbacks not as a list of strings
    request = holdrpc.HoldInvoiceRequest(
        description="Invalid fallbacks",
        amount_msat=holdrpc.Amount(msat=800000),
        label=generate_random_label(),
        fallbacks="invalid_fallback",
        cltv=144,
    )
    with pytest.raises(_InactiveRpcError, match=r"Fallback address not valid"):
        hold_stub.HoldInvoice(request)

    # missing cltv
    request = holdrpc.HoldInvoiceRequest(
        description="Missing cltv",
        amount_msat=holdrpc.Amount(msat=800000),
        label=generate_random_label(),
    )
    with pytest.raises(_InactiveRpcError, match=r"missing required parameter: cltv"):
        hold_stub.HoldInvoice(request)

    request = holdrpc.HoldInvoiceRequest(
        description="Expose private channel",
        amount_msat=holdrpc.Amount(msat=1000000),
        label=generate_random_label(),
        cltv=144,
        exposeprivatechannels=[cl2],
    )
    result = hold_stub.HoldInvoice(request)
    assert result is not None
    assert isinstance(result, holdrpc.HoldInvoiceResponse) is True
    assert result.payment_hash is not None

    decode_result = l2.rpc.decode(result.bolt11)
    assert "routes" in decode_result
    assert len(decode_result["routes"]) == 1
    assert len(decode_result["routes"][0]) == 1
    assert cl2 == decode_result["routes"][0][0]["short_channel_id"]


def test_valid_hold_then_settle(node_factory, bitcoind, get_plugin):  # noqa: F811
    LOGGER = logging.getLogger(__name__)
    port = find_unused_port()
    l1, l2 = node_factory.get_nodes(
        2, opts=[{}, {"important-plugin": get_plugin, "grpc-hold-port": port}]
    )
    l1.rpc.connect(l2.info["id"], "localhost", l2.port)
    cl1, _ = l1.fundchannel(l2, 1_000_000)
    cl2, _ = l1.fundchannel(l2, 1_000_000)

    bitcoind.generate_block(6)

    l1.wait_channel_active(cl1)
    l1.wait_channel_active(cl2)

    l2info = l2.rpc.getinfo()

    CLN_DIR = l2info["lightning-dir"]
    LOGGER.info(l2info["lightning-dir"])

    with open(os.path.join(CLN_DIR, "client.pem"), "rb") as f:
        client_cert = f.read()
    with open(os.path.join(CLN_DIR, "client-key.pem"), "rb") as f:
        client_key = f.read()

    # Load the server's certificate
    with open(os.path.join(CLN_DIR, "server.pem"), "rb") as f:
        server_cert = f.read()

    CLN_GRPC_HOLD_HOST = f"localhost:{port}"

    os.environ["GRPC_SSL_CIPHER_SUITES"] = "HIGH+ECDSA"

    # Create the SSL credentials object
    creds = grpc.ssl_channel_credentials(
        root_certificates=server_cert,
        private_key=client_key,
        certificate_chain=client_cert,
    )
    # Create the gRPC channel using the SSL credentials
    holdchannel = grpc.secure_channel(
        CLN_GRPC_HOLD_HOST,
        creds,
        options=(("grpc.ssl_target_name_override", "cln"),),
    )

    # Create the gRPC stub
    hold_stub = holdstub.HoldStub(holdchannel)

    request = holdrpc.HoldInvoiceRequest(
        description="Valid invoice description",
        amount_msat=holdrpc.Amount(msat=1_000_100_000),
        label=generate_random_label(),
        cltv=144,
    )
    result = hold_stub.HoldInvoice(request)
    assert result is not None
    assert (isinstance(result, holdrpc.HoldInvoiceResponse)) is True
    assert result.payment_hash is not None

    request_lookup = holdrpc.HoldInvoiceLookupRequest(payment_hash=result.payment_hash)
    result_lookup = hold_stub.HoldInvoiceLookup(request_lookup)
    assert result_lookup is not None
    assert isinstance(result_lookup, holdrpc.HoldInvoiceLookupResponse) is True
    assert result_lookup.state is not None
    LOGGER.info(f"{result_lookup}")
    assert result_lookup.state == holdrpc.Holdstate.OPEN
    assert result_lookup.htlc_expiry == 0

    # test that it won't settle if it's still open
    request_settle = holdrpc.HoldInvoiceSettleRequest(payment_hash=result.payment_hash)
    with pytest.raises(
        _InactiveRpcError,
        match=r"Holdinvoice is in wrong state: \\\'OPEN\\\'\\",
    ):
        hold_stub.HoldInvoiceSettle(request_settle)

    threading.Thread(target=pay_with_thread, args=(l1, result.bolt11)).start()

    timeout = 10
    start_time = time.time()

    while time.time() - start_time < timeout:
        request_lookup = holdrpc.HoldInvoiceLookupRequest(
            payment_hash=result.payment_hash
        )
        result_lookup = hold_stub.HoldInvoiceLookup(request_lookup)
        assert result_lookup is not None
        assert isinstance(result_lookup, holdrpc.HoldInvoiceLookupResponse) is True

        if result_lookup.state == holdrpc.Holdstate.ACCEPTED:
            break
        else:
            time.sleep(1)

    assert result_lookup.state == holdrpc.Holdstate.ACCEPTED
    assert result_lookup.htlc_expiry > 0

    # test that it's actually holding the htlcs
    # and not letting them through
    doublecheck = l2.rpc.listinvoices(payment_hash=result.payment_hash.hex())[
        "invoices"
    ]
    assert doublecheck[0]["status"] == "unpaid"

    request_settle = holdrpc.HoldInvoiceSettleRequest(payment_hash=result.payment_hash)
    result_settle = hold_stub.HoldInvoiceSettle(request_settle)
    assert result_settle is not None
    assert isinstance(result_settle, holdrpc.HoldInvoiceSettleResponse) is True
    assert result_settle.state == holdrpc.Holdstate.SETTLED

    request_lookup = holdrpc.HoldInvoiceLookupRequest(payment_hash=result.payment_hash)
    result_lookup = hold_stub.HoldInvoiceLookup(request_lookup)
    assert result_lookup is not None
    assert isinstance(result_lookup, holdrpc.HoldInvoiceLookupResponse) is True
    assert result_lookup.state == holdrpc.Holdstate.SETTLED
    assert result_lookup.htlc_expiry == 0

    # ask cln if the invoice is actually paid
    # should not be necessary because lookup does this aswell
    doublecheck = l2.rpc.listinvoices(payment_hash=result.payment_hash.hex())[
        "invoices"
    ]
    assert doublecheck[0]["status"] == "paid"

    request_cancel_settled = holdrpc.HoldInvoiceCancelRequest(
        payment_hash=result.payment_hash
    )

    with pytest.raises(
        _InactiveRpcError,
        match=r"Holdinvoice is in wrong state: \\\'SETTLED\\\'\\",
    ):
        hold_stub.HoldInvoiceCancel(request_cancel_settled)


def test_valid_hold_then_cancel(node_factory, bitcoind, get_plugin):  # noqa: F811
    LOGGER = logging.getLogger(__name__)
    port = find_unused_port()
    l1, l2 = node_factory.get_nodes(
        2, opts=[{}, {"important-plugin": get_plugin, "grpc-hold-port": port}]
    )
    l1.rpc.connect(l2.info["id"], "localhost", l2.port)
    cl1, _ = l1.fundchannel(l2, 1_000_000)
    cl2, _ = l1.fundchannel(l2, 1_000_000)

    bitcoind.generate_block(6)

    l1.wait_channel_active(cl1)
    l1.wait_channel_active(cl2)

    l2info = l2.rpc.getinfo()

    CLN_DIR = l2info["lightning-dir"]
    LOGGER.info(l2info["lightning-dir"])

    with open(os.path.join(CLN_DIR, "client.pem"), "rb") as f:
        client_cert = f.read()
    with open(os.path.join(CLN_DIR, "client-key.pem"), "rb") as f:
        client_key = f.read()

    # Load the server's certificate
    with open(os.path.join(CLN_DIR, "server.pem"), "rb") as f:
        server_cert = f.read()

    CLN_GRPC_HOLD_HOST = f"localhost:{port}"

    os.environ["GRPC_SSL_CIPHER_SUITES"] = "HIGH+ECDSA"

    # Create the SSL credentials object
    creds = grpc.ssl_channel_credentials(
        root_certificates=server_cert,
        private_key=client_key,
        certificate_chain=client_cert,
    )
    # Create the gRPC channel using the SSL credentials
    holdchannel = grpc.secure_channel(
        CLN_GRPC_HOLD_HOST,
        creds,
        options=(("grpc.ssl_target_name_override", "cln"),),
    )

    # Create the gRPC stub
    hold_stub = holdstub.HoldStub(holdchannel)

    request = holdrpc.HoldInvoiceRequest(
        description="Valid invoice description",
        amount_msat=holdrpc.Amount(msat=1_000_100_000),
        label=generate_random_label(),
        cltv=144,
    )
    result = hold_stub.HoldInvoice(request)
    assert result is not None
    assert (isinstance(result, holdrpc.HoldInvoiceResponse)) is True
    assert result.payment_hash is not None

    request_lookup = holdrpc.HoldInvoiceLookupRequest(payment_hash=result.payment_hash)
    result_lookup = hold_stub.HoldInvoiceLookup(request_lookup)
    assert result_lookup is not None
    assert isinstance(result_lookup, holdrpc.HoldInvoiceLookupResponse) is True
    assert result_lookup.state is not None
    LOGGER.info(f"{result_lookup}")
    assert result_lookup.state == holdrpc.Holdstate.OPEN
    assert result_lookup.htlc_expiry == 0

    # test that it won't settle if it's still open
    request_settle = holdrpc.HoldInvoiceSettleRequest(payment_hash=result.payment_hash)
    with pytest.raises(
        _InactiveRpcError,
        match=r"Holdinvoice is in wrong state: \\\'OPEN\\\'\\",
    ):
        hold_stub.HoldInvoiceSettle(request_settle)

    threading.Thread(target=pay_with_thread, args=(l1, result.bolt11)).start()

    timeout = 10
    start_time = time.time()

    while time.time() - start_time < timeout:
        request_lookup = holdrpc.HoldInvoiceLookupRequest(
            payment_hash=result.payment_hash
        )
        result_lookup = hold_stub.HoldInvoiceLookup(request_lookup)
        assert result_lookup is not None
        assert isinstance(result_lookup, holdrpc.HoldInvoiceLookupResponse) is True

        if result_lookup.state == holdrpc.Holdstate.ACCEPTED:
            break
        else:
            time.sleep(1)

    assert result_lookup.state == holdrpc.Holdstate.ACCEPTED
    assert result_lookup.htlc_expiry > 0

    # test that it's actually holding the htlcs
    # and not letting them through
    doublecheck = l2.rpc.listinvoices(payment_hash=result.payment_hash.hex())[
        "invoices"
    ]
    assert doublecheck[0]["status"] == "unpaid"

    request_cancel = holdrpc.HoldInvoiceCancelRequest(payment_hash=result.payment_hash)
    result_cancel = hold_stub.HoldInvoiceCancel(request_cancel)
    assert result_cancel is not None
    assert isinstance(result_cancel, holdrpc.HoldInvoiceCancelResponse) is True
    assert result_cancel.state == holdrpc.Holdstate.CANCELED

    request_lookup = holdrpc.HoldInvoiceLookupRequest(payment_hash=result.payment_hash)
    result_lookup = hold_stub.HoldInvoiceLookup(request_lookup)
    assert result_lookup is not None
    assert isinstance(result_lookup, holdrpc.HoldInvoiceLookupResponse) is True
    assert result_lookup.state == holdrpc.Holdstate.CANCELED
    assert result_lookup.htlc_expiry == 0

    # ask cln if the invoice is actually unpaid
    # should not be necessary because lookup does this aswell
    doublecheck = l2.rpc.listinvoices(payment_hash=result.payment_hash.hex())[
        "invoices"
    ]
    assert doublecheck[0]["status"] == "unpaid"

    request_settle_canceled = holdrpc.HoldInvoiceSettleRequest(
        payment_hash=result.payment_hash
    )

    with pytest.raises(
        _InactiveRpcError,
        match=r"Holdinvoice is in wrong state: \\\'CANCELED\\\'\\",
    ):
        hold_stub.HoldInvoiceSettle(request_settle_canceled)
