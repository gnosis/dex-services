from abc import abstractmethod

from django_eth_events.chainevents import AbstractEventReceiver
from .event_pusher import post_deposit, post_transition, update_accounts
from typing import Dict, Any

import logging

logger = logging.getLogger(__name__)


class GenericEventReceiver(AbstractEventReceiver):
    name = None

    def ensure_name(self, _name):
        return self.name == _name

    def parse_event(self, decoded_event):
        res = {param['name']: param['value'] for param in decoded_event['params']}
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

        try:
            post_deposit(parsed_event)
        except AssertionError as exc:
            logging.critical("Failed to record Deposit [{}] - {}".format(exc, parsed_event))

    def real_rollback(self, decoded_event, block_info=None):
        # TODO - remove event from db
        pass


class StateTransitionReceiver(GenericEventReceiver):
    name = 'StateTransition'

    def real_save(self, parsed_event: Dict[str, Any], block_info=None):

        # Convert byte strings to hex
        for k, v in parsed_event.items():
            if isinstance(v, bytes):
                parsed_event[k] = v.hex()

        # Verify integrity of post data
        assert parsed_event.keys() == {'transitionType', 'to', 'from', 'slot'}, "Unexpected Event Keys"
        _to = parsed_event['to']
        _from = parsed_event['from']
        _type = parsed_event['transitionType']

        assert isinstance(_to, str) and len(_to) == 64, "Transition to has unexpected values"
        assert isinstance(_from, str) and len(_from) == 64, "Transition from has unexpected values"
        assert isinstance(_type, int) and _type in {0, 1, 2}, "Transition type not recognized"
        assert isinstance(parsed_event['slot'], int), "Transition slot not recognized"
        # TODO - move the above assertions into a generic type for StateTransition

        try:
            post_transition(parsed_event)
            update_accounts(parsed_event)
        except AssertionError as exc:
            logging.critical("Failed to record StateTransition [{}] - {}".format(exc, parsed_event))

    def real_rollback(self, decoded_event, block_info=None):
        # TODO - remove event from db
        pass




