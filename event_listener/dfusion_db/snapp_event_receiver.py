from .database_interface import MongoDbInterface
from abc import ABC, abstractmethod
from typing import Dict, Any, Union, List

import logging

class SnappEventListener(ABC):
    """Abstract SnappEventReceiver class."""
    def __init__(self, database_interface=MongoDbInterface()):
        self.db = database_interface
        self.logger = logging.getLogger(__name__)

    @abstractmethod
    def save(self, event:Dict[str, Any], block_info): pass


class DepositReceiver(SnappEventListener):
    def save(self, parsed_event: Dict[str, Any], block_info):

        # Verify integrity of post data
        assert parsed_event.keys() == {'accountId', 'tokenId', 'amount', 'slot', 'slotIndex'}, "Unexpected Event Keys"
        assert all(isinstance(val, int) for val in parsed_event.values()), "One or more of event values not integer"

        try:
            self.db.write_deposit(parsed_event)
        except AssertionError as exc:
            logging.critical("Failed to record Deposit [{}] - {}".format(exc, parsed_event))

class StateTransitionReceiver(SnappEventListener):
    TRANSITION_TYPES = {
        'Deposit': 0,
        'Withdraw': 1,
        'Auction': 2
    }

    def save(self, parsed_event: Dict[str, Any], block_info):

        # Verify integrity of post data
        assert parsed_event.keys() == {'transitionType', 'stateIndex', 'stateHash', 'slot'}, \
            "Unexpected Event Keys: got {}".format(parsed_event.keys())
        _hash = parsed_event['stateHash']
        _type = parsed_event['transitionType']

        assert isinstance(parsed_event['stateIndex'], int), "Transition to has unexpected values"
        assert isinstance(_hash, str) and len(_hash) == 64, "Transition from has unexpected values"
        assert isinstance(_type, int) and _type in {0, 1, 2}, "Transition type not recognized"
        assert isinstance(parsed_event['slot'], int), "Transition slot not recognized"
        # TODO - move the above assertions into a generic type for StateTransition

        try:
            self.update_accounts(parsed_event)
            logging.info("Successfully updated state and balances")
        except AssertionError as exc:
            logging.critical("Failed to record StateTransition [{}] - {}".format(exc, parsed_event))
    
    def update_accounts(self, event: Dict[str, Union[int, str, str, int]]):
        """
        :param event: dict
        :return: bson.objectid.ObjectId
        """
        transition_type = event['transitionType']
        state_index = event['stateIndex']
        state_hash = event['stateHash']

        balances = self.db.get_account_state(state_index - 1)['balances']
        num_tokens = self.db.get_num_tokens()

        applied_data = self.get_data_to_apply(transition_type, event['slot'])

        for datum in applied_data:
            a_id = datum['accountId']
            t_id = datum['tokenId']
            amount = datum['amount']

            # Balances are stored as [b(a1, t1), b(a1, t2), ... b(a1, T), b(a2, t1), ...]
            index = num_tokens * (a_id - 1) + (t_id - 1)

            if transition_type == self.TRANSITION_TYPES['Deposit']:

                self.logger.info("Incrementing balance of account {} - token {} by {}".format(a_id, t_id, amount))
                balances[num_tokens * (a_id - 1) + (t_id - 1)] += amount

            elif transition_type == self.TRANSITION_TYPES['Withdraw']:

                if balances[index] - amount >= 0:
                    self.logger.info("Decreasing balance of account {} - token {} by {}".format(a_id, t_id, amount))
                    balances[index] -= amount
                else:
                    self.logger.info("Insufficient balance: account {} - token {} for amount {}".format(a_id, t_id, amount))

            elif transition_type == self.TRANSITION_TYPES['Auction']:
                pass

            else:
                # This can not happen
                self.logger.error("Unrecognized transition type - this should never happen")

        new_account_record = {
            'stateIndex': state_index,
            'stateHash': state_hash,
            'balances': balances
        }
        self.db.write_account_state(new_account_record)

    def get_data_to_appy(self, transition_type, slot):
        if transition_type == 0:
            return self.db.get_deposits(slot)
        elif transition_type == 1:
            return self.db.get_withdraws(slot)
        else:
            throw RuntimeError("Invalid transition type: " + transition_type)


class SnappInitializationReceiver(SnappEventListener):
    def save(self, parsed_event: Dict[str, Any], block_info):

        # Verify integrity of post data
        assert parsed_event.keys() == {'stateHash', 'maxTokens', 'maxAccounts'}, "Unexpected Event Keys"
        state_hash = parsed_event['stateHash']
        assert isinstance(state_hash, str) and len(state_hash) == 64, "StateHash has unexpected values %s" % state_hash
        assert isinstance(parsed_event['maxTokens'], int), "maxTokens has unexpected values"
        assert isinstance(parsed_event['maxAccounts'], int), "maxAccounts has unexpected values"

        try:
            self.initialize_accounts(parsed_event)
        except AssertionError as exc:
            logging.critical("Failed to record SnappInitialization [{}] - {}".format(exc, parsed_event))
    
    def initialize_accounts(self, event: Dict[str, Union[str, int, int]]):
        num_tokens = event['maxTokens']
        num_accounts = event['maxAccounts']

        account_record: Dict[str, Union[str, str, List[int]]] = {
            'stateIndex': 0,
            'stateHash': event['stateHash'],
            'balances': [0 for _ in range(num_tokens * num_accounts)]
        }

        self.db.write_constants(num_tokens, num_accounts)
        self.db.write_account_state(account_record)


class WithdrawRequestReceiver(SnappEventListener):
    def save(self, parsed_event: Dict[str, Any], block_info):

        # Verify integrity of post data
        assert parsed_event.keys() == {'accountId', 'tokenId', 'amount', 'slot', 'slotIndex'}, "Unexpected Event Keys"
        assert all(isinstance(val, int) for val in parsed_event.values()), "One or more of event values not integer"

        try:
            self.db.write_withdraw(parsed_event)
        except AssertionError as exc:
            logging.critical("Failed to record Deposit [{}] - {}".format(exc, parsed_event))