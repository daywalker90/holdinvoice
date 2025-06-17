#!/usr/bin/python

import logging
import secrets
import threading
import time

import pytest
from pyln.client import Millisatoshi, RpcError
from pyln.testing.fixtures import *  # noqa: F403
from pyln.testing.utils import (
    only_one,
    sync_blockheight,
    wait_for,
    mine_funding_to_announce,
)
from util import (
    get_plugin,  # noqa: F401
    xpay_with_thread,
)

LOGGER = logging.getLogger(__name__)


def test_inputs(node_factory, bitcoind, get_plugin):  # noqa: F811
    node, l2, l3 = node_factory.get_nodes(
        3,
        opts=[
            {
                "important-plugin": get_plugin,
            },
            {},
            {},
        ],
    )
    l2.fundchannel(node, 1_000_000, announce_channel=False)
    l2.fundchannel(node, 1_000_000, announce_channel=False)
    l3.fundchannel(l2, 1_000_000)
    bitcoind.generate_block(6)
    sync_blockheight(bitcoind, [node, l2, l3])

    result = node.rpc.call(
        "holdinvoice",
        {
            "amount_msat": 1000000,
            "description": "Valid invoice description",
            "cltv": 144,
        },
    )
    assert result is not None
    assert isinstance(result, dict) is True
    assert "payment_hash" in result

    result = node.rpc.call(
        "holdinvoice",
        {
            "amount_msat": 1000000,
            "description": "",
            "cltv": 144,
        },
    )
    assert result is not None
    assert isinstance(result, dict) is True
    assert "payment_hash" in result

    with pytest.raises(
        RpcError, match=r"missing required parameter: amount_msat|msatoshi"
    ):
        node.rpc.call(
            "holdinvoice",
            {
                "description": "Missing amount",
                "cltv": 144,
            },
        )

    with pytest.raises(RpcError, match=r"missing required parameter: description"):
        node.rpc.call(
            "holdinvoice",
            {"amount_msat": 1000000, "cltv": 144},
        )

    random_hex = secrets.token_hex(32)
    now = int(time.time())
    result = node.rpc.call(
        "holdinvoice",
        {
            "amount_msat": 2000000,
            "description": "Invoice with optional fields",
            "expiry": 3600,
            "preimage": random_hex,
            "cltv": 144,
            "deschashonly": True,
        },
    )
    assert result is not None
    assert isinstance(result, dict) is True
    assert "payment_hash" in result
    assert "description" not in result
    assert result["expires_at"] == pytest.approx(now + 3600, abs=5)

    # Negative amount_msat
    with pytest.raises(
        RpcError,
        match=(
            "amount_msat|msatoshi: should be an unsigned "
            "64 bit integer: invalid token `-1000`"
        ),
    ):
        node.rpc.call(
            "holdinvoice",
            {
                "amount_msat": -1000,
                "description": "Invalid amount negative",
                "cltv": 144,
            },
        )

    # 0 amount_msat
    with pytest.raises(RpcError, match=r"amount_msat: should be positive msat"):
        node.rpc.call(
            "holdinvoice",
            {
                "amount_msat": 0,
                "description": "Invalid amount 0",
                "cltv": 144,
            },
        )

    # Negative expiry value
    with pytest.raises(
        RpcError,
        match=r"expiry: should be an unsigned 64 bit integer: invalid token `-3600`",
    ):
        node.rpc.call(
            "holdinvoice",
            {
                "amount_msat": 500000,
                "description": "Invalid expiry",
                "expiry": -3600,
                "cltv": 144,
            },
        )

    # Negative cltv value
    with pytest.raises(
        RpcError,
        match=r"cltv: should be an unsigned 64 bit integer: invalid token `-144`",
    ):
        node.rpc.call(
            "holdinvoice",
            {
                "amount_msat": 1200000,
                "description": "Invalid cltv",
                "cltv": -144,
            },
        )

    # Invalid payment_hash
    with pytest.raises(RpcError, match=r"payment_hash: should be a 32 byte hex value"):
        node.rpc.call(
            "holdinvoice",
            {
                "amount_msat": 1200000,
                "description": "Invalid payment_hash",
                "payment_hash": "2a2a2a2a2a",
            },
        )

    node.rpc.call(
        "holdinvoice",
        {
            "amount_msat": 1200000,
            "description": "Valid payment_hash",
            "payment_hash": "2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a",
        },
    )

    with pytest.raises(
        RpcError,
        match="payment_hash: `2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a` does not match preimage: `2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a`",
    ):
        node.rpc.call(
            "holdinvoice",
            {
                "amount_msat": 1200000,
                "description": "Valid payment_hash",
                "payment_hash": "2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a",
                "preimage": "2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a",
            },
        )

    result = node.rpc.call(
        "holdinvoice",
        {
            "amount_msat": 1200000,
            "description": "Valid payment_hash",
            "payment_hash": "b14d3dd74ca0ed86ac4521685f30d719143851b7e2c419178e5e669d5f4810ab",
            "preimage": "17a670173e3da757bdfd0748d935b509b25ddc199c933b6bd843fc26c74ca9b8",
        },
    )
    assert (
        result["payment_hash"]
        == "b14d3dd74ca0ed86ac4521685f30d719143851b7e2c419178e5e669d5f4810ab"
    )
    assert (
        result["preimage"]
        == "17a670173e3da757bdfd0748d935b509b25ddc199c933b6bd843fc26c74ca9b8"
    )

    result = node.rpc.call(
        "holdinvoice",
        {
            "amount_msat": 1200000,
            "description": "Valid payment_hash",
            "preimage": "2bdccc7dbfd7568ede039047cdd9ee1736031444ea3fb8c9884fc03192958f1e",
        },
    )
    assert (
        result["payment_hash"]
        == "eb59352a2dd9e80a6df0f4b5d4498660619539f795d99547153b38773c20c315"
    )
    assert (
        result["preimage"]
        == "2bdccc7dbfd7568ede039047cdd9ee1736031444ea3fb8c9884fc03192958f1e"
    )

    result = node.rpc.call(
        "holdinvoice",
        {
            "amount_msat": 1200000,
            "description": "Valid payment_hash",
            "payment_hash": "3e1ef316e6d2ba9ab807a3840388709235e20e9430d1048a5321c86c20fd4cfd",
        },
    )
    assert (
        result["payment_hash"]
        == "3e1ef316e6d2ba9ab807a3840388709235e20e9430d1048a5321c86c20fd4cfd"
    )
    assert "preimage" not in result

    invoice = node.rpc.call(
        "invoice",
        {
            "label": "test-expose-priv-channels",
            "description": "",
            "amount_msat": 1_200_000_000,
        },
    )
    holdinvoice = node.rpc.call(
        "holdinvoice", {"description": "", "amount_msat": 1_200_000_000}
    )

    decoded_invoice = node.rpc.call("decode", {"string": invoice["bolt11"]})
    assert "routes" in decoded_invoice
    decoded_hold_invoice = node.rpc.call("decode", {"string": holdinvoice["bolt11"]})
    assert "routes" in decoded_hold_invoice
    assert sorted(
        decoded_invoice["routes"], key=lambda x: x[0]["short_channel_id"]
    ) == sorted(decoded_hold_invoice["routes"], key=lambda x: x[0]["short_channel_id"])

    holdinvoices = node.rpc.call("holdinvoicelookup", {})
    assert len(holdinvoices["holdinvoices"]) == 8


def test_valid_hold_then_settle(node_factory, bitcoind, get_plugin):  # noqa: F811
    l1, l2 = node_factory.get_nodes(
        2,
        opts=[
            {},
            {
                "important-plugin": get_plugin,
            },
        ],
    )
    l1.rpc.connect(l2.info["id"], "localhost", l2.port)
    cl1, _ = l1.fundchannel(l2, 1_000_000)
    cl2, _ = l1.fundchannel(l2, 1_000_000)

    bitcoind.generate_block(6)

    l1.wait_channel_active(cl1)
    l1.wait_channel_active(cl2)

    inv_amt = 1_000_100_000
    invoice = l2.rpc.call(
        "holdinvoice",
        {
            "amount_msat": inv_amt,
            "description": "test_valid_hold_then_settle",
            "cltv": 144,
        },
    )
    assert invoice is not None
    assert isinstance(invoice, dict) is True
    assert "payment_hash" in invoice

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )["holdinvoices"][0]
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert "state" in result_lookup
    assert result_lookup["state"] == "OPEN"
    assert "htlc_expiry" not in result_lookup
    assert "preimage" in result_lookup
    assert "bolt11" in result_lookup
    assert "description" not in result_lookup

    # test that it won't settle if it's still open
    with pytest.raises(RpcError, match=r"Holdinvoice is in wrong state: `OPEN`"):
        l2.rpc.call("holdinvoicesettle", {"payment_hash": invoice["payment_hash"]})

    threading.Thread(target=xpay_with_thread, args=(l1, invoice["bolt11"])).start()

    wait_for(
        lambda: l2.rpc.call(
            "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
        )["holdinvoices"][0]["state"]
        == "ACCEPTED"
    )

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )["holdinvoices"][0]
    assert result_lookup["state"] == "ACCEPTED"
    assert "htlc_expiry" in result_lookup

    # test that it's actually holding the htlcs
    # and not letting them through
    time.sleep(5)
    hold_amt = 0
    htlcs = l2.rpc.call("listhtlcs", {})["htlcs"]
    for htlc in htlcs:
        if htlc["payment_hash"] == invoice["payment_hash"]:
            hold_amt += htlc["amount_msat"]
            assert htlc["state"] == "RCVD_ADD_ACK_REVOCATION"

    assert hold_amt >= inv_amt

    wait_for(
        lambda: l1.rpc.call("listpays", {"payment_hash": invoice["payment_hash"]})[
            "pays"
        ][0]["status"]
        == "pending"
    )

    result_settle = l2.rpc.call(
        "holdinvoicesettle", {"payment_hash": invoice["payment_hash"]}
    )
    assert result_settle is not None
    assert isinstance(result_settle, dict) is True
    assert result_settle["state"] == "SETTLED"

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )["holdinvoices"][0]
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert result_lookup["state"] == "SETTLED"
    assert "htlc_expiry" not in result_lookup

    # ask cln if the invoice is actually paid
    # should not be necessary because lookup does this aswell
    wait_for(
        lambda: l1.rpc.call("listpays", {"payment_hash": invoice["payment_hash"]})[
            "pays"
        ][0]["status"]
        == "complete"
    )

    with pytest.raises(RpcError, match=r"Holdinvoice is in wrong state: `SETTLED`"):
        l2.rpc.call("holdinvoicesettle", {"payment_hash": invoice["payment_hash"]})

    invoice = l2.rpc.call(
        "holdinvoice",
        {
            "amount_msat": inv_amt // 1000,
            "description": "111fff",
            "cltv": 144,
            "deschashonly": True,
            "payment_hash": "39ac4f63dceebc75aa91493ca4f7b97e9f044d98719e4e14be936bd68ebc954e",
        },
    )
    assert (
        invoice["description_hash"]
        == "1f8dd4b85ce66745aa90ef075b0874b41685e60eb5c8cc86e173b435b6b254a8"
    )
    assert "description" not in invoice
    assert (
        invoice["payment_hash"]
        == "39ac4f63dceebc75aa91493ca4f7b97e9f044d98719e4e14be936bd68ebc954e"
    )
    assert "preimage" not in invoice

    threading.Thread(target=xpay_with_thread, args=(l1, invoice["bolt11"])).start()

    wait_for(
        lambda: l2.rpc.call(
            "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
        )["holdinvoices"][0]["state"]
        == "ACCEPTED"
    )

    l2.rpc.call(
        "holdinvoicesettle",
        {
            "preimage": "ae0123af6f1a9853d5d6f0c00c65f4c316ea37482fcd34e4ce9fadf1e08e6bdf"
        },
    )

    wait_for(
        lambda: l2.rpc.call(
            "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
        )["holdinvoices"][0]["state"]
        == "SETTLED"
    )

    invoice = l2.rpc.call(
        "holdinvoice",
        {
            "amount_msat": inv_amt // 1000,
            "description": "111fff",
            "cltv": 144,
            "deschashonly": True,
            "preimage": "1b79769fa9a5499681200b165847abf369317a5bbda69509701f6c6099af97cb",
        },
    )
    assert (
        invoice["payment_hash"]
        == "ef7aa96025cbcefb5bdad2ee149f5bff21b8bc2c6ba8fd8ef5e038e8aa6b026b"
    )
    assert (
        invoice["preimage"]
        == "1b79769fa9a5499681200b165847abf369317a5bbda69509701f6c6099af97cb"
    )

    threading.Thread(target=xpay_with_thread, args=(l1, invoice["bolt11"])).start()

    wait_for(
        lambda: l2.rpc.call(
            "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
        )["holdinvoices"][0]["state"]
        == "ACCEPTED"
    )

    l2.rpc.call("holdinvoicesettle", {"payment_hash": invoice["payment_hash"]})

    wait_for(
        lambda: l2.rpc.call(
            "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
        )["holdinvoices"][0]["state"]
        == "SETTLED"
    )

    invoice = l2.rpc.call(
        "holdinvoice",
        {
            "amount_msat": inv_amt // 1000,
            "description": "111fff",
            "cltv": 144,
            "deschashonly": True,
            "payment_hash": "c8e264ee1d94980d99e64d030b7764193e2ca41bd8e4d3475940d6a4273dc16c",
            "preimage": "63e67a1d4b428024e60b262b2573ee17adf2d4e645128713d73c88d4fa086c4e",
        },
    )
    assert (
        invoice["payment_hash"]
        == "c8e264ee1d94980d99e64d030b7764193e2ca41bd8e4d3475940d6a4273dc16c"
    )
    assert (
        invoice["preimage"]
        == "63e67a1d4b428024e60b262b2573ee17adf2d4e645128713d73c88d4fa086c4e"
    )

    threading.Thread(target=xpay_with_thread, args=(l1, invoice["bolt11"])).start()

    wait_for(
        lambda: l2.rpc.call(
            "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
        )["holdinvoices"][0]["state"]
        == "ACCEPTED"
    )

    l2.rpc.call("holdinvoicesettle", {"payment_hash": invoice["payment_hash"]})

    wait_for(
        lambda: l2.rpc.call(
            "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
        )["holdinvoices"][0]["state"]
        == "SETTLED"
    )


def test_fc_hold_then_settle(node_factory, bitcoind, get_plugin):  # noqa: F811
    l1, l2 = node_factory.get_nodes(
        2,
        opts=[
            {},
            {
                "important-plugin": get_plugin,
            },
        ],
    )
    l1.rpc.connect(l2.info["id"], "localhost", l2.port)
    cl1, _ = l1.fundchannel(l2, 1_000_000)

    bitcoind.generate_block(6)

    l1.wait_channel_active(cl1)

    fundsres = l2.rpc.call("listfunds")["outputs"]
    total_funds = 0
    for utxo in fundsres:
        total_funds += utxo["amount_msat"]
    assert total_funds == Millisatoshi(0)

    invoice_amt = 10_000_000

    invoice = l2.rpc.call(
        "holdinvoice",
        {
            "amount_msat": invoice_amt,
            "description": "test_fc_hold_then_settle",
            "cltv": 50,
        },
    )
    assert invoice is not None
    assert isinstance(invoice, dict) is True
    assert "payment_hash" in invoice

    threading.Thread(target=xpay_with_thread, args=(l1, invoice["bolt11"])).start()

    wait_for(
        lambda: l2.rpc.call(
            "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
        )["holdinvoices"][0]["state"]
        == "ACCEPTED"
    )

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )["holdinvoices"][0]
    assert result_lookup["state"] == "ACCEPTED"
    assert "htlc_expiry" in result_lookup
    assert "preimage" in result_lookup
    assert "bolt11" in result_lookup
    assert "description" not in result_lookup

    # test that it's actually holding the htlcs
    # and not letting them through
    time.sleep(5)
    hold_amt = 0
    htlcs = l2.rpc.call("listhtlcs", {})["htlcs"]
    for htlc in htlcs:
        if htlc["payment_hash"] == invoice["payment_hash"]:
            hold_amt += htlc["amount_msat"]
            assert htlc["state"] == "RCVD_ADD_ACK_REVOCATION"

    assert hold_amt >= invoice_amt

    wait_for(
        lambda: l1.rpc.call("listpays", {"payment_hash": invoice["payment_hash"]})[
            "pays"
        ][0]["status"]
        == "pending"
    )

    l1.rpc.close(cl1, 1)
    bitcoind.generate_block(1, wait_for_mempool=1)
    wait_for(
        lambda: l1.rpc.listpeerchannels(l2.info["id"])["channels"][0]["state"]
        == "ONCHAIN"
    )
    wait_for(
        lambda: l2.rpc.listpeerchannels(l1.info["id"])["channels"][0]["state"]
        == "ONCHAIN"
    )
    assert l2.channel_state(l1) == "ONCHAIN"

    result_settle = l2.rpc.call(
        "holdinvoicesettle", {"payment_hash": invoice["payment_hash"]}
    )
    assert result_settle is not None
    assert isinstance(result_settle, dict) is True
    assert result_settle["state"] == "SETTLED"

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )["holdinvoices"][0]
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert result_lookup["state"] == "SETTLED"
    assert "htlc_expiry" not in result_lookup

    fundsres = l2.rpc.call("listfunds")["outputs"]
    total_funds = 0
    for utxo in fundsres:
        total_funds += utxo["amount_msat"]
    assert total_funds == Millisatoshi(0)

    for _ in range(15):
        bitcoind.generate_block(5)
        time.sleep(0.2)

    wait_for(
        lambda: any(
            "ONCHAIN:All outputs resolved" in status_str
            for status_str in l1.rpc.listpeerchannels(l2.info["id"])["channels"][0][
                "status"
            ]
        )
    )
    wait_for(
        lambda: any(
            "ONCHAIN:All outputs resolved" in status_str
            for status_str in l2.rpc.listpeerchannels(l1.info["id"])["channels"][0][
                "status"
            ]
        )
    )

    payres = only_one(
        l1.rpc.call("listpays", {"payment_hash": invoice["payment_hash"]})["pays"]
    )
    assert payres["status"] == "complete"

    fundsres = l2.rpc.call("listfunds")["outputs"]
    total_funds = 0
    for utxo in fundsres:
        total_funds += utxo["amount_msat"]
    assert total_funds > Millisatoshi(0)


def test_valid_hold_then_cancel(node_factory, bitcoind, get_plugin):  # noqa: F811
    l1, l2 = node_factory.get_nodes(
        2,
        opts=[
            {},
            {
                "important-plugin": get_plugin,
            },
        ],
    )
    l1.rpc.connect(l2.info["id"], "localhost", l2.port)
    cl1, _ = l1.fundchannel(l2, 1_000_000)
    cl2, _ = l1.fundchannel(l2, 1_000_000)

    bitcoind.generate_block(6)

    l1.wait_channel_active(cl1)
    l1.wait_channel_active(cl2)

    invoice = l2.rpc.call(
        "holdinvoice",
        {
            "amount_msat": 1_000_100_000,
            "description": "test_valid_hold_then_cancel",
            "cltv": 144,
        },
    )
    assert invoice is not None
    assert isinstance(invoice, dict) is True
    assert "payment_hash" in invoice

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )["holdinvoices"][0]
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert "state" in result_lookup
    assert result_lookup["state"] == "OPEN"
    assert "htlc_expiry" not in result_lookup
    assert "preimage" in result_lookup
    assert "bolt11" in result_lookup
    assert "description" not in result_lookup

    threading.Thread(target=xpay_with_thread, args=(l1, invoice["bolt11"])).start()

    wait_for(
        lambda: l2.rpc.call(
            "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
        )["holdinvoices"][0]["state"]
        == "ACCEPTED"
    )

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )["holdinvoices"][0]
    assert result_lookup["state"] == "ACCEPTED"
    assert "htlc_expiry" in result_lookup

    result_cancel = l2.rpc.call(
        "holdinvoicecancel", {"payment_hash": invoice["payment_hash"]}
    )
    assert result_cancel is not None
    assert isinstance(result_cancel, dict) is True
    assert result_cancel["state"] == "CANCELED"

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )["holdinvoices"][0]
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert result_lookup["state"] == "CANCELED"
    assert "htlc_expiry" not in result_lookup

    LOGGER.info(
        f"Waiting for payment to fail {l1.rpc.call('listpays', {'payment_hash': invoice['payment_hash']})}"
    )
    wait_for(
        lambda: l1.rpc.call("listpays", {"payment_hash": invoice["payment_hash"]})[
            "pays"
        ][0]["status"]
        == "failed"
    )

    htlcs = l2.rpc.call("listhtlcs", {})["htlcs"]
    for htlc in htlcs:
        if htlc["payment_hash"] == invoice["payment_hash"]:
            assert htlc["state"] == "SENT_REMOVE_ACK_REVOCATION"

    # if we cancel we cannot settle after
    with pytest.raises(RpcError, match=r"Holdinvoice is in wrong state: `CANCELED`"):
        l2.rpc.call("holdinvoicesettle", {"payment_hash": invoice["payment_hash"]})


def test_hold_then_block_timeout_soft(node_factory, bitcoind, get_plugin):  # noqa: F811
    l1, l2 = node_factory.get_nodes(
        2,
        opts=[
            {},
            {
                "important-plugin": get_plugin,
            },
        ],
    )
    l1.rpc.connect(l2.info["id"], "localhost", l2.port)
    cl1, _ = l1.fundchannel(l2, 1_000_000)
    cl2, _ = l1.fundchannel(l2, 1_000_000)

    bitcoind.generate_block(6)

    l1.wait_channel_active(cl1)
    l1.wait_channel_active(cl2)

    invoice = l2.rpc.call(
        "holdinvoice",
        {
            "amount_msat": 1_000_100_000,
            "description": "test_hold_then_block_timeout",
            "cltv": 14,
        },
    )
    assert invoice is not None
    assert isinstance(invoice, dict) is True
    assert "payment_hash" in invoice

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )["holdinvoices"][0]
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert "state" in result_lookup
    assert result_lookup["state"] == "OPEN"
    assert "htlc_expiry" not in result_lookup
    assert "preimage" in result_lookup
    assert "bolt11" in result_lookup
    assert "description" not in result_lookup

    threading.Thread(target=xpay_with_thread, args=(l1, invoice["bolt11"])).start()

    wait_for(
        lambda: l2.rpc.call(
            "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
        )["holdinvoices"][0]["state"]
        == "ACCEPTED"
    )

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )["holdinvoices"][0]
    assert result_lookup["state"] == "ACCEPTED"
    assert "htlc_expiry" in result_lookup

    bitcoind.generate_block(10)
    sync_blockheight(bitcoind, [l1, l2])

    wait_for(
        lambda: l2.rpc.call(
            "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
        )["holdinvoices"][0]["state"]
        == "SETTLED"
    )

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )["holdinvoices"][0]
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert "state" in result_lookup
    assert result_lookup["state"] == "SETTLED"
    assert "htlc_expiry" not in result_lookup

    assert l1.is_local_channel_active(cl1) is True
    assert l1.is_local_channel_active(cl2) is True


def test_hold_then_invoice_expiry(node_factory, bitcoind, get_plugin):  # noqa: F811
    l1, l2 = node_factory.get_nodes(
        2,
        opts=[
            {"may_reconnect": True},
            {
                "important-plugin": get_plugin,
                "may_reconnect": True,
            },
        ],
    )
    l1.rpc.connect(l2.info["id"], "localhost", l2.port)
    cl1, _ = l1.fundchannel(l2, 1_000_000)
    cl2, _ = l1.fundchannel(l2, 1_000_000)

    bitcoind.generate_block(6)

    l1.wait_channel_active(cl1)
    l1.wait_channel_active(cl2)

    invoice = l2.rpc.call(
        "holdinvoice",
        {
            "amount_msat": 1_000_100_000,
            "description": "test_hold_then_block_timeout",
            "cltv": 144,
            "expiry": 15,
        },
    )
    assert invoice is not None
    assert isinstance(invoice, dict) is True
    assert "payment_hash" in invoice

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )["holdinvoices"][0]
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert "state" in result_lookup
    assert result_lookup["state"] == "OPEN"
    assert "htlc_expiry" not in result_lookup
    assert "preimage" in result_lookup
    assert "bolt11" in result_lookup
    assert "description" not in result_lookup

    threading.Thread(target=xpay_with_thread, args=(l1, invoice["bolt11"])).start()

    wait_for(
        lambda: l2.rpc.call(
            "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
        )["holdinvoices"][0]["state"]
        == "ACCEPTED"
    )

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )["holdinvoices"][0]
    assert result_lookup["state"] == "ACCEPTED"
    assert "htlc_expiry" in result_lookup

    assert invoice["expires_at"] > time.time()
    while invoice["expires_at"] - time.time() >= -5:
        time.sleep(1)

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )["holdinvoices"][0]
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert "state" in result_lookup
    assert result_lookup["state"] == "ACCEPTED"
    assert "htlc_expiry" in result_lookup

    l2.restart()
    l1.rpc.connect(l2.info["id"], "localhost", l2.port)

    result_settle = l2.rpc.call(
        "holdinvoicesettle", {"payment_hash": invoice["payment_hash"]}
    )
    assert result_settle is not None
    assert isinstance(result_settle, dict) is True
    assert result_settle["state"] == "SETTLED"

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )["holdinvoices"][0]
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert "state" in result_lookup
    assert result_lookup["state"] == "SETTLED"
    assert "htlc_expiry" not in result_lookup


def test_hold_then_block_timeout_hard(node_factory, bitcoind, get_plugin):  # noqa: F811
    l1, l2 = node_factory.get_nodes(
        2,
        opts=[
            {},
            {
                "important-plugin": get_plugin,
            },
        ],
    )
    l1.rpc.connect(l2.info["id"], "localhost", l2.port)
    cl1, _ = l1.fundchannel(l2, 1_000_000)
    cl2, _ = l1.fundchannel(l2, 1_000_000)

    bitcoind.generate_block(6)

    l1.wait_channel_active(cl1)
    l1.wait_channel_active(cl2)

    invoice = l2.rpc.call(
        "holdinvoice",
        {
            "amount_msat": 1_000_100_000,
            "description": "test_hold_then_block_timeout",
            "cltv": 14,
        },
    )
    assert invoice is not None
    assert isinstance(invoice, dict) is True
    assert "payment_hash" in invoice

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )["holdinvoices"][0]
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert "state" in result_lookup
    assert result_lookup["state"] == "OPEN"
    assert "htlc_expiry" not in result_lookup
    assert "preimage" in result_lookup
    assert "bolt11" in result_lookup
    assert "description" not in result_lookup

    threading.Thread(target=xpay_with_thread, args=(l1, invoice["bolt11"])).start()

    wait_for(
        lambda: l2.rpc.call(
            "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
        )["holdinvoices"][0]["state"]
        == "ACCEPTED"
    )

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )["holdinvoices"][0]
    assert result_lookup["state"] == "ACCEPTED"
    assert "htlc_expiry" in result_lookup

    l1.stop()
    l2.stop()
    bitcoind.generate_block(20)
    l2.start()

    wait_for(
        lambda: l2.rpc.call(
            "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
        )["holdinvoices"][0]["state"]
        == "CANCELED"
    )

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )["holdinvoices"][0]
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert "state" in result_lookup
    assert result_lookup["state"] == "CANCELED"
    assert "htlc_expiry" not in result_lookup

    assert l2.is_local_channel_active(cl1) is True
    assert l2.is_local_channel_active(cl2) is True


def test_autoclean(node_factory, bitcoind, get_plugin):  # noqa: F811
    l1, l2 = node_factory.get_nodes(
        2,
        opts=[
            {
                "important-plugin": get_plugin,
                "autoclean-cycle": 1,
            },
            {
                "important-plugin": get_plugin,
                "autoclean-cycle": 1,
                "autoclean-paidinvoices-age": 10,
                "autoclean-expiredinvoices-age": 10,
            },
        ],
    )
    l1.rpc.connect(l2.info["id"], "localhost", l2.port)
    cl1, _ = l1.fundchannel(l2, 1_000_000)
    cl2, _ = l2.fundchannel(l1, 1_000_000)
    mine_funding_to_announce(bitcoind, [l1, l2])
    l1.wait_channel_active(cl1)
    l2.wait_channel_active(cl2)

    invoice1 = l2.rpc.call(
        "holdinvoice", {"amount_msat": 1000, "description": "", "expiry": 11}
    )
    invoice2 = l2.rpc.call("holdinvoice", {"amount_msat": 1000, "description": ""})

    threading.Thread(target=xpay_with_thread, args=(l1, invoice2["bolt11"])).start()

    wait_for(
        lambda: l2.rpc.call(
            "holdinvoicelookup", {"payment_hash": invoice2["payment_hash"]}
        )["holdinvoices"][0]["state"]
        == "ACCEPTED"
    )

    l2.rpc.call("holdinvoicesettle", {"payment_hash": invoice2["payment_hash"]})

    time.sleep(5)

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice1["payment_hash"]}
    )["holdinvoices"][0]
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert "state" in result_lookup
    assert result_lookup["state"] == "OPEN"
    assert "htlc_expiry" not in result_lookup

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice2["payment_hash"]}
    )["holdinvoices"][0]
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert "state" in result_lookup
    assert result_lookup["state"] == "SETTLED"
    assert "htlc_expiry" not in result_lookup

    time.sleep(20)

    with pytest.raises(RpcError, match="payment_hash not found"):
        l2.rpc.call("holdinvoicelookup", {"payment_hash": invoice1["payment_hash"]})

    with pytest.raises(RpcError, match="payment_hash not found"):
        l2.rpc.call("holdinvoicelookup", {"payment_hash": invoice2["payment_hash"]})

    # test default autoclean settings
    invoice1 = l1.rpc.call(
        "holdinvoice", {"amount_msat": 1000, "description": "", "expiry": 11}
    )
    invoice2 = l1.rpc.call("holdinvoice", {"amount_msat": 1000, "description": ""})

    threading.Thread(target=xpay_with_thread, args=(l2, invoice2["bolt11"])).start()

    wait_for(
        lambda: l1.rpc.call(
            "holdinvoicelookup", {"payment_hash": invoice2["payment_hash"]}
        )["holdinvoices"][0]["state"]
        == "ACCEPTED"
    )

    l1.rpc.call("holdinvoicesettle", {"payment_hash": invoice2["payment_hash"]})

    time.sleep(5)

    result_lookup = l1.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice1["payment_hash"]}
    )["holdinvoices"][0]
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert "state" in result_lookup
    assert result_lookup["state"] == "OPEN"
    assert "htlc_expiry" not in result_lookup

    result_lookup = l1.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice2["payment_hash"]}
    )["holdinvoices"][0]
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert "state" in result_lookup
    assert result_lookup["state"] == "SETTLED"
    assert "htlc_expiry" not in result_lookup

    time.sleep(20)

    result_lookup = l1.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice1["payment_hash"]}
    )["holdinvoices"][0]
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert "state" in result_lookup
    assert result_lookup["state"] == "CANCELED"
    assert "htlc_expiry" not in result_lookup

    result_lookup = l1.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice2["payment_hash"]}
    )["holdinvoices"][0]
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert "state" in result_lookup
    assert result_lookup["state"] == "SETTLED"
    assert "htlc_expiry" not in result_lookup
