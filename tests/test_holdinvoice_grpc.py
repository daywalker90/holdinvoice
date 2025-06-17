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
from pyln.testing.utils import wait_for
from util import (
    find_unused_port,
    get_plugin,  # noqa: F401
    xpay_with_thread,
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

    request = holdrpc.HoldInvoiceVersionRequest()
    result = hold_stub.HoldInvoiceVersion(request)
    assert result is not None
    assert isinstance(result, holdrpc.HoldInvoiceVersionResponse) is True

    request = holdrpc.HoldInvoiceRequest(
        description="Valid invoice description",
        amount_msat=holdrpc.Amount(msat=1000000),
        cltv=144,
    )
    result = hold_stub.HoldInvoice(request)
    assert result is not None
    assert isinstance(result, holdrpc.HoldInvoiceResponse) is True
    assert result.payment_hash is not None

    request = holdrpc.HoldInvoiceRequest(
        description="",
        amount_msat=holdrpc.Amount(msat=1000000),
        cltv=144,
    )
    result = hold_stub.HoldInvoice(request)
    assert result is not None
    assert isinstance(result, holdrpc.HoldInvoiceResponse) is True
    assert result.payment_hash is not None
    assert result.bolt11 is not None
    assert result.payment_secret is not None
    assert result.expires_at is not None
    assert result.preimage is not None
    assert result.description is not None
    assert not result.HasField("description_hash")

    random_hex = secrets.token_hex(32)
    request = holdrpc.HoldInvoiceRequest(
        amount_msat=holdrpc.Amount(msat=2000000),
        description="Invoice with optional fields",
        expiry=3600,
        preimage=bytes.fromhex(random_hex),
        cltv=144,
        deschashonly=True,
    )
    result = hold_stub.HoldInvoice(request)
    assert result is not None
    assert isinstance(result, holdrpc.HoldInvoiceResponse) is True
    assert result.payment_hash is not None
    assert result.HasField("preimage")
    assert not result.HasField("description")
    assert result.HasField("description_hash")

    p_hash_2 = result.payment_hash

    random_hex = secrets.token_hex(32)
    request = holdrpc.HoldInvoiceRequest(
        amount_msat=holdrpc.Amount(msat=2000000),
        description="Invoice with optional fields 2",
        expiry=3600,
        payment_hash=bytes.fromhex(random_hex),
        cltv=144,
    )
    result = hold_stub.HoldInvoice(request)
    assert result is not None
    assert isinstance(result, holdrpc.HoldInvoiceResponse) is True
    assert result.payment_hash is not None
    assert not result.HasField("preimage")
    assert result.HasField("description")
    assert not result.HasField("description_hash")

    p_hash = result.payment_hash

    # 0 amount_msat
    request = holdrpc.HoldInvoiceRequest(
        description="Invalid amount",
        amount_msat=holdrpc.Amount(msat=0),
        cltv=144,
    )
    with pytest.raises(
        _InactiveRpcError,
        match=r"amount_msat|msatoshi: should be positive msat or ",
    ):
        hold_stub.HoldInvoice(request)

    request = holdrpc.HoldInvoiceRequest(
        description="Expose private channel",
        amount_msat=holdrpc.Amount(msat=1000000),
        cltv=144,
        exposeprivatechannels=[cl2],
    )
    result = hold_stub.HoldInvoice(request)
    assert result is not None
    assert isinstance(result, holdrpc.HoldInvoiceResponse) is True
    assert result.payment_hash is not None
    assert result.HasField("preimage")
    assert result.HasField("description")
    assert not result.HasField("description_hash")

    decode_result = l2.rpc.decode(result.bolt11)
    assert "routes" in decode_result
    assert len(decode_result["routes"]) == 1
    assert len(decode_result["routes"][0]) == 1

    assert cl2 == decode_result["routes"][0][0]["short_channel_id"]

    request = holdrpc.HoldInvoiceLookupRequest(
        payment_hash=p_hash_2,
    )
    result = hold_stub.HoldInvoiceLookup(request)
    assert result is not None
    assert isinstance(result, holdrpc.HoldInvoiceLookupResponse) is True
    assert result.holdinvoices[0].state == holdrpc.Holdstate.OPEN
    assert not result.holdinvoices[0].HasField("htlc_expiry")
    assert result.holdinvoices[0].bolt11 is not None
    assert result.holdinvoices[0].description == "Invoice with optional fields"
    assert result.holdinvoices[0].preimage is not None
    assert not result.holdinvoices[0].HasField("paid_at")

    threading.Thread(
        target=xpay_with_thread, args=(l1, result.holdinvoices[0].bolt11)
    ).start()

    wait_for(
        lambda: hold_stub.HoldInvoiceLookup(request).holdinvoices[0].state
        == holdrpc.Holdstate.ACCEPTED
    )

    request = holdrpc.HoldInvoiceSettleRequest(
        payment_hash=p_hash_2,
    )
    result = hold_stub.HoldInvoiceSettle(request)
    assert result is not None
    assert isinstance(result, holdrpc.HoldInvoiceSettleResponse) is True
    assert result.state == holdrpc.Holdstate.SETTLED

    request = holdrpc.HoldInvoiceCancelRequest(
        payment_hash=p_hash,
    )
    result = hold_stub.HoldInvoiceCancel(request)
    assert result is not None
    assert isinstance(result, holdrpc.HoldInvoiceCancelResponse) is True
    assert result.state == holdrpc.Holdstate.CANCELED


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

    inv_amt = 1_000_100_000
    request = holdrpc.HoldInvoiceRequest(
        description="Valid invoice description",
        amount_msat=holdrpc.Amount(msat=inv_amt),
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
    assert result_lookup.holdinvoices[0].state is not None
    LOGGER.info(f"{result_lookup.holdinvoices[0]}")
    assert result_lookup.holdinvoices[0].state == holdrpc.Holdstate.OPEN
    assert result_lookup.holdinvoices[0].htlc_expiry == 0

    # test that it won't settle if it's still open
    request_settle = holdrpc.HoldInvoiceSettleRequest(payment_hash=result.payment_hash)
    with pytest.raises(
        _InactiveRpcError,
        match=r"Holdinvoice is in wrong state: `OPEN`",
    ):
        hold_stub.HoldInvoiceSettle(request_settle)

    threading.Thread(target=xpay_with_thread, args=(l1, result.bolt11)).start()

    request_lookup = holdrpc.HoldInvoiceLookupRequest(payment_hash=result.payment_hash)
    wait_for(
        lambda: hold_stub.HoldInvoiceLookup(request_lookup).holdinvoices[0].state
        == holdrpc.Holdstate.ACCEPTED
    )
    result_lookup = hold_stub.HoldInvoiceLookup(request_lookup)
    assert result_lookup is not None
    assert isinstance(result_lookup, holdrpc.HoldInvoiceLookupResponse) is True
    assert result_lookup.holdinvoices[0].state == holdrpc.Holdstate.ACCEPTED
    assert result_lookup.holdinvoices[0].htlc_expiry > 0

    # test that it's actually holding the htlcs
    # and not letting them through
    time.sleep(5)
    hold_amt = 0
    htlcs = l2.rpc.call("listhtlcs", {})["htlcs"]
    for htlc in htlcs:
        if htlc["payment_hash"] == result.payment_hash.hex():
            hold_amt += htlc["amount_msat"]
            assert htlc["state"] == "RCVD_ADD_ACK_REVOCATION"

    assert hold_amt >= inv_amt

    wait_for(
        lambda: l1.rpc.call("listpays", {"payment_hash": result.payment_hash.hex()})[
            "pays"
        ][0]["status"]
        == "pending"
    )

    request_settle = holdrpc.HoldInvoiceSettleRequest(payment_hash=result.payment_hash)
    result_settle = hold_stub.HoldInvoiceSettle(request_settle)
    assert result_settle is not None
    assert isinstance(result_settle, holdrpc.HoldInvoiceSettleResponse) is True
    assert result_settle.state == holdrpc.Holdstate.SETTLED

    request_lookup = holdrpc.HoldInvoiceLookupRequest(payment_hash=result.payment_hash)
    result_lookup = hold_stub.HoldInvoiceLookup(request_lookup)
    assert result_lookup is not None
    assert isinstance(result_lookup, holdrpc.HoldInvoiceLookupResponse) is True
    assert result_lookup.holdinvoices[0].state == holdrpc.Holdstate.SETTLED
    assert result_lookup.holdinvoices[0].htlc_expiry == 0

    # ask cln if the invoice is actually paid
    # should not be necessary because lookup does this aswell
    wait_for(
        lambda: l1.rpc.call("listpays", {"payment_hash": result.payment_hash.hex()})[
            "pays"
        ][0]["status"]
        == "complete"
    )

    request_cancel_settled = holdrpc.HoldInvoiceCancelRequest(
        payment_hash=result.payment_hash
    )

    with pytest.raises(
        _InactiveRpcError,
        match=r"Holdinvoice is in wrong state: `SETTLED`",
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

    inv_amt = 1_000_100_000
    request = holdrpc.HoldInvoiceRequest(
        description="Valid invoice description",
        amount_msat=holdrpc.Amount(msat=inv_amt),
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
    assert result_lookup.holdinvoices[0].state is not None
    LOGGER.info(f"{result_lookup.holdinvoices[0]}")
    assert result_lookup.holdinvoices[0].state == holdrpc.Holdstate.OPEN
    assert result_lookup.holdinvoices[0].htlc_expiry == 0

    # test that it won't settle if it's still open
    request_settle = holdrpc.HoldInvoiceSettleRequest(payment_hash=result.payment_hash)
    with pytest.raises(
        _InactiveRpcError,
        match=r"Holdinvoice is in wrong state: `OPEN`",
    ):
        hold_stub.HoldInvoiceSettle(request_settle)

    threading.Thread(target=xpay_with_thread, args=(l1, result.bolt11)).start()

    request_lookup = holdrpc.HoldInvoiceLookupRequest(payment_hash=result.payment_hash)
    wait_for(
        lambda: hold_stub.HoldInvoiceLookup(request_lookup).holdinvoices[0].state
        == holdrpc.Holdstate.ACCEPTED
    )
    result_lookup = hold_stub.HoldInvoiceLookup(request_lookup)
    assert result_lookup is not None
    assert isinstance(result_lookup, holdrpc.HoldInvoiceLookupResponse) is True
    assert result_lookup.holdinvoices[0].state == holdrpc.Holdstate.ACCEPTED
    assert result_lookup.holdinvoices[0].htlc_expiry > 0

    # test that it's actually holding the htlcs
    # and not letting them through
    time.sleep(5)
    hold_amt = 0
    htlcs = l2.rpc.call("listhtlcs", {})["htlcs"]
    for htlc in htlcs:
        if htlc["payment_hash"] == result.payment_hash.hex():
            hold_amt += htlc["amount_msat"]
            assert htlc["state"] == "RCVD_ADD_ACK_REVOCATION"

    assert hold_amt >= inv_amt

    wait_for(
        lambda: l1.rpc.call("listpays", {"payment_hash": result.payment_hash.hex()})[
            "pays"
        ][0]["status"]
        == "pending"
    )

    request_cancel = holdrpc.HoldInvoiceCancelRequest(payment_hash=result.payment_hash)
    result_cancel = hold_stub.HoldInvoiceCancel(request_cancel)
    assert result_cancel is not None
    assert isinstance(result_cancel, holdrpc.HoldInvoiceCancelResponse) is True
    assert result_cancel.state == holdrpc.Holdstate.CANCELED

    request_lookup = holdrpc.HoldInvoiceLookupRequest(payment_hash=result.payment_hash)
    result_lookup = hold_stub.HoldInvoiceLookup(request_lookup)
    assert result_lookup is not None
    assert isinstance(result_lookup, holdrpc.HoldInvoiceLookupResponse) is True
    assert result_lookup.holdinvoices[0].state == holdrpc.Holdstate.CANCELED
    assert result_lookup.holdinvoices[0].htlc_expiry == 0

    # ask cln if the invoice is actually unpaid
    # should not be necessary because lookup does this aswell
    wait_for(
        lambda: l1.rpc.call("listpays", {"payment_hash": result.payment_hash.hex()})[
            "pays"
        ][0]["status"]
        == "failed"
    )

    htlcs = l2.rpc.call("listhtlcs", {})["htlcs"]
    for htlc in htlcs:
        if htlc["payment_hash"] == result.payment_hash.hex():
            assert htlc["state"] == "SENT_REMOVE_ACK_REVOCATION"

    request_settle_canceled = holdrpc.HoldInvoiceSettleRequest(
        payment_hash=result.payment_hash
    )

    with pytest.raises(
        _InactiveRpcError,
        match=r"Holdinvoice is in wrong state: `CANCELED`",
    ):
        hold_stub.HoldInvoiceSettle(request_settle_canceled)
