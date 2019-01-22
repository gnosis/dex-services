from pymongo import MongoClient
from django.conf import settings
from typing import Dict, List, Union

import logging

_log = logging.getLogger(__name__)

client = MongoClient(
    host=settings.DB_HOST,
    port=settings.DB_PORT
)
db = client.get_database(settings.DB_NAME)


def post_deposit(event: dict):
    """
    :param event: dict
    :return: bson.objectid.ObjectId
    """

    deposits = db.deposits
    deposit_id = deposits.insert_one(event).inserted_id
    return deposit_id


def post_transition(event: dict):
    """
    :param event: dict
    :return: bson.objectid.ObjectId
    """

    transitions = db.transitions
    transition_id = transitions.insert_one(event).inserted_id
    return transition_id


def update_accounts(event: Dict[str, Union[int, str, str, int]]):
    """
    :param event: dict
    :return: bson.objectid.ObjectId
    """
    transition_type = event['transitionType']
    prev_state = event['from']
    new_state = event['to']

    balances = db.accounts.find_one({'currState': prev_state})['balances']

    num_tokens = db.constants.find_one()['num_tokens']
    num_accounts = db.constants.find_one()['num_accounts']

    if transition_type == 0:  # Deposit

        applied_deposits = db.deposits.find({'slot': event['slot']})

        for deposit in applied_deposits:

            a_id = deposit['accountId']
            t_id = deposit['tokenId']
            # Assuming index by accounts - tokens
            balances[num_tokens*(a_id - 1) + (t_id - 1)] += deposit['amount']

            # # Assuming index by accounts - tokens
            # balances[num_accounts * (t_id - 1) + (a_id - 1)] += deposit['amount']

        new_account_record = {
            'prevState': prev_state,
            'currState': new_state,
            'balances': balances
        }

        db.accounts.insert_one(new_account_record)

    elif transition_type == 1:  # Withdraw
        pass
    elif transition_type == 2:
        pass
    else:
        pass


def initialize_accounts(event: Dict[str, Union[str, int, int]]):
    # Will only ever be called once

    # Verify integrity of post data
    assert event.keys() == {'stateHash', 'maxTokens', 'maxAccounts'}, "Unexpected Event Keys"
    state_hash = event['stateHash']
    num_tokens = event['maxTokens']
    num_accounts = event['maxAccounts']

    assert isinstance(state_hash, str) and len(state_hash) == 64, "StateHash has unexpected values"
    assert isinstance(num_tokens, int), "maxTokens has unexpected values"
    assert isinstance(num_accounts, int), "maxAccounts has unexpected values"

    account_record: Dict[str, Union[str, str, List[int]]] = {
        'prevState': "0"*64,
        'currState': state_hash,
        'balances': [0 for _ in range(num_tokens*num_accounts)]
    }

    constants: Dict[str, int] = {
        'num_tokens': num_tokens,
        'num_accounts': num_accounts
    }

    db.constants.insert_one(constants)

    db.accounts.insert_one(account_record)
