from pathlib import Path
import string
import random
import logging
import os
import pytest
import socket


RUST_PROFILE = os.environ.get("RUST_PROFILE", "debug")
COMPILED_PATH = Path.cwd() / "target" / RUST_PROFILE / \
    "holdinvoice"
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
    random_label = ''.join(random.choice(string.ascii_letters)
                           for _ in range(label_length))
    return random_label


def generate_random_number():
    return random.randint(1, 20_000_000_000_000_00_000)


def pay_with_thread(rpc, bolt11):
    LOGGER = logging.getLogger(__name__)
    try:
        rpc.dev_pay(bolt11, dev_use_shadow=False)
    except Exception as e:
        LOGGER.debug(f"holdinvoice: Error paying payment hash:{e}")
        pass


def find_unused_port():
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(('localhost', 0))
        _, port = s.getsockname()
    return port
