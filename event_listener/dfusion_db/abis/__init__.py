import json
import os


def abi_file_path(file):
    return os.path.abspath(os.path.join(os.path.dirname(__file__), file))


def load_json_file(path):
    with open(path) as f:
        return json.load(f)
