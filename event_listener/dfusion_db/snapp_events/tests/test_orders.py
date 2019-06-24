import unittest
from unittest.mock import Mock

from ..orders import OrderReceiver, StandingOrderBatchReceiver
from event_listener.dfusion_db.models import Order, StandingOrder


class OrderReceiverTest(unittest.TestCase):
    @staticmethod
    def test_save() -> None:
        database = Mock()
        receiver = OrderReceiver(database)
        event = {
            "auctionId": 1,
            "slotIndex": 2,
            "accountId": 3,
            "buyToken": 4,
            "sellToken": 5,
            "buyAmount": "67",
            "sellAmount": "89",
        }
        receiver.save(event, block_info={})
        database.write_order.assert_called_with(Order.from_dictionary(event))

    @staticmethod
    def test_writes_order() -> None:
        database = Mock()
        receiver = OrderReceiver(database)
        order = Order(1, 1, 2, 1, 1, 1, 1)
        receiver.save_parsed(order)
        database.write_order.assert_called_with(order)


class StandingOrderBatchReceiverTest(unittest.TestCase):

    @staticmethod
    def test_writes_order() -> None:
        database = Mock()
        receiver = StandingOrderBatchReceiver(database)
        order = StandingOrder(1, 2, 3, [])
        receiver.save_parsed(order)
        database.write_standing_order.assert_called_with(order)
