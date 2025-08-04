#!/usr/bin/python

import logging
import threading
import time

from pyln.testing.fixtures import *  # noqa: F403
from pyln.testing.utils import mine_funding_to_announce, wait_for
from util import get_plugin, xpay_with_thread  # noqa: F401

# number of invoices to create, pay, hold and then cancel
num_iterations = 210
# seconds to hold the invoices with inflight htlcs
delay_seconds = 30
# amount to be used in msat
amount_msat = 1_000_100_000


def lookup_stats(rpc, payment_hashes):
    LOGGER = logging.getLogger(__name__)
    state_counts = {"OPEN": 0, "SETTLED": 0, "CANCELED": 0, "ACCEPTED": 0}
    for payment_hash in payment_hashes:
        try:
            invoice_info = rpc.holdinvoicelookup(payment_hash)
            state = invoice_info["state"]
            state_counts[state] = state_counts.get(state, 0) + 1
        except Exception as e:
            LOGGER.error(
                f"holdinvoice: Error looking up payment hash {payment_hash}:", e
            )
    return state_counts


def test_stress(node_factory, bitcoind, get_plugin):  # noqa: F811
    LOGGER = logging.getLogger(__name__)
    l1, l2, l3, l4, l5, l6, l7, l8, l9, l10, l11 = node_factory.get_nodes(
        11,
        opts=[
            {"important-plugin": get_plugin, "log-level": "debug"},
            {"log-level": "debug"},
            {"log-level": "debug"},
            {"log-level": "debug"},
            {"log-level": "debug"},
            {"log-level": "debug"},
            {"log-level": "debug"},
            {"log-level": "debug"},
            {"log-level": "debug"},
            {"log-level": "debug"},
            {"log-level": "debug"},
        ],
    )
    nodes = [l2, l3, l4, l5, l6, l7, l8, l9, l10, l11]
    for node in nodes:
        node.fundwallet((amount_msat / 1000) * num_iterations * 2)
    LOGGER.info("holdinvoice: Wallet funded")

    opened = 0
    batch = 10
    LOGGER.info(
        f"holdinvoice: Opening {(int(num_iterations / 50) + 1) * batch * len(nodes)} channels..."
    )
    for _ in range(int(num_iterations / 50) + 1):
        for _ in range(batch):
            for node in nodes:
                res = node.rpc.fundchannel(
                    l1.info["id"] + "@localhost:" + str(l1.port),
                    int((amount_msat * 0.95) / 1000),
                    minconf=0,
                )
                opened += 1
        blockid = bitcoind.generate_block(1, wait_for_mempool=res["txid"])[0]

        for i, txid in enumerate(bitcoind.rpc.getblock(blockid)["tx"]):
            if txid == res["txid"]:
                txnum = i

        scid = "{}x{}x{}".format(bitcoind.rpc.getblockcount(), txnum, res["outnum"])
        mine_funding_to_announce(bitcoind, [l1])

        LOGGER.info(f"holdinvoice: Opened {opened} channels")

    l1.wait_channel_active(scid)
    wait_for(
        lambda: all(
            channel["state"] == "CHANNELD_NORMAL"
            for channel in l1.rpc.listpeerchannels()["channels"]
        )
    )

    payment_hashes = []
    invoices = []

    LOGGER.info(f"holdinvoice: Creating {num_iterations} invoices...")
    for _ in range(num_iterations):
        try:
            invoice = l1.rpc.call(
                "holdinvoice",
                {
                    "amount_msat": amount_msat,
                    "description": "masstest",
                    "cltv": 144,
                    "expiry": 3600,
                },
            )
            payment_hash = invoice["payment_hash"]
            payment_hashes.append(payment_hash)
            invoices.append(invoice["bolt11"])
        except Exception as e:
            LOGGER.error("holdinvoice: Error executing command:", e)

    stats = lookup_stats(l1.rpc, payment_hashes)
    LOGGER.info(stats)
    assert stats["OPEN"] == num_iterations

    LOGGER.info(f"holdinvoice: Paying {num_iterations} invoices...")
    i = 0
    for bolt11 in invoices:
        i += 1
        # Pay the invoice using a separate thread
        for node in nodes:
            threading.Thread(
                target=xpay_with_thread, args=(node, bolt11, int(amount_msat / 10))
            ).start()
        LOGGER.info(f"Queued {i * len(nodes)}/{num_iterations * len(nodes)} payments")
        time.sleep(1)

    LOGGER.info(f"holdinvoice: Done paying {num_iterations} invoices!")
    # wait a little more for payments to arrive
    wait_for(lambda: lookup_stats(l1.rpc, payment_hashes)["ACCEPTED"] == num_iterations)

    stats = lookup_stats(l1.rpc, payment_hashes)
    LOGGER.info(stats)
    assert stats["ACCEPTED"] == num_iterations

    htlc_count = 0
    for chan in l1.rpc.listpeerchannels()["channels"]:
        htlc_count += len(chan["htlcs"])

    LOGGER.info(
        f"holdinvoice: Holding {htlc_count} htlcs for {delay_seconds} seconds..."
    )
    time.sleep(delay_seconds)

    stats = lookup_stats(l1.rpc, payment_hashes)
    LOGGER.info(stats)
    assert stats["ACCEPTED"] == num_iterations

    LOGGER.info("holdinvoice: Restarting node...")
    l1.restart()

    stats = lookup_stats(l1.rpc, payment_hashes)
    LOGGER.info(stats)
    assert stats["ACCEPTED"] == num_iterations

    mid = num_iterations // 2

    LOGGER.info(f"holdinvoice: Cancelling {mid} invoices...")
    for payment_hash in payment_hashes[:mid]:
        try:
            l1.rpc.call("holdinvoicecancel", {"payment_hash": payment_hash})
        except Exception as e:
            LOGGER.error(
                f"holdinvoice: Error cancelling payment hash {payment_hash}:",
                e,
            )

    wait_for(lambda: lookup_stats(l1.rpc, payment_hashes)["CANCELED"] == mid)

    LOGGER.info(f"holdinvoice: Settling {num_iterations - mid} invoices...")
    for payment_hash in payment_hashes[mid:]:
        try:
            l1.rpc.call("holdinvoicesettle", {"payment_hash": payment_hash})
        except Exception as e:
            LOGGER.error(
                f"holdinvoice: Error settling payment hash {payment_hash}:",
                e,
            )

    wait_for(
        lambda: lookup_stats(l1.rpc, payment_hashes)["SETTLED"] == num_iterations - mid
    )

    stats = lookup_stats(l1.rpc, payment_hashes)
    LOGGER.info(stats)
    assert stats["CANCELED"] == mid
    assert stats["SETTLED"] == num_iterations - mid
