#!/usr/bin/python

import secrets
import threading
import time

import pytest
from pyln.client import Millisatoshi, RpcError
from pyln.testing.fixtures import *  # noqa: F403
from pyln.testing.utils import only_one, sync_blockheight, wait_for
from util import (
    generate_random_label,
    generate_random_number,
    get_plugin,  # noqa: F401
    pay_with_thread,
)


def test_inputs(node_factory, get_plugin):  # noqa: F811
    node = node_factory.get_node(
        options={
            "important-plugin": get_plugin,
            "log-level": "debug",
        }
    )
    result = node.rpc.call(
        "holdinvoice",
        {
            "amount_msat": 1000000,
            "description": "Valid invoice description",
            "label": generate_random_label(),
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
            "label": generate_random_label(),
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
            "description": "Numbers only as label",
            "label": generate_random_number(),
            "cltv": 144,
        },
    )
    assert result is not None
    assert isinstance(result, dict) is True
    assert "payment_hash" in result

    result = node.rpc.call(
        "holdinvoice",
        {
            "description": "Missing amount",
            "label": generate_random_label(),
            "cltv": 144,
        },
    )
    assert result is not None
    assert isinstance(result, dict) is True
    expected_message = "missing required parameter: amount_msat|msatoshi"
    assert result["message"] == expected_message

    result = node.rpc.call(
        "holdinvoice",
        {"amount_msat": 1000000, "description": "Missing label", "cltv": 144},
    )
    assert result is not None
    assert isinstance(result, dict) is True
    assert result["message"] == "missing required parameter: label"

    result = node.rpc.call(
        "holdinvoice",
        {"amount_msat": 1000000, "label": generate_random_label(), "cltv": 144},
    )
    assert result is not None
    assert isinstance(result, dict) is True
    assert result["message"] == "missing required parameter: description"

    random_hex = secrets.token_hex(32)
    result = node.rpc.call(
        "holdinvoice",
        {
            "amount_msat": 2000000,
            "description": "Invoice with optional fields",
            "label": generate_random_label(),
            "expiry": 3600,
            "fallbacks": [
                "bcrt1qcpw242j4xsjth7ueq9dgmrqtxjyutuvmraeryr",
                "bcrt1qdwydlys0f8khnp87mx688vq4kskjyr68nrx58j",
            ],
            "preimage": random_hex,
            "cltv": 144,
            "deschashonly": True,
        },
    )
    assert result is not None
    assert isinstance(result, dict) is True
    assert "payment_hash" in result

    # Negative amount_msat
    result = node.rpc.call(
        "holdinvoice",
        {
            "amount_msat": -1000,
            "description": "Invalid amount negative",
            "label": generate_random_label(),
            "cltv": 144,
        },
    )
    assert result is not None
    assert isinstance(result, dict) is True
    expected_message = (
        "amount_msat|msatoshi: should be an unsigned "
        "64 bit integer: invalid token '-1000'"
    )
    assert result["message"] == expected_message

    # 0 amount_msat
    with pytest.raises(RpcError, match=r"amount_msat: should be positive msat or"):
        node.rpc.call(
            "holdinvoice",
            {
                "amount_msat": 0,
                "description": "Invalid amount 0",
                "label": generate_random_label(),
                "cltv": 144,
            },
        )

    # Negative expiry value
    result = node.rpc.call(
        "holdinvoice",
        {
            "amount_msat": 500000,
            "description": "Invalid expiry",
            "label": generate_random_label(),
            "expiry": -3600,
            "cltv": 144,
        },
    )
    assert result is not None
    assert isinstance(result, dict) is True
    expected_message = (
        "expiry: should be an unsigned 64 bit integer: invalid token '-3600'"
    )
    assert result["message"] == expected_message

    # Fallbacks not as a list of strings
    result = node.rpc.call(
        "holdinvoice",
        {
            "amount_msat": 800000,
            "description": "Invalid fallbacks",
            "label": generate_random_label(),
            "fallbacks": "invalid_fallback",
            "cltv": 144,
        },
    )
    assert result is not None
    assert isinstance(result, dict) is True
    expected_message = (
        "fallbacks: should be an array: invalid token '\"invalid_fallback\"'"
    )
    assert result["message"] == expected_message

    # Negative cltv value
    result = node.rpc.call(
        "holdinvoice",
        {
            "amount_msat": 1200000,
            "description": "Invalid cltv",
            "label": generate_random_label(),
            "cltv": -144,
        },
    )
    assert result is not None
    assert isinstance(result, dict) is True
    expected_message = "cltv: should be an integer: invalid token '-144'"
    assert result["message"] == expected_message

    # Missing cltv option
    result = node.rpc.call(
        "holdinvoice",
        {
            "amount_msat": 1200000,
            "description": "Missing cltv",
            "label": generate_random_label(),
        },
    )
    assert result is not None
    assert isinstance(result, dict) is True
    expected_message = "missing required parameter: cltv"
    assert result["message"] == expected_message


def test_valid_hold_then_settle(node_factory, bitcoind, get_plugin):  # noqa: F811
    l1, l2 = node_factory.get_nodes(
        2,
        opts={
            "important-plugin": get_plugin,
            "log-level": "debug",
        },
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
            "description": "test_valid_hold_then_settle",
            "label": generate_random_label(),
            "cltv": 144,
        },
    )
    assert invoice is not None
    assert isinstance(invoice, dict) is True
    assert "payment_hash" in invoice

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert "state" in result_lookup
    assert result_lookup["state"] == "OPEN"
    assert "htlc_expiry" not in result_lookup

    # test that it won't settle if it's still open
    result_settle = l2.rpc.call(
        "holdinvoicesettle", {"payment_hash": invoice["payment_hash"]}
    )
    assert result_settle is not None
    assert isinstance(result_settle, dict) is True
    expected_message = "Holdinvoice is in wrong state: 'OPEN'"
    assert result_settle["message"] == expected_message

    threading.Thread(target=pay_with_thread, args=(l1, invoice["bolt11"])).start()

    timeout = 10
    start_time = time.time()

    while time.time() - start_time < timeout:
        result_lookup = l2.rpc.call(
            "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
        )
        assert result_lookup is not None
        assert isinstance(result_lookup, dict) is True

        if result_lookup["state"] == "ACCEPTED":
            break
        else:
            time.sleep(1)

    assert result_lookup["state"] == "ACCEPTED"
    assert "htlc_expiry" in result_lookup

    # test that it's actually holding the htlcs
    # and not letting them through
    doublecheck = only_one(
        l2.rpc.call("listinvoices", {"payment_hash": invoice["payment_hash"]})[
            "invoices"
        ]
    )
    assert doublecheck["status"] == "unpaid"

    result_settle = l2.rpc.call(
        "holdinvoicesettle", {"payment_hash": invoice["payment_hash"]}
    )
    assert result_settle is not None
    assert isinstance(result_settle, dict) is True
    assert result_settle["state"] == "SETTLED"

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert result_lookup["state"] == "SETTLED"
    assert "htlc_expiry" not in result_lookup

    # ask cln if the invoice is actually paid
    # should not be necessary because lookup does this aswell
    doublecheck = only_one(
        l2.rpc.call("listinvoices", {"payment_hash": invoice["payment_hash"]})[
            "invoices"
        ]
    )
    assert doublecheck["status"] == "paid"

    result_cancel_settled = l2.rpc.call(
        "holdinvoicecancel", {"payment_hash": invoice["payment_hash"]}
    )
    assert result_cancel_settled is not None
    assert isinstance(result_cancel_settled, dict) is True
    expected_message = "Holdinvoice is in wrong state: 'SETTLED'"
    assert result_cancel_settled["message"] == expected_message


def test_fc_hold_then_settle(node_factory, bitcoind, get_plugin):  # noqa: F811
    l1, l2 = node_factory.get_nodes(
        2,
        opts={
            "important-plugin": get_plugin,
            "log-level": "debug",
        },
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
            "description": "test_valid_hold_then_settle",
            "label": generate_random_label(),
            "cltv": 50,
        },
    )
    assert invoice is not None
    assert isinstance(invoice, dict) is True
    assert "payment_hash" in invoice

    threading.Thread(target=pay_with_thread, args=(l1, invoice["bolt11"])).start()

    timeout = 10
    start_time = time.time()

    while time.time() - start_time < timeout:
        result_lookup = l2.rpc.call(
            "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
        )
        assert result_lookup is not None
        assert isinstance(result_lookup, dict) is True

        if result_lookup["state"] == "ACCEPTED":
            break
        else:
            time.sleep(1)

    assert result_lookup["state"] == "ACCEPTED"
    assert "htlc_expiry" in result_lookup

    # test that it's actually holding the htlcs
    # and not letting them through
    doublecheck = only_one(
        l2.rpc.call("listinvoices", {"payment_hash": invoice["payment_hash"]})[
            "invoices"
        ]
    )
    assert doublecheck["status"] == "unpaid"

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
    )
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert result_lookup["state"] == "SETTLED"
    assert "htlc_expiry" not in result_lookup

    # ask cln if the invoice is actually paid
    # should not be necessary because lookup does this aswell
    doublecheck = only_one(
        l2.rpc.call("listinvoices", {"payment_hash": invoice["payment_hash"]})[
            "invoices"
        ]
    )
    assert doublecheck["status"] == "paid"

    # payres = only_one(l1.rpc.call(
    #     "listpays", {"payment_hash": invoice["payment_hash"]})["pays"])
    # assert payres["status"] == "complete"

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
        opts={
            "important-plugin": get_plugin,
            "log-level": "debug",
        },
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
            "label": generate_random_label(),
            "cltv": 144,
        },
    )
    assert invoice is not None
    assert isinstance(invoice, dict) is True
    assert "payment_hash" in invoice

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert "state" in result_lookup
    assert result_lookup["state"] == "OPEN"
    assert "htlc_expiry" not in result_lookup

    threading.Thread(target=pay_with_thread, args=(l1, invoice["bolt11"])).start()

    timeout = 10
    start_time = time.time()

    while time.time() - start_time < timeout:
        result_lookup = l2.rpc.call(
            "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
        )
        assert result_lookup is not None
        assert isinstance(result_lookup, dict) is True

        if result_lookup["state"] == "ACCEPTED":
            break
        else:
            time.sleep(1)

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
    )
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert result_lookup["state"] == "CANCELED"
    assert "htlc_expiry" not in result_lookup

    doublecheck = only_one(
        l2.rpc.call("listinvoices", {"payment_hash": invoice["payment_hash"]})[
            "invoices"
        ]
    )
    assert doublecheck["status"] == "unpaid"

    # if we cancel we cannot settle after
    result_settle_canceled = l2.rpc.call(
        "holdinvoicesettle", {"payment_hash": invoice["payment_hash"]}
    )
    assert result_settle_canceled is not None
    assert isinstance(result_settle_canceled, dict) is True
    expected_message = "Holdinvoice is in wrong state: 'CANCELED'"
    result_settle_canceled["message"] == expected_message


def test_hold_then_block_timeout_soft(node_factory, bitcoind, get_plugin):  # noqa: F811
    l1, l2 = node_factory.get_nodes(
        2,
        opts={
            "important-plugin": get_plugin,
            "log-level": "debug",
        },
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
            "label": generate_random_label(),
            "cltv": 14,
        },
    )
    assert invoice is not None
    assert isinstance(invoice, dict) is True
    assert "payment_hash" in invoice

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert "state" in result_lookup
    assert result_lookup["state"] == "OPEN"
    assert "htlc_expiry" not in result_lookup

    threading.Thread(target=pay_with_thread, args=(l1, invoice["bolt11"])).start()

    timeout = 10
    start_time = time.time()

    while time.time() - start_time < timeout:
        result_lookup = l2.rpc.call(
            "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
        )
        assert result_lookup is not None
        assert isinstance(result_lookup, dict) is True

        if result_lookup["state"] == "ACCEPTED":
            break
        else:
            time.sleep(1)

    assert result_lookup["state"] == "ACCEPTED"
    assert "htlc_expiry" in result_lookup

    bitcoind.generate_block(10)
    sync_blockheight(bitcoind, [l1, l2])

    timeout = 10
    start_time = time.time()

    while time.time() - start_time < timeout:
        result_lookup = l2.rpc.call(
            "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
        )
        assert result_lookup is not None
        assert isinstance(result_lookup, dict) is True

        if result_lookup["state"] == "SETTLED":
            break
        else:
            time.sleep(1)

    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert "state" in result_lookup
    assert result_lookup["state"] == "SETTLED"
    assert "htlc_expiry" not in result_lookup

    assert l1.is_local_channel_active(cl1) is True
    assert l1.is_local_channel_active(cl2) is True


def test_hold_then_invoice_timeout_soft(node_factory, bitcoind, get_plugin):  # noqa: F811
    l1, l2 = node_factory.get_nodes(
        2,
        opts={
            "important-plugin": get_plugin,
            "holdinvoice-cancel-before-invoice-expiry": 20,
        },
    )
    l1.rpc.connect(l2.info["id"], "localhost", l2.port)
    cl1, _ = l1.fundchannel(l2, 1_000_000)
    cl2, _ = l1.fundchannel(l2, 1_000_000)

    bitcoind.generate_block(6)

    l1.wait_channel_active(cl1)
    l1.wait_channel_active(cl2)

    invoice_time = time.time() + 35
    invoice = l2.rpc.call(
        "holdinvoice",
        {
            "amount_msat": 1_000_100_000,
            "description": "test_hold_then_block_timeout",
            "label": generate_random_label(),
            "cltv": 144,
            "expiry": 35,
        },
    )
    assert invoice is not None
    assert isinstance(invoice, dict) is True
    assert "payment_hash" in invoice

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert "state" in result_lookup
    assert result_lookup["state"] == "OPEN"
    assert "htlc_expiry" not in result_lookup

    threading.Thread(target=pay_with_thread, args=(l1, invoice["bolt11"])).start()

    timeout = 10
    start_time = time.time()

    while time.time() - start_time < timeout:
        result_lookup = l2.rpc.call(
            "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
        )
        assert result_lookup is not None
        assert isinstance(result_lookup, dict) is True

        if result_lookup["state"] == "ACCEPTED":
            break
        else:
            time.sleep(1)

    assert result_lookup["state"] == "ACCEPTED"
    assert "htlc_expiry" in result_lookup

    assert invoice_time > time.time()
    while invoice_time - time.time() >= 17:
        time.sleep(1)

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert "state" in result_lookup
    assert result_lookup["state"] == "SETTLED"
    assert "htlc_expiry" not in result_lookup


def test_hold_then_block_timeout_hard(node_factory, bitcoind, get_plugin):  # noqa: F811
    l1, l2 = node_factory.get_nodes(
        2,
        opts={"important-plugin": get_plugin, "log-level": "debug"},
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
            "label": generate_random_label(),
            "cltv": 14,
        },
    )
    assert invoice is not None
    assert isinstance(invoice, dict) is True
    assert "payment_hash" in invoice

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert "state" in result_lookup
    assert result_lookup["state"] == "OPEN"
    assert "htlc_expiry" not in result_lookup

    threading.Thread(target=pay_with_thread, args=(l1, invoice["bolt11"])).start()

    timeout = 10
    start_time = time.time()

    while time.time() - start_time < timeout:
        result_lookup = l2.rpc.call(
            "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
        )
        assert result_lookup is not None
        assert isinstance(result_lookup, dict) is True

        if result_lookup["state"] == "ACCEPTED":
            break
        else:
            time.sleep(1)

    assert result_lookup["state"] == "ACCEPTED"
    assert "htlc_expiry" in result_lookup

    l1.stop()
    l2.stop()
    bitcoind.generate_block(20)
    l2.start()

    timeout = 10
    start_time = time.time()

    while time.time() - start_time < timeout:
        result_lookup = l2.rpc.call(
            "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
        )
        assert result_lookup is not None
        assert isinstance(result_lookup, dict) is True

        if result_lookup["state"] == "SETTLED":
            break
        else:
            time.sleep(1)

    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert "state" in result_lookup
    assert result_lookup["state"] == "CANCELED"
    assert "htlc_expiry" not in result_lookup

    assert l2.is_local_channel_active(cl1) is True
    assert l2.is_local_channel_active(cl2) is True


def test_hold_then_invoice_timeout_hard(node_factory, bitcoind, get_plugin):  # noqa: F811
    l1, l2 = node_factory.get_nodes(
        2,
        opts={
            "important-plugin": get_plugin,
            "holdinvoice-cancel-before-invoice-expiry": 10,
            "log-level": "debug",
        },
    )
    l1.rpc.connect(l2.info["id"], "localhost", l2.port)
    cl1, _ = l1.fundchannel(l2, 1_000_000)
    cl2, _ = l1.fundchannel(l2, 1_000_000)

    bitcoind.generate_block(6)

    l1.wait_channel_active(cl1)
    l1.wait_channel_active(cl2)

    invoice_time = time.time() + 25
    invoice = l2.rpc.call(
        "holdinvoice",
        {
            "amount_msat": 1_000_100_000,
            "description": "test_hold_then_block_timeout",
            "label": generate_random_label(),
            "cltv": 144,
            "expiry": 25,
        },
    )
    assert invoice is not None
    assert isinstance(invoice, dict) is True
    assert "payment_hash" in invoice

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert "state" in result_lookup
    assert result_lookup["state"] == "OPEN"
    assert "htlc_expiry" not in result_lookup

    threading.Thread(target=pay_with_thread, args=(l1, invoice["bolt11"])).start()

    timeout = 10
    start_time = time.time()

    while time.time() - start_time < timeout:
        result_lookup = l2.rpc.call(
            "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
        )
        assert result_lookup is not None
        assert isinstance(result_lookup, dict) is True

        if result_lookup["state"] == "ACCEPTED":
            break
        else:
            time.sleep(1)

    assert result_lookup["state"] == "ACCEPTED"
    assert "htlc_expiry" in result_lookup

    l2.stop()
    assert invoice_time > time.time()
    while invoice_time - time.time() >= 0:
        time.sleep(1)
    l2.start()

    result_lookup = l2.rpc.call(
        "holdinvoicelookup", {"payment_hash": invoice["payment_hash"]}
    )
    assert result_lookup is not None
    assert isinstance(result_lookup, dict) is True
    assert "state" in result_lookup
    assert result_lookup["state"] == "CANCELED"
    assert "htlc_expiry" not in result_lookup
