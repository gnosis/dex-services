import unittest
from unittest.mock import Mock

from event_listener.dfusion_db.models import AccountRecord
from .constants import EMPTY_STATE_HASH
from ..snapp_initialization import SnappInitializationReceiver, AuctionInitializationReceiver


class SnappInitializationReceiverTest(unittest.TestCase):

    @staticmethod
    def test_generic_save() -> None:
        database = Mock()
        receiver = SnappInitializationReceiver(database)

        num_tokens = 2
        num_accounts = 3

        event = {
            "stateHash": EMPTY_STATE_HASH,
            "maxTokens": num_tokens,
            "maxAccounts": num_accounts
        }
        receiver.save(event, block_info={})

        database.write_snapp_constants.assert_called_with(2, 3)
        database.write_account_state.assert_called_with(
            AccountRecord(0, EMPTY_STATE_HASH, [0 for _ in range(num_tokens * num_accounts)]))

    @staticmethod
    def test_create_empty_balances() -> None:
        database = Mock()
        receiver = SnappInitializationReceiver(database)

        receiver.initialize_accounts(30, 100, "initial hash")

        state = AccountRecord(0, "initial hash", [0] * 30 * 100)
        database.write_account_state.assert_called_with(state)
        database.write_snapp_constants.assert_called_with(30, 100)


class AuctionInitializationReceiverTest(unittest.TestCase):

    @staticmethod
    def test_generic_save() -> None:
        database = Mock()
        receiver = AuctionInitializationReceiver(database)

        event = {"maxOrders": 2, 'numReservedAccounts': 1, 'ordersPerReservedAccount': 1}
        receiver.save(event, block_info={})

        database.write_auction_constants.assert_called_with(2, 1, 1)

    def test_unexpected_save(self) -> None:
        database = Mock()
        receiver = AuctionInitializationReceiver(database)

        # Bad Value
        with self.assertRaises(AssertionError):
            receiver.save({"maxOrders": "not an integer"}, block_info={})

        # Bad Key
        with self.assertRaises(AssertionError):
            receiver.save({"badKey": 1}, block_info={})
