#!/usr/bin/python

import logging

from pyln.testing.fixtures import *  # noqa: F403
from pyln.testing.utils import wait_for
from util import get_plugin, xpay_with_thread  # noqa: F401

# number of invoices to create and then clean
num_iterations = 10_000


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
    l1 = node_factory.get_node(
        options={
            "important-plugin": get_plugin,
            "log-level": ["debug", "info:holdinvoice"],
            "holdinvoice-cancel-before-invoice-expiry": 10,
            "autoclean-cycle": 60,
            "autoclean-paidinvoices-age": 600,
            "autoclean-expiredinvoices-age": 600,
        },
    )

    for i in range(num_iterations):
        l1.rpc.call(
            "holdinvoice",
            {
                "amount_msat": 1000,
                "description": "ughfeiurghielughieurhgieuhiuhojdaoidjoaidjoaidjoawiwdjoaiwdjoaidjoai",
                "expiry": 11,
            },
        )
        if i % 100 == 0:
            LOGGER.info(f"holdinvoice: {i} invoices created")

    wait_for(
        lambda: len(
            l1.rpc.call("listdatastore", {"key": ["holdinvoice_v2"]})["datastore"]
        )
        == 0
    )
