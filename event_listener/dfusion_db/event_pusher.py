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
    state_index = event['stateIndex']
    state_hash = event['stateHash']

    balances = db.accounts.find_one({'stateIndex': state_index - 1})['balances']
    num_tokens = db.constants.find_one()['num_tokens']

    if transition_type == 0:  # Deposit

        applied_deposits = db.deposits.find({'slot': event['slot']})

        for deposit in applied_deposits:
            a_id = deposit['accountId']
            t_id = deposit['tokenId']
            amount = deposit['amount']
            _log.info("Incrementing balance of account {} - token {} by {}".format(a_id, t_id, amount))

            # Balances are stored as [b(a1, t1), b(a1, t2), ... b(a1, T), b(a2, t1), ...]
            balances[num_tokens * (a_id - 1) + (t_id - 1)] += amount

        new_account_record = {
            'stateIndex': state_index,
            'stateHash': state_hash,
            'balances': balances
        }

        db.accounts.insert_one(new_account_record)

    elif transition_type == 1:  # Withdraw
        pass
    elif transition_type == 2:  # Auction
        pass
    else:
        pass


def initialize_accounts(event: Dict[str, Union[str, int, int]]):
    # Will only ever be called once

    num_tokens = event['maxTokens']
    num_accounts = event['maxAccounts']

    account_record: Dict[str, Union[str, str, List[int]]] = {
        'stateIndex': 0,
        'stateHash': event['stateHash'],
        'balances': [0 for _ in range(num_tokens * num_accounts)]
    }

    constants: Dict[str, int] = {
        'num_tokens': num_tokens,
        'num_accounts': num_accounts
    }

    db.constants.insert_one(constants)
    db.accounts.insert_one(account_record)
