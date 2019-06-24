import unittest
from unittest.mock import Mock
from typing import List

from ..auction_settlement import AuctionSettlementReceiver
from event_listener.dfusion_db.models import AccountRecord, Order

EMPTY_STATE_HASH = "0x00000000000000000000000000000000000000000000000000000000000000"


class AuctionSettlementReceiverTest(unittest.TestCase):

    @staticmethod
    def test_save() -> None:
        num_tokens = 2
        num_accounts = 2
        num_orders = 2
        old_balances = [42] * num_accounts * num_tokens
        dummy_account_record = AccountRecord(1, EMPTY_STATE_HASH, old_balances)

        def int_list_to_hex_bytes(arr: List[int], num_bits: int) -> str:
            assert num_bits % 4 == 0
            hex_length = num_bits // 4
            return "".join(map(lambda t: str(hex(t))[2:].rjust(hex_length, "0"), arr))

        database = Mock()
        receiver = AuctionSettlementReceiver(database)

        orders = [
            Order.from_dictionary({
                "auctionId": 1,
                "slotIndex": 4,
                "accountId": 0,
                "buyToken": 1,
                "sellToken": 0,
                "buyAmount": 10,
                "sellAmount": 10,
            }),
            Order.from_dictionary({
                "auctionId": 1,
                "slotIndex": 4,
                "accountId": 1,
                "buyToken": 0,
                "sellToken": 1,
                "buyAmount": 8,
                "sellAmount": 16,
            }),
        ]

        prices = [16, 10]
        executed_amounts = [16, 10, 10, 16]
        encoded_solution = int_list_to_hex_bytes(prices, 96) + int_list_to_hex_bytes(executed_amounts, 96)

        event = {
            "auctionId": 1,
            "stateIndex": 2,
            "stateHash": EMPTY_STATE_HASH,
            "pricesAndVolumes": encoded_solution
        }

        database.get_account_state.return_value = dummy_account_record
        database.get_orders.return_value = orders
        database.get_num_tokens.return_value = num_tokens
        database.get_num_orders.return_value = num_orders
        receiver.save(event, block_info={})

        new_balances = [42 - 10, 42 + 16, 42 + 10, 42 - 16]

        new_account_record = AccountRecord(2, EMPTY_STATE_HASH, new_balances)
        database.write_account_state.assert_called_with(new_account_record)
