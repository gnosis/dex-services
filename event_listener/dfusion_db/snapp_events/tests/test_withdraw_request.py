import unittest
from unittest.mock import Mock

from event_listener.dfusion_db.models import Withdraw
from ..withdraw_request import WithdrawRequestReceiver

EMPTY_STATE_HASH = "0x00000000000000000000000000000000000000000000000000000000000000"

class WithdrawRequestReceiverTest(unittest.TestCase):
    @staticmethod
    def test_save() -> None:
        database = Mock()
        receiver = WithdrawRequestReceiver(database)
        event = {
            "accountId": 1,
            "tokenId": 2,
            "amount": "3",
            "slot": 4,
            "slotIndex": 5,
        }
        receiver.save(event, block_info={})
        database.write_withdraw.assert_called_with(Withdraw.from_dictionary(event))

    @staticmethod
    def test_writes_withdraw() -> None:
        database = Mock()
        receiver = WithdrawRequestReceiver(database)
        withdraw = Withdraw(1, 2, 10, 42, 51)
        receiver.save_parsed(withdraw)
        database.write_withdraw.assert_called_with(withdraw)
