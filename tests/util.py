from pathlib import Path
import string
import random
import logging
import os
import pytest
import subprocess


RUST_PROFILE = os.environ.get("RUST_PROFILE", "debug")
COMPILED_PATH = Path.cwd() / "target" / RUST_PROFILE / \
    "holdinvoice"
DOWNLOAD_PATH = Path.cwd() / "tests" / "holdinvoice"


@pytest.fixture
def get_plugin(directory):
    LOGGER = logging.getLogger(__name__)
    proto_folder = os.path.join(Path.cwd(), "proto")
    grpc_out_folder = os.path.join(Path.cwd(), "tests")

    command = [
        'python', '-m', 'grpc_tools.protoc',
        f'--proto_path={proto_folder}',
        f'--python_out={grpc_out_folder}',
        f'--grpc_python_out={grpc_out_folder}',
        'hold.proto', 'primitives.proto'
    ]

    # Run the command
    result = subprocess.run(command)
    LOGGER.info(f"COCK {result}")

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
