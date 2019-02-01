import json
import os


def abi_file_path(file):
    relative_path = os.path.join(os.path.relpath('../dex-contracts/build/contracts'), file)
    return os.path.abspath(relative_path)


def load_contract_abi(path):
    with open(path) as f:
        contract = json.load(f)
        return contract.get('abi')
