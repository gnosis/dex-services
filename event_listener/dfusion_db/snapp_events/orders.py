import logging
from typing import Dict, Any

from event_listener.dfusion_db.snapp_event_receiver import SnappEventListener
from ..models import Order, StandingOrder


class OrderReceiver(SnappEventListener):
    def save(self, event: Dict[str, Any], block_info: Dict[str, Any]) -> None:
        self.save_parsed(Order.from_dictionary(event))

    def save_parsed(self, order: Order) -> None:
        self.database.write_order(order)


class StandingOrderBatchReceiver(SnappEventListener):
    def save(self, event: Dict[str, Any], block_info: Dict[str, Any]) -> None:
        self.save_parsed(StandingOrder.from_dictionary(event))

    def save_parsed(self, standing_order: StandingOrder) -> None:
        self.database.write_standing_order(standing_order)
