import logging
from typing import Dict, Any

from django_eth_events.chainevents import AbstractEventReceiver

from .snapp_events.auction_settlement import AuctionSettlementReceiver
from .snapp_events.deposit import DepositReceiver
from .snapp_events.orders import OrderReceiver, StandingOrderBatchReceiver
from .snapp_events.snapp_initialization import SnappInitializationReceiver, AuctionInitializationReceiver
from .snapp_events.state_transition import StateTransitionReceiver
from .snapp_events.withdraw_request import WithdrawRequestReceiver

RECEIVER_MAPPING = {
    'Deposit': DepositReceiver(),
    'WithdrawRequest': WithdrawRequestReceiver(),
    'StateTransition': StateTransitionReceiver(),
    'SnappInitialization': SnappInitializationReceiver(),
    'SellOrder': OrderReceiver(),
    'AuctionSettlement': AuctionSettlementReceiver(),
    'AuctionInitialization': AuctionInitializationReceiver(),
    'StandingSellOrderBatch': StandingOrderBatchReceiver(),
}


def parse_event(decoded_event: Dict[str, Any]) -> Dict[str, Any]:
    res = {param['name']: param['value'] for param in decoded_event['params']}

    # Convert byte strings to hex
    for k, v in res.items():
        if isinstance(v, bytes):
            res[k] = v.hex()
    return res


class EventDispatcher(AbstractEventReceiver):  # type: ignore
    def save(self, decoded_event: Dict[str, Any], block_info: Dict[str, Any]) -> None:
        event_name = decoded_event['name']
        listener = RECEIVER_MAPPING.get(event_name, None)
        if listener:
            parsed_event = parse_event(decoded_event)
            logging.info("{} received {}".format(event_name, parsed_event))
            listener.save(parsed_event, block_info)
        else:
            logging.warning("Unhandled Event: {}".format(event_name))

    def rollback(self, decoded_event: Dict[str, Any], block_info: Dict[str, Any]) -> None:
        # TODO - remove event from db
        pass
