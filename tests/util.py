import logging
import os
import random
import socket
import string
from pathlib import Path

import pytest

RUST_PROFILE = os.environ.get("RUST_PROFILE", "debug")
COMPILED_PATH = Path.cwd() / "target" / RUST_PROFILE / "holdinvoice"
DOWNLOAD_PATH = Path.cwd() / "tests" / "holdinvoice"


@pytest.fixture
def get_plugin(directory):
    if COMPILED_PATH.is_file():
        return COMPILED_PATH
    elif DOWNLOAD_PATH.is_file():
        return DOWNLOAD_PATH
    else:
        raise ValueError("No plugin was found.")


def generate_random_label():
    label_length = 8
    random_label = "".join(
        random.choice(string.ascii_letters) for _ in range(label_length)
    )
    return random_label


def generate_random_number():
    return random.randint(1, 20_000_000_000_000_00_000)


def pay_with_thread(node, bolt11, partial_msat=None):
    LOGGER = logging.getLogger(__name__)
    try:
        if partial_msat:
            node.rpc.call(
                "pay",
                {
                    "bolt11": bolt11,
                    "dev_use_shadow": False,
                    "retry_for": 20,
                    "partial_msat": partial_msat,
                },
            )
        else:
            node.rpc.call(
                "pay",
                {
                    "bolt11": bolt11,
                    "dev_use_shadow": False,
                    "retry_for": 20,
                },
            )
    except Exception as e:
        LOGGER.info(f"holdinvoice: Error paying payment hash:{e}")
        pass


def find_unused_port():
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("localhost", 0))
        _, port = s.getsockname()
    return port
