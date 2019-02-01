from .event_pusher import post_deposit, post_transition, update_accounts, initialize_accounts, post_withdraw
from abc import ABC, abstractmethod
from typing import Dict, Any

import logging
logger = logging.getLogger(__name__)


class SnappEventListener(ABC):
    """Abstract SnappEventReceiver class."""
    @abstractmethod
    def save(self, event:Dict[str, Any], block_info): pass


class DepositReceiver(SnappEventListener):
    def save(self, parsed_event: Dict[str, Any], block_info):

        # Verify integrity of post data
        assert parsed_event.keys() == {'accountId', 'tokenId', 'amount', 'slot', 'slotIndex'}, "Unexpected Event Keys"
        assert all(isinstance(val, int) for val in parsed_event.values()), "One or more of event values not integer"

        try:
            deposit_id = post_deposit(parsed_event)
            logging.info("Successfully included Deposit - {}".format(deposit_id))
        except AssertionError as exc:
            logging.critical("Failed to record Deposit [{}] - {}".format(exc, parsed_event))


class StateTransitionReceiver(SnappEventListener):
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
            post_transition(parsed_event)
            account_state = update_accounts(parsed_event)
            logging.info("Successfully updated state and updated balances - {}".format(account_state))
        except AssertionError as exc:
            logging.critical("Failed to record StateTransition [{}] - {}".format(exc, parsed_event))


class SnappInitializationReceiver(SnappEventListener):
    def save(self, parsed_event: Dict[str, Any], block_info):

        # Verify integrity of post data
        assert parsed_event.keys() == {'stateHash', 'maxTokens', 'maxAccounts'}, "Unexpected Event Keys"
        state_hash = parsed_event['stateHash']
        assert isinstance(state_hash, str) and len(state_hash) == 64, "StateHash has unexpected values %s" % state_hash
        assert isinstance(parsed_event['maxTokens'], int), "maxTokens has unexpected values"
        assert isinstance(parsed_event['maxAccounts'], int), "maxAccounts has unexpected values"

        try:
            initialize_accounts(parsed_event)
        except AssertionError as exc:
            logging.critical("Failed to record SnappInitialization [{}] - {}".format(exc, parsed_event))


class WithdrawRequestReceiver(SnappEventListener):
    name = 'WithdrawRequest'

    def save(self, parsed_event: Dict[str, Any], block_info):

        # Verify integrity of post data
        assert parsed_event.keys() == {'accountId', 'tokenId', 'amount', 'slot', 'slotIndex'}, "Unexpected Event Keys"
        assert all(isinstance(val, int) for val in parsed_event.values()), "One or more of event values not integer"

        try:
            withdraw_id = post_withdraw(parsed_event)
            logging.info("Successfully included Deposit - {}".format(withdraw_id))
        except AssertionError as exc:
            logging.critical("Failed to record Deposit [{}] - {}".format(exc, parsed_event))
