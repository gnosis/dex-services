from abc import abstractmethod

from django_eth_events.chainevents import AbstractEventReceiver
from .event_pusher import post_deposit, post_transition, update_accounts, initialize_accounts, post_withdraw
from typing import Dict, Any

import logging

logger = logging.getLogger(__name__)


class GenericEventReceiver(AbstractEventReceiver):
    name = None

    def ensure_name(self, _name):
        return self.name == _name

    def parse_event(self, decoded_event):
        res = {param['name']: param['value'] for param in decoded_event['params']}

        # Convert byte strings to hex
        for k, v in res.items():
            if isinstance(v, bytes):
                res[k] = v.hex()

        logging.info("{} received {}".format(self.name, res))
        return res

    def save(self, decoded_event, block_info=None):
        if not self.ensure_name(decoded_event['name']):
            return

        parsed_event = self.parse_event(decoded_event)

        self.real_save(parsed_event, block_info)

    def rollback(self, decoded_event, block_info=None):
        if not self.ensure_name(decoded_event['name']):
            return

        self.real_rollback(decoded_event, block_info)

    @abstractmethod
    def real_save(self, decoded_event, block_info=None):
        pass

    @abstractmethod
    def real_rollback(self, decoded_event, block_info=None):
        pass


class DepositReceiver(GenericEventReceiver):
    name = 'Deposit'

    def real_save(self, parsed_event: Dict[str, int], block_info=None):

        # Verify integrity of post data
        assert parsed_event.keys() == {'accountId', 'tokenId', 'amount', 'slot', 'slotIndex'}, "Unexpected Event Keys"
        assert all(isinstance(val, int) for val in parsed_event.values()), "One or more of event values not integer"

        try:
            deposit_id = post_deposit(parsed_event)
            logging.info("Successfully included Deposit - {}".format(deposit_id))
        except AssertionError as exc:
            logging.critical("Failed to record Deposit [{}] - {}".format(exc, parsed_event))

    def real_rollback(self, decoded_event, block_info=None):
        # TODO - remove event from db
        pass


class StateTransitionReceiver(GenericEventReceiver):
    name = 'StateTransition'

    def real_save(self, parsed_event: Dict[str, Any], block_info=None):

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

    def real_rollback(self, decoded_event, block_info=None):
        # TODO - remove event from db
        pass


class SnappInitializationReceiver(GenericEventReceiver):
    name = 'SnappInitialization'

    def real_save(self, parsed_event: Dict[str, Any], block_info=None):

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

    def real_rollback(self, decoded_event, block_info=None):
        # TODO - remove event from db
        pass


class WithdrawRequestReceiver(GenericEventReceiver):
    name = 'WithdrawRequest'

    def real_save(self, parsed_event: Dict[str, Any], block_info=None):

        # Verify integrity of post data
        assert parsed_event.keys() == {'accountId', 'tokenId', 'amount', 'slot', 'slotIndex'}, "Unexpected Event Keys"
        assert all(isinstance(val, int) for val in parsed_event.values()), "One or more of event values not integer"

        try:
            withdraw_id = post_withdraw(parsed_event)
            logging.info("Successfully included Deposit - {}".format(withdraw_id))
        except AssertionError as exc:
            logging.critical("Failed to record Deposit [{}] - {}".format(exc, parsed_event))

    def real_rollback(self, decoded_event, block_info=None):
        # TODO - remove event from db
        pass

