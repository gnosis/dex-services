from django_eth_events.chainevents import AbstractEventReceiver
from .snapp_event_receiver import DepositReceiver, StateTransitionReceiver, SnappInitializationReceiver

import logging
logger = logging.getLogger(__name__)

RECEIVER_MAPPING = {
    'Deposit': DepositReceiver(),
    'StateTransition': StateTransitionReceiver(),
    'SnappInitialization': SnappInitializationReceiver(),
}

def parse_event(decoded_event):
    res = {param['name']: param['value'] for param in decoded_event['params']}

    # Convert byte strings to hex
    for k, v in res.items():
        if isinstance(v, bytes):
            res[k] = v.hex()
    return res

class EventDispatcher(AbstractEventReceiver):
    def save(self, decoded_event, block_info=None):
        event_name = decoded_event['name']
        listener = RECEIVER_MAPPING.get(event_name, None)
        if listener:
            parsed_event = parse_event(decoded_event)
            logging.info("{} received {}".format(event_name, parsed_event))
            listener.save(parsed_event, block_info)
        else:
            logging.warning("Unhandled Event: {}".format(event_name))

    def rollback(self, decoded_event, block_info=None):
        # TODO - remove event from db
        pass
