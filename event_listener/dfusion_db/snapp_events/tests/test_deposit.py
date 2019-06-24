import unittest
from unittest.mock import Mock

from ..deposit import DepositReceiver
from event_listener.dfusion_db.models import Deposit


class DepositReceiverTest(unittest.TestCase):
    @staticmethod
    def test_save_parsed() -> None:
        database = Mock()
        receiver = DepositReceiver(database)
        deposit = Deposit(1, 2, 10, 42, 51)
        receiver.save_parsed(deposit)
        database.write_deposit.assert_called_with(deposit)

    @staticmethod
    def test_save() -> None:
        database = Mock()
        receiver = DepositReceiver(database)
        event = {
            "accountId": 1,
            "tokenId": 2,
            "amount": 3,
            "slot": 4,
            "slotIndex": 5
        }
        receiver.save(event, block_info={})
        database.write_deposit.assert_called_with(Deposit.from_dictionary(event))